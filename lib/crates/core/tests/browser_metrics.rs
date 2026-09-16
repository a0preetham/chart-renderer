//! Pins our text measurement to what a browser reports for the same font.
//!
//! Layout is driven entirely by `measure_width` — it decides axis insets, label
//! placement and therefore the plot origin. If our advances drift from the
//! browser's, every comparison against Vega picks up an offset that looks like a
//! layout bug.
//!
//! The expected values were measured in Chrome via `canvas.measureText` against
//! the *same* embedded font file, at 10px:
//!
//! ```js
//! ctx.font = '10px "ChartRenderer Sans"'   // LiberationSans-Subset.ttf
//! ctx.measureText('Alpha').width           // -> 25.5762
//! ```
//!
//! Regenerate with `apps/chart-renderer/metrics.html` in the demo, which prints
//! these alongside the font's baseline metrics.

use chart_renderer::text::{default_shaper, TextShaper};

/// Chrome's `measureText(...).width` at 10px, same font file.
const CHROME_WIDTHS_10PX: &[(&str, f32)] = &[
    ("A", 6.6699),
    ("W", 9.4385),
    ("0", 5.5615),
    ("100", 16.6846),
    ("Alpha", 25.5762),
    ("category", 38.3545),
    ("Hxg0", 23.3447),
];

#[test]
fn advance_widths_match_the_browser() {
    let shaper = default_shaper();
    for (text, expected) in CHROME_WIDTHS_10PX {
        let ours = shaper.measure_width(text, 10.0);
        assert!(
            (ours - expected).abs() < 0.05,
            "{text:?}: we measure {ours}, Chrome measures {expected}"
        );
    }
}

#[test]
fn advance_widths_scale_linearly() {
    // Chrome's values are for 10px; the same font at 20px must be exactly double,
    // since neither side applies hinting or rounding to advances.
    let shaper = default_shaper();
    for (text, expected) in CHROME_WIDTHS_10PX {
        let ours = shaper.measure_width(text, 20.0);
        assert!(
            (ours - expected * 2.0).abs() < 0.1,
            "{text:?} at 20px: {ours}, expected {}",
            expected * 2.0
        );
    }
}

/// Chrome's baseline offsets for this font at 10px, derived from where the ink of
/// a capital `H` lands when drawn with each `textBaseline`.
///
/// These are **not** the em-relative metrics. Canvas normalises baselines to the
/// font's height — `size * ascent / (ascent + descent)` — which is 8.10px for
/// this font at 10px, where the em-relative ascent would be 9.05px. Advances go
/// the other way and *are* em-relative; see `SimpleShaper::advance_scale`.
/// Getting this backwards puts every axis label about a pixel out: invisible on
/// its own, obvious in the demo's difference view.
#[test]
fn baseline_offsets_match_the_browsers() {
    let shaper = default_shaper();
    let ascent = shaper.ascent(10.0);
    let descent = shaper.descent(10.0);

    // The probe quantises to whole pixels, hence a tolerance rather than equality.
    // `textBaseline: bottom` puts the alphabetic baseline `descent` above y.
    assert!(
        (descent - 1.9).abs() < 0.3,
        "descent {descent} should be ~1.9 (Chrome measured 2.0)"
    );
    // `textBaseline: top` puts it `ascent` below y.
    assert!(
        (ascent - 8.1).abs() < 0.3,
        "ascent {ascent} should be ~8.1 (Chrome measured 8.0)"
    );
    // `textBaseline: middle` splits the difference.
    let middle = (ascent - descent) / 2.0;
    assert!(
        (middle - 3.1).abs() < 0.3,
        "middle offset {middle} should be ~3.1 (Chrome measured 3.0)"
    );
}
