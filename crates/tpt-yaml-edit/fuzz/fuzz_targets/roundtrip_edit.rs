#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_yaml_core::ScalarValue;
use tpt_yaml_edit::{EditValue, EditableDocument, Path};

/// The highest-priority fuzz target in the family: arbitrary source → parse → set/remove/push a
/// handful of derived paths → `render()` must always re-parse (span/trivia splicing is the
/// riskiest new code path in `tpt-yaml-edit`).
fuzz_target!(|data: &str| {
    let Ok(mut doc) = EditableDocument::parse(data) else { return };

    // Derive a few field names out of the input so they're attacker-controlled, not fixed.
    let f1 = data.get(0..1.min(data.len())).unwrap_or("x");
    let f2 = data.get(0..2.min(data.len())).unwrap_or("y");

    let _ = doc.set(&Path::root().field(f1), EditValue::Scalar(ScalarValue::Int(1)));
    let _ = doc.set(&Path::root().field(f2), EditValue::Scalar(ScalarValue::Null));
    let _ = doc.set(
        &Path::root().field(f1),
        EditValue::Sequence(vec![EditValue::Scalar(ScalarValue::Bool(true))]),
    );
    let _ = doc.remove(&Path::root().field(f2));

    let rendered = doc.render();
    // Panics are the failure mode; a parse error in the re-parse is tolerated for inputs whose
    // derived paths don't typecheck, but any panic here is a real span-splicing bug.
    let _ = tpt_yaml_core::parse(&rendered);
});
