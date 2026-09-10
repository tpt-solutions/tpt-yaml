//! Proptest coverage for `tpt-yaml-schema`: build arbitrary-ish schema YAML source and arbitrary
//! document YAML source compositionally (flow-style YAML, so indentation never has to be
//! computed), then assert `validate`/`validate_with` never panics and that validating the same
//! document against the same schema twice yields identical `ValidationReport`s — the determinism
//! invariant `ValidationReport::deterministic()` (sort by `(path, kind)`) exists to support, and
//! that the unit test `validation_is_deterministic` in `src/lib.rs` models for a single fixed
//! case.
//!
//! Scope note: schema generation here skips `$ref` (the loader always requires a `$defs` mapping
//! to resolve against, which would need extra plumbing to keep in sync with generated names) and
//! never emits the boolean `false` schema (the loader's `compile_schema`/`compile_mapping_entries`
//! only ever parse a schema *node* as a YAML mapping, so `Schema::Constant(false)` isn't
//! reachable from `from_yaml_str` at all — only `Schema::Constant(true)`, via `{}`). Neither
//! omission affects the panic-freedom/determinism properties under test, since both keywords are
//! validated by the same generic `validate_node` machinery exercised by every other variant.

use proptest::prelude::*;
use tpt_yaml_schema::SchemaDocument;

/// A small compositional schema shape, rendered to flow-style YAML schema source.
#[derive(Debug, Clone)]
enum SchemaSpec {
    /// `{}` — the "matches anything" schema.
    Empty,
    Null,
    Boolean,
    String { min_length: Option<u8>, max_length: Option<u8>, pattern: Option<&'static str> },
    Number { minimum: Option<i32>, maximum: Option<i32> },
    Array { items: Option<Box<SchemaSpec>>, min_items: Option<u8>, max_items: Option<u8> },
    Object { properties: Vec<(String, SchemaSpec)>, required: Vec<String>, forbid_additional: bool },
    Enum(Vec<EnumLiteral>),
    Const(EnumLiteral),
    Not(Box<SchemaSpec>),
    OneOf(Vec<SchemaSpec>),
    AnyOf(Vec<SchemaSpec>),
    AllOf(Vec<SchemaSpec>),
}

#[derive(Debug, Clone)]
enum EnumLiteral {
    Null,
    Bool(bool),
    Int(i32),
    Str(String),
}

impl EnumLiteral {
    fn to_yaml(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
            Self::Str(s) => format!("{:?}", s), // double-quoted YAML scalar
        }
    }
}

fn enum_literal() -> impl Strategy<Value = EnumLiteral> {
    prop_oneof![
        Just(EnumLiteral::Null),
        any::<bool>().prop_map(EnumLiteral::Bool),
        any::<i32>().prop_map(EnumLiteral::Int),
        "[a-zA-Z0-9_]{0,6}".prop_map(EnumLiteral::Str),
    ]
}

fn ident() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9_]{0,5}".prop_map(|s| s.to_string())
}

