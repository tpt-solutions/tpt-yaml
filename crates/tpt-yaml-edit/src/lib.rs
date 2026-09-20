//! Lossless YAML editing over `tpt-yaml-core`'s arena: change a value, and
//! [`EditableDocument::render`] re-renders only the parts of the tree an edit actually touched —
//! everything else is copied byte-for-byte from the original source.
//!
//! # Design
//!
//! An edit doesn't mutate a node's own data in place; it builds a brand-new node (via
//! [`EditValue`]) and splices its [`tpt_yaml_core::NodeId`] into the parent container, marking
//! every ancestor on the path back to the root "dirty". [`EditableDocument::render`] then walks
//! the tree: a node with a source span that isn't dirty is blitted verbatim from the original
//! source; a dirty node (or one with no span at all, i.e. freshly built) is reconstructed by
//! recursing into its own children with the same rule.
//!
//! This means editing one field deep in a large document leaves sibling fields — and any
//! subtree not on the edited node's ancestor path — byte-identical, including their comments
//! and formatting. The cost: a container that becomes dirty (something inside it changed)
//! loses *its own* directly-attached trivia (comments immediately before/between/after its
//! entries), since `tpt_yaml_core`'s trivia model attaches comments to the whole container, not
//! per-entry, so there's no per-entry position to reinsert them at once that container is being
//! reconstructed rather than blitted as one unit. Nested clean subtrees are unaffected.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod error;
pub mod path;
pub mod value;

pub use error::Error;
pub use path::{child_index, resolve_path, resolve_trail, Key, Path};
pub use value::EditValue;

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use tpt_yaml_core::node::{is_nonempty_collection, render_inline};
use tpt_yaml_core::{Document, NodeData, NodeId, NodeKind, Scalar, ScalarStyle, ScalarValue};

/// A parsed document plus a record of which nodes have been edited, supporting a
/// span-preserving [`render`](EditableDocument::render).
pub struct EditableDocument {
    source: String,
    document: Document,
    dirty: BTreeSet<NodeId>,
}

impl EditableDocument {
    pub fn parse(source: &str) -> Result<Self, Error> {
        let document = tpt_yaml_core::parse(source)?;
        Ok(Self { source: source.to_string(), document, dirty: BTreeSet::new() })
    }

    /// The underlying arena, including any edits made so far.
    pub fn document(&self) -> &Document {
        &self.document
    }

    fn root(&self) -> Result<NodeId, Error> {
        self.document.root().ok_or(Error::EmptyDocument)
    }

    /// Replaces the value at `path` (upserting a missing mapping field; a missing sequence
    /// index is an error — use [`Self::push`] to extend a sequence). `Path::root()` replaces
    /// the whole document.
    pub fn set(&mut self, path: &Path, value: EditValue) -> Result<(), Error> {
        let new_id = self.build(value);
        let Some((parent_path, key)) = path.split_last() else {
            self.document.documents[0] = new_id;
            // The root was replaced wholesale; mark the tree dirty so `render()` doesn't
            // short-circuit back to the original source text.
            self.dirty.insert(new_id);
            return Ok(());
        };
        let root = self.root()?;
        let trail = resolve_trail(&self.document, root, &parent_path)?;
        let parent_id = *trail.last().expect("trail always has at least `root`");
        self.splice_child(parent_id, &key, new_id)?;
        self.dirty.extend(trail);
        Ok(())
    }

    /// Removes the entry/item at `path` from its parent mapping/sequence.
    pub fn remove(&mut self, path: &Path) -> Result<(), Error> {
        let (parent_path, key) = path.split_last().ok_or(Error::EmptyPath)?;
        let root = self.root()?;
        let trail = resolve_trail(&self.document, root, &parent_path)?;
        let parent_id = *trail.last().expect("trail always has at least `root`");
        let index = child_index(&self.document, parent_id, &key)?.ok_or(Error::KeyNotFound)?;
        let node = self.document.node_mut(parent_id).ok_or(Error::DanglingNode)?;
        match &mut node.kind {
            NodeKind::Mapping(entries) => {
                entries.remove(index);
            }
            NodeKind::Sequence(items) => {
                items.remove(index);
            }
            _ => return Err(Error::TypeMismatch),
        }
        self.dirty.extend(trail);
        Ok(())
    }

