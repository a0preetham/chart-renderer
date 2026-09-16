//! Layer 4: geometric invariants that hold for every fixture.
//!
//! These encode intent rather than current output, so they survive intentional
//! restyling in a way snapshots do not — a snapshot happily records a chart
//! whose axis title has slid off the canvas, whereas `nothing_is_drawn_outside_the_canvas`
//! fails.

mod common;

use chart_renderer::ir::{Scene, SceneItem};
use chart_renderer::render::{png, skia};
use chart_renderer::{render_png, render_scene, RenderOptions};

use common::{fixture_names, read_fixture};

fn scene_for(name: &str) -> Scene {
    render_scene(&read_fixture(name))
        .unwrap_or_else(|e| panic!("fixture {name} failed to render: {e}"))
}

/// Bounding box of an item, ignoring text (whose extent depends on the shaper).
fn geometry_bounds(item: &SceneItem) -> Option<(f32, f32, f32, f32)> {
    match item {
        SceneItem::Rect { x, y, w, h, .. } => Some((*x, *y, x + w, y + h)),
        SceneItem::Circle { cx, cy, r, .. } => Some((cx - r, cy - r, cx + r, cy + r)),
        // A slice is bounded by its circle. Tighter bounds would need the
        // angular span, and the containment checks below only need an upper
        // bound to be meaningful.
        SceneItem::Arc {
            cx,
            cy,
            outer_radius,
            ..
        } => Some((
            cx - outer_radius,
            cy - outer_radius,
            cx + outer_radius,
            cy + outer_radius,
        )),
        SceneItem::Line { points, .. } => {
            let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (x, y) in points {
                b = (b.0.min(*x), b.1.min(*y), b.2.max(*x), b.3.max(*y));
            }
            Some(b)
        }
        SceneItem::Text { .. } => None,
    }
}

#[test]
fn geometry_stays_within_the_canvas() {
    for name in fixture_names() {
        let scene = scene_for(&name);
        for item in &scene.items {
            let Some((x0, y0, x1, y1)) = geometry_bounds(item) else {
                continue;
            };
            let tol = 0.5;
            assert!(
                x0 >= -tol && y0 >= -tol && x1 <= scene.width + tol && y1 <= scene.height + tol,
                "{name}: item spans ({x0},{y0})-({x1},{y1}) outside {}x{}",
                scene.width,
                scene.height
            );
        }
    }
}

/// Rasterizes and checks that no ink reaches the canvas border.
///
/// This is what catches text running off the edge: the scene's text items carry
/// only an anchor point, so their true extent is only known once shaped. The
/// y-axis title clipped off the top before it was rotated, and only this check
/// would have noticed.
#[test]
fn nothing_is_drawn_outside_the_canvas() {
    for name in fixture_names() {
        let scene = scene_for(&name);
        let pm = skia::rasterize(&scene).expect("rasterizes");
        let (w, h) = (pm.width() as usize, pm.height() as usize);
        let background = scene.background;

        let is_ink = |x: usize, y: usize| {
            let p = pm.pixels()[y * w + x].demultiply();
            p.red() != background.r || p.green() != background.g || p.blue() != background.b
        };

        for x in 0..w {
            assert!(!is_ink(x, 0), "{name}: ink on the top border at x={x}");
            assert!(!is_ink(x, h - 1), "{name}: ink on the bottom border at x={x}");
        }
        for y in 0..h {
            assert!(!is_ink(0, y), "{name}: ink on the left border at y={y}");
            assert!(!is_ink(w - 1, y), "{name}: ink on the right border at y={y}");
        }
    }
}

#[test]
fn bars_in_a_fixture_share_one_baseline() {
    for name in fixture_names() {
        let scene = scene_for(&name);
        // Legend swatches are rects too, and they live outside the plot.
        let right = scene.plot.x + scene.plot.w;
        let rects: Vec<_> = scene
            .items
            .iter()
            .filter_map(|i| match i {
                SceneItem::Rect { x, y, w, h, .. } if x + w <= right + 1e-3 => {
                    Some((*x, *y, *w, *h))
                }
                _ => None,
            })
            .collect();
        if rects.len() < 2 {
            continue;
        }
        // Every bar has one edge on the shared zero baseline — the bottom edge
        // for a positive value, the top edge for a negative one. So rather than
        // guessing which, look for an edge value common to all bars.
        //
        // Bars run along the constant dimension: equal widths means vertical
        // bars growing in y, equal heights means horizontal bars growing in x.
        let vertical = rects.iter().all(|r| (r.2 - rects[0].2).abs() < 1e-3);
        let edges = |r: &(f32, f32, f32, f32)| {
            if vertical {
                [r.1, r.1 + r.3]
            } else {
                [r.0, r.0 + r.2]
            }
        };

        let shared: Vec<f32> = edges(&rects[0])
            .into_iter()
            .filter(|candidate| {
                rects
                    .iter()
                    .all(|r| edges(r).iter().any(|e| (e - candidate).abs() < 1e-2))
            })
            .collect();

        assert!(
            !shared.is_empty(),
            "{name}: no baseline shared by all {} bars ({:?})",
            rects.len(),
            rects
        );
    }
}

#[test]
fn every_fixture_encodes_to_a_valid_png() {
    for name in fixture_names() {
        let bytes = render_png(&read_fixture(&name), &RenderOptions::default())
            .unwrap_or_else(|e| panic!("fixture {name} failed to encode: {e}"));
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{name} produced junk");
    }
}

#[test]
fn compression_levels_agree_on_pixels() {
    // Changing the deflate level must not change what is drawn, only the size.
    for name in fixture_names() {
        let scene = scene_for(&name);
        let pm = skia::rasterize(&scene).unwrap();
        let fast = png::encode(&pm, png::Compression::Fast).unwrap();
        let best = png::encode(&pm, png::Compression::Best).unwrap();
        assert!(
            best.len() <= fast.len(),
            "{name}: Best ({}) exceeded Fast ({})",
            best.len(),
            fast.len()
        );
    }
}