fn schema_spec() -> impl Strategy<Value = SchemaSpec> {
    let leaf = prop_oneof![
        Just(SchemaSpec::Empty),
        Just(SchemaSpec::Null),
        Just(SchemaSpec::Boolean),
        (
            proptest::option::of(0u8..8),
            proptest::option::of(0u8..8),
            proptest::option::of(prop_oneof![
                Just("^[a-z]+$"),
                Just("[0-9]+"),
                Just(".*"),
            ]),
        )
            .prop_map(|(min_length, max_length, pattern)| SchemaSpec::String {
                min_length,
                max_length,
                pattern
            }),
        (proptest::option::of(-10i32..10), proptest::option::of(-10i32..10))
            .prop_map(|(minimum, maximum)| SchemaSpec::Number { minimum, maximum }),
        prop::collection::vec(enum_literal(), 1..4).prop_map(SchemaSpec::Enum),
        enum_literal().prop_map(SchemaSpec::Const),
    ];

    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            (
                proptest::option::of(inner.clone().prop_map(Box::new)),
                proptest::option::of(0u8..4),
                proptest::option::of(0u8..4),
            )
                .prop_map(|(items, min_items, max_items)| SchemaSpec::Array {
                    items,
                    min_items,
                    max_items
                }),
            (
                prop::collection::vec((ident(), inner.clone()), 0..3),
                prop::collection::vec(ident(), 0..2),
                any::<bool>(),
            )
                .prop_map(|(properties, required, forbid_additional)| SchemaSpec::Object {
                    properties,
                    required,
                    forbid_additional,
                }),
            inner.clone().prop_map(|s| SchemaSpec::Not(Box::new(s))),
            prop::collection::vec(inner.clone(), 1..3).prop_map(SchemaSpec::OneOf),
            prop::collection::vec(inner.clone(), 1..3).prop_map(SchemaSpec::AnyOf),
            prop::collection::vec(inner, 1..3).prop_map(SchemaSpec::AllOf),
        ]
    })
}

/// Renders a [`SchemaSpec`] to flow-style JSON-Schema-subset YAML source (valid YAML, so no
/// indentation bookkeeping is needed).
fn schema_to_yaml(spec: &SchemaSpec) -> String {
    match spec {
        SchemaSpec::Empty => "{}".to_string(),
        SchemaSpec::Null => "{type: null}".to_string(),
        SchemaSpec::Boolean => "{type: boolean}".to_string(),
        SchemaSpec::String { min_length, max_length, pattern } => {
            let mut fields = vec!["type: string".to_string()];
            if let Some(min) = min_length {
                fields.push(format!("minLength: {min}"));
            }
            if let Some(max) = max_length {
                fields.push(format!("maxLength: {max}"));
            }
            if let Some(pat) = pattern {
                fields.push(format!("pattern: {:?}", pat));
            }
            format!("{{{}}}", fields.join(", "))
        }
        SchemaSpec::Number { minimum, maximum } => {
            let mut fields = vec!["type: number".to_string()];
            if let Some(min) = minimum {
                fields.push(format!("minimum: {min}"));
            }
            if let Some(max) = maximum {
                fields.push(format!("maximum: {max}"));
            }
            format!("{{{}}}", fields.join(", "))
        }
        SchemaSpec::Array { items, min_items, max_items } => {
            let mut fields = vec!["type: array".to_string()];
            if let Some(items) = items {
                fields.push(format!("items: {}", schema_to_yaml(items)));
            }
            if let Some(min) = min_items {
                fields.push(format!("minItems: {min}"));
            }
            if let Some(max) = max_items {
                fields.push(format!("maxItems: {max}"));
            }
            format!("{{{}}}", fields.join(", "))
        }
        SchemaSpec::Object { properties, required, forbid_additional } => {
            let mut fields = vec!["type: object".to_string()];
            if !properties.is_empty() {
                let props = properties
                    .iter()
                    .map(|(name, s)| format!("{name}: {}", schema_to_yaml(s)))
                    .collect::<Vec<_>>()
                    .join(", ");
                fields.push(format!("properties: {{{props}}}"));
            }
            if !required.is_empty() {
                fields.push(format!("required: [{}]", required.join(", ")));
            }
            if *forbid_additional {
                fields.push("additionalProperties: false".to_string());
            }
            format!("{{{}}}", fields.join(", "))
        }
        SchemaSpec::Enum(values) => {
            let items = values.iter().map(|v| v.to_yaml()).collect::<Vec<_>>().join(", ");
            format!("{{enum: [{items}]}}")
        }
        SchemaSpec::Const(value) => format!("{{const: {}}}", value.to_yaml()),
        SchemaSpec::Not(inner) => format!("{{not: {}}}", schema_to_yaml(inner)),
        SchemaSpec::OneOf(items) => combinator_to_yaml("oneOf", items),
        SchemaSpec::AnyOf(items) => combinator_to_yaml("anyOf", items),
        SchemaSpec::AllOf(items) => combinator_to_yaml("allOf", items),
    }
}

