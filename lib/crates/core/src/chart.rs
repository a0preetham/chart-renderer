//! Orchestration: spec -> domains -> insets -> layout -> scales -> scene.
//!
//! The ordering here is the load-bearing part. Domains and tick *labels* depend
//! only on the data, so they are computed first; measuring them gives the axis
//! and legend insets; the insets give the canvas size and plot origin; only then
//! can tick *positions* and mark geometry be placed. Doing it in this order is
//! what lets `width: 300` mean a 300px plot regardless of label length, which is
//! the Vega-Lite convention.

use crate::axis::{self, AxisKind, AxisStyle, ResolvedAxis};
use crate::data::{self, DiscreteColumn, NumericColumn};
use crate::error::{Error, Result};
use crate::ir::{Color, PlotRect, Scene};
use crate::layout::{self, Insets, Rect};
use crate::legend::{self, Legend, LegendStyle};
use crate::marks;
use crate::palette;
use crate::scale::{self, BandScale, LinearScale};
use crate::spec::{Channel, FieldType, MarkType, Spec};
use crate::text::{default_shaper, TextShaper};
use crate::transform;

/// Vega's default tick density: roughly one tick per 40px of axis.
fn default_tick_count(extent_px: f32) -> usize {
    ((extent_px / 40.0).ceil() as usize).max(2)
}

/// Which positional channel carries the discrete field, for a bar mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Discrete on x, quantitative on y — upright bars.
    Vertical,
    /// Discrete on y, quantitative on x — sideways bars.
    Horizontal,
}

/// One channel's data, already extracted into a column.
enum Values {
    Numeric(NumericColumn),
    Discrete(DiscreteColumn),
}

impl Values {
    fn len(&self) -> usize {
        match self {
            Values::Numeric(c) => c.len(),
            Values::Discrete(c) => c.len(),
        }
    }
}

/// Which positional channel a plan is for. Vega's `zero` default differs
/// between them, so this is not cosmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

/// A resolved positional channel: its type, title, data, and scale overrides.
struct Positional {
    ftype: FieldType,
    title: Option<String>,
    values: Values,
    /// Explicit `scale.zero`, which overrides the per-mark default.
    zero: Option<bool>,
}

/// Whether a quantitative domain is extended to include zero.
///
/// Verified against Vega rather than assumed, because the rule is not uniform:
/// y always includes zero, and x does for `bar` and `point` but **not** for
/// `line`. It is genuinely channel-based, not value-axis-based — transposing a
/// line chart keeps zero off x and on y.
fn zero_default(mark: MarkType, axis: Axis) -> bool {
    !(mark == MarkType::Line && axis == Axis::X)
}

/// What a positional channel contributes to its axis.
enum Plan {
    Quantitative {
        lo: f64,
        hi: f64,
        ticks: Vec<f64>,
        labels: Vec<String>,
    },
    Discrete {
        domain: Vec<String>,
    },
}

impl Plan {
    fn labels(&self) -> &[String] {
        match self {
            Plan::Quantitative { labels, .. } => labels,
            Plan::Discrete { domain } => domain,
        }
    }

    fn is_discrete(&self) -> bool {
        matches!(self, Plan::Discrete { .. })
    }
}

/// A positional scale, once the plot rectangle is known.
enum Placed {
    Linear(LinearScale),
    Band(BandScale),
}

impl Placed {
    /// Where row `i` sits along this axis. Discrete values use the band centre,
    /// which is where a point or line vertex belongs.
    fn at(&self, values: &Values, i: usize) -> Option<f32> {
        match (self, values) {
            (Placed::Linear(s), Values::Numeric(c)) => {
                c.get(i).copied().flatten().map(|v| s.scale(v))
            }
            (Placed::Band(s), Values::Discrete(c)) => c.get(i)?.as_deref().and_then(|v| s.center(v)),
            _ => None,
        }
    }

    fn band(&self) -> Option<&BandScale> {
        match self {
            Placed::Band(s) => Some(s),
            Placed::Linear(_) => None,
        }
    }

    fn linear(&self) -> Option<&LinearScale> {
        match self {
            Placed::Linear(s) => Some(s),
            Placed::Band(_) => None,
        }
    }
}

pub fn build(spec: &Spec) -> Result<Scene> {
    let shaper = default_shaper();
    build_with_shaper(spec, &shaper)
}

