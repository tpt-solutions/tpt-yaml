//! `typed`-feature coverage: `EditValue::typed` routes a `T: serde::Serialize` through
//! `tpt-yaml-serde`'s serializer, and the resulting arena content renders and re-parses like
//! any hand-built `EditValue`.

#![cfg(feature = "typed")]

use serde::Serialize;
use tpt_yaml_edit::{resolve_path, EditValue, EditableDocument, Path};
use tpt_yaml_serde::Value;

#[derive(Debug, Serialize)]
struct Config {
    name: &'static str,
    count: i32,
    tags: Vec<&'static str>,
}

#[test]
fn typed_editvalue_replaces_the_whole_document() {
    let config = Config { name: "demo", count: 3, tags: vec!["a", "b"] };
    let value = EditValue::typed(&config).unwrap();

    let mut doc = EditableDocument::parse("old: true\n").unwrap();
    doc.set(&Path::root(), value).unwrap();
    let rendered = doc.render();

    let reparsed: Value = tpt_yaml_serde::from_str(&rendered).unwrap();
    let entries = match reparsed {
        Value::Mapping(entries) => entries,
        other => panic!("expected mapping, got {other:?}"),
    };
    let by_key: Vec<(String, Value)> = entries
        .into_iter()
        .map(|(k, v)| (match k { Value::String(s) => s, other => panic!("bad key {other:?}") }, v))
        .collect();
    assert_eq!(by_key[0], ("name".to_string(), Value::String("demo".to_string())));
    assert_eq!(by_key[1], ("count".to_string(), Value::Int(3)));
    assert_eq!(
        by_key[2],
        (
            "tags".to_string(),
            Value::Sequence(vec![
                Value::String("a".to_string()),
                Value::String("b".to_string()),
            ])
        )
    );
}

#[test]
fn typed_editvalue_splices_into_an_existing_field() {
    let config = Config { name: "demo", count: 3, tags: vec!["a"] };
    let mut doc = EditableDocument::parse("a: 1\nconfig:\n  placeholder: 0\n").unwrap();
    doc.set(&Path::root().field("config"), EditValue::typed(&config).unwrap()).unwrap();
    let rendered = doc.render();

    let reparsed = tpt_yaml_core::parse(&rendered).unwrap();
    let root = reparsed.root().unwrap();
    let config_id = resolve_path(&reparsed, root, &Path::root().field("config")).unwrap();
    match &reparsed.node(config_id).unwrap().kind {
        tpt_yaml_core::NodeKind::Mapping(entries) => assert_eq!(entries.len(), 3),
        other => panic!("expected mapping, got {other:?}"),
    }
    // untouched sibling stays byte-identical
    assert!(rendered.contains("a: 1\n"));
}