fn combinator_to_yaml(keyword: &str, items: &[SchemaSpec]) -> String {
    let rendered = items.iter().map(schema_to_yaml).collect::<Vec<_>>().join(", ");
    format!("{{{keyword}: [{rendered}]}}")
}

/// A small arbitrary YAML document value, rendered to flow-style YAML source.
#[derive(Debug, Clone)]
enum DocSpec {
    Null,
    Bool(bool),
    Int(i32),
    Str(String),
    Seq(Vec<DocSpec>),
    Map(Vec<(String, DocSpec)>),
}

fn doc_spec() -> impl Strategy<Value = DocSpec> {
    let leaf = prop_oneof![
        Just(DocSpec::Null),
        any::<bool>().prop_map(DocSpec::Bool),
        any::<i32>().prop_map(DocSpec::Int),
        "[a-zA-Z0-9_ ]{0,6}".prop_map(DocSpec::Str),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(DocSpec::Seq),
            prop::collection::vec((ident(), inner), 0..4).prop_map(DocSpec::Map),
        ]
    })
}

fn doc_to_yaml(spec: &DocSpec) -> String {
    match spec {
        DocSpec::Null => "null".to_string(),
        DocSpec::Bool(b) => b.to_string(),
        DocSpec::Int(i) => i.to_string(),
        DocSpec::Str(s) => format!("{:?}", s),
        DocSpec::Seq(items) => {
            format!("[{}]", items.iter().map(doc_to_yaml).collect::<Vec<_>>().join(", "))
        }
        DocSpec::Map(entries) => {
            let rendered = entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", doc_to_yaml(v)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{rendered}}}")
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// `validate` never panics, and validating the same document against the same schema twice
    /// yields identical (deterministic) reports.
    #[test]
    fn validate_is_panic_free_and_deterministic(schema in schema_spec(), doc in doc_spec()) {
        let schema_source = schema_to_yaml(&schema);
        let Ok(schema_doc) = SchemaDocument::from_yaml_str(&schema_source) else {
            // A generated schema that the loader rejects isn't a bug to chase here: the shapes
            // above are built to be accepted, but if they weren't, that's out of scope for this
            // panic/determinism property.
            return Ok(());
        };

        let doc_source = doc_to_yaml(&doc);
        let Ok(document) = tpt_yaml_core::parse(&doc_source) else {
            return Ok(());
        };
        let Some(root) = document.root() else {
            return Ok(());
        };

        let first = schema_doc.validate(&document, root);
        let second = schema_doc.validate(&document, root);
        prop_assert_eq!(first, second);
    }

    /// Same as above but routed through `validate_with` and a non-default [`ValidationSettings`]
    /// (only a subset of check groups enabled), to exercise the settings-gating paths too.
    #[test]
    fn validate_with_is_panic_free_and_deterministic(
        schema in schema_spec(),
        doc in doc_spec(),
        groups in prop::collection::vec(
            prop_oneof![
                Just("types"), Just("enums"), Just("strings"), Just("numbers"),
                Just("arrays"), Just("objects"), Just("combinators"), Just("refs"),
            ],
            0..8,
        ),
    ) {
        let schema_source = schema_to_yaml(&schema);
        let Ok(schema_doc) = SchemaDocument::from_yaml_str(&schema_source) else {
            return Ok(());
        };
        let doc_source = doc_to_yaml(&doc);
        let Ok(document) = tpt_yaml_core::parse(&doc_source) else {
            return Ok(());
        };
        let Some(root) = document.root() else {
            return Ok(());
        };

        let settings = tpt_yaml_schema::ValidationSettings::new().only(&groups);
        let first = schema_doc.validate_with(&document, root, &settings);
        let second = schema_doc.validate_with(&document, root, &settings);
        prop_assert_eq!(first, second);
    }
}