/// Exposed for tests and benchmarks that want to pin a specific shaper.
pub fn build_with_shaper(spec: &Spec, shaper: &dyn TextShaper) -> Result<Scene> {
    let mark = spec.mark.mark_type()?;
    // An arc chart shares nothing structural with the others: no positional
    // scales, no axes, and a centred rather than rectangular layout.
    if mark == MarkType::Arc {
        return build_arc_chart(spec, shaper);
    }

    let rows = spec.data.rows()?;
    let (plot_w, plot_h) = spec.plot_size()?;

    let x = resolve(spec.encoding.x.as_ref(), "x", rows)?;
    let y = resolve(spec.encoding.y.as_ref(), "y", rows)?;

    // A bar needs exactly one discrete positional channel; line and point need a
    // quantitative y and will take either kind of x.
    let orientation = match (mark, x.ftype.is_discrete(), y.ftype.is_discrete()) {
        (MarkType::Bar, true, false) => Some(Orientation::Vertical),
        (MarkType::Bar, false, true) => Some(Orientation::Horizontal),
        (MarkType::Bar, _, _) => {
            return Err(Error::UnsupportedEncoding(
                "a bar mark needs exactly one discrete and one quantitative positional channel"
                    .into(),
            ))
        }
        (_, _, true) => {
            return Err(Error::UnsupportedEncoding(
                "line and point marks need a quantitative y channel".into(),
            ))
        }
        _ => None,
    };

    let x_plan = plan(&x, mark, Axis::X, plot_w);
    let y_plan = plan(&y, mark, Axis::Y, plot_h);

    // Colour, if encoded. Only discrete colour fields are in scope for v0.
    let (color_column, color_domain, color_title) = match spec.encoding.color.as_ref() {
        Some(channel) => {
            let resolved = resolve(Some(channel), "color", rows)?;
            let Values::Discrete(column) = resolved.values else {
                return Err(Error::UnsupportedEncoding(
                    "only discrete colour fields are supported".into(),
                ));
            };
            let domain = data::distinct(&column);
            (Some(column), domain, resolved.title)
        }
        None => (None, Vec::new(), None),
    };

    let legend_style = LegendStyle::default();
    let legend_model = Legend {
        title: color_title,
        entries: color_domain
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), palette::categorical(i)))
            .collect(),
        mark,
    };

    // Phase 1: measure, to find how much room the axes and legend need.
    let style = AxisStyle::default();
    let label_line_box =
        shaper.ascent(style.label_font_size) + shaper.descent(style.label_font_size);

    let insets = Insets {
        left: axis::left_inset(y_plan.labels(), y.title.as_deref(), shaper, &style),
        bottom: axis::bottom_inset(
            x_plan.labels(),
            x.title.as_deref(),
            x_plan.is_discrete(),
            shaper,
            &style,
        ),
        // A quantitative bottom axis ends its last tick on the plot's right edge,
        // so half of that centred label hangs outside.
        right: {
            let overhang = if x_plan.is_discrete() {
                0.0
            } else {
                x_plan
                    .labels()
                    .last()
                    .map(|l| shaper.measure_width(l, style.label_font_size) / 2.0)
                    .unwrap_or(0.0)
            };
            overhang.max(legend::width(&legend_model, shaper, &legend_style))
        },
        // Likewise the topmost tick label on a quantitative left axis.
        top: if y_plan.is_discrete() {
            0.0
        } else {
            label_line_box / 2.0
        },
    };

    // Phase 2: place everything now that the plot rectangle is known.
    let padding = spec.padding.unwrap_or(layout::DEFAULT_PADDING);
    let lay = layout::compute(plot_w, plot_h, padding, insets);
    let plot = lay.plot;
    let mut scene =
        Scene::new(lay.canvas_width, lay.canvas_height, background(spec)).with_plot(PlotRect {
            x: plot.x,
            y: plot.y,
            w: plot.w,
            h: plot.h,
        });

    let x_scale = place(&x_plan, mark, (plot.x, plot.right()));
    let y_scale = place(&y_plan, mark, (plot.bottom(), plot.y));

    emit_axis(
        &mut scene,
        &plot,
        AxisKind::Left,
        &y_plan,
        &y_scale,
        y.title.clone(),
        &style,
        shaper,
    );
    emit_axis(
        &mut scene,
        &plot,
        AxisKind::Bottom,
        &x_plan,
        &x_scale,
        x.title.clone(),
        &style,
        shaper,
    );

    let colors = row_colors(x.values.len(), color_column.as_ref(), &color_domain, spec);

    match mark {
        MarkType::Bar => {
            let orientation = orientation.expect("bar orientation validated above");
            let (categories, values, band, linear) = match orientation {
                Orientation::Vertical => (&x.values, &y.values, &x_scale, &y_scale),
                Orientation::Horizontal => (&y.values, &x.values, &y_scale, &x_scale),
            };
            let (Values::Discrete(categories), Values::Numeric(values)) = (categories, values)
            else {
                return Err(Error::UnsupportedEncoding("mismatched bar channels".into()));
            };
            let (Some(band), Some(linear)) = (band.band(), linear.linear()) else {
                return Err(Error::UnsupportedEncoding("mismatched bar scales".into()));
            };
            marks::bars(
                &mut scene,
                orientation,
                band,
                linear,
                categories,
                values,
                &colors,
            );
        }
        MarkType::Line => {
            let points = positions(&x_scale, &x, &y_scale, &y);
            marks::lines(&mut scene, &points, &colors);
        }
        MarkType::Point => {
            let points = positions(&x_scale, &x, &y_scale, &y);
            marks::points(&mut scene, &points, &colors);
        }
        // Handled by its own builder before this point; reachable only if that
        // early return is ever removed, so report rather than draw nothing.
        MarkType::Arc => {
            return Err(Error::UnsupportedEncoding(
                "arc marks are built by build_arc_chart".into(),
            ))
        }
    }

    legend::emit(&mut scene, &plot, &legend_model, &legend_style, shaper);

    Ok(scene)
}

