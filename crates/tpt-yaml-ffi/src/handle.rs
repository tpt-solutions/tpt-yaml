//! Opaque handle types and node/scalar tag enums exposed across the C ABI.

use tpt_yaml_core::{Document, NodeId};

/// An opaque handle to a parsed YAML document, obtained from [`crate::tpt_yaml_parse`] and
/// released with [`crate::tpt_yaml_document_free`].
///
/// C code must treat this as an opaque pointer — never dereference its fields, which don't
/// exist in the C header at all (`cbindgen` emits it as an incomplete `struct`). Internally it
/// just wraps a boxed [`tpt_yaml_core::Document`], the same arena the rest of the `tpt-yaml`
/// family works with.
pub struct TptYamlDocument {
    pub(crate) document: Document,
}

impl TptYamlDocument {
    pub(crate) fn new(document: Document) -> Self {
        Self { document }
    }
}

/// Node handles are plain `u32` arena indices ([`tpt_yaml_core::NodeId`] is a `Copy` newtype
/// around a `u32` with no lifetime or generic parameter), passed by value rather than through a
/// second heap-allocated opaque type. Convert with [`node_id_to_ffi`]/[`node_id_from_ffi`].
pub type TptYamlNodeId = u32;

pub(crate) fn node_id_from_ffi(id: TptYamlNodeId) -> NodeId {
    NodeId(id)
}

pub(crate) fn node_id_to_ffi(id: NodeId) -> TptYamlNodeId {
    id.get()
}

/// The structural kind of a node, mirroring [`tpt_yaml_core::NodeKind`]'s variants.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TptYamlNodeKind {
    Scalar = 0,
    Mapping = 1,
    Sequence = 2,
    Alias = 3,
}

/// The resolved type tag of a scalar node, mirroring [`tpt_yaml_core::ScalarValue`]'s variants.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TptYamlScalarType {
    Null = 0,
    Bool = 1,
    Int = 2,
    Float = 3,
    String = 4,
    Timestamp = 5,
}
