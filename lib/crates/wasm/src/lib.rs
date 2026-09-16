//! wasm-bindgen surface.
//!
//! Two entry points, for two audiences:
//!
//! * [`render_png`] — one call, spec in, PNG out. What a Worker or a browser
//!   demo actually wants.
//! * [`Chart`] — the same pipeline split into its stages behind a handle, so a
//!   caller can time each one with `performance.now()`. The handoff's CPU
//!   budget is per-stage, and measuring stages from the outside otherwise means
//!   re-parsing the spec for every measurement.
//!
//! No stage may panic: a panic in wasm aborts the whole isolate, taking
//! unrelated in-flight requests with it. Every fallible path returns a `JsError`.

use chart_renderer::render::{png, skia, Pixmap};
use chart_renderer::{chart, ir::Scene, Compression, RenderOptions, Spec};
use wasm_bindgen::prelude::*;

/// Maps the library's compression levels onto a small integer, so callers do not
/// need a second enum across the wasm boundary.
fn compression_from(level: u8) -> Compression {
    match level {
        0 => Compression::Fast,
        1 => Compression::Balanced,
        _ => Compression::Best,
    }
}

/// Renders a Vega-Lite-style spec to PNG bytes.
///
/// `scale` is device pixels per scene unit — pass `devicePixelRatio` to match
/// what a canvas renderer does on a HiDPI display.
#[wasm_bindgen]
pub fn render_png(spec: &str, compression: u8, scale: f32) -> Result<Vec<u8>, JsError> {
    chart_renderer::render_png(
        spec,
        &RenderOptions {
            compression: compression_from(compression),
            scale,
        },
    )
    .map_err(|e| JsError::new(&e.to_string()))
}

/// The library version, so a demo can show what it is running.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// A chart part-way through the pipeline, for stage-by-stage timing.
#[wasm_bindgen]
pub struct Chart {
    scene: Scene,
    pixmap: Option<Pixmap>,
}

#[wasm_bindgen]
impl Chart {
    /// Stage 1: parse the spec and build the scene (scales, marks, axes).
    #[wasm_bindgen(constructor)]
    pub fn new(spec: &str) -> Result<Chart, JsError> {
        let spec = Spec::parse(spec).map_err(|e| JsError::new(&e.to_string()))?;
        let scene = chart::build(&spec).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(Chart {
            scene,
            pixmap: None,
        })
    }

    /// Stage 2: rasterize the scene at `scale` device pixels per scene unit.
    pub fn rasterize(&mut self, scale: f32) -> Result<(), JsError> {
        self.pixmap = Some(
            skia::rasterize_scaled(&self.scene, scale)
                .ok_or_else(|| JsError::new("invalid scale, or canvas rounds down to zero"))?,
        );
        Ok(())
    }

    /// Stage 3: encode the raster as PNG.
    pub fn encode(&self, compression: u8) -> Result<Vec<u8>, JsError> {
        let pixmap = self
            .pixmap
            .as_ref()
            .ok_or_else(|| JsError::new("call rasterize() before encode()"))?;
        png::encode(pixmap, compression_from(compression)).map_err(|e| JsError::new(&e.to_string()))
    }

    /// Canvas width in pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> f32 {
        self.scene.width
    }

    /// Canvas height in pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> f32 {
        self.scene.height
    }

    /// Number of items in the scene, a rough proxy for chart complexity.
    #[wasm_bindgen(getter, js_name = itemCount)]
    pub fn item_count(&self) -> usize {
        self.scene.items.len()
    }

    /// The data rectangle in canvas coordinates.
    ///
    /// Exposed so a caller can align this chart against another renderer's output
    /// — the two canvases differ in size, but their plot rectangles are what
    /// should coincide.
    #[wasm_bindgen(getter, js_name = plotX)]
    pub fn plot_x(&self) -> f32 {
        self.scene.plot.x
    }

    #[wasm_bindgen(getter, js_name = plotY)]
    pub fn plot_y(&self) -> f32 {
        self.scene.plot.y
    }

    #[wasm_bindgen(getter, js_name = plotWidth)]
    pub fn plot_width(&self) -> f32 {
        self.scene.plot.w
    }

    #[wasm_bindgen(getter, js_name = plotHeight)]
    pub fn plot_height(&self) -> f32 {
        self.scene.plot.h
    }
}