/// Builds a pie or donut.
///
/// Deliberately a separate path. It has no x/y scales, no axes and no insets for
/// them; the legend is the only chrome, and the geometry is polar. Threading that
/// through the rectangular pipeline would complicate every step of it for one
/// mark.
fn build_arc_chart(spec: &Spec, shaper: &dyn TextShaper) -> Result<Scene> {
    let rows = spec.data.rows()?;
    let (plot_w, plot_h) = spec.plot_size()?;

    let theta = spec
        .encoding
        .theta
        .as_ref()
        .ok_or(Error::MissingEncoding("theta"))?;
    let theta_field = theta.field_name("theta")?;
    let values = data::numeric_column(rows, theta_field)?;

    // Colour, if any. It also decides the stacking order, so it is resolved
    // before the transform rather than after.
    let (color_column, color_domain, color_title) = match spec.encoding.color.as_ref() {
        Some(channel) => {
            let resolved = resolve(Some(channel), "color", rows)?;
            let Values::Discrete(column) = resolved.values else {
                return Err(Error::UnsupportedEncoding(
                    "only discrete colour fields are supported".into(),
                ));
            };
            let domain = data::distinct(&column);
            (Some(column), domain, resolved.title)
        }
        None => (None, Vec::new(), None),
    };

    // Vega stacks in the order of the colour scale's *domain*, which is sorted —
    // not in row order. Without a colour encoding there is no domain to follow,
    // so rows stack as they come.
    let order: Vec<usize> = match &color_column {
        Some(column) => {
            let mut order: Vec<usize> = (0..rows.len()).collect();
            order.sort_by_key(|&row| {
                column[row]
                    .as_deref()
                    .and_then(|v| color_domain.iter().position(|d| d == v))
                    .unwrap_or(usize::MAX)
            });
            order
        }
        None => (0..rows.len()).collect(),
    };

    let stacked = transform::stack(&values, &order);

    let legend_style = LegendStyle::default();
    let legend_model = Legend {
        title: color_title,
        entries: color_domain
            .iter()
            .enumerate()
            .map(|(i, name)| (name.clone(), palette::categorical(i)))
            .collect(),
        mark: MarkType::Arc,
    };

    let insets = Insets {
        left: 0.0,
        top: 0.0,
        bottom: 0.0,
        right: legend::width(&legend_model, shaper, &legend_style),
    };

    let padding = spec.padding.unwrap_or(layout::DEFAULT_PADDING);
    let lay = layout::compute(plot_w, plot_h, padding, insets);
    let plot = lay.plot;
    let mut scene =
        Scene::new(lay.canvas_width, lay.canvas_height, background(spec)).with_plot(PlotRect {
            x: plot.x,
            y: plot.y,
            w: plot.w,
            h: plot.h,
        });

    let props = spec.mark.props();
    // Vega fits the pie to the smaller dimension and centres it in the plot.
    let outer_radius = props
        .outer_radius
        .unwrap_or_else(|| plot.w.min(plot.h) / 2.0);
    let inner_radius = props.inner_radius.unwrap_or(0.0);
    let centre = (plot.x + plot.w / 2.0, plot.y + plot.h / 2.0);

    let colors = row_colors(values.len(), color_column.as_ref(), &color_domain, spec);

    marks::arcs(
        &mut scene,
        centre,
        inner_radius,
        outer_radius,
        &stacked,
        &colors,
    );

    legend::emit(&mut scene, &plot, &legend_model, &legend_style, shaper);

    Ok(scene)
}

fn background(spec: &Spec) -> Color {
    spec.background
        .as_deref()
        .and_then(Color::from_hex)
        .unwrap_or(Color::WHITE)
}

/// Extracts a channel's column and resolves its type.
fn resolve(
    channel: Option<&Channel>,
    name: &'static str,
    rows: &[serde_json::Value],
) -> Result<Positional> {
    let channel = channel.ok_or(Error::MissingEncoding(name))?;
    let field = channel.field_name(name)?;
    let ftype = channel.resolve_type(data::looks_numeric(rows, field))?;
    let values = if ftype.is_discrete() {
        Values::Discrete(data::discrete_column(rows, field)?)
    } else {
        Values::Numeric(data::numeric_column(rows, field)?)
    };
    Ok(Positional {
        ftype,
        // Vega-Lite titles an axis with the field name unless told otherwise.
        title: channel.title.clone().or_else(|| Some(field.to_string())),
        values,
        zero: channel.scale.as_ref().and_then(|s| s.zero),
    })
}

