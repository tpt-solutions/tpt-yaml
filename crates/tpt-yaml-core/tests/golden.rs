//! Golden-fixture smoke tests: every file under `tests/samples/*.yaml` must parse without
//! error, and re-rendering every node in the resulting document must not panic.

use std::fs;
use std::path::Path;

fn samples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples")
}

#[test]
fn all_samples_parse_and_render() {
    let dir = samples_dir();
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("tests/samples directory should exist") {
        let entry = entry.expect("readable directory entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        checked += 1;
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
        let doc = tpt_yaml_core::parse(&source)
            .unwrap_or_else(|e| panic!("parsing {path:?} failed: {e}"));
        assert!(!doc.documents.is_empty(), "{path:?} produced no documents");
        for &root in &doc.documents {
            let _ = tpt_yaml_core::pretty_print(root, &doc);
        }
    }
    assert!(checked > 0, "expected at least one .yaml fixture in {dir:?}");
}
