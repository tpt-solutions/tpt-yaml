//! The flagship `tpt-yaml-edit` proptest: parse → random edit sequence → `render()` always
//! re-parses, and the re-parsed content matches applying the same edits to a plain data model.
//! Also asserts untouched byte regions stay byte-identical: a leaf line is only rewritten when
//! that leaf (or the whole document) was edited.

use proptest::prelude::*;
use tpt_yaml_core::ScalarValue;
use tpt_yaml_edit::{resolve_path, EditValue, EditableDocument, Path};

const SOURCE: &str = "a: 1\nb: 2\nseq:\n  - 10\n  - 20\n  - 30\n";

#[derive(Debug, Clone)]
enum Op {
    SetField(&'static str, i64),
    SetString(&'static str, String),
    RemoveField(&'static str),
    PushSeq(i64),
    RemoveIndex(usize),
}

fn field() -> impl Strategy<Value = &'static str> {
    prop_oneof![Just("a"), Just("b")]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (field(), any::<i64>()).prop_map(|(f, v)| Op::SetField(f, v)),
        (field(), "[a-z]{0,6}").prop_map(|(f, s)| Op::SetString(f, s)),
        (field(), 0usize..3usize).prop_map(|(f, _)| Op::RemoveField(f)),
        (any::<i64>()).prop_map(Op::PushSeq),
        (0usize..3usize).prop_map(Op::RemoveIndex),
    ]
}

/// Reads an `Int`/`String` scalar at `path` out of the re-parsed document, or `None` if absent.
fn scalar_at(doc: &tpt_yaml_core::Document, path: &Path) -> Option<ScalarValue> {
    let root = doc.root()?;
    let id = resolve_path(doc, root, path).ok()?;
    match &doc.node(id)?.kind {
        tpt_yaml_core::NodeKind::Scalar(s) => Some(s.value.clone()),
        _ => None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn edits_render_reparse_and_match_the_model(ops in prop::collection::vec(op(), 0..12)) {
        let mut doc = EditableDocument::parse(SOURCE).unwrap();
        // Plain data model: `None` = field absent.
        let mut a: Option<ScalarValue> = Some(ScalarValue::Int(1));
        let mut b: Option<ScalarValue> = Some(ScalarValue::Int(2));
        let mut seq: Vec<i64> = vec![10, 20, 30];
        let (mut a_touched, mut b_touched, mut seq_touched) = (false, false, false);

        for op in &ops {
            match op {
                Op::SetField(f, v) => {
                    doc.set(&Path::root().field(*f), EditValue::Scalar(ScalarValue::Int(*v)))
                        .unwrap();
                    if *f == "a" { a = Some(ScalarValue::Int(*v)); a_touched = true; }
                    else { b = Some(ScalarValue::Int(*v)); b_touched = true; }
                }
                Op::SetString(f, s) => {
                    doc.set(&Path::root().field(*f), EditValue::Scalar(ScalarValue::String(s.clone())))
                        .unwrap();
                    if *f == "a" { a = Some(ScalarValue::String(s.clone())); a_touched = true; }
                    else { b = Some(ScalarValue::String(s.clone())); b_touched = true; }
                }
                Op::RemoveField(f) => {
                    // Removing an already-absent field is a no-op for both doc and model.
                    let present = if *f == "a" { a.is_some() } else { b.is_some() };
                    if present {
                        doc.remove(&Path::root().field(*f)).unwrap();
                        if *f == "a" { a = None; } else { b = None; }
                        a_touched = true;
                        b_touched = true;
                    }
                }
                Op::PushSeq(v) => {
                    doc.push(&Path::root().field("seq"), EditValue::Scalar(ScalarValue::Int(*v)))
                        .unwrap();
                    seq.push(*v);
                    seq_touched = true;
                }
                Op::RemoveIndex(i) => {
                    if *i < seq.len() {
                        doc.remove(&Path::root().field("seq").index(*i)).unwrap();
                        seq.remove(*i);
                        seq_touched = true;
                    }
                }
            }
        }

        let rendered = doc.render();
        let reparsed = tpt_yaml_core::parse(&rendered).unwrap();

        prop_assert_eq!(scalar_at(&reparsed, &Path::root().field("a")), a);
        prop_assert_eq!(scalar_at(&reparsed, &Path::root().field("b")), b);
        let root = reparsed.root().unwrap();
        let seq_id = match &reparsed.node(root).unwrap().kind {
            tpt_yaml_core::NodeKind::Mapping(entries) => entries
                .iter()
                .find(|&(k, _)| matches!(
                    &reparsed.node(*k).unwrap().kind,
                    tpt_yaml_core::NodeKind::Scalar(s) if s.raw == "seq"
                ))
                .map(|(_, v)| *v),
            _ => None,
        };
        match seq_id {
            Some(id) => match &reparsed.node(id).unwrap().kind {
                tpt_yaml_core::NodeKind::Sequence(actual) => {
                    prop_assert_eq!(actual.len(), seq.len())
                }
                _ => prop_assert!(seq.is_empty()),
            },
            None => prop_assert!(seq.is_empty()),
        }

        // Untouched byte regions stay byte-identical.
        if !a_touched {
            prop_assert!(rendered.contains("a: 1\n"));
        }
        if !b_touched {
            prop_assert!(rendered.contains("b: 2\n"));
        }
        if !seq_touched {
            prop_assert!(rendered.contains("  - 10\n"));
        }
    }
}