/// Domain and ticks for one channel.
fn plan(channel: &Positional, mark: MarkType, axis: Axis, extent_px: f32) -> Plan {
    match &channel.values {
        Values::Discrete(column) => Plan::Discrete {
            domain: data::distinct(column),
        },
        Values::Numeric(column) => {
            // With no data at all the domain collapses to a point, which is what
            // Vega does too — a single "0" tick, not an invented 0..1 range.
            let (mut lo, mut hi) = data::extent(column).unwrap_or((0.0, 0.0));
            if channel.zero.unwrap_or_else(|| zero_default(mark, axis)) {
                lo = lo.min(0.0);
                hi = hi.max(0.0);
            }
            // `nice()` rounds against a fixed count of 10, not the axis tick
            // count — see `scale::NICE_COUNT`.
            let (lo, hi) = LinearScale::new((lo, hi), (0.0, 1.0))
                .nice(scale::NICE_COUNT)
                .domain();
            let ticks = scale::ticks(lo, hi, default_tick_count(extent_px));
            let labels = axis::format_ticks(&ticks);
            Plan::Quantitative {
                lo,
                hi,
                ticks,
                labels,
            }
        }
    }
}

/// Builds the scale for a planned channel over a pixel range.
fn place(plan: &Plan, mark: MarkType, range: (f32, f32)) -> Placed {
    match plan {
        Plan::Quantitative { lo, hi, .. } => Placed::Linear(LinearScale::new((*lo, *hi), range)),
        Plan::Discrete { domain } => {
            // A band scale gives bars their width; line and point marks sit on a
            // point scale, which is a band scale with zero bandwidth.
            let ordered = (range.0.min(range.1), range.0.max(range.1));
            Placed::Band(match mark {
                // Arc never reaches here — it has no positional scale — but a
                // band is the harmless default if that ever changes.
                MarkType::Bar | MarkType::Arc => BandScale::new(domain.clone(), ordered),
                MarkType::Line | MarkType::Point => BandScale::point(domain.clone(), ordered),
            })
        }
    }
}

/// Per-row colours, from the colour encoding if present.
fn row_colors(
    len: usize,
    column: Option<&DiscreteColumn>,
    domain: &[String],
    spec: &Spec,
) -> Vec<Color> {
    let default = marks::mark_color(spec);
    match column {
        None => vec![default; len],
        Some(column) => column
            .iter()
            .map(|cell| {
                cell.as_deref()
                    .and_then(|v| domain.iter().position(|d| d == v))
                    .map(palette::categorical)
                    .unwrap_or(default)
            })
            .collect(),
    }
}

