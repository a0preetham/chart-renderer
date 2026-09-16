//! Text measurement and shaping, behind a swappable backend.
//!
//! Two implementations sit behind [`TextShaper`], selected by Cargo feature at
//! compile time rather than at runtime — a deployment picks its locale
//! requirements once and builds accordingly, and the unused backend never enters
//! the binary.
//!
//! * `simple-text` (default): `ab_glyph` against one embedded Latin font. No
//!   ligatures, no bidi, no complex-script reordering. Correct for the axis
//!   labels and titles this library draws.
//! * `full-text`: `cosmic-text`, for scripts that need real shaping.
//!
//! The split that matters: layout calls only [`TextShaper::measure_width`], while
//! outlining happens in the render backend. That is what lets the IR carry
//! strings instead of baked glyph outlines.

use tiny_skia::Path;

#[cfg(feature = "full-text")]
mod cosmic;
#[cfg(feature = "simple-text")]
mod simple;

#[cfg(feature = "simple-text")]
pub use simple::SimpleShaper;

/// One glyph, as a filled path in text-local coordinates: origin at the text
/// anchor, x increasing right, **y increasing down** (screen convention, not the
/// font's y-up convention).
pub struct PositionedGlyph {
    pub path: Path,
}

pub struct ShapedText {
    pub glyphs: Vec<PositionedGlyph>,
    /// Total advance width.
    pub width: f32,
    /// Ascent + descent, i.e. the line box height.
    pub height: f32,
}

pub trait TextShaper {
    fn shape(&self, text: &str, size: f32) -> ShapedText;

    /// Advance width of `text`. The hot path for layout, which needs widths but
    /// not outlines.
    fn measure_width(&self, text: &str, size: f32) -> f32;

    /// Baseline to top of the line box, positive upward.
    fn ascent(&self, size: f32) -> f32;

    /// Baseline to bottom of the line box, positive downward.
    fn descent(&self, size: f32) -> f32;
}

/// The shaper for the compiled-in backend.
#[cfg(feature = "simple-text")]
pub fn default_shaper() -> impl TextShaper {
    SimpleShaper::new()
}

#[cfg(all(feature = "full-text", not(feature = "simple-text")))]
pub fn default_shaper() -> impl TextShaper {
    cosmic::CosmicShaper::new()
}
