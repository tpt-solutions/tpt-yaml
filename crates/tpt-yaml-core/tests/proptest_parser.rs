//! Property tests for the lexer/parser: panic-freedom over arbitrary bytes, and a span-nesting
//! invariant (every child node's span is contained within its parent's) over generated
//! "plausible YAML" trees.

use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use tpt_yaml_core::{Document, NodeId, NodeKind};

proptest! {
    /// Parsing must never panic, no matter what bytes it's fed.
    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        let source = String::from_utf8_lossy(&bytes);
        let _ = tpt_yaml_core::parse(&source);
    }
}

#[derive(Debug, Clone)]
enum TestValue {
    Scalar(String),
    Seq(Vec<TestValue>),
    Map(Vec<(String, TestValue)>),
}

fn value_strategy() -> impl Strategy<Value = TestValue> {
    let leaf = "[a-z]{1,6}".prop_map(TestValue::Scalar);
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 1..4).prop_map(TestValue::Seq),
            prop::collection::vec(("[a-z]{1,6}", inner), 1..4).prop_map(TestValue::Map),
        ]
    })
}

fn render(value: &TestValue, indent: usize, out: &mut String) {
    let pad = " ".repeat(indent);
    match value {
        TestValue::Scalar(s) => {
            out.push_str(s);
            out.push('\n');
        }
        TestValue::Seq(items) => {
            for item in items {
                out.push_str(&pad);
                out.push_str("- ");
                match item {
                    TestValue::Scalar(s) => {
                        out.push_str(s);
                        out.push('\n');
                    }
                    _ => {
                        out.push('\n');
                        render(item, indent + 2, out);
                    }
                }
            }
        }
        TestValue::Map(entries) => {
            for (k, v) in entries {
                out.push_str(&pad);
                out.push_str(k);
                out.push(':');
                match v {
                    TestValue::Scalar(s) => {
                        out.push(' ');
                        out.push_str(s);
                        out.push('\n');
                    }
                    _ => {
                        out.push('\n');
                        render(v, indent + 2, out);
                    }
                }
            }
        }
    }
}

/// Asserts every child of `id` has a span contained within `id`'s own span, recursively.
fn check_span_nesting(id: NodeId, doc: &Document) -> Result<(), TestCaseError> {
    let Some(node) = doc.node(id) else { return Ok(()) };
    let Some(parent_span) = node.span else { return Ok(()) };

    let check_child = |child: NodeId| -> Result<(), TestCaseError> {
        if let Some(child_span) = doc.node(child).and_then(|n| n.span) {
            prop_assert!(
                child_span.start >= parent_span.start && child_span.end <= parent_span.end,
                "child span {:?} not contained in parent span {:?} (parent {:?}, child {:?})",
                child_span,
                parent_span,
                id,
                child
            );
        }
        check_span_nesting(child, doc)
    };

    match &node.kind {
        NodeKind::Mapping(entries) => {
            for (k, v) in entries {
                check_child(*k)?;
                check_child(*v)?;
            }
        }
        NodeKind::Sequence(items) => {
            for item in items {
                check_child(*item)?;
            }
        }
        NodeKind::Scalar(_) | NodeKind::Alias(_) => {}
    }
    Ok(())
}

proptest! {
    #[test]
    fn span_nesting_holds_over_generated_yaml(value in value_strategy()) {
        let mut source = String::new();
        render(&value, 0, &mut source);
        let doc = tpt_yaml_core::parse(&source);
        prop_assert!(doc.is_ok(), "generated YAML failed to parse: {:?}\nsource:\n{}", doc.err(), source);
        let doc = doc.unwrap();
        for &root in &doc.documents {
            check_span_nesting(root, &doc)?;
        }
    }
}
