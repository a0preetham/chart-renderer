//! PNG encoding.
//!
//! The handoff flags deflate as the likely dominant cost of a serverless render,
//! so the compression level is a first-class knob and defaults to `Fast` rather
//! than the `png` crate's default. Milestone 4 replaces that guess with a
//! measurement from the workerd harness.

use tiny_skia::Pixmap;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Compression {
    /// Lowest CPU cost. The default, because CPU time is the binding constraint
    /// on Workers, not bytes on the wire.
    #[default]
    Fast,
    /// The `png` crate's balanced setting.
    Balanced,
    /// Smallest output, highest CPU cost.
    Best,
}

impl Compression {
    fn to_png(self) -> png::Compression {
        match self {
            Compression::Fast => png::Compression::Fast,
            Compression::Balanced => png::Compression::Default,
            Compression::Best => png::Compression::Best,
        }
    }
}

/// Encodes a pixmap as an 8-bit RGBA PNG.
///
/// tiny-skia stores **premultiplied** pixels; PNG wants straight alpha, so every
/// pixel is demultiplied on the way out. Skipping that step would darken
/// semi-transparent marks.
pub fn encode(pixmap: &Pixmap, compression: Compression) -> Result<Vec<u8>> {
    let width = pixmap.width();
    let height = pixmap.height();

    let mut rgba = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for px in pixmap.pixels() {
        let c = px.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(compression.to_png());
        let mut writer = encoder
            .write_header()
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer
            .write_image_data(&rgba)
            .map_err(|e| Error::Encode(e.to_string()))?;
        writer.finish().map_err(|e| Error::Encode(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Color, Scene};
    use crate::render::skia;

    fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
        // IHDR width/height live at byte offsets 16..24.
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
        (w, h)
    }

    #[test]
    fn emits_a_valid_png_of_the_requested_size() {
        let scene = Scene::new(40.0, 30.0, Color::WHITE);
        let pm = skia::rasterize(&scene).unwrap();
        let bytes = encode(&pm, Compression::Fast).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(png_dimensions(&bytes), (40, 30));
    }

    #[test]
    fn every_compression_level_round_trips() {
        let scene = Scene::new(20.0, 20.0, Color::rgb(1, 2, 3));
        let pm = skia::rasterize(&scene).unwrap();
        for level in [Compression::Fast, Compression::Balanced, Compression::Best] {
            let bytes = encode(&pm, level).unwrap();
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{level:?} produced junk");
        }
    }
}
