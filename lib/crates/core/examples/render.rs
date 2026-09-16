//! Renders a spec file to a PNG.
//!
//! ```text
//! cargo run --example render -- tests/fixtures/simple_bar.json out.png [scale]
//! ```
//!
//! `scale` is device pixels per scene unit; pass 2 or 3 to see what a HiDPI
//! display gets.

use std::{env, fs, process};

use chart_renderer::{render_png, RenderOptions};

fn main() {
    let mut args = env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: render <spec.json> <out.png> [scale]");
        process::exit(2);
    };
    let scale: f32 = match args.next() {
        Some(raw) => match raw.parse() {
            Ok(v) => v,
            Err(_) => {
                eprintln!("scale must be a number, got {raw:?}");
                process::exit(2);
            }
        },
        None => 1.0,
    };

    let spec = match fs::read_to_string(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("reading {input}: {e}");
            process::exit(1);
        }
    };

    match render_png(
        &spec,
        &RenderOptions {
            scale,
            ..Default::default()
        },
    ) {
        Ok(png) => {
            if let Err(e) = fs::write(&output, &png) {
                eprintln!("writing {output}: {e}");
                process::exit(1);
            }
            println!("{output}: {} bytes", png.len());
        }
        Err(e) => {
            eprintln!("{input}: {e}");
            process::exit(1);
        }
    }
}