    /// Appends `value` to the sequence at `path`.
    pub fn push(&mut self, path_to_seq: &Path, value: EditValue) -> Result<(), Error> {
        let new_id = self.build(value);
        let root = self.root()?;
        let trail = resolve_trail(&self.document, root, path_to_seq)?;
        let seq_id = *trail.last().expect("trail always has at least `root`");
        let node = self.document.node_mut(seq_id).ok_or(Error::DanglingNode)?;
        match &mut node.kind {
            NodeKind::Sequence(items) => items.push(new_id),
            _ => return Err(Error::TypeMismatch),
        }
        self.dirty.extend(trail);
        Ok(())
    }

    fn splice_child(
        &mut self,
        parent_id: NodeId,
        key: &Key,
        new_child: NodeId,
    ) -> Result<(), Error> {
        match key {
            Key::Field(name) => match child_index(&self.document, parent_id, key)? {
                Some(index) => {
                    let node = self.document.node_mut(parent_id).ok_or(Error::DanglingNode)?;
                    match &mut node.kind {
                        NodeKind::Mapping(entries) => {
                            entries[index].1 = new_child;
                            Ok(())
                        }
                        _ => Err(Error::TypeMismatch),
                    }
                }
                None => {
                    let key_id = self.build_key(name);
                    let node = self.document.node_mut(parent_id).ok_or(Error::DanglingNode)?;
                    match &mut node.kind {
                        NodeKind::Mapping(entries) => {
                            entries.push((key_id, new_child));
                            Ok(())
                        }
                        _ => Err(Error::TypeMismatch),
                    }
                }
            },
            Key::Index(_) => {
                let index =
                    child_index(&self.document, parent_id, key)?.ok_or(Error::IndexOutOfBounds)?;
                let node = self.document.node_mut(parent_id).ok_or(Error::DanglingNode)?;
                match &mut node.kind {
                    NodeKind::Sequence(items) => {
                        items[index] = new_child;
                        Ok(())
                    }
                    _ => Err(Error::TypeMismatch),
                }
            }
        }
    }

    fn build_key(&mut self, name: &str) -> NodeId {
        self.document.add_node(NodeData::new(
            NodeId(0),
            NodeKind::Scalar(Scalar {
                value: ScalarValue::String(name.to_string()),
                style: ScalarStyle::Plain,
                raw: name.to_string(),
                tag: None,
            }),
            None,
        ))
    }

    fn build(&mut self, value: EditValue) -> NodeId {
        // Typed payloads were converted to a dynamic Value at construction time; flatten them
        // into the plain variants here so the arena build never sees the serde surface.
        #[cfg(feature = "typed")]
        let value = value.into_plain();
        match value {
            EditValue::Scalar(v) => {
                let (style, raw) = match &v {
                    ScalarValue::String(s) => (ScalarStyle::DoubleQuoted, s.clone()),
                    ScalarValue::Null => (ScalarStyle::Plain, String::new()),
                    ScalarValue::Bool(b) => (ScalarStyle::Plain, b.to_string()),
                    ScalarValue::Int(i) => (ScalarStyle::Plain, i.to_string()),
                    ScalarValue::Float(f) => (ScalarStyle::Plain, f.to_string()),
                    ScalarValue::Timestamp(s) => (ScalarStyle::Plain, s.clone()),
                };
                self.document.add_node(NodeData::new(
                    NodeId(0),
                    NodeKind::Scalar(Scalar { value: v, style, raw, tag: None }),
                    None,
                ))
            }
            EditValue::Sequence(items) => {
                let ids: Vec<NodeId> = items.into_iter().map(|item| self.build(item)).collect();
                self.document.add_node(NodeData::new(NodeId(0), NodeKind::Sequence(ids), None))
            }
            EditValue::Mapping(entries) => {
                let ids: Vec<(NodeId, NodeId)> = entries
                    .into_iter()
                    .map(|(k, v)| {
                        let key_id = self.build_key(&k);
                        let value_id = self.build(v);
                        (key_id, value_id)
                    })
                    .collect();
                self.document.add_node(NodeData::new(NodeId(0), NodeKind::Mapping(ids), None))
            }
            #[cfg(feature = "typed")]
            // The `into_plain` flattening above never yields `Typed`, so this arm is
            // unreachable — it exists only to keep the match exhaustive under the feature.
            EditValue::Typed(_) => unreachable!("into_plain flattens Typed payloads"),
        }
    }

