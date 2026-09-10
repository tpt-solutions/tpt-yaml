use alloc::string::{String, ToString};
use core::fmt;

/// The error type returned by every fallible operation in this crate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The document has no root node (should only happen for a document built by hand).
    EmptyDocument,
    /// A [`crate::Path`] passed to [`crate::EditableDocument::remove`] had no segments.
    EmptyPath,
    /// A [`crate::Key::Field`] didn't name an existing entry in the mapping it was resolved against.
    KeyNotFound,
    /// A [`crate::Key::Index`] was out of bounds for the sequence it was resolved against.
    IndexOutOfBounds,
    /// A path segment expected a mapping or sequence but found something else (or vice versa).
    TypeMismatch,
    /// A `NodeId` referenced by the document's own structure didn't resolve to a node — an
    /// internal-consistency error, not something a caller's input should be able to trigger.
    DanglingNode,
    /// The source failed to parse.
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDocument => f.write_str("document has no root node"),
            Self::EmptyPath => f.write_str("path has no segments"),
            Self::KeyNotFound => f.write_str("key not found"),
            Self::IndexOutOfBounds => f.write_str("index out of bounds"),
            Self::TypeMismatch => f.write_str("path segment does not match the node's kind"),
            Self::DanglingNode => f.write_str("dangling node id"),
            Self::Parse(msg) => f.write_str(msg),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl From<tpt_yaml_core::YamlError> for Error {
    fn from(e: tpt_yaml_core::YamlError) -> Self {
        Self::Parse(e.to_string())
    }
}
