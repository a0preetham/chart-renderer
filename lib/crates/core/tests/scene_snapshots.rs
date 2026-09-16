//! Layer 3: snapshot the Scene IR for every fixture.
//!
//! Snapshotting geometry rather than pixels is the reason the IR exists. A
//! review diff here reads `"x": 142.5` -> `"x": 141.0`, which says exactly what
//! moved; a PNG diff only says that something did.

mod common;

use chart_renderer::render_scene;

use common::{fixture_names, read_fixture};

/// Rounds every number in a JSON tree to 3 decimal places.
///
/// Scene coordinates are `f32`, and serializing them widens to `f64` with
/// artefacts like `28.571428298950195`. Those digits are noise, and left in they
/// would make snapshot diffs unreadable — which would defeat the point.
fn round_numbers(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(n) => {
            let rounded = (n.as_f64().unwrap_or(0.0) * 1000.0).round() / 1000.0;
            // Keep whole numbers whole, so colour channels read as `255` rather
            // than `255.0`.
            if rounded.fract() == 0.0 && rounded.abs() < 9e15 {
                serde_json::Value::Number((rounded as i64).into())
            } else {
                serde_json::Number::from_f64(rounded)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            }
        }
        serde_json::Value::Array(a) => {
            serde_json::Value::Array(a.into_iter().map(round_numbers).collect())
        }
        serde_json::Value::Object(o) => serde_json::Value::Object(
            o.into_iter().map(|(k, v)| (k, round_numbers(v))).collect(),
        ),
        other => other,
    }
}

#[test]
fn scene_ir_matches_snapshots() {
    let names = fixture_names();
    assert!(!names.is_empty(), "no fixtures found");

    for name in names {
        let scene = render_scene(&read_fixture(&name))
            .unwrap_or_else(|e| panic!("fixture {name} failed to render: {e}"));
        let json = round_numbers(serde_json::to_value(&scene).unwrap());
        insta::assert_json_snapshot!(name, json);
    }
}
