//! Renders a text specimen straight through the Scene IR, for eyeballing glyph
//! quality without a chart around it.
//!
//! ```text
//! cargo run --example text_specimen -- specimen.png
//! ```

use std::{env, fs, process};

use chart_renderer::ir::{Anchor, Baseline, Color, Scene, SceneItem};
use chart_renderer::render::{png, skia, Compression};

const SAMPLE: &str = "Handgloves 0123456789 aeo@&%";

fn main() {
    let output = env::args().nth(1).unwrap_or_else(|| "specimen.png".into());

    let mut scene = Scene::new(760.0, 300.0, Color::WHITE);
    let mut y = 40.0;

    // The sizes a chart actually uses come first (10 and 11), then progressively
    // larger ones where outline defects become obvious.
    for size in [10.0f32, 11.0, 14.0, 20.0, 32.0, 56.0] {
        scene.push(SceneItem::Text {
            x: 12.0,
            y,
            content: format!("{size:.0}px  {SAMPLE}"),
            size,
            anchor: Anchor::Start,
            baseline: Baseline::Alphabetic,
            fill: Color::BLACK,
            angle: 0.0,
        });
        y += size * 1.5 + 8.0;
    }

    let pixmap = skia::rasterize(&scene).expect("rasterizes");
    let bytes = png::encode(&pixmap, Compression::Best).expect("encodes");
    if let Err(e) = fs::write(&output, &bytes) {
        eprintln!("writing {output}: {e}");
        process::exit(1);
    }
    println!("{output}: {} bytes", bytes.len());
}
