//! The `simple-text` backend: `ab_glyph` against one embedded, subsetted font.
//!
//! No shaping engine. Characters map through `cmap` to glyphs, advance widths
//! come from `hmtx` (plus kerning), and outlines are converted straight to
//! tiny-skia paths. That is exactly enough for Latin-family axis labels, and it
//! keeps the wasm binary small.

use ab_glyph::{Font, FontRef, GlyphId, Outline, OutlineCurve, Point, ScaleFont};
use tiny_skia::{Path, PathBuilder};

use super::{PositionedGlyph, ShapedText, TextShaper};

/// Liberation Sans, subsetted to Latin, Latin-1, Latin Extended-A and common
/// punctuation. Liberation Sans is metrically compatible with Arial, which is
/// what browsers resolve `sans-serif` to for Vega's default font — so our
/// measured widths track the reference implementation's closely.
///
/// Regenerate with:
/// ```text
/// pyftsubset LiberationSans-Regular.ttf \
///   --output-file=assets/LiberationSans-Subset.ttf \
///   --unicodes="U+0020-007E,U+00A0-00FF,U+0100-017F,U+2013-2014,U+2018-201D,U+2026,U+2212,U+00D7" \
///   --layout-features="" --no-hinting --desubroutinize
/// ```
const FONT_BYTES: &[u8] = include_bytes!("../../assets/LiberationSans-Subset.ttf");

pub struct SimpleShaper {
    font: FontRef<'static>,
}

impl Default for SimpleShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleShaper {
    /// The font is embedded at compile time and covered by a test, so the parse
    /// cannot fail on user input — no request can reach this expect.
    pub fn new() -> Self {
        Self {
            font: FontRef::try_from_slice(FONT_BYTES).expect("embedded font must be valid"),
        }
    }

    fn glyph_id(&self, c: char) -> GlyphId {
        self.font.glyph_id(c)
    }

    /// Scale for **advances and outlines**, which are em-relative.
    ///
    /// `ab_glyph`'s `PxScale` is relative to the font's total height
    /// (ascent - descent + line gap), whereas CSS and canvas define a font size
    /// as pixels **per em**. Passing the size straight through makes every
    /// advance too narrow — by ~12% for Liberation Sans, whose height is 2288
    /// units against an em of 2048.
    ///
    /// That error compounds: advances drive label widths, label widths drive axis
    /// insets, and insets place the plot.
    ///
    /// Note the deliberate asymmetry with [`Self::baseline_scale`]: a canvas uses
    /// em-relative advances but height-normalised baselines. Both were measured
    /// against Chrome in `tests/browser_metrics.rs` rather than assumed.
    fn advance_scale(&self, size: f32) -> f32 {
        let units_per_em = self.font.units_per_em().unwrap_or(1000.0);
        if units_per_em <= 0.0 {
            return size;
        }
        size * self.font.height_unscaled() / units_per_em
    }

    /// Scale for **baseline placement**, normalised to the font's height rather
    /// than its em.
    ///
    /// Canvas positions `textBaseline: top`/`middle`/`bottom` using an ascent of
    /// `size * ascent / (ascent + descent)` — 8.10px for this font at 10px, where
    /// the em-relative value would be 9.05px. Using the em-relative figure puts
    /// every label about a pixel out.
    ///
    /// This is `ab_glyph`'s native convention, so the size passes straight through.
    fn baseline_scale(&self, size: f32) -> f32 {
        size
    }
}

impl TextShaper for SimpleShaper {
    fn measure_width(&self, text: &str, size: f32) -> f32 {
        let scaled = self.font.as_scaled(self.advance_scale(size));
        let mut width = 0.0;
        let mut previous: Option<GlyphId> = None;
        for c in text.chars() {
            let id = self.glyph_id(c);
            if let Some(prev) = previous {
                width += scaled.kern(prev, id);
            }
            width += scaled.h_advance(id);
            previous = Some(id);
        }
        width
    }

    fn ascent(&self, size: f32) -> f32 {
        self.font.as_scaled(self.baseline_scale(size)).ascent()
    }

    fn descent(&self, size: f32) -> f32 {
        // ab_glyph reports descent as a negative number; callers want a positive
        // downward distance.
        -self.font.as_scaled(self.baseline_scale(size)).descent()
    }

    fn shape(&self, text: &str, size: f32) -> ShapedText {
        let scaled = self.font.as_scaled(self.advance_scale(size));
        let units_per_em = self.font.units_per_em().unwrap_or(1000.0);
        // Outlines are scaled em-relative, which is already the CSS definition —
        // only the advances needed the `advance_scale` correction above.
        let scale = size / units_per_em;

        let mut glyphs = Vec::new();
        let mut pen_x = 0.0f32;
        let mut previous: Option<GlyphId> = None;

        for c in text.chars() {
            let id = self.glyph_id(c);
            if let Some(prev) = previous {
                pen_x += scaled.kern(prev, id);
            }
            if let Some(outline) = self.font.outline(id) {
                if let Some(path) = outline_to_path(&outline, scale, pen_x) {
                    glyphs.push(PositionedGlyph { path });
                }
            }
            pen_x += scaled.h_advance(id);
            previous = Some(id);
        }

        ShapedText {
            glyphs,
            width: pen_x,
            height: self.ascent(size) + self.descent(size),
        }
    }
}

