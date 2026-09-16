//! The `full-text` backend: `cosmic-text` for scripts that need real shaping
//! (Arabic/Hebrew bidi and contextual forms, Indic/Thai reordering).
//!
//! Scaffolded but not implemented. The trait and the feature gate are in place
//! so that dropping the implementation in later touches nothing above it.
//!
//! CJK stays out of scope for bundling either way: Noto Sans CJK alone is 5-15MB,
//! which busts both the 3MB and 10MB Workers script limits. If CJK is ever
//! needed, fonts must be lazy-loaded from R2/KV at request time.

use super::{ShapedText, TextShaper};

pub struct CosmicShaper {
    _private: (),
}

impl Default for CosmicShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl CosmicShaper {
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl TextShaper for CosmicShaper {
    fn shape(&self, _text: &str, _size: f32) -> ShapedText {
        unimplemented!("the full-text backend lands in a later milestone")
    }

    fn measure_width(&self, _text: &str, _size: f32) -> f32 {
        unimplemented!("the full-text backend lands in a later milestone")
    }

    fn ascent(&self, _size: f32) -> f32 {
        unimplemented!("the full-text backend lands in a later milestone")
    }

    fn descent(&self, _size: f32) -> f32 {
        unimplemented!("the full-text backend lands in a later milestone")
    }
}
