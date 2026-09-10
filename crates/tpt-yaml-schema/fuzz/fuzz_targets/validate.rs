#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_yaml_schema::SchemaDocument;

// Arbitrary bytes as schema YAML source: `SchemaDocument::from_yaml_str` must never panic, and
// if it does compile into a schema, `validate` against a small fixed set of sample documents
// (covering every top-level `JsonType`) must never panic either. A `Parse`/`InvalidSchema`/
// `InvalidPattern` `Err` is an expected, graceful outcome — only a panic is a bug.
fuzz_target!(|data: &str| {
    let Ok(schema) = SchemaDocument::from_yaml_str(data) else { return };

    // One sample document per JSON type, plus a small nested mapping, so combinators
    // (`oneOf`/`anyOf`/`allOf`/`not`), `properties`, and `items` all get exercised regardless of
    // what shape the fuzzed schema turns out to be.
    const SAMPLES: &[&str] = &[
        "null",
        "true",
        "{}",
        "[]",
        "{a: {b: 1, c: [1, 2, true, null, \"x\"]}}",
    ];

    for sample in SAMPLES {
        // Any fixed sample is expected to parse; a panic here would be a bug in `tpt-yaml-core`,
        // not in this fuzz target.
        let Ok(document) = tpt_yaml_core::parse(sample) else { continue };
        let Some(root) = document.root() else { continue };
        let _ = schema.validate(&document, root);
    }
});