/// Converts an unscaled `ab_glyph` outline into a tiny-skia path.
///
/// Two conversions happen here: font units scale down to pixels, and the font's
/// y-up axis flips to the screen's y-down. `ab_glyph` hands back a flat list of
/// curves with no contour markers, so a new contour is inferred wherever a
/// curve's start point does not continue from the previous curve's end.
fn outline_to_path(outline: &Outline, scale: f32, offset_x: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let mut cursor: Option<Point> = None;

    let to_screen = |p: Point| (offset_x + p.x * scale, -p.y * scale);

    for curve in &outline.curves {
        let (from, _) = curve_endpoints(curve);
        let starts_new_contour = match cursor {
            None => true,
            Some(prev) => (prev.x - from.x).abs() > 1e-4 || (prev.y - from.y).abs() > 1e-4,
        };
        if starts_new_contour {
            if cursor.is_some() {
                pb.close();
            }
            let (x, y) = to_screen(from);
            pb.move_to(x, y);
        }

        match curve {
            OutlineCurve::Line(_, to) => {
                let (x, y) = to_screen(*to);
                pb.line_to(x, y);
            }
            OutlineCurve::Quad(_, ctrl, to) => {
                let (cx, cy) = to_screen(*ctrl);
                let (x, y) = to_screen(*to);
                pb.quad_to(cx, cy, x, y);
            }
            OutlineCurve::Cubic(_, ctrl1, ctrl2, to) => {
                let (c1x, c1y) = to_screen(*ctrl1);
                let (c2x, c2y) = to_screen(*ctrl2);
                let (x, y) = to_screen(*to);
                pb.cubic_to(c1x, c1y, c2x, c2y, x, y);
            }
        }

        cursor = Some(curve_endpoints(curve).1);
    }

    if cursor.is_some() {
        pb.close();
    }
    pb.finish()
}

fn curve_endpoints(curve: &OutlineCurve) -> (Point, Point) {
    match curve {
        OutlineCurve::Line(from, to) => (*from, *to),
        OutlineCurve::Quad(from, _, to) => (*from, *to),
        OutlineCurve::Cubic(from, _, _, to) => (*from, *to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_font_parses() {
        // Guards the `expect` in `SimpleShaper::new`.
        assert!(FontRef::try_from_slice(FONT_BYTES).is_ok());
    }

    #[test]
    fn width_scales_linearly_with_size() {
        let s = SimpleShaper::new();
        let at10 = s.measure_width("Revenue", 10.0);
        let at20 = s.measure_width("Revenue", 20.0);
        assert!((at20 / at10 - 2.0).abs() < 1e-3, "{at10} vs {at20}");
    }

    #[test]
    fn width_grows_with_content() {
        let s = SimpleShaper::new();
        assert!(s.measure_width("", 10.0) == 0.0);
        assert!(s.measure_width("W", 10.0) > 0.0);
        assert!(s.measure_width("WW", 10.0) > s.measure_width("W", 10.0));
    }

    #[test]
    fn digits_are_tabular_so_tick_labels_line_up() {
        // Arial-metric fonts give every digit the same advance; axis labels rely
        // on it for alignment.
        let s = SimpleShaper::new();
        let widths: Vec<f32> = "0123456789"
            .chars()
            .map(|c| s.measure_width(&c.to_string(), 10.0))
            .collect();
        for w in &widths {
            assert!((w - widths[0]).abs() < 1e-4, "digit widths differ: {widths:?}");
        }
    }

    #[test]
    fn shaping_produces_a_path_per_visible_glyph() {
        let s = SimpleShaper::new();
        let shaped = s.shape("AB", 12.0);
        assert_eq!(shaped.glyphs.len(), 2);
        assert!(shaped.width > 0.0);
        assert!(shaped.height > 0.0);
    }

    #[test]
    fn whitespace_advances_without_emitting_an_outline() {
        let s = SimpleShaper::new();
        let shaped = s.shape("A B", 12.0);
        assert_eq!(shaped.glyphs.len(), 2, "the space should have no outline");
        assert!(shaped.width > s.shape("AB", 12.0).width);
    }

    #[test]
    fn glyphs_sit_above_the_baseline() {
        // y is flipped on the way out of the font, so an uppercase glyph should
        // occupy negative y — above the baseline in screen coordinates.
        let s = SimpleShaper::new();
        let shaped = s.shape("A", 100.0);
        let bounds = shaped.glyphs[0].path.bounds();
        assert!(bounds.top() < 0.0, "top was {}", bounds.top());
        assert!(bounds.bottom() <= 1.0, "bottom was {}", bounds.bottom());
    }

    #[test]
    fn unmapped_characters_are_skipped_rather_than_panicking() {
        let s = SimpleShaper::new();
        // CJK is deliberately not in the subset.
        let shaped = s.shape("A\u{4e2d}B", 12.0);
        assert!(shaped.glyphs.len() <= 3);
        assert!(s.measure_width("\u{4e2d}", 12.0) >= 0.0);
    }

    #[test]
    fn ascent_and_descent_are_positive_and_scale() {
        let s = SimpleShaper::new();
        assert!(s.ascent(10.0) > 0.0);
        assert!(s.descent(10.0) > 0.0);
        assert!((s.ascent(20.0) / s.ascent(10.0) - 2.0).abs() < 1e-3);
    }
}
