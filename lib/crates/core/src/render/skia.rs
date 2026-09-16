//! Lowers a [`Scene`] onto a tiny-skia pixmap.
//!
//! This is the only module that knows about tiny-skia types. Everything above it
//! deals in the IR.

use tiny_skia::{
    FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Rect, Stroke as SkStroke,
    Transform,
};

use crate::ir::{Anchor, Baseline, Color, Scene, SceneItem, Stroke};
use crate::text::{default_shaper, TextShaper};

/// Largest supersampling factor worth honouring. Beyond this a request is more
/// likely a mistake than an intent, and the pixmap allocation gets serious.
pub const MAX_SCALE: f32 = 8.0;

/// Rasterizes `scene` at 1:1 with the compiled-in text backend.
pub fn rasterize(scene: &Scene) -> Option<Pixmap> {
    rasterize_scaled(scene, 1.0)
}

/// Rasterizes `scene` at `scale` device pixels per scene unit.
///
/// This is how a chart is rendered for a HiDPI display: the scene is laid out
/// once in logical units, then drawn into a pixmap `scale` times larger, which
/// the caller displays at the logical size. Text is the reason it matters —
/// 10px labels rasterized at 1:1 and then scaled up by the browser look soft,
/// which is exactly what a canvas-based renderer avoids by drawing at
/// `devicePixelRatio`.
///
/// Layout is unaffected: shaping and measurement still happen at the logical
/// size, so the geometry is identical and only the sampling density changes.
pub fn rasterize_scaled(scene: &Scene, scale: f32) -> Option<Pixmap> {
    let shaper = default_shaper();
    rasterize_with_shaper(scene, scale, &shaper)
}

pub fn rasterize_with_shaper(
    scene: &Scene,
    scale: f32,
    shaper: &dyn TextShaper,
) -> Option<Pixmap> {
    if !scale.is_finite() || scale <= 0.0 || scale > MAX_SCALE {
        return None;
    }
    let w = (scene.width * scale).round().max(1.0) as u32;
    let h = (scene.height * scale).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h)?;
    pixmap.fill(to_skia_color(scene.background));

    // One scale transform carries everything: fills, glyph outlines, and stroke
    // widths alike, since tiny-skia strokes in path space before transforming.
    let base = Transform::from_scale(scale, scale);
    for item in &scene.items {
        draw(&mut pixmap, item, shaper, base, scale);
    }
    Some(pixmap)
}

/// Nudges an axis-aligned stroke so its edges land on device-pixel boundaries.
///
/// A 1px stroke centred on an integer coordinate straddles two rows at half
/// coverage each, which is why un-nudged gridlines look washed out. Shifting the
/// centre so the stroke's *leading edge* is integral puts it on exactly one row.
///
/// Vega does the same thing, and in the same place: its scenegraph holds whole
/// numbers and the canvas renderer offsets at draw time. Keeping the nudge here
/// rather than in the scene means the IR still matches Vega's scenegraph
/// exactly, which is what the cross-check compares.
fn snap_stroke(scene_coord: f32, stroke_width: f32, scale: f32) -> f32 {
    if scale <= 0.0 {
        return scene_coord;
    }
    let device = scene_coord * scale;
    let width = stroke_width * scale;
    ((device - width / 2.0).round() + width / 2.0) / scale
}

/// Converts Vega's angle convention (0 at 12 o'clock, clockwise) into the
/// standard parametric one used below, where a point is
/// `(cx + r cos t, cy + r sin t)`.
///
/// Screen y grows downward, so an increasing parameter already sweeps clockwise;
/// only the quarter-turn offset is needed.
fn to_param(angle: f32) -> f32 {
    angle - std::f32::consts::FRAC_PI_2
}

