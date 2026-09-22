//! Demonstrate `tpt-yaml-edit`'s lossless editing: parse a document with comments, edit one
//! value, and show that everything untouched (including comments) survives byte-for-byte.
//!
//! Run with: `cargo run --example round_trip_edit -p tpt-yaml-edit`

use tpt_yaml_core::ScalarValue;
use tpt_yaml_edit::{EditValue, EditableDocument, Path};

fn main() {
    // Two independent sections. Editing one leaves the other's source — including its
    // comment — completely untouched.
    let source = "\
server:
  host: localhost # do not change
  port: 8080
logging:
  level: info # will be bumped
";

    println!("--- before ---");
    println!("{source}");

    let mut doc = EditableDocument::parse(source).expect("valid YAML");
    doc.set(
        &Path::root().field("logging").field("level"),
        EditValue::Scalar(ScalarValue::String("debug".into())),
    )
    .expect("`logging.level` exists in the document");

    let rendered = doc.render();

    println!("--- after (only `logging.level` changed) ---");
    println!("{rendered}");

    // The `server` section was never touched by the edit, so it survives byte-for-byte,
    // comment included — this is `tpt-yaml-edit`'s "lossless editor" pitch in action.
    assert!(rendered.contains("host: localhost # do not change"));
    assert!(rendered.contains("port: 8080"));
    // The edited value shows up somewhere in the re-rendered `logging` section.
    assert!(rendered.contains("debug"));

    println!("lossless edit verified: the untouched `server` section survived byte-for-byte");
}
