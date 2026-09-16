//! Axis tick generation, label formatting, and geometry.
//!
//! Split into two phases on purpose. Tick *values and labels* depend only on the
//! domain, so they can be computed before layout; measuring them is what
//! determines how much room the axes need. Tick *positions* need the final plot
//! rectangle, so they are resolved after. That ordering is what keeps
//! `width: 300` meaning a 300px plot no matter how long the y labels are.

use crate::ir::{Anchor, Baseline, Color, Scene, SceneItem, Stroke};
use crate::layout::Rect;
use crate::text::TextShaper;

/// Vega's default axis styling.
#[derive(Debug, Clone, Copy)]
pub struct AxisStyle {
    pub label_font_size: f32,
    pub title_font_size: f32,
    pub tick_size: f32,
    pub label_padding: f32,
    pub title_padding: f32,
    pub domain_color: Color,
    pub tick_color: Color,
    pub grid_color: Color,
    pub label_color: Color,
    pub title_color: Color,
    pub line_width: f32,
}

impl Default for AxisStyle {
    fn default() -> Self {
        Self {
            label_font_size: 10.0,
            title_font_size: 11.0,
            tick_size: 5.0,
            label_padding: 2.0,
            title_padding: 4.0,
            domain_color: Color::rgb(0x88, 0x88, 0x88),
            tick_color: Color::rgb(0x88, 0x88, 0x88),
            grid_color: Color::rgb(0xdd, 0xdd, 0xdd),
            label_color: Color::BLACK,
            title_color: Color::BLACK,
            line_width: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AxisKind {
    /// Quantitative or discrete axis down the left edge of the plot.
    Left,
    /// Axis along the bottom edge of the plot.
    Bottom,
}

/// An axis with its ticks placed at final pixel coordinates.
#[derive(Debug, Clone)]
pub struct ResolvedAxis {
    pub labels: Vec<String>,
    /// Along-axis coordinate of each tick mark and gridline: y for
    /// [`AxisKind::Left`], x for [`AxisKind::Bottom`]. Snapped to whole pixels so
    /// the 1px strokes stay crisp.
    pub positions: Vec<f32>,
    /// Along-axis coordinate of each label, *unsnapped*.
    ///
    /// Labels sit on the true tick value rather than the rounded one — Vega does
    /// the same. Snapping a glyph run buys nothing (it is antialiased either way)
    /// and shifts text off the centre of its band by up to half a pixel.
    pub label_positions: Vec<f32>,
    pub title: Option<String>,
    pub grid: bool,
    /// Clockwise rotation applied to each label.
    ///
    /// Vega-Lite compiles `labelAngle: 270` for a **discrete x** axis by default,
    /// and 0 everywhere else — including a discrete *y* axis. Leaving these
    /// horizontal is a visible mismatch against the reference.
    pub label_angle: f32,
}

impl ResolvedAxis {
    /// Pairs each label with its unsnapped position, tolerating a short list.
    fn labelled(&self) -> impl Iterator<Item = (&String, &f32)> {
        self.labels.iter().zip(&self.label_positions)
    }
}

/// d3-format's default locale renders negatives with U+2212 MINUS SIGN rather
/// than an ASCII hyphen, and Vega inherits that. The hyphen is visibly shorter
/// and sits at the wrong height next to digits.
pub const MINUS_SIGN: char = '\u{2212}';

/// Formats tick values the way d3 does: one shared decimal precision across the
/// whole axis, derived from the tick step, so labels line up.
pub fn format_ticks(values: &[f64]) -> Vec<String> {
    if values.is_empty() {
        return Vec::new();
    }
    let step = if values.len() > 1 {
        (values[1] - values[0]).abs()
    } else {
        values[0].abs().max(1.0)
    };
    let decimals = if step <= 0.0 || !step.is_finite() {
        0
    } else {
        (-step.log10().floor()).clamp(0.0, 10.0) as usize
    };
    values
        .iter()
        .map(|v| {
            // Avoid "-0" for a negative zero produced by rounding.
            let v = if *v == 0.0 { 0.0 } else { *v };
            let formatted = format!("{v:.decimals$}");
            match formatted.strip_prefix('-') {
                Some(rest) => format!("{MINUS_SIGN}{rest}"),
                None => formatted,
            }
        })
        .collect()
}

/// Rounds an axis extent up to a whole pixel, as Vega does.
///
/// Vega reports integral axis extents — for a chart whose parts sum to 33.12 it
/// reserves 34. Leaving the fraction in place offsets the plot origin by up to a
/// pixel against the reference, which is small enough to be invisible on its own
/// and clearly visible in a difference view.
fn whole_pixels(extent: f32) -> f32 {
    extent.ceil()
}

/// Widest label in the set. Shared by the inset calculation and by `emit`, so
/// the space reserved for a left axis and the space it actually uses agree.
fn widest_label(labels: &[String], shaper: &dyn TextShaper, style: &AxisStyle) -> f32 {
    labels
        .iter()
        .map(|l| shaper.measure_width(l, style.label_font_size))
        .fold(0.0f32, f32::max)
}

/// Distance from the plot's left edge out to where a rotated y title sits.
fn left_title_offset(labels: &[String], shaper: &dyn TextShaper, style: &AxisStyle) -> f32 {
    style.tick_size
        + style.label_padding
        + widest_label(labels, shaper, style)
        + style.title_padding
}

/// Space a left axis needs, outside the plot rectangle.
pub fn left_inset(
    labels: &[String],
    title: Option<&str>,
    shaper: &dyn TextShaper,
    style: &AxisStyle,
) -> f32 {
    let widest = widest_label(labels, shaper, style);
    let mut inset = style.tick_size + style.label_padding + widest;
    if title.is_some() {
        inset += style.title_padding + style.title_font_size;
    }
    whole_pixels(inset)
}

/// Space a bottom axis needs, outside the plot rectangle.
///
/// `rotated` labels stand on end, so what they consume vertically is their
/// *width*, not a line box — the difference is the whole axis for long category
/// names.
pub fn bottom_inset(
    labels: &[String],
    title: Option<&str>,
    rotated: bool,
    shaper: &dyn TextShaper,
    style: &AxisStyle,
) -> f32 {
    if labels.is_empty() && title.is_none() {
        return 0.0;
    }
    let label_extent = if rotated {
        widest_label(labels, shaper, style)
    } else {
        shaper.ascent(style.label_font_size) + shaper.descent(style.label_font_size)
    };
    let mut inset = style.tick_size + style.label_padding + label_extent;
    if title.is_some() {
        inset += style.title_padding + style.title_font_size;
    }
    whole_pixels(inset)
}

/// Emits gridlines, the domain line, tick marks, labels, and the title.
///
/// Gridlines go in first so that marks drawn afterwards sit on top of them.
pub fn emit(
    scene: &mut Scene,
    plot: &Rect,
    kind: AxisKind,
    axis: &ResolvedAxis,
    style: &AxisStyle,
    shaper: &dyn TextShaper,
) {
    let line = Stroke::new(style.domain_color, style.line_width);
    let tick = Stroke::new(style.tick_color, style.line_width);
    let grid = Stroke::new(style.grid_color, style.line_width);

    match kind {
        AxisKind::Left => {
            if axis.grid {
                for y in &axis.positions {
                    scene.push(SceneItem::Line {
                        points: vec![(plot.x, *y), (plot.right(), *y)],
                        stroke: grid,
                    });
                }
            }
            scene.push(SceneItem::Line {
                points: vec![(plot.x, plot.y), (plot.x, plot.bottom())],
                stroke: line,
            });
            for y in &axis.positions {
                scene.push(SceneItem::Line {
                    points: vec![(plot.x - style.tick_size, *y), (plot.x, *y)],
                    stroke: tick,
                });
            }
            for (label, y) in axis.labelled() {
                scene.push(SceneItem::Text {
                    x: plot.x - style.tick_size - style.label_padding,
                    y: *y,
                    content: label.clone(),
                    size: style.label_font_size,
                    anchor: Anchor::End,
                    baseline: Baseline::Middle,
                    fill: style.label_color,
                    angle: 0.0,
                });
            }
            if let Some(title) = &axis.title {
                // Rotated -90 so it reads bottom-to-top, in the same space
                // `left_inset` reserved for it. Drawing it horizontally above the
                // plot instead would run off the top of the canvas.
                scene.push(SceneItem::Text {
                    x: plot.x - left_title_offset(&axis.labels, shaper, style),
                    y: plot.y + plot.h / 2.0,
                    content: title.clone(),
                    size: style.title_font_size,
                    anchor: Anchor::Middle,
                    baseline: Baseline::Alphabetic,
                    fill: style.title_color,
                    angle: -90.0,
                });
            }
        }

        AxisKind::Bottom => {
            if axis.grid {
                for x in &axis.positions {
                    scene.push(SceneItem::Line {
                        points: vec![(*x, plot.y), (*x, plot.bottom())],
                        stroke: grid,
                    });
                }
            }
            scene.push(SceneItem::Line {
                points: vec![(plot.x, plot.bottom()), (plot.right(), plot.bottom())],
                stroke: line,
            });
            let label_top = plot.bottom() + style.tick_size + style.label_padding;
            for x in &axis.positions {
                scene.push(SceneItem::Line {
                    points: vec![(*x, plot.bottom()), (*x, plot.bottom() + style.tick_size)],
                    stroke: tick,
                });
            }
            // A rotated label is right-aligned and vertically centred on the
            // tick, matching Vega's `labelAlign: right, labelBaseline: middle`.
            let (anchor, baseline) = if axis.label_angle == 0.0 {
                (Anchor::Middle, Baseline::Top)
            } else {
                (Anchor::End, Baseline::Middle)
            };
            for (label, x) in axis.labelled() {
                scene.push(SceneItem::Text {
                    x: *x,
                    y: label_top,
                    content: label.clone(),
                    size: style.label_font_size,
                    anchor,
                    baseline,
                    fill: style.label_color,
                    angle: axis.label_angle,
                });
            }
            if let Some(title) = &axis.title {
                let label_extent = if axis.label_angle == 0.0 {
                    shaper.ascent(style.label_font_size) + shaper.descent(style.label_font_size)
                } else {
                    widest_label(&axis.labels, shaper, style)
                };
                scene.push(SceneItem::Text {
                    x: plot.x + plot.w / 2.0,
                    y: label_top + label_extent + style.title_padding,
                    content: title.clone(),
                    size: style.title_font_size,
                    anchor: Anchor::Middle,
                    baseline: Baseline::Top,
                    fill: style.title_color,
                    angle: 0.0,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{default_shaper, TextShaper};

    #[test]
    fn integral_ticks_format_without_decimals() {
        assert_eq!(
            format_ticks(&[0.0, 20.0, 40.0]),
            vec!["0", "20", "40"]
        );
    }

    #[test]
    fn fractional_ticks_share_one_precision() {
        assert_eq!(
            format_ticks(&[0.0, 0.2, 0.4, 0.6]),
            vec!["0.0", "0.2", "0.4", "0.6"]
        );
        assert_eq!(
            format_ticks(&[0.0, 0.05, 0.10]),
            vec!["0.00", "0.05", "0.10"]
        );
    }

    #[test]
    fn negative_zero_does_not_leak_into_a_label() {
        assert_eq!(format_ticks(&[-0.0, 1.0])[0], "0");
    }

    #[test]
    fn negatives_use_a_real_minus_sign() {
        // Matching d3-format, and therefore Vega: U+2212, not an ASCII hyphen.
        assert_eq!(format_ticks(&[-20.0, -10.0, 0.0]), vec!["\u{2212}20", "\u{2212}10", "0"]);
    }

    #[test]
    fn empty_and_single_tick_lists_are_handled() {
        assert!(format_ticks(&[]).is_empty());
        assert_eq!(format_ticks(&[7.0]), vec!["7"]);
    }

    #[test]
    fn left_inset_grows_with_the_widest_label() {
        let shaper = default_shaper();
        let style = AxisStyle::default();
        let narrow = left_inset(&["1".into()], None, &shaper, &style);
        let wide = left_inset(&["1".into(), "100000".into()], None, &shaper, &style);
        assert!(wide > narrow);
        // The inset must clear the tick marks even for an empty label set.
        assert!(left_inset(&[], None, &shaper, &style) >= style.tick_size);
    }

    #[test]
    fn a_title_reserves_extra_space() {
        let shaper = default_shaper();
        let style = AxisStyle::default();
        let without = left_inset(&["10".into()], None, &shaper, &style);
        let with = left_inset(&["10".into()], Some("Amount"), &shaper, &style);
        assert!(with > without);
    }

    #[test]
    fn bottom_inset_reserves_a_full_line_box() {
        let shaper = default_shaper();
        let style = AxisStyle::default();
        let inset = bottom_inset(&["A".into()], None, false, &shaper, &style);
        let line_box =
            shaper.ascent(style.label_font_size) + shaper.descent(style.label_font_size);
        assert!(inset >= line_box + style.tick_size);
    }
}
