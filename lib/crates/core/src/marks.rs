//! Mark emitters: scales + data columns -> [`SceneItem`]s.
//!
//! Geometry only. These functions take already-positioned scales and append to a
//! scene; deciding the scales and the plot rectangle is [`crate::chart`]'s job.
//! Keeping the split means a test can assert a bar's exact x and width without
//! going anywhere near a rasterizer.

use crate::chart::Orientation;
use crate::data::{DiscreteColumn, NumericColumn};
use crate::ir::{Color, Scene, SceneItem, Stroke};
use crate::scale::{BandScale, LinearScale};
use crate::spec::Spec;
use crate::transform::Stacked;

/// Vega-Lite's default mark colour, and the first entry of `tableau10`.
pub const DEFAULT_MARK_COLOR: Color = Color::rgb(0x4c, 0x78, 0xa8);

/// Vega's default symbol size, as an **area** in px².
pub const DEFAULT_SYMBOL_SIZE: f32 = 30.0;

/// Vega's default stroke width for line and point marks.
pub const DEFAULT_STROKE_WIDTH: f32 = 2.0;

pub fn mark_color(spec: &Spec) -> Color {
    let props = spec.mark.props();
    props
        .color
        .as_deref()
        .or(props.fill.as_deref())
        .and_then(Color::from_hex)
        .unwrap_or(DEFAULT_MARK_COLOR)
}

/// Radius for a symbol of the given area. Vega sizes symbols by area, so a
/// naive `size / 2` would scale points quadratically wrong.
pub fn symbol_radius(area: f32) -> f32 {
    (area.max(0.0) / std::f32::consts::PI).sqrt()
}

/// Appends one rect per datum, spanning from the value scale's zero to the datum.
///
/// Rows whose category or value is missing are skipped rather than failing the
/// chart — a null in one row should drop that datum, not the whole render.
#[allow(clippy::too_many_arguments)]
pub fn bars(
    scene: &mut Scene,
    orientation: Orientation,
    cat_scale: &BandScale,
    val_scale: &LinearScale,
    categories: &DiscreteColumn,
    values: &NumericColumn,
    colors: &[Color],
) {
    let baseline = val_scale.scale(0.0);
    for (i, (category, value)) in categories.iter().zip(values.iter()).enumerate() {
        let (Some(category), Some(value)) = (category.as_deref(), value) else {
            continue;
        };
        let Some(start) = cat_scale.scale(category) else {
            continue;
        };
        let edge = val_scale.scale(*value);
        let (near, extent) = (edge.min(baseline), (edge - baseline).abs());
        let fill = colors.get(i).copied().unwrap_or(DEFAULT_MARK_COLOR);

        scene.push(match orientation {
            Orientation::Vertical => SceneItem::Rect {
                x: start,
                y: near,
                w: cat_scale.bandwidth(),
                h: extent,
                fill: Some(fill),
                stroke: None,
            },
            Orientation::Horizontal => SceneItem::Rect {
                x: near,
                y: start,
                w: extent,
                h: cat_scale.bandwidth(),
                fill: Some(fill),
                stroke: None,
            },
        });
    }
}

/// Appends one stroked circle per datum.
///
/// Vega draws point marks stroked and unfilled, which is why `fill` is `None`
/// here rather than the series colour.
pub fn points(scene: &mut Scene, positions: &[Option<(f32, f32)>], colors: &[Color]) {
    let r = symbol_radius(DEFAULT_SYMBOL_SIZE);
    for (i, position) in positions.iter().enumerate() {
        let Some((x, y)) = position else { continue };
        let color = colors.get(i).copied().unwrap_or(DEFAULT_MARK_COLOR);
        scene.push(SceneItem::Circle {
            cx: *x,
            cy: *y,
            r,
            fill: None,
            stroke: Some(Stroke::new(color, DEFAULT_STROKE_WIDTH)),
        });
    }
}

/// Appends one polyline per colour series.
///
/// Two behaviours worth naming, both matching Vega:
/// * vertices are sorted by x, so input row order does not change the shape;
/// * a colour encoding splits the data into one line per series rather than
///   producing a single line that zig-zags between them.
pub fn lines(scene: &mut Scene, positions: &[Option<(f32, f32)>], colors: &[Color]) {
    // Group by colour, preserving the order each series first appears so the
    // draw order is stable.
    let mut series: Vec<(Color, Vec<(f32, f32)>)> = Vec::new();
    for (i, position) in positions.iter().enumerate() {
        let Some(point) = position else { continue };
        let color = colors.get(i).copied().unwrap_or(DEFAULT_MARK_COLOR);
        match series.iter_mut().find(|(c, _)| *c == color) {
            Some((_, points)) => points.push(*point),
            None => series.push((color, vec![*point])),
        }
    }

    for (color, mut points) in series {
        if points.len() < 2 {
            continue;
        }
        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        scene.push(SceneItem::Line {
            points,
            stroke: Stroke::new(color, DEFAULT_STROKE_WIDTH),
        });
    }
}

