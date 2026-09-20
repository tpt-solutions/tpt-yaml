//! Property tests for `stream::EventParser`: panic-freedom over arbitrary bytes, and a
//! structural-equivalence check against the arena parser over generated "plausible YAML" (same
//! generator as `proptest_parser.rs`'s `span_nesting_holds_over_generated_yaml`) — the two
//! parsers must agree on the shape and resolved values of every document they both accept.

use proptest::prelude::*;
use tpt_yaml_core::stream::{Event, EventParser};
use tpt_yaml_core::{Document, NodeId, NodeKind, ParserOptions, ScalarValue};

proptest! {
    /// The event parser must never panic, no matter what bytes it's fed.
    #[test]
    fn never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..200)) {
        let source = String::from_utf8_lossy(&bytes);
        let mut parser = EventParser::new(&source, &ParserOptions::default());
        while let Ok(Some(_)) = parser.next_event() {}
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

/// A parser-agnostic value shape, comparable between the arena parser's `Document` and the event
/// parser's `Event` stream.
#[derive(Debug, Clone, PartialEq)]
enum Shape {
    Scalar(ScalarValue),
    Seq(Vec<Shape>),
    Map(Vec<(Shape, Shape)>),
}

fn shape_from_arena(doc: &Document, id: NodeId) -> Shape {
    match doc.node(id).map(|n| &n.kind) {
        Some(NodeKind::Scalar(scalar)) => Shape::Scalar(scalar.value.clone()),
        Some(NodeKind::Sequence(items)) => {
            Shape::Seq(items.iter().map(|&item| shape_from_arena(doc, item)).collect())
        }
        Some(NodeKind::Mapping(entries)) => Shape::Map(
            entries
                .iter()
                .map(|&(k, v)| (shape_from_arena(doc, k), shape_from_arena(doc, v)))
                .collect(),
        ),
        Some(NodeKind::Alias(target)) => shape_from_arena(doc, *target),
        None => Shape::Scalar(ScalarValue::Null),
    }
}

/// Consumes one value's worth of events (a leaf scalar, or a container through its matching
/// `*End`) and builds the equivalent [`Shape`].
fn shape_from_events(parser: &mut EventParser<'_>) -> Shape {
    let first = parser.next_event().unwrap().expect("value event expected");
    shape_from_events_starting_with(parser, first)
}

/// Like [`shape_from_events`], but the first event of the value has already been pulled.
fn shape_from_events_starting_with(parser: &mut EventParser<'_>, first: Event) -> Shape {
    match first {
        Event::Scalar(scalar) => Shape::Scalar(scalar.value),
        Event::SequenceStart(_) => {
            let mut items = Vec::new();
            loop {
                match parser.next_event().unwrap().expect("sequence event expected") {
                    Event::SequenceEnd => break,
                    other => items.push(shape_from_events_starting_with(parser, other)),
                }
            }
            Shape::Seq(items)
        }
        Event::MappingStart(_) => {
            let mut entries = Vec::new();
            loop {
                let key_event = parser.next_event().unwrap().expect("mapping event expected");
                if matches!(key_event, Event::MappingEnd) {
                    break;
                }
                let key = shape_from_events_starting_with(parser, key_event);
                let value = shape_from_events(parser);
                entries.push((key, value));
            }
            Shape::Map(entries)
        }
        other => panic!("unexpected event in value position: {other:?}"),
    }
}

proptest! {
    /// The event parser and the arena parser must agree on the shape and resolved values of
    /// every generated document.
    #[test]
    fn agrees_with_the_arena_parser_over_generated_yaml(value in value_strategy()) {
        let mut source = String::new();
        render(&value, 0, &mut source);

        let doc = tpt_yaml_core::parse(&source)
            .unwrap_or_else(|e| panic!("arena parser failed on:\n{source}\n{e}"));
        let root = doc.root().expect("generated document has a root");
        let arena_shape = shape_from_arena(&doc, root);

        let mut parser = EventParser::new(&source, &ParserOptions::default());
        assert_eq!(parser.next_event().unwrap(), Some(Event::StreamStart));
        assert_eq!(parser.next_event().unwrap(), Some(Event::DocumentStart));
        let event_shape = shape_from_events(&mut parser);
        assert_eq!(
            parser.next_event().unwrap(),
            Some(Event::DocumentEnd),
            "trailing events after the document's value, source:\n{source}"
        );

        prop_assert_eq!(event_shape, arena_shape, "shape mismatch for source:\n{}", source);
    }
}
