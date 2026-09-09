//! Runs `tpt-yaml-core` against the (locally-fetched, gitignored) yaml-test-suite corpus.
//! See `tests/conformance/README.md` for how to fetch it and run this harness.
//!
//! `#[ignore]`d and additionally gated on the corpus directory existing, so its absence never
//! fails a plain `cargo test --workspace`.

use std::fs;
use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/yaml-test-suite/src")
}

#[test]
#[ignore = "requires the yaml-test-suite corpus to be fetched locally; see tests/conformance/README.md"]
fn yaml_test_suite() {
    let corpus = corpus_dir();
    if !corpus.is_dir() {
        eprintln!(
            "yaml-test-suite corpus not found at {corpus:?}; see tests/conformance/README.md"
        );
        return;
    }

    let mut total = 0usize;
    let mut failures = Vec::new();

    for entry in fs::read_dir(&corpus).expect("reading corpus directory") {
        let entry = entry.expect("readable corpus entry");
        let case_dir = entry.path();
        if !case_dir.is_dir() {
            continue;
        }
        let in_yaml = case_dir.join("in.yaml");
        if !in_yaml.is_file() {
            continue;
        }
        total += 1;

        let source = match fs::read_to_string(&in_yaml) {
            Ok(s) => s,
            Err(_) => continue, // not valid UTF-8; out of scope for this harness
        };
        let expect_error = case_dir.join("error").is_file();
        let result = tpt_yaml_core::parse(&source);

        match (expect_error, result) {
            (true, Ok(_)) => failures.push(format!("{case_dir:?}: expected a parse error, got Ok")),
            (false, Err(e)) => failures.push(format!("{case_dir:?}: expected Ok, got error: {e}")),
            _ => {}
        }
    }

    assert!(total > 0, "found no cases with an in.yaml under {corpus:?}");
    let pass = total - failures.len();
    eprintln!("yaml-test-suite: {pass}/{total} cases passed");
    if !failures.is_empty() {
        eprintln!("first failures:");
        for f in failures.iter().take(20) {
            eprintln!("  {f}");
        }
    }
    // Not yet conformant across the whole suite (see tests/conformance/README.md), so this
    // intentionally doesn't assert failures.is_empty() yet — it just reports the count.
}