/// Appends one slice per stacked row.
///
/// The angular scale is the whole of the polar machinery: a linear map from the
/// stacked cumulative total onto a full turn. Angles follow Vega — 0 points up
/// and increases clockwise.
pub fn arcs(
    scene: &mut Scene,
    centre: (f32, f32),
    inner_radius: f32,
    outer_radius: f32,
    stacked: &Stacked,
    colors: &[Color],
) {
    if stacked.total <= 0.0 || !stacked.total.is_finite() {
        return;
    }
    let theta = LinearScale::new((0.0, stacked.total), (0.0, std::f32::consts::TAU));

    for (i, band) in stacked.bands.iter().enumerate() {
        let Some((start, end)) = band else { continue };
        scene.push(SceneItem::Arc {
            cx: centre.0,
            cy: centre.1,
            inner_radius,
            outer_radius,
            start_angle: theta.scale(*start),
            end_angle: theta.scale(*end),
            fill: Some(colors.get(i).copied().unwrap_or(DEFAULT_MARK_COLOR)),
            stroke: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene::new(100.0, 100.0, Color::WHITE)
    }

    #[test]
    fn symbol_radius_treats_size_as_an_area() {
        // Doubling the area scales the radius by sqrt(2), not by 2.
        let a = symbol_radius(30.0);
        let b = symbol_radius(60.0);
        assert!((b / a - 2f32.sqrt()).abs() < 1e-4);
        assert_eq!(symbol_radius(0.0), 0.0);
        assert_eq!(symbol_radius(-5.0), 0.0, "negative area must not produce NaN");
    }

    #[test]
    fn a_value_exactly_at_the_baseline_produces_a_zero_height_bar() {
        let mut s = scene();
        let cat = BandScale::new(vec!["A".into()], (0.0, 100.0));
        let val = LinearScale::new((0.0, 10.0), (100.0, 0.0));
        bars(
            &mut s,
            Orientation::Vertical,
            &cat,
            &val,
            &vec![Some("A".into())],
            &vec![Some(0.0)],
            &[DEFAULT_MARK_COLOR],
        );
        match s.items[0] {
            SceneItem::Rect { h, .. } => assert!(h.abs() < 1e-4),
            _ => panic!("expected a rect"),
        }
    }

    #[test]
    fn a_category_outside_the_scale_domain_is_skipped() {
        let mut s = scene();
        let cat = BandScale::new(vec!["A".into()], (0.0, 100.0));
        let val = LinearScale::new((0.0, 10.0), (100.0, 0.0));
        bars(
            &mut s,
            Orientation::Vertical,
            &cat,
            &val,
            &vec![Some("A".into()), Some("ZZ".into())],
            &vec![Some(5.0), Some(5.0)],
            &[DEFAULT_MARK_COLOR; 2],
        );
        assert_eq!(s.items.len(), 1);
    }

    #[test]
    fn unplottable_rows_are_dropped_from_lines_and_points() {
        let positions = vec![Some((0.0, 0.0)), None, Some((10.0, 10.0))];
        let colors = [DEFAULT_MARK_COLOR; 3];

        let mut s = scene();
        points(&mut s, &positions, &colors);
        assert_eq!(s.items.len(), 2);

        let mut s = scene();
        lines(&mut s, &positions, &colors);
        match &s.items[0] {
            SceneItem::Line { points, .. } => assert_eq!(points.len(), 2),
            _ => panic!("expected a line"),
        }
    }

    #[test]
    fn a_single_point_series_draws_no_line() {
        // One vertex is not a line; drawing it would be a stray dot.
        let mut s = scene();
        lines(&mut s, &[Some((1.0, 1.0))], &[DEFAULT_MARK_COLOR]);
        assert!(s.items.is_empty());
    }

    #[test]
    fn each_colour_becomes_its_own_line() {
        let mut s = scene();
        let positions = vec![
            Some((0.0, 0.0)),
            Some((1.0, 1.0)),
            Some((2.0, 2.0)),
            Some((3.0, 3.0)),
        ];
        let a = Color::rgb(1, 0, 0);
        let b = Color::rgb(0, 1, 0);
        lines(&mut s, &positions, &[a, b, a, b]);
        assert_eq!(s.items.len(), 2);
        for item in &s.items {
            match item {
                SceneItem::Line { points, .. } => assert_eq!(points.len(), 2),
                _ => panic!("expected lines"),
            }
        }
    }
}
