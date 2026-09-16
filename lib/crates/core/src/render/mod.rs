pub mod png;
pub mod skia;

pub use png::Compression;
/// Re-exported because it appears in this module's public API: `skia::rasterize`
/// hands one back and `png::encode` takes one, so callers need to name the type
/// without depending on tiny-skia directly.
pub use tiny_skia::Pixmap;
