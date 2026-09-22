//! Load a JSON-Schema-shaped YAML schema, validate a config document against it, and print
//! the resulting `ValidationReport` — both for a passing and a failing document.
//!
//! Run with: `cargo run --example validate_config -p tpt-yaml-schema`

use tpt_yaml_schema::SchemaDocument;

fn main() {
    let schema = SchemaDocument::from_yaml_str(
        "\
type: object
required: [name, ports]
properties:
  name: {type: string}
  ports: {type: array, items: {type: number, minimum: 1, maximum: 65535}}
",
    )
    .expect("valid schema source");

    // A document that satisfies the schema.
    let valid_doc = tpt_yaml_core::parse("name: alpha\nports: [80, 443]\n").expect("valid YAML");
    let report = schema.validate(&valid_doc, valid_doc.root().unwrap());
    println!("valid document -> is_valid: {}", report.is_valid());
    assert!(report.is_valid());

    // A document that violates it: `ports` contains an out-of-range number, and `name` is missing.
    let invalid_doc =
        tpt_yaml_core::parse("ports: [80, 99999]\n").expect("valid YAML, invalid per schema");
    let report = schema.validate(&invalid_doc, invalid_doc.root().unwrap());
    println!("\ninvalid document -> is_valid: {}", report.is_valid());
    println!("issues ({}):", report.issue_count());
    for issue in report.issues() {
        println!("  [{}] {:?}: {}", issue.path, issue.kind, issue.message);
    }
    assert!(!report.is_valid());
}
