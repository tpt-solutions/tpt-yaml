use crate::error::Error;
use alloc::string::String;
use alloc::vec::Vec;
use tpt_yaml_core::{Document, NodeId, NodeKind};

/// One segment of a [`Path`]: a mapping field or a sequence index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Field(String),
    Index(usize),
}

/// A path from a document's root down to some node, e.g. `Path::root().field("a").index(0)`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Path(Vec<Key>);

impl Path {
    pub fn root() -> Self {
        Self(Vec::new())
    }

    pub fn field(mut self, name: impl Into<String>) -> Self {
        self.0.push(Key::Field(name.into()));
        self
    }

    pub fn index(mut self, i: usize) -> Self {
        self.0.push(Key::Index(i));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn keys(&self) -> &[Key] {
        &self.0
    }

    /// Splits off the last segment: `(everything before it, the last segment)`, or `None` for
    /// the root path.
    pub fn split_last(&self) -> Option<(Path, Key)> {
        let (last, init) = self.0.split_last()?;
        Some((Path(init.to_vec()), last.clone()))
    }
}

fn key_matches(document: &Document, key_id: NodeId, name: &str) -> bool {
    matches!(document.node(key_id).map(|n| &n.kind), Some(NodeKind::Scalar(s)) if s.raw == name)
}

/// The index of the entry/item `key` names within `parent`, or `Ok(None)` if `parent` is the
/// right kind of container but has no such entry/item (`Err(TypeMismatch)` if it's the wrong
/// kind of container entirely).
pub fn child_index(document: &Document, parent: NodeId, key: &Key) -> Result<Option<usize>, Error> {
    match (document.node(parent).map(|n| &n.kind), key) {
        (Some(NodeKind::Mapping(entries)), Key::Field(name)) => {
            Ok(entries.iter().position(|&(k, _)| key_matches(document, k, name)))
        }
        (Some(NodeKind::Sequence(items)), Key::Index(i)) => {
            Ok(if *i < items.len() { Some(*i) } else { None })
        }
        _ => Err(Error::TypeMismatch),
    }
}

fn step(document: &Document, current: NodeId, key: &Key) -> Result<NodeId, Error> {
    let index = child_index(document, current, key)?.ok_or(match key {
        Key::Field(_) => Error::KeyNotFound,
        Key::Index(_) => Error::IndexOutOfBounds,
    })?;
    match (document.node(current).map(|n| &n.kind), key) {
        (Some(NodeKind::Mapping(entries)), Key::Field(_)) => Ok(entries[index].1),
        (Some(NodeKind::Sequence(items)), Key::Index(_)) => Ok(items[index]),
        _ => Err(Error::TypeMismatch),
    }
}

/// Resolves `path` from `root`, returning the [`NodeId`] it names.
pub fn resolve_path(document: &Document, root: NodeId, path: &Path) -> Result<NodeId, Error> {
    let mut current = root;
    for key in &path.0 {
        current = step(document, current, key)?;
    }
    Ok(current)
}

/// Like [`resolve_path`], but returns every [`NodeId`] visited along the way (`[root, ...,
/// target]`) — the ancestor chain a mutation needs to mark dirty.
pub fn resolve_trail(document: &Document, root: NodeId, path: &Path) -> Result<Vec<NodeId>, Error> {
    let mut trail = alloc::vec![root];
    let mut current = root;
    for key in &path.0 {
        current = step(document, current, key)?;
        trail.push(current);
    }
    Ok(trail)
}