/// Appends a circular arc to `pb` as cubic Béziers.
///
/// A cubic can only approximate a circular arc well over a limited sweep, so the
/// span is split into segments of at most a quarter turn. The control-point
/// distance `k = 4/3 * tan(Δ/4)` is the standard choice that makes the curve
/// touch the circle at both ends and at the midpoint.
fn append_arc(pb: &mut PathBuilder, cx: f32, cy: f32, r: f32, from: f32, to: f32) {
    let sweep = to - from;
    if sweep.abs() < 1e-6 || r <= 0.0 {
        return;
    }
    let segments = (sweep.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = sweep / segments as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();

    let mut t0 = from;
    for _ in 0..segments {
        let t1 = t0 + step;
        let (c0, s0) = (t0.cos(), t0.sin());
        let (c1, s1) = (t1.cos(), t1.sin());
        pb.cubic_to(
            cx + r * (c0 - k * s0),
            cy + r * (s0 + k * c0),
            cx + r * (c1 + k * s1),
            cy + r * (s1 - k * c1),
            cx + r * c1,
            cy + r * s1,
        );
        t0 = t1;
    }
}

/// Builds a filled pie or donut slice.
fn arc_path(
    cx: f32,
    cy: f32,
    inner_radius: f32,
    outer_radius: f32,
    start_angle: f32,
    end_angle: f32,
) -> Option<Path> {
    let outer = outer_radius.max(0.0);
    let inner = inner_radius.clamp(0.0, outer);
    // Normalise so the slice always sweeps forward; Vega stores the pair either
    // way round and a filled slice covers the same region regardless.
    let (from, to) = if start_angle <= end_angle {
        (to_param(start_angle), to_param(end_angle))
    } else {
        (to_param(end_angle), to_param(start_angle))
    };
    if outer <= 0.0 || (to - from).abs() < 1e-6 {
        return None;
    }

    let mut pb = PathBuilder::new();
    let at = |r: f32, t: f32| (cx + r * t.cos(), cy + r * t.sin());

    if inner <= 0.0 {
        // A wedge: centre, out to the rim, around, and back.
        pb.move_to(cx, cy);
        let (x, y) = at(outer, from);
        pb.line_to(x, y);
        append_arc(&mut pb, cx, cy, outer, from, to);
    } else {
        // An annular segment: out along one edge, around the rim, back along the
        // other edge, then around the hole in reverse so it winds the opposite
        // way and is cut out.
        let (x, y) = at(inner, from);
        pb.move_to(x, y);
        let (x, y) = at(outer, from);
        pb.line_to(x, y);
        append_arc(&mut pb, cx, cy, outer, from, to);
        let (x, y) = at(inner, to);
        pb.line_to(x, y);
        append_arc(&mut pb, cx, cy, inner, to, from);
    }

    pb.close();
    pb.finish()
}

fn to_skia_color(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn fill_paint(c: Color) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(to_skia_color(c));
    paint.anti_alias = true;
    paint
}

fn stroke_spec(s: Stroke) -> SkStroke {
    SkStroke {
        width: s.width,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Miter,
        ..SkStroke::default()
    }
}

fn draw(
    pixmap: &mut Pixmap,
    item: &SceneItem,
    shaper: &dyn TextShaper,
    base: Transform,
    scale: f32,
) {
    // Every draw goes through the caller's base transform, which carries the
    // device-pixel scale.
    let transform = base;
    match item {
        SceneItem::Rect {
            x,
            y,
            w,
            h,
            fill,
            stroke,
        } => {
            // A zero-height bar (value exactly at the baseline) has no rect to
            // build; skip rather than letting `from_xywh` return None silently.
            let Some(rect) = Rect::from_xywh(*x, *y, w.max(f32::EPSILON), h.max(f32::EPSILON))
            else {
                return;
            };
            if let Some(c) = fill {
                pixmap.fill_rect(rect, &fill_paint(*c), transform, None);
            }
            if let Some(s) = stroke {
                if let Some(path) = PathBuilder::from_rect(rect).stroke(&stroke_spec(*s), 1.0) {
                    pixmap.fill_path(
                        &path,
                        &fill_paint(s.color),
                        FillRule::Winding,
                        transform,
                        None,
                    );
                }
            }
        }

        SceneItem::Line { points, stroke } => {
            if points.len() < 2 {
                return;
            }
            // Gridlines, axis rules and tick marks are axis-aligned hairlines, and
            // only those are worth snapping — a diagonal data line is antialiased
            // whatever we do.
            let flat_y = points.iter().all(|p| p.1 == points[0].1);
            let flat_x = points.iter().all(|p| p.0 == points[0].0);
            let place = |(x, y): (f32, f32)| match (flat_x, flat_y) {
                (true, false) => (snap_stroke(x, stroke.width, scale), y),
                (false, true) => (x, snap_stroke(y, stroke.width, scale)),
                _ => (x, y),
            };

            let mut pb = PathBuilder::new();
            let first = place(points[0]);
            pb.move_to(first.0, first.1);
            for point in &points[1..] {
                let (x, y) = place(*point);
                pb.line_to(x, y);
            }
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(
                    &path,
                    &fill_paint(stroke.color),
                    &stroke_spec(*stroke),
                    transform,
                    None,
                );
            }
        }

        SceneItem::Circle {
            cx,
            cy,
            r,
            fill,
            stroke,
        } => {
            let Some(path) = PathBuilder::from_circle(*cx, *cy, r.max(f32::EPSILON)) else {
                return;
            };
            if let Some(c) = fill {
                pixmap.fill_path(&path, &fill_paint(*c), FillRule::Winding, transform, None);
            }
            if let Some(s) = stroke {
                pixmap.stroke_path(
                    &path,
                    &fill_paint(s.color),
                    &stroke_spec(*s),
                    transform,
                    None,
                );
            }
        }

        SceneItem::Arc {
            cx,
            cy,
            inner_radius,
            outer_radius,
            start_angle,
            end_angle,
            fill,
            stroke,
        } => {
            let Some(path) = arc_path(
                *cx,
                *cy,
                *inner_radius,
                *outer_radius,
                *start_angle,
                *end_angle,
            ) else {
                return;
            };
            if let Some(c) = fill {
                pixmap.fill_path(&path, &fill_paint(*c), FillRule::Winding, transform, None);
            }
            if let Some(s) = stroke {
                pixmap.stroke_path(
                    &path,
                    &fill_paint(s.color),
                    &stroke_spec(*s),
                    transform,
                    None,
                );
            }
        }

        SceneItem::Text {
            x,
            y,
            content,
            size,
            anchor,
            baseline,
            fill,
            angle,
        } => {
            if content.is_empty() {
                return;
            }
            let shaped = shaper.shape(content, *size);
            // Glyph paths come back with the text origin at (0, 0) and the
            // baseline on y = 0, so alignment is a pure translation.
            let dx = match anchor {
                Anchor::Start => 0.0,
                Anchor::Middle => -shaped.width / 2.0,
                Anchor::End => -shaped.width,
            };
            let ascent = shaper.ascent(*size);
            let descent = shaper.descent(*size);
            let dy = match baseline {
                Baseline::Alphabetic => 0.0,
                Baseline::Top => ascent,
                Baseline::Bottom => -descent,
                Baseline::Middle => (ascent - descent) / 2.0,
            };
            // Rotate about the anchor point, then offset within the rotated
            // frame, so alignment means the same thing at any angle.
            let transform = base
                .pre_translate(*x, *y)
                .pre_concat(Transform::from_rotate(*angle))
                .pre_translate(dx, dy);
            let paint = fill_paint(*fill);
            for glyph in &shaped.glyphs {
                pixmap.fill_path(&glyph.path, &paint, FillRule::Winding, transform, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Scene;

    #[test]
    fn background_fills_the_canvas() {
        let scene = Scene::new(4.0, 4.0, Color::rgb(10, 20, 30));
        let pm = rasterize(&scene).unwrap();
        let px = pm.pixels()[0].demultiply();
        assert_eq!((px.red(), px.green(), px.blue()), (10, 20, 30));
    }

    #[test]
    fn a_rect_lands_where_the_scene_says() {
        let mut scene = Scene::new(10.0, 10.0, Color::WHITE);
        scene.push(SceneItem::Rect {
            x: 2.0,
            y: 2.0,
            w: 6.0,
            h: 6.0,
            fill: Some(Color::BLACK),
            stroke: None,
        });
        let pm = rasterize(&scene).unwrap();
        let at = |x: u32, y: u32| pm.pixels()[(y * 10 + x) as usize].demultiply();
        assert_eq!(at(5, 5).red(), 0, "centre should be filled");
        assert_eq!(at(0, 0).red(), 255, "corner should be background");
    }

    #[test]
    fn zero_size_marks_do_not_panic() {
        let mut scene = Scene::new(10.0, 10.0, Color::WHITE);
        scene.push(SceneItem::Rect {
            x: 1.0,
            y: 1.0,
            w: 0.0,
            h: 0.0,
            fill: Some(Color::BLACK),
            stroke: None,
        });
        scene.push(SceneItem::Circle {
            cx: 5.0,
            cy: 5.0,
            r: 0.0,
            fill: Some(Color::BLACK),
            stroke: None,
        });
        scene.push(SceneItem::Line {
            points: vec![(1.0, 1.0)],
            stroke: Stroke::new(Color::BLACK, 1.0),
        });
        scene.push(SceneItem::Text {
            x: 5.0,
            y: 5.0,
            content: String::new(),
            size: 10.0,
            anchor: Anchor::Start,
            baseline: Baseline::Alphabetic,
            fill: Color::BLACK,
            angle: 0.0,
        });
        assert!(rasterize(&scene).is_some());
    }

    /// Counts pixels darker than the white background.
    fn ink(pm: &Pixmap) -> usize {
        pm.pixels()
            .iter()
            .filter(|p| p.demultiply().red() < 200)
            .count()
    }

    #[test]
    fn text_actually_puts_ink_on_the_canvas() {
        let mut scene = Scene::new(60.0, 30.0, Color::WHITE);
        scene.push(SceneItem::Text {
            x: 5.0,
            y: 20.0,
            content: "Hello".into(),
            size: 14.0,
            anchor: Anchor::Start,
            baseline: Baseline::Alphabetic,
            fill: Color::BLACK,
            angle: 0.0,
        });
        assert!(ink(&rasterize(&scene).unwrap()) > 20);
    }

    #[test]
    fn anchor_and_baseline_move_the_text() {
        // The same string at the same point, aligned three ways, must land in
        // three different places.
        let render = |anchor, baseline| {
            let mut scene = Scene::new(80.0, 40.0, Color::WHITE);
            scene.push(SceneItem::Text {
                x: 40.0,
                y: 20.0,
                content: "Wg".into(),
                size: 14.0,
                anchor,
                baseline,
                fill: Color::BLACK,
                angle: 0.0,
            });
            let pm = rasterize(&scene).unwrap();
            // Centre of mass of the inked pixels.
            let (mut sx, mut sy, mut n) = (0.0f64, 0.0f64, 0.0f64);
            for (i, p) in pm.pixels().iter().enumerate() {
                if p.demultiply().red() < 200 {
                    sx += (i % 80) as f64;
                    sy += (i / 80) as f64;
                    n += 1.0;
                }
            }
            assert!(n > 0.0, "nothing was drawn");
            (sx / n, sy / n)
        };

        let start = render(Anchor::Start, Baseline::Alphabetic);
        let middle = render(Anchor::Middle, Baseline::Alphabetic);
        let end = render(Anchor::End, Baseline::Alphabetic);
        assert!(start.0 > middle.0, "start should sit right of middle");
        assert!(middle.0 > end.0, "middle should sit right of end");

        let top = render(Anchor::Start, Baseline::Top);
        let bottom = render(Anchor::Start, Baseline::Bottom);
        assert!(top.1 > bottom.1, "top-aligned text sits lower on screen");
    }
}

#[cfg(test)]
mod hairline_tests {
    use super::*;

    /// Ink coverage (0-255) of each row in a column.
    fn column_ink(pm: &Pixmap, x: u32) -> Vec<u8> {
        (0..pm.height())
            .map(|y| 255 - pm.pixels()[(y * pm.width() + x) as usize].demultiply().red())
            .collect()
    }

    #[test]
    fn a_one_pixel_line_lands_on_exactly_one_row() {
        // A 1px stroke centred on an integer coordinate straddles two pixel rows
        // at half coverage each, which is why gridlines look soft. Crisp means
        // the stroke's edges land on pixel boundaries.
        let mut scene = Scene::new(20.0, 20.0, Color::WHITE);
        scene.push(SceneItem::Line {
            points: vec![(0.0, 10.0), (20.0, 10.0)],
            stroke: Stroke::new(Color::BLACK, 1.0),
        });
        let pm = rasterize(&scene).unwrap();
        let inked: Vec<(usize, u8)> = column_ink(&pm, 10)
            .iter()
            .enumerate()
            .filter(|(_, v)| **v > 0)
            .map(|(i, v)| (i, *v))
            .collect();
        assert_eq!(inked.len(), 1, "expected one fully-inked row, got {inked:?}");
        assert_eq!(inked[0].1, 255, "the row should be fully covered");
    }
}