    fn is_clean(&self, id: NodeId) -> bool {
        self.document.span(id).is_some() && !self.dirty.contains(&id)
    }

    /// Blits a clean node's source text into the output, normalized to the block context it is
    /// being rendered into (`indent`). A node's span starts at its first token, so the source
    /// leading indentation of its first line is *not* part of the blitted text — without this
    /// prefix, a clean container blitted under `key:\n` emitted its first item at column 0 and
    /// the rendered document no longer re-parsed (a real bug found by the flagship proptest).
    /// Intermediate lines already carry their own source leading spaces, so they only need the
    /// extra `shift` (= `indent - source_col`) added to keep every relative nesting level
    /// intact; when `indent` matches the node's own source column the blit is byte-identical.
    fn blit_indented(&self, id: NodeId, indent: usize, out: &mut String) {
        let span = self.document.span(id).expect("caller checked is_clean");
        let source_col = span.column.saturating_sub(1);
        let shift = indent.saturating_sub(source_col);
        let text = &self.source[span.start..span.end];
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if line.is_empty() {
                out.push_str(line);
            } else {
                out.push_str(&" ".repeat(if i == 0 { source_col + shift } else { shift }));
                out.push_str(line);
            }
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
    }

    /// Renders an inline (non-container, or empty-container) value: blit it if untouched,
    /// otherwise synthesize it fresh via the shared pretty-printer's leaf-rendering logic.
    fn render_leaf(&self, id: NodeId, out: &mut String) {
        if self.is_clean(id) {
            let span = self.document.span(id).expect("checked above");
            out.push_str(&self.source[span.start..span.end]);
            return;
        }
        if let Some(node) = self.document.node(id) {
            out.push_str(&render_inline(node, &self.document));
        }
    }

    fn render_node(&self, id: NodeId, indent: usize, out: &mut String) {
        if self.is_clean(id) {
            self.blit_indented(id, indent, out);
            return;
        }
        let Some(node) = self.document.node(id) else { return };
        let prefix = " ".repeat(indent);
        match &node.kind {
            NodeKind::Scalar(_) | NodeKind::Alias(_) => {
                out.push_str(&prefix);
                out.push_str(&render_inline(node, &self.document));
                out.push('\n');
            }
            NodeKind::Mapping(entries) => {
                if entries.is_empty() {
                    out.push_str(&prefix);
                    out.push_str("{}\n");
                    return;
                }
                for (key_id, value_id) in entries {
                    out.push_str(&prefix);
                    self.render_leaf(*key_id, out);
                    let value_node = self.document.node(*value_id);
                    if value_node.map(is_nonempty_collection).unwrap_or(false) {
                        out.push_str(":\n");
                        self.render_node(*value_id, indent + 2, out);
                    } else {
                        out.push_str(": ");
                        self.render_leaf(*value_id, out);
                        out.push('\n');
                    }
                }
            }
            NodeKind::Sequence(items) => {
                if items.is_empty() {
                    out.push_str(&prefix);
                    out.push_str("[]\n");
                    return;
                }
                for item in items {
                    let item_node = self.document.node(*item);
                    if item_node.map(is_nonempty_collection).unwrap_or(false) {
                        out.push_str(&prefix);
                        out.push_str("-\n");
                        self.render_node(*item, indent + 2, out);
                    } else {
                        out.push_str(&prefix);
                        out.push_str("- ");
                        self.render_leaf(*item, out);
                        out.push('\n');
                    }
                }
            }
        }
    }

