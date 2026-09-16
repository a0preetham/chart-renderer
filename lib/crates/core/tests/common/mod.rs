//! Shared fixture loading. The same specs drive the snapshot tests, the
//! invariant tests, and (later) the demo gallery, so the suite and the demo
//! cannot drift apart.

use std::fs;
use std::path::{Path, PathBuf};

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every `*.json` fixture, by stem, sorted for stable ordering.
pub fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixtures_dir())
        .expect("fixtures directory should exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        // Reference scenegraphs live alongside the specs; they are not fixtures.
        .filter(|p| !p.to_string_lossy().ends_with(".vega.json"))
        .filter_map(|p| p.file_stem()?.to_str().map(String::from))
        .collect();
    names.sort();
    names
}

pub fn read_fixture(name: &str) -> String {
    let path = fixtures_dir().join(format!("{name}.json"));
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}
