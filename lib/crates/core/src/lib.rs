//! A lightweight, Vega-Lite-style declarative chart renderer.
//!
//! Not Vega: there is no reactive dataflow engine and no view composition. The
//! pipeline is a straight line —
//!
//! ```text
//! spec -> data columns -> scales -> marks (Scene IR) -> raster -> PNG
//! ```
//!
//! ```no_run
//! use chart_renderer::{render_png, RenderOptions};
//!
//! let spec = r#"{
//!   "data": {"values": [{"a": "A", "b": 28}, {"a": "B", "b": 55}]},
//!   "mark": "bar",
//!   "encoding": {
//!     "x": {"field": "a", "type": "nominal"},
//!     "y": {"field": "b", "type": "quantitative"}
//!   }
//! }"#;
//! let png: Vec<u8> = render_png(spec, &RenderOptions::default()).unwrap();
//! ```
//!
//! Nothing in the render path panics: a malformed spec comes back as an
//! [`Error`], because a panic under WASM takes the whole Worker isolate with it.

pub mod axis;
pub mod chart;
pub mod data;
pub mod error;
pub mod ir;
pub mod layout;
pub mod legend;
pub mod marks;
pub mod palette;
pub mod render;
pub mod scale;
pub mod spec;
pub mod text;
pub mod transform;

pub use error::{Error, Result};
pub use ir::{Scene, SceneItem};
pub use render::Compression;
pub use spec::Spec;

#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub compression: Compression,
    /// Device pixels per scene unit.
    ///
    /// Layout happens in logical units regardless; this only changes sampling
    /// density. Pass a display's `devicePixelRatio` to get text that is crisp on
    /// a HiDPI screen instead of a 1:1 bitmap the browser then scales up.
    pub scale: f32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            compression: Compression::default(),
            scale: 1.0,
        }
    }
}

/// Builds the intermediate [`Scene`] for a spec without rasterizing it.
///
/// Exposed because it is what the test suite asserts against — geometry is
/// cheaper and far more precise to check than pixels.
pub fn render_scene(spec_json: &str) -> Result<Scene> {
    let spec = Spec::parse(spec_json)?;
    chart::build(&spec)
}

/// Renders a spec to PNG bytes.
pub fn render_png(spec_json: &str, options: &RenderOptions) -> Result<Vec<u8>> {
    let scene = render_scene(spec_json)?;
    let pixmap = render::skia::rasterize_scaled(&scene, options.scale).ok_or_else(|| {
        Error::InvalidSize(format!(
            "cannot rasterize at scale {} — must be in (0, {}]",
            options.scale,
            render::skia::MAX_SCALE
        ))
    })?;
    render::png::encode(&pixmap, options.compression)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: &str = r#"{
        "width": 300, "height": 200,
        "data": {"values": [{"a":"A","b":28},{"a":"B","b":55},{"a":"C","b":43}]},
        "mark": "bar",
        "encoding": {
            "x": {"field":"a","type":"nominal"},
            "y": {"field":"b","type":"quantitative"}
        }
    }"#;

    #[test]
    fn renders_a_bar_chart_end_to_end() {
        let png = render_png(BAR, &RenderOptions::default()).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        // `width`/`height` size the plot area, Vega-Lite style, so the image is
        // larger: 300x200 plus padding plus whatever the axes need.
        assert!(w > 310 && h > 210, "image was {w}x{h}");
        // The rasterizer rounds the scene's fractional canvas size to whole pixels.
        let scene = render_scene(BAR).unwrap();
        assert_eq!(
            (w, h),
            (scene.width.round() as u32, scene.height.round() as u32)
        );
    }

    #[test]
    fn scale_multiplies_the_pixel_dimensions_but_not_the_layout() {
        let dims = |scale: f32| {
            let png = render_png(
                BAR,
                &RenderOptions {
                    scale,
                    ..Default::default()
                },
            )
            .unwrap();
            (
                u32::from_be_bytes(png[16..20].try_into().unwrap()),
                u32::from_be_bytes(png[20..24].try_into().unwrap()),
            )
        };
        let (w1, h1) = dims(1.0);
        let (w2, h2) = dims(2.0);
        assert!((w2 as i64 - 2 * w1 as i64).abs() <= 1, "{w1} -> {w2}");
        assert!((h2 as i64 - 2 * h1 as i64).abs() <= 1, "{h1} -> {h2}");

        // The scene itself is unchanged: only sampling density differs, so a
        // HiDPI render cannot shift a single mark.
        let scene = render_scene(BAR).unwrap();
        assert!((scene.plot.w - 300.0).abs() < 1e-3);
    }

    #[test]
    fn an_unusable_scale_is_an_error_not_a_panic() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 1e6] {
            let result = render_png(
                BAR,
                &RenderOptions {
                    scale,
                    ..Default::default()
                },
            );
            assert!(result.is_err(), "scale {scale} should be rejected");
        }
    }

    #[test]
    fn malformed_input_errors_instead_of_panicking() {
        let cases = [
            "",
            "null",
            "[]",
            "{}",
            r#"{"data":{"values":[]}}"#,
            r#"{"data":{"values":[]},"mark":"pie","encoding":{}}"#,
            r#"{"data":{"url":"http://x"},"mark":"bar","encoding":{}}"#,
            r#"{"data":{"values":[{"a":1}]},"mark":"bar","encoding":{"x":{"type":"nominal"}}}"#,
        ];
        for case in cases {
            let result = render_png(case, &RenderOptions::default());
            assert!(result.is_err(), "expected an error for {case:?}");
        }
    }

    #[test]
    fn compression_level_changes_output_size_but_not_validity() {
        let fast = render_png(
            BAR,
            &RenderOptions {
                compression: Compression::Fast,
                ..Default::default()
            },
        )
        .unwrap();
        let best = render_png(
            BAR,
            &RenderOptions {
                compression: Compression::Best,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(&fast[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&best[..8], b"\x89PNG\r\n\x1a\n");
        assert!(
            best.len() <= fast.len(),
            "Best ({}) should not exceed Fast ({})",
            best.len(),
            fast.len()
        );
    }
}