/// Row positions for line and point marks; `None` where a row is unplottable.
fn positions(
    x_scale: &Placed,
    x: &Positional,
    y_scale: &Placed,
    y: &Positional,
) -> Vec<Option<(f32, f32)>> {
    (0..x.values.len())
        .map(|i| Some((x_scale.at(&x.values, i)?, y_scale.at(&y.values, i)?)))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn emit_axis(
    scene: &mut Scene,
    plot: &Rect,
    kind: AxisKind,
    plan: &Plan,
    placed: &Placed,
    title: Option<String>,
    style: &AxisStyle,
    shaper: &dyn TextShaper,
) {
    // Vega rounds axis tick positions to whole pixels so gridlines and tick marks
    // stay crisp, while leaving mark geometry unrounded. The rounding happens in
    // the plot's own frame — the frame Vega's axis group works in. Rounding the
    // absolute coordinate instead just reintroduces the fractional plot origin as
    // a uniform offset.
    let origin = match kind {
        AxisKind::Left => plot.y,
        AxisKind::Bottom => plot.x,
    };
    let snap = |v: f32| origin + (v - origin).round();

    let (labels, raw): (Vec<String>, Vec<f32>) = match (plan, placed) {
        (Plan::Quantitative { ticks, labels, .. }, Placed::Linear(s)) => {
            (labels.clone(), ticks.iter().map(|t| s.scale(*t)).collect())
        }
        (Plan::Discrete { domain }, Placed::Band(s)) => {
            let mut labels = Vec::new();
            let mut raw = Vec::new();
            for name in domain {
                if let Some(centre) = s.center(name) {
                    labels.push(name.clone());
                    raw.push(centre);
                }
            }
            (labels, raw)
        }
        _ => (Vec::new(), Vec::new()),
    };

    let resolved = ResolvedAxis {
        // Vega-Lite rotates the labels of a discrete **x** axis by 270 degrees,
        // and nothing else — a discrete y axis stays horizontal.
        label_angle: match (kind, plan.is_discrete()) {
            (AxisKind::Bottom, true) => 270.0,
            _ => 0.0,
        },
        labels,
        positions: raw.iter().map(|v| snap(*v)).collect(),
        // Labels sit on the true tick value, not the snapped one — Vega does the
        // same, and snapping a glyph run just shifts it off centre.
        label_positions: raw,
        title,
        // Vega-Lite draws gridlines for continuous axes only.
        grid: !plan.is_discrete(),
    };

    axis::emit(scene, plot, kind, &resolved, style, shaper);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::SceneItem;

    fn parse(json: &str) -> Spec {
        Spec::parse(json).unwrap()
    }

    fn bar_spec_with_mark(mark: &str) -> Spec {
        parse(&format!(
            r#"{{
                "width": 300, "height": 200,
                "data": {{"values": [
                    {{"a": "A", "b": 28}},
                    {{"a": "B", "b": 55}},
                    {{"a": "C", "b": 43}}
                ]}},
                "mark": {mark},
                "encoding": {{
                    "x": {{"field": "a", "type": "nominal"}},
                    "y": {{"field": "b", "type": "quantitative"}}
                }}
            }}"#
        ))
    }

    fn bar_spec() -> Spec {
        bar_spec_with_mark(r#""bar""#)
    }

    fn rects(scene: &Scene) -> Vec<(f32, f32, f32, f32)> {
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Rect { x, y, w, h, .. } => Some((*x, *y, *w, *h)),
                _ => None,
            })
            .collect()
    }

    fn circles(scene: &Scene) -> Vec<(f32, f32, f32)> {
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Circle { cx, cy, r, .. } => Some((*cx, *cy, *r)),
                _ => None,
            })
            .collect()
    }

    /// Data lines only.
    ///
    /// Stroke width separates them from axis rules and gridlines (a two-point
    /// series is a perfectly ordinary line, so vertex count will not do it), and
    /// the plot rectangle separates them from the legend's line symbols, which
    /// share the mark's stroke width by design.
    fn polylines(scene: &Scene) -> Vec<Vec<(f32, f32)>> {
        let right = scene.plot.x + scene.plot.w;
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Line { points, stroke }
                    if stroke.width == marks::DEFAULT_STROKE_WIDTH
                        && points.iter().all(|p| p.0 <= right + 1e-3) =>
                {
                    Some(points.clone())
                }
                _ => None,
            })
            .collect()
    }

    fn texts(scene: &Scene) -> Vec<String> {
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Text { content, .. } => Some(content.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn emits_one_rect_per_datum() {
        assert_eq!(rects(&build(&bar_spec()).unwrap()).len(), 3);
    }

    #[test]
    fn bar_heights_are_proportional_to_their_values() {
        let r = rects(&build(&bar_spec()).unwrap());
        let ratio = r[1].3 / r[0].3;
        assert!((ratio - 55.0 / 28.0).abs() < 1e-3, "got ratio {ratio}");
    }

    #[test]
    fn the_plot_keeps_its_requested_size_and_the_canvas_grows() {
        let scene = build(&bar_spec()).unwrap();
        assert!((scene.plot.w - 300.0).abs() < 1e-3);
        assert!((scene.plot.h - 200.0).abs() < 1e-3);
        assert!(scene.width > 310.0 && scene.height > 210.0);
    }

    #[test]
    fn category_labels_and_tick_labels_are_both_drawn() {
        let labels = texts(&build(&bar_spec()).unwrap());
        for expected in ["A", "B", "C", "0"] {
            assert!(
                labels.contains(&expected.to_string()),
                "missing {expected} in {labels:?}"
            );
        }
    }

    #[test]
    fn axis_titles_default_to_the_field_name() {
        let labels = texts(&build(&bar_spec()).unwrap());
        assert!(labels.contains(&"a".to_string()));
        assert!(labels.contains(&"b".to_string()));
    }

    #[test]
    fn an_explicit_channel_title_wins() {
        let spec = parse(
            r#"{
                "data": {"values": [{"a":"A","b":1}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"a","type":"nominal","title":"Category"},
                    "y": {"field":"b","type":"quantitative","title":"Amount"}
                }
            }"#,
        );
        let labels = texts(&build(&spec).unwrap());
        assert!(labels.contains(&"Category".to_string()));
        assert!(labels.contains(&"Amount".to_string()));
    }

    #[test]
    fn longer_tick_labels_widen_the_canvas_but_not_the_plot() {
        let spec = |v: &str| {
            parse(&format!(
                r#"{{"width":300,"height":200,
                    "data":{{"values":[{{"a":"A","b":{v}}}]}},
                    "mark":"bar",
                    "encoding":{{"x":{{"field":"a","type":"nominal"}},
                                "y":{{"field":"b","type":"quantitative"}}}}}}"#
            ))
        };
        let a = build(&spec("5")).unwrap();
        let b = build(&spec("5000000")).unwrap();
        assert!(b.width > a.width, "wider labels should widen the canvas");
        assert!((a.plot.w - b.plot.w).abs() < 1e-3, "plot width changed");
    }

    #[test]
    fn negative_values_hang_below_the_zero_baseline() {
        let spec = parse(
            r#"{
                "width": 300, "height": 200,
                "data": {"values": [{"a":"A","b":10},{"a":"B","b":-10}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"a","type":"nominal"},
                    "y": {"field":"b","type":"quantitative"}
                }
            }"#,
        );
        let r = rects(&build(&spec).unwrap());
        assert!((r[0].3 - r[1].3).abs() < 1e-3);
        assert!((r[0].1 + r[0].3 - r[1].1).abs() < 1e-3);
    }

    #[test]
    fn horizontal_orientation_is_chosen_from_the_discrete_channel() {
        let spec = parse(
            r#"{
                "width": 300, "height": 200,
                "data": {"values": [{"a":"A","b":10},{"a":"B","b":20}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"b","type":"quantitative"},
                    "y": {"field":"a","type":"nominal"}
                }
            }"#,
        );
        let r = rects(&build(&spec).unwrap());
        assert_eq!(r.len(), 2);
        assert!((r[1].2 / r[0].2 - 2.0).abs() < 1e-3);
        assert!((r[0].3 - r[1].3).abs() < 1e-3);
    }

    #[test]
    fn rows_with_missing_cells_are_skipped_not_fatal() {
        let spec = parse(
            r#"{
                "data": {"values": [{"a":"A","b":10},{"a":null,"b":5},{"a":"C","b":null}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"a","type":"nominal"},
                    "y": {"field":"b","type":"quantitative"}
                }
            }"#,
        );
        assert_eq!(rects(&build(&spec).unwrap()).len(), 1);
    }

    #[test]
    fn empty_data_renders_axes_but_no_bars() {
        let spec = parse(
            r#"{
                "data": {"values": []},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"a","type":"nominal"},
                    "y": {"field":"b","type":"quantitative"}
                }
            }"#,
        );
        let scene = build(&spec).unwrap();
        assert!(rects(&scene).is_empty());
        assert!(scene.width > 0.0 && scene.height > 0.0);
    }

    #[test]
    fn all_equal_values_do_not_produce_a_degenerate_scale() {
        let spec = parse(
            r#"{
                "data": {"values": [{"a":"A","b":7},{"a":"B","b":7}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"a","type":"nominal"},
                    "y": {"field":"b","type":"quantitative"}
                }
            }"#,
        );
        let r = rects(&build(&spec).unwrap());
        assert_eq!(r.len(), 2);
        assert!(r[0].3 > 0.0);
        assert!((r[0].3 - r[1].3).abs() < 1e-3);
    }

    #[test]
    fn two_quantitative_channels_are_rejected_for_a_bar() {
        let spec = parse(
            r#"{
                "data": {"values": [{"a":1,"b":2}]},
                "mark": "bar",
                "encoding": {
                    "x": {"field":"a","type":"quantitative"},
                    "y": {"field":"b","type":"quantitative"}
                }
            }"#,
        );
        assert!(matches!(build(&spec), Err(Error::UnsupportedEncoding(_))));
    }

    #[test]
    fn missing_y_encoding_is_a_typed_error() {
        let spec = parse(
            r#"{
                "data": {"values": [{"a":"A"}]},
                "mark": "bar",
                "encoding": {"x": {"field":"a","type":"nominal"}}
            }"#,
        );
        assert!(matches!(build(&spec), Err(Error::MissingEncoding("y"))));
    }

    #[test]
    fn mark_color_overrides_the_default() {
        let scene = build(&bar_spec_with_mark(r##"{"type":"bar","color":"#ff0000"}"##)).unwrap();
        let fills: Vec<_> = scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Rect { fill, .. } => *fill,
                _ => None,
            })
            .collect();
        assert!(!fills.is_empty());
        assert!(fills.iter().all(|c| *c == Color::rgb(255, 0, 0)));
    }

    // --- line and point marks ---

    fn scatter_spec(mark: &str) -> Spec {
        parse(&format!(
            r#"{{
                "width": 300, "height": 200,
                "data": {{"values": [
                    {{"x": 3, "y": 28}},
                    {{"x": 7, "y": 55}},
                    {{"x": 12, "y": 43}}
                ]}},
                "mark": "{mark}",
                "encoding": {{
                    "x": {{"field": "x", "type": "quantitative"}},
                    "y": {{"field": "y", "type": "quantitative"}}
                }}
            }}"#
        ))
    }

    #[test]
    fn a_point_mark_emits_one_circle_per_datum() {
        let scene = build(&scatter_spec("point")).unwrap();
        assert_eq!(circles(&scene).len(), 3);
    }

    #[test]
    fn points_are_stroked_not_filled_like_vega() {
        let scene = build(&scatter_spec("point")).unwrap();
        let item = scene
            .items
            .iter()
            .find(|i| matches!(i, SceneItem::Circle { .. }))
            .unwrap();
        match item {
            SceneItem::Circle { fill, stroke, .. } => {
                assert!(fill.is_none(), "vega leaves point marks unfilled");
                assert!(stroke.is_some());
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn a_line_mark_emits_one_polyline_through_every_datum() {
        let scene = build(&scatter_spec("line")).unwrap();
        let lines = polylines(&scene);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 3);
    }

    #[test]
    fn line_vertices_ascend_in_x() {
        // Vega sorts a line's points by the x field; input order must not matter.
        let spec = parse(
            r#"{
                "width": 300, "height": 200,
                "data": {"values": [{"x":12,"y":43},{"x":3,"y":28},{"x":7,"y":55}]},
                "mark": "line",
                "encoding": {
                    "x": {"field":"x","type":"quantitative"},
                    "y": {"field":"y","type":"quantitative"}
                }
            }"#,
        );
        let lines = polylines(&build(&spec).unwrap());
        let xs: Vec<f32> = lines[0].iter().map(|p| p.0).collect();
        for pair in xs.windows(2) {
            assert!(pair[1] > pair[0], "unsorted line vertices: {xs:?}");
        }
    }

    #[test]
    fn a_line_over_a_discrete_x_uses_a_point_scale() {
        // A point scale has no bandwidth, so the first and last vertices sit
        // inside the plot rather than on its edges.
        let spec = parse(
            r#"{
                "width": 300, "height": 200,
                "data": {"values": [{"a":"A","b":1},{"a":"B","b":2},{"a":"C","b":3}]},
                "mark": "line",
                "encoding": {
                    "x": {"field":"a","type":"nominal"},
                    "y": {"field":"b","type":"quantitative"}
                }
            }"#,
        );
        let scene = build(&spec).unwrap();
        let lines = polylines(&scene);
        assert_eq!(lines[0].len(), 3);
        let first = lines[0][0].0 - scene.plot.x;
        let last = lines[0][2].0 - scene.plot.x;
        assert!(first > 0.0, "first vertex on the plot edge");
        assert!(last < scene.plot.w, "last vertex on the plot edge");
        assert!(
            (first - (scene.plot.w - last)).abs() < 1e-2,
            "point-scale padding should be symmetric"
        );
    }

    #[test]
    fn a_discrete_y_is_rejected_for_line_and_point() {
        for mark in ["line", "point"] {
            let spec = parse(&format!(
                r#"{{
                    "data": {{"values": [{{"a":"A","b":1}}]}},
                    "mark": "{mark}",
                    "encoding": {{
                        "x": {{"field":"b","type":"quantitative"}},
                        "y": {{"field":"a","type":"nominal"}}
                    }}
                }}"#
            ));
            assert!(
                matches!(build(&spec), Err(Error::UnsupportedEncoding(_))),
                "{mark} should reject a discrete y"
            );
        }
    }

    // --- colour encoding and legend ---

    fn colored_spec(mark: &str) -> Spec {
        parse(&format!(
            r#"{{
                "width": 300, "height": 200,
                "data": {{"values": [
                    {{"x": 1, "y": 10, "s": "alpha"}},
                    {{"x": 2, "y": 20, "s": "beta"}},
                    {{"x": 3, "y": 30, "s": "alpha"}},
                    {{"x": 4, "y": 25, "s": "beta"}}
                ]}},
                "mark": "{mark}",
                "encoding": {{
                    "x": {{"field": "x", "type": "quantitative"}},
                    "y": {{"field": "y", "type": "quantitative"}},
                    "color": {{"field": "s", "type": "nominal"}}
                }}
            }}"#
        ))
    }

    #[test]
    fn equal_categories_share_a_colour_and_distinct_ones_differ() {
        let scene = build(&colored_spec("point")).unwrap();
        let strokes: Vec<Color> = scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Circle { stroke, .. } => stroke.map(|s| s.color),
                _ => None,
            })
            .collect();
        // Three data points plus two legend symbols.
        assert!(strokes.len() >= 3);
        assert_eq!(strokes[0], strokes[2], "same category, same colour");
        assert_ne!(strokes[0], strokes[1], "different category, different colour");
    }

    #[test]
    fn a_colour_encoding_adds_a_legend() {
        let labels = texts(&build(&colored_spec("point")).unwrap());
        assert!(labels.contains(&"alpha".to_string()));
        assert!(labels.contains(&"beta".to_string()));
        assert!(labels.contains(&"s".to_string()), "legend title");
    }

    #[test]
    fn the_legend_widens_the_canvas_without_shrinking_the_plot() {
        let plain = build(&scatter_spec("point")).unwrap();
        let colored = build(&colored_spec("point")).unwrap();
        assert!(colored.width > plain.width);
        assert!((colored.plot.w - plain.plot.w).abs() < 1e-3);
    }

    #[test]
    fn a_colour_encoding_splits_a_line_into_one_series_each() {
        let lines = polylines(&build(&colored_spec("line")).unwrap());
        assert_eq!(lines.len(), 2, "one polyline per series");
    }

    // --- arc marks ---

    fn arcs(scene: &Scene) -> Vec<(f32, f32, f32, f32)> {
        scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Arc {
                    start_angle,
                    end_angle,
                    inner_radius,
                    outer_radius,
                    ..
                } => Some((*start_angle, *end_angle, *inner_radius, *outer_radius)),
                _ => None,
            })
            .collect()
    }

    fn pie_spec(mark: &str) -> Spec {
        parse(&format!(
            r#"{{
                "width": 200, "height": 200,
                "data": {{"values": [
                    {{"c": "Zeta", "v": 10}},
                    {{"c": "Alpha", "v": 20}},
                    {{"c": "Mid", "v": 5}}
                ]}},
                "mark": {mark},
                "encoding": {{
                    "theta": {{"field": "v", "type": "quantitative"}},
                    "color": {{"field": "c", "type": "nominal"}}
                }}
            }}"#
        ))
    }

    #[test]
    fn slices_cover_a_full_turn_without_gaps() {
        let a = arcs(&build(&pie_spec(r#""arc""#)).unwrap());
        assert_eq!(a.len(), 3);
        let total: f32 = a.iter().map(|s| (s.1 - s.0).abs()).sum();
        assert!(
            (total - std::f32::consts::TAU).abs() < 1e-3,
            "slices sum to {total}, expected a full turn"
        );
    }

    #[test]
    fn slice_angles_are_proportional_to_their_values() {
        // Zeta=10, Alpha=20, Mid=5 of 35.
        let a = arcs(&build(&pie_spec(r#""arc""#)).unwrap());
        let span = |i: usize| (a[i].1 - a[i].0).abs();
        assert!((span(1) / span(0) - 2.0).abs() < 1e-3, "Alpha should be twice Zeta");
        assert!((span(0) / span(2) - 2.0).abs() < 1e-3, "Zeta should be twice Mid");
    }

    #[test]
    fn slices_stack_in_colour_domain_order_not_row_order() {
        // Rows are Zeta, Alpha, Mid; the sorted domain is Alpha, Mid, Zeta. So
        // Alpha owns the first wedge starting at zero, even though it is row 1.
        let a = arcs(&build(&pie_spec(r#""arc""#)).unwrap());
        let start = |i: usize| a[i].0.min(a[i].1);
        assert!(start(1) < 1e-6, "Alpha should start the turn, got {}", start(1));
        assert!(start(2) > start(1), "Mid should follow Alpha");
        assert!(start(0) > start(2), "Zeta should come last");
    }

    #[test]
    fn a_pie_has_no_hole_and_a_donut_does() {
        let pie = arcs(&build(&pie_spec(r#""arc""#)).unwrap());
        assert!(pie.iter().all(|s| s.2 == 0.0));

        let donut = arcs(&build(&pie_spec(r#"{"type":"arc","innerRadius":40}"#)).unwrap());
        assert!(donut.iter().all(|s| s.2 == 40.0));
        // The hole must not swallow the slice.
        assert!(donut.iter().all(|s| s.3 > s.2));
    }

    #[test]
    fn the_pie_fits_the_smaller_plot_dimension() {
        let spec = parse(
            r#"{
                "width": 400, "height": 200,
                "data": {"values": [{"c":"A","v":1}]},
                "mark": "arc",
                "encoding": {
                    "theta": {"field":"v","type":"quantitative"},
                    "color": {"field":"c","type":"nominal"}
                }
            }"#,
        );
        let a = arcs(&build(&spec).unwrap());
        assert!((a[0].3 - 100.0).abs() < 1e-3, "radius should be min(400,200)/2");
    }

    #[test]
    fn an_arc_without_a_theta_channel_is_a_typed_error() {
        let spec = parse(
            r#"{
                "data": {"values": [{"c":"A","v":1}]},
                "mark": "arc",
                "encoding": {"color": {"field":"c","type":"nominal"}}
            }"#,
        );
        assert!(matches!(build(&spec), Err(Error::MissingEncoding("theta"))));
    }

    #[test]
    fn an_all_zero_pie_draws_nothing_rather_than_dividing_by_zero() {
        let spec = parse(
            r#"{
                "data": {"values": [{"c":"A","v":0},{"c":"B","v":0}]},
                "mark": "arc",
                "encoding": {
                    "theta": {"field":"v","type":"quantitative"},
                    "color": {"field":"c","type":"nominal"}
                }
            }"#,
        );
        let scene = build(&spec).unwrap();
        assert!(arcs(&scene).is_empty());
        assert!(scene.width > 0.0);
    }

    #[test]
    fn a_single_slice_covers_the_whole_circle() {
        let spec = parse(
            r#"{
                "data": {"values": [{"c":"A","v":7}]},
                "mark": "arc",
                "encoding": {
                    "theta": {"field":"v","type":"quantitative"},
                    "color": {"field":"c","type":"nominal"}
                }
            }"#,
        );
        let a = arcs(&build(&spec).unwrap());
        assert_eq!(a.len(), 1);
        assert!(((a[0].1 - a[0].0).abs() - std::f32::consts::TAU).abs() < 1e-3);
    }

    #[test]
    fn a_quantitative_colour_field_is_rejected_rather_than_mis_drawn() {
        let spec = parse(
            r#"{
                "data": {"values": [{"x":1,"y":2,"c":3}]},
                "mark": "point",
                "encoding": {
                    "x": {"field":"x","type":"quantitative"},
                    "y": {"field":"y","type":"quantitative"},
                    "color": {"field":"c","type":"quantitative"}
                }
            }"#,
        );
        assert!(matches!(build(&spec), Err(Error::UnsupportedEncoding(_))));
    }
}