    /// Renders the document, blitting untouched byte ranges verbatim from the original source
    /// and pretty-printing only what an edit actually touched.
    pub fn render(&self) -> String {
        if self.dirty.is_empty() {
            return self.source.clone();
        }
        let Ok(root) = self.root() else { return String::new() };
        let mut out = String::new();
        self.render_node(root, 0, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder() {
        assert!(EditableDocument::parse("ok: true").is_ok());
    }

    #[test]
    fn unedited_document_renders_byte_identical() {
        let source = "a: 1\nb: 2 # comment\n";
        let doc = EditableDocument::parse(source).unwrap();
        assert_eq!(doc.render(), source);
    }

    #[test]
    fn set_replaces_a_scalar_value_and_reparses() {
        let source = "a: 1\nb: 2\n";
        let mut doc = EditableDocument::parse(source).unwrap();
        doc.set(&Path::root().field("b"), EditValue::Scalar(ScalarValue::Int(99))).unwrap();
        let rendered = doc.render();

        let reparsed = tpt_yaml_core::parse(&rendered).unwrap();
        let root = reparsed.root().unwrap();
        let b = resolve_path(&reparsed, root, &Path::root().field("b")).unwrap();
        match &reparsed.node(b).unwrap().kind {
            NodeKind::Scalar(s) => assert_eq!(s.value, ScalarValue::Int(99)),
            other => panic!("expected scalar, got {other:?}"),
        }
        // untouched sibling stays byte-identical
        assert!(rendered.contains("a: 1\n"));
    }

    #[test]
    fn set_upserts_a_missing_key() {
        let mut doc = EditableDocument::parse("a: 1\n").unwrap();
        doc.set(&Path::root().field("c"), EditValue::Scalar(ScalarValue::Int(3))).unwrap();
        let rendered = doc.render();
        let reparsed = tpt_yaml_core::parse(&rendered).unwrap();
        let root = reparsed.root().unwrap();
        let c = resolve_path(&reparsed, root, &Path::root().field("c")).unwrap();
        match &reparsed.node(c).unwrap().kind {
            NodeKind::Scalar(s) => assert_eq!(s.value, ScalarValue::Int(3)),
            other => panic!("expected scalar, got {other:?}"),
        }
    }

    #[test]
    fn remove_deletes_a_mapping_entry() {
        let mut doc = EditableDocument::parse("a: 1\nb: 2\n").unwrap();
        doc.remove(&Path::root().field("b")).unwrap();
        let rendered = doc.render();
        let reparsed = tpt_yaml_core::parse(&rendered).unwrap();
        let root = reparsed.root().unwrap();
        assert!(resolve_path(&reparsed, root, &Path::root().field("b")).is_err());
        assert!(resolve_path(&reparsed, root, &Path::root().field("a")).is_ok());
    }

    #[test]
    fn push_appends_to_a_sequence() {
        let mut doc = EditableDocument::parse("items:\n  - 1\n  - 2\n").unwrap();
        doc.push(&Path::root().field("items"), EditValue::Scalar(ScalarValue::Int(3))).unwrap();
        let rendered = doc.render();
        let reparsed = tpt_yaml_core::parse(&rendered).unwrap();
        let root = reparsed.root().unwrap();
        let items = resolve_path(&reparsed, root, &Path::root().field("items")).unwrap();
        match &reparsed.node(items).unwrap().kind {
            NodeKind::Sequence(seq) => assert_eq!(seq.len(), 3),
            other => panic!("expected sequence, got {other:?}"),
        }
    }

    #[test]
    fn set_replaces_the_whole_document() {
        let mut doc = EditableDocument::parse("a: 1\n").unwrap();
        doc.set(&Path::root(), EditValue::Scalar(ScalarValue::String("hello".to_string())))
            .unwrap();
        let rendered = doc.render();
        let reparsed = tpt_yaml_core::parse(&rendered).unwrap();
        match &reparsed.node(reparsed.root().unwrap()).unwrap().kind {
            NodeKind::Scalar(s) => assert_eq!(s.value, ScalarValue::String("hello".to_string())),
            other => panic!("expected scalar, got {other:?}"),
        }
    }
}
