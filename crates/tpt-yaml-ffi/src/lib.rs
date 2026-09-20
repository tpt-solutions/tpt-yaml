//! C ABI foundation for the `tpt-yaml` family.
//!
//! This crate exposes a stable, panic-free `extern "C"` surface over [`tpt_yaml_core::Document`]:
//! parse YAML source into an opaque document handle, then query it (root node, mapping/sequence
//! traversal, scalar value extraction) without needing to link against Rust directly. It is the
//! foundation the planned Python (`pyo3`) and JS/WASM (`wasm-bindgen`) binding crates will sit
//! on top of — see `todo.md` §6 for the overall plan. Only the C ABI itself is implemented here;
//! there is no Python or JS code in this crate.
//!
//! # Design
//!
//! - **Document handle**: [`TptYamlDocument`] is an opaque pointer type wrapping a boxed
//!   [`tpt_yaml_core::Document`] (a `Vec<NodeData>` arena). Obtain one from [`tpt_yaml_parse`],
//!   release it with [`tpt_yaml_document_free`].
//! - **Node handles**: [`tpt_yaml_core::NodeId`] is a plain `Copy` newtype around a `u32` with no
//!   lifetime or generic parameter, so nodes are addressed by a raw `u32` index
//!   ([`handle::TptYamlNodeId`]) passed by value — no second opaque heap type needed.
//! - **Errors**: every function returns (or writes via an out-parameter) a
//!   [`error::TptYamlErrorCode`]; a human-readable message for the most recent failure *on the
//!   calling thread* is available via [`error::tpt_yaml_last_error_message`].
//! - **No panics cross the FFI boundary**: every function body runs inside
//!   [`std::panic::catch_unwind`] (via the internal `error::guard` helper) and converts any
//!   caught panic into [`error::TptYamlErrorCode::PanicCaught`], since unwinding across an FFI
//!   boundary is undefined behavior.
//! - **Null-checked**: every raw pointer argument is checked against null before being
//!   dereferenced.
//!
//! # Memory-safety contract
//!
//! - [`tpt_yaml_parse`] transfers ownership of a new document to the caller; free it exactly
//!   once with [`tpt_yaml_document_free`].
//! - Calling [`tpt_yaml_document_free`] a second time on the same pointer, or using a document
//!   pointer after it has been freed, is undefined behavior — this crate does not (and, given a
//!   plain C ABI, cannot in general) detect either case. See that function's doc comment.
//! - Strings returned by [`tpt_yaml_scalar_string`] and
//!   [`error::tpt_yaml_last_error_message`] are borrowed: they point into memory owned by the
//!   document (or, for the error message, by thread-local storage) and must not be freed by the
//!   caller or used after the owning document is freed / another call overwrites the error slot.

use std::os::raw::c_char;
use std::panic::UnwindSafe;
use std::slice;

pub mod error;
mod handle;

pub use error::{tpt_yaml_last_error_message, TptYamlErrorCode};
pub use handle::{TptYamlDocument, TptYamlNodeId, TptYamlNodeKind, TptYamlScalarType};

use error::{clear_last_error, guard, set_last_error};
use handle::{node_id_from_ffi, node_id_to_ffi};
use tpt_yaml_core::{Document, NodeData, NodeKind, ScalarValue};

// ---------------------------------------------------------------------------------------------
// Small internal helpers (not part of the public C ABI)
// ---------------------------------------------------------------------------------------------

/// Dereference a document pointer, or fail with `NullPointer`.
///
/// # Safety
/// `doc` must be either null or a pointer previously returned by [`tpt_yaml_parse`] and not yet
/// passed to [`tpt_yaml_document_free`].
unsafe fn deref_doc<'a>(doc: *const TptYamlDocument) -> Result<&'a Document, TptYamlErrorCode> {
    if doc.is_null() {
        set_last_error("null document pointer");
        return Err(TptYamlErrorCode::NullPointer);
    }
    Ok(&(*doc).document)
}

fn get_node(document: &Document, node: TptYamlNodeId) -> Result<&NodeData, TptYamlErrorCode> {
    document.node(node_id_from_ffi(node)).ok_or_else(|| {
        set_last_error(format!("node id {node} is out of bounds for this document"));
        TptYamlErrorCode::IndexOutOfBounds
    })
}

/// Write `value` through `out`, or fail with `NullPointer` if `out` is null.
///
/// # Safety
/// `out` must be either null or a valid, aligned, writable pointer to a `T`.
unsafe fn write_out<T>(out: *mut T, value: T) -> TptYamlErrorCode {
    if out.is_null() {
        set_last_error("null out-parameter pointer");
        return TptYamlErrorCode::NullPointer;
    }
    *out = value;
    TptYamlErrorCode::Ok
}

fn node_kind_tag(kind: &NodeKind) -> TptYamlNodeKind {
    match kind {
        NodeKind::Scalar(_) => TptYamlNodeKind::Scalar,
        NodeKind::Mapping(_) => TptYamlNodeKind::Mapping,
        NodeKind::Sequence(_) => TptYamlNodeKind::Sequence,
        NodeKind::Alias(_) => TptYamlNodeKind::Alias,
    }
}

fn scalar_type_tag(value: &ScalarValue) -> TptYamlScalarType {
    match value {
        ScalarValue::Null => TptYamlScalarType::Null,
        ScalarValue::Bool(_) => TptYamlScalarType::Bool,
        ScalarValue::Int(_) => TptYamlScalarType::Int,
        ScalarValue::Float(_) => TptYamlScalarType::Float,
        ScalarValue::String(_) => TptYamlScalarType::String,
        ScalarValue::Timestamp(_) => TptYamlScalarType::Timestamp,
    }
}

// ---------------------------------------------------------------------------------------------
// Parse / free
// ---------------------------------------------------------------------------------------------

/// Parse `len` bytes at `source` as a YAML document (YAML 1.2, default parser options — same as
/// [`tpt_yaml_core::parse`]).
///
/// `source` need not be null-terminated; exactly `len` bytes are read and must be valid UTF-8.
/// `source` may be null only if `len` is `0` (an empty document).
///
/// On success, returns a non-null document handle that the caller must eventually pass to
/// exactly one [`tpt_yaml_document_free`] call, and (if `out_error` is non-null) writes `Ok` to
/// `*out_error`.
///
/// On failure, returns null and (if `out_error` is non-null) writes the specific
/// [`TptYamlErrorCode`] to `*out_error`; [`tpt_yaml_last_error_message`] then describes the
/// failure (the parse error's rendered `Display` message, including line/column, for
/// `ParseError`).
///
/// # Safety
/// `source` must be null or point to at least `len` readable bytes. `out_error` must be null or
/// a valid, aligned, writable pointer to a `TptYamlErrorCode`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_parse(
    source: *const u8,
    len: usize,
    out_error: *mut TptYamlErrorCode,
) -> *mut TptYamlDocument {
    guard(
        || unsafe {
            let _ = write_out(out_error, TptYamlErrorCode::PanicCaught);
            std::ptr::null_mut()
        },
        move || unsafe {
            clear_last_error();
            if source.is_null() && len != 0 {
                set_last_error("null source pointer with nonzero length");
                let _ = write_out(out_error, TptYamlErrorCode::NullPointer);
                return std::ptr::null_mut();
            }
            let bytes = if len == 0 { &[] } else { slice::from_raw_parts(source, len) };
            let text = match std::str::from_utf8(bytes) {
                Ok(text) => text,
                Err(err) => {
                    set_last_error(format!("source is not valid UTF-8: {err}"));
                    let _ = write_out(out_error, TptYamlErrorCode::InvalidUtf8);
                    return std::ptr::null_mut();
                }
            };
            match tpt_yaml_core::parse(text) {
                Ok(document) => {
                    let _ = write_out(out_error, TptYamlErrorCode::Ok);
                    Box::into_raw(Box::new(TptYamlDocument::new(document)))
                }
                Err(err) => {
                    set_last_error(err.to_string());
                    let _ = write_out(out_error, TptYamlErrorCode::ParseError);
                    std::ptr::null_mut()
                }
            }
        },
    )
}

/// Free a document handle previously returned by [`tpt_yaml_parse`].
///
/// `doc` may be null, in which case this is a no-op.
///
/// # Memory-safety contract
/// - Each successful [`tpt_yaml_parse`] result must be freed **exactly once**.
/// - Calling this twice on the same non-null pointer (double-free) is undefined behavior — it is
///   the caller's responsibility not to do so; this function does not null out or poison the
///   pointer it's given (a plain C ABI has no way to reach back into the caller's variable to do
///   that).
/// - Using `doc`, or any node id / borrowed string obtained from it, after this call
///   (use-after-free) is undefined behavior.
///
/// # Safety
/// `doc` must be null or a pointer previously returned by [`tpt_yaml_parse`] and not already
/// freed.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_document_free(doc: *mut TptYamlDocument) {
    guard(
        || (),
        move || unsafe {
            if !doc.is_null() {
                drop(Box::from_raw(doc));
            }
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Document-level queries
// ---------------------------------------------------------------------------------------------

/// Write the document's root node id to `*out_node`.
///
/// Fails with `IndexOutOfBounds` if the document contains no documents (an empty stream).
///
/// # Safety
/// `doc` must be null or a live [`tpt_yaml_parse`] result. `out_node` must be null or a valid,
/// aligned, writable pointer to a `TptYamlNodeId` (`u32`).
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_document_root(
    doc: *const TptYamlDocument,
    out_node: *mut TptYamlNodeId,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            match document.root() {
                Some(root) => write_out(out_node, node_id_to_ffi(root)),
                None => {
                    set_last_error("document has no root node (empty stream)");
                    TptYamlErrorCode::IndexOutOfBounds
                }
            }
        },
    )
}

/// Write the resolved YAML version as `*out_major`/`*out_minor` (`1`/`1` or `1`/`2`).
///
/// # Safety
/// `doc` must be null or a live [`tpt_yaml_parse`] result. `out_major`/`out_minor` must each be
/// null or a valid, aligned, writable pointer to a `u32`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_document_version(
    doc: *const TptYamlDocument,
    out_major: *mut u32,
    out_minor: *mut u32,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let (major, minor) = match document.version() {
                tpt_yaml_core::YamlVersion::Version11 => (1u32, 1u32),
                tpt_yaml_core::YamlVersion::Version12 => (1u32, 2u32),
            };
            let code = write_out(out_major, major);
            if code != TptYamlErrorCode::Ok {
                return code;
            }
            write_out(out_minor, minor)
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Node-level queries
// ---------------------------------------------------------------------------------------------

/// Write `node`'s structural kind to `*out_kind`.
///
/// # Safety
/// `doc` must be null or a live [`tpt_yaml_parse`] result. `out_kind` must be null or a valid,
/// aligned, writable pointer to a `TptYamlNodeKind`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_node_kind(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_kind: *mut TptYamlNodeKind,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            write_out(out_kind, node_kind_tag(&node.kind))
        },
    )
}

/// Write the target of an alias node to `*out_node`. Fails with `TypeMismatch` if `node` isn't
/// [`TptYamlNodeKind::Alias`].
///
/// # Safety
/// See [`tpt_yaml_node_kind`].
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_alias_target(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_node: *mut TptYamlNodeId,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Alias(target) => write_out(out_node, node_id_to_ffi(*target)),
                _ => {
                    set_last_error("node is not an alias");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Scalar queries
// ---------------------------------------------------------------------------------------------

/// Write `node`'s resolved scalar type tag to `*out_type`. Fails with `TypeMismatch` if `node`
/// isn't a scalar.
///
/// # Safety
/// See [`tpt_yaml_node_kind`].
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_scalar_type(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_type: *mut TptYamlScalarType,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Scalar(scalar) => write_out(out_type, scalar_type_tag(&scalar.value)),
                _ => {
                    set_last_error("node is not a scalar");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Write a scalar node's boolean value to `*out_value`. Fails with `TypeMismatch` if `node`
/// isn't a `Bool` scalar.
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; `out_value` must be null or a valid, aligned, writable pointer to
/// a `bool`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_scalar_as_bool(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_value: *mut bool,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Scalar(scalar) => match scalar.value.as_bool() {
                    Some(value) => write_out(out_value, value),
                    None => {
                        set_last_error("scalar is not a bool");
                        TptYamlErrorCode::TypeMismatch
                    }
                },
                _ => {
                    set_last_error("node is not a scalar");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Write a scalar node's integer value to `*out_value`. Fails with `TypeMismatch` if `node`
/// isn't an `Int` scalar.
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; `out_value` must be null or a valid, aligned, writable pointer to
/// an `i64`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_scalar_as_int(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_value: *mut i64,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Scalar(scalar) => match scalar.value.as_i64() {
                    Some(value) => write_out(out_value, value),
                    None => {
                        set_last_error("scalar is not an int");
                        TptYamlErrorCode::TypeMismatch
                    }
                },
                _ => {
                    set_last_error("node is not a scalar");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Write a scalar node's float value to `*out_value`. Fails with `TypeMismatch` if `node` isn't
/// a `Float` scalar.
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; `out_value` must be null or a valid, aligned, writable pointer to
/// an `f64`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_scalar_as_float(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_value: *mut f64,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Scalar(scalar) => match &scalar.value {
                    ScalarValue::Float(value) => write_out(out_value, *value),
                    _ => {
                        set_last_error("scalar is not a float");
                        TptYamlErrorCode::TypeMismatch
                    }
                },
                _ => {
                    set_last_error("node is not a scalar");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Write a borrowed pointer/length pair for a scalar node's string value (covers both `String`
/// and `Timestamp` scalars, matching [`tpt_yaml_core::ScalarValue::as_str`]) to
/// `*out_ptr`/`*out_len`. Fails with `TypeMismatch` for any other scalar type or non-scalar node.
///
/// The returned pointer is **not** null-terminated and is borrowed from the document: it is
/// valid only until `doc` is freed via [`tpt_yaml_document_free`], and must not be freed by the
/// caller.
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; `out_ptr` must be null or a valid, aligned, writable pointer to a
/// `*const u8`; `out_len` must be null or a valid, aligned, writable pointer to a `usize`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_scalar_string(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_ptr: *mut *const c_char,
    out_len: *mut usize,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            let text = match &node.kind {
                NodeKind::Scalar(scalar) => match scalar.value.as_str() {
                    Some(text) => text,
                    None => {
                        set_last_error("scalar is not a string or timestamp");
                        return TptYamlErrorCode::TypeMismatch;
                    }
                },
                _ => {
                    set_last_error("node is not a scalar");
                    return TptYamlErrorCode::TypeMismatch;
                }
            };
            let code = write_out(out_ptr, text.as_ptr().cast::<c_char>());
            if code != TptYamlErrorCode::Ok {
                return code;
            }
            write_out(out_len, text.len())
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Mapping queries
// ---------------------------------------------------------------------------------------------

/// Write a mapping node's entry count to `*out_len`. Fails with `TypeMismatch` if `node` isn't a
/// mapping.
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; `out_len` must be null or a valid, aligned, writable pointer to a
/// `usize`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_mapping_len(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_len: *mut usize,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Mapping(entries) => write_out(out_len, entries.len()),
                _ => {
                    set_last_error("node is not a mapping");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Write the key node id of a mapping's `index`-th entry to `*out_node`. Fails with
/// `TypeMismatch` if `node` isn't a mapping, or `IndexOutOfBounds` if `index >= ` its length.
///
/// # Safety
/// See [`tpt_yaml_node_kind`].
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_mapping_key_at(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    index: usize,
    out_node: *mut TptYamlNodeId,
) -> TptYamlErrorCode {
    mapping_entry_at(doc, node, index, out_node, |(key, _)| *key)
}

/// Write the value node id of a mapping's `index`-th entry to `*out_node`. Fails with
/// `TypeMismatch` if `node` isn't a mapping, or `IndexOutOfBounds` if `index >= ` its length.
///
/// # Safety
/// See [`tpt_yaml_node_kind`].
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_mapping_value_at(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    index: usize,
    out_node: *mut TptYamlNodeId,
) -> TptYamlErrorCode {
    mapping_entry_at(doc, node, index, out_node, |(_, value)| *value)
}

unsafe fn mapping_entry_at(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    index: usize,
    out_node: *mut TptYamlNodeId,
    pick: impl FnOnce(&(tpt_yaml_core::NodeId, tpt_yaml_core::NodeId)) -> tpt_yaml_core::NodeId
        + UnwindSafe,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Mapping(entries) => match entries.get(index) {
                    Some(entry) => write_out(out_node, node_id_to_ffi(pick(entry))),
                    None => {
                        set_last_error(format!(
                            "mapping index {index} is out of bounds ({} entries)",
                            entries.len()
                        ));
                        TptYamlErrorCode::IndexOutOfBounds
                    }
                },
                _ => {
                    set_last_error("node is not a mapping");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Look up a mapping entry by a plain string key (matched against scalar keys' resolved string
/// value — i.e. an unquoted or quoted plain string key, not e.g. an int or nested-collection
/// key), writing the matching entry's value node id to `*out_node`.
///
/// Fails with `TypeMismatch` if `node` isn't a mapping, or `IndexOutOfBounds` if no entry has a
/// matching string key (reused here as "lookup failed" rather than adding a dedicated
/// not-found code).
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; additionally, `key_ptr` must be null (only if `key_len` is `0`)
/// or point to at least `key_len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_mapping_get(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    key_ptr: *const u8,
    key_len: usize,
    out_node: *mut TptYamlNodeId,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            if key_ptr.is_null() && key_len != 0 {
                set_last_error("null key pointer with nonzero length");
                return TptYamlErrorCode::NullPointer;
            }
            let key_bytes =
                if key_len == 0 { &[] } else { slice::from_raw_parts(key_ptr, key_len) };
            let key = match std::str::from_utf8(key_bytes) {
                Ok(key) => key,
                Err(err) => {
                    set_last_error(format!("key is not valid UTF-8: {err}"));
                    return TptYamlErrorCode::InvalidUtf8;
                }
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            let entries = match &node.kind {
                NodeKind::Mapping(entries) => entries,
                _ => {
                    set_last_error("node is not a mapping");
                    return TptYamlErrorCode::TypeMismatch;
                }
            };
            for (key_id, value_id) in entries {
                let matches = document.node(*key_id).is_some_and(|key_node| match &key_node.kind {
                    NodeKind::Scalar(scalar) => scalar.value.as_str() == Some(key),
                    _ => false,
                });
                if matches {
                    return write_out(out_node, node_id_to_ffi(*value_id));
                }
            }
            set_last_error(format!("no mapping entry with key {key:?}"));
            TptYamlErrorCode::IndexOutOfBounds
        },
    )
}

// ---------------------------------------------------------------------------------------------
// Sequence queries
// ---------------------------------------------------------------------------------------------

/// Write a sequence node's item count to `*out_len`. Fails with `TypeMismatch` if `node` isn't a
/// sequence.
///
/// # Safety
/// See [`tpt_yaml_node_kind`]; `out_len` must be null or a valid, aligned, writable pointer to a
/// `usize`.
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_sequence_len(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    out_len: *mut usize,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Sequence(items) => write_out(out_len, items.len()),
                _ => {
                    set_last_error("node is not a sequence");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

/// Write the node id of a sequence's `index`-th item to `*out_node`. Fails with `TypeMismatch`
/// if `node` isn't a sequence, or `IndexOutOfBounds` if `index >= ` its length.
///
/// # Safety
/// See [`tpt_yaml_node_kind`].
#[no_mangle]
pub unsafe extern "C" fn tpt_yaml_sequence_item(
    doc: *const TptYamlDocument,
    node: TptYamlNodeId,
    index: usize,
    out_node: *mut TptYamlNodeId,
) -> TptYamlErrorCode {
    guard(
        || TptYamlErrorCode::PanicCaught,
        move || unsafe {
            let document = match deref_doc(doc) {
                Ok(document) => document,
                Err(code) => return code,
            };
            let node = match get_node(document, node) {
                Ok(node) => node,
                Err(code) => return code,
            };
            match &node.kind {
                NodeKind::Sequence(items) => match items.get(index) {
                    Some(item) => write_out(out_node, node_id_to_ffi(*item)),
                    None => {
                        set_last_error(format!(
                            "sequence index {index} is out of bounds ({} items)",
                            items.len()
                        ));
                        TptYamlErrorCode::IndexOutOfBounds
                    }
                },
                _ => {
                    set_last_error("node is not a sequence");
                    TptYamlErrorCode::TypeMismatch
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    fn parse_ok(source: &str) -> *mut TptYamlDocument {
        let mut error = TptYamlErrorCode::Ok;
        let doc = unsafe { tpt_yaml_parse(source.as_ptr(), source.len(), &mut error) };
        assert_eq!(error, TptYamlErrorCode::Ok);
        assert!(!doc.is_null());
        doc
    }

    #[test]
    fn parses_and_queries_a_simple_mapping() {
        let doc = parse_ok("name: Ada\ncount: 3\nok: true\n");

        let mut root = 0u32;
        assert_eq!(unsafe { tpt_yaml_document_root(doc, &mut root) }, TptYamlErrorCode::Ok);

        let mut kind = TptYamlNodeKind::Scalar;
        assert_eq!(unsafe { tpt_yaml_node_kind(doc, root, &mut kind) }, TptYamlErrorCode::Ok);
        assert_eq!(kind, TptYamlNodeKind::Mapping);

        let mut len = 0usize;
        assert_eq!(unsafe { tpt_yaml_mapping_len(doc, root, &mut len) }, TptYamlErrorCode::Ok);
        assert_eq!(len, 3);

        let key = "name";
        let mut value_node = 0u32;
        assert_eq!(
            unsafe { tpt_yaml_mapping_get(doc, root, key.as_ptr(), key.len(), &mut value_node) },
            TptYamlErrorCode::Ok
        );
        let mut ptr_out: *const c_char = ptr::null();
        let mut len_out = 0usize;
        assert_eq!(
            unsafe { tpt_yaml_scalar_string(doc, value_node, &mut ptr_out, &mut len_out) },
            TptYamlErrorCode::Ok
        );
        let bytes = unsafe { slice::from_raw_parts(ptr_out.cast::<u8>(), len_out) };
        assert_eq!(bytes, b"Ada");

        let key = "count";
        let mut value_node = 0u32;
        assert_eq!(
            unsafe { tpt_yaml_mapping_get(doc, root, key.as_ptr(), key.len(), &mut value_node) },
            TptYamlErrorCode::Ok
        );
        let mut int_out = 0i64;
        assert_eq!(
            unsafe { tpt_yaml_scalar_as_int(doc, value_node, &mut int_out) },
            TptYamlErrorCode::Ok
        );
        assert_eq!(int_out, 3);

        let key = "ok";
        let mut value_node = 0u32;
        assert_eq!(
            unsafe { tpt_yaml_mapping_get(doc, root, key.as_ptr(), key.len(), &mut value_node) },
            TptYamlErrorCode::Ok
        );
        let mut bool_out = false;
        assert_eq!(
            unsafe { tpt_yaml_scalar_as_bool(doc, value_node, &mut bool_out) },
            TptYamlErrorCode::Ok
        );
        assert!(bool_out);

        unsafe { tpt_yaml_document_free(doc) };
    }

    #[test]
    fn parses_and_walks_a_sequence() {
        let doc = parse_ok("- 1\n- 2\n- 3\n");
        let mut root = 0u32;
        assert_eq!(unsafe { tpt_yaml_document_root(doc, &mut root) }, TptYamlErrorCode::Ok);

        let mut len = 0usize;
        assert_eq!(unsafe { tpt_yaml_sequence_len(doc, root, &mut len) }, TptYamlErrorCode::Ok);
        assert_eq!(len, 3);

        let mut item = 0u32;
        assert_eq!(
            unsafe { tpt_yaml_sequence_item(doc, root, 1, &mut item) },
            TptYamlErrorCode::Ok
        );
        let mut value = 0i64;
        assert_eq!(unsafe { tpt_yaml_scalar_as_int(doc, item, &mut value) }, TptYamlErrorCode::Ok);
        assert_eq!(value, 2);

        assert_eq!(
            unsafe { tpt_yaml_sequence_item(doc, root, 99, &mut item) },
            TptYamlErrorCode::IndexOutOfBounds
        );

        unsafe { tpt_yaml_document_free(doc) };
    }

    #[test]
    fn null_pointer_arguments_are_rejected() {
        let mut error = TptYamlErrorCode::Ok;
        let doc = unsafe { tpt_yaml_parse(std::ptr::null(), 5, &mut error) };
        assert!(doc.is_null());
        assert_eq!(error, TptYamlErrorCode::NullPointer);

        let mut root = 0u32;
        assert_eq!(
            unsafe { tpt_yaml_document_root(ptr::null(), &mut root) },
            TptYamlErrorCode::NullPointer
        );

        // A null document handle everywhere else should also come back `NullPointer`, never a
        // segfault.
        let mut kind = TptYamlNodeKind::Scalar;
        assert_eq!(
            unsafe { tpt_yaml_node_kind(ptr::null(), 0, &mut kind) },
            TptYamlErrorCode::NullPointer
        );

        // `tpt_yaml_document_free(null)` must be a safe no-op.
        unsafe { tpt_yaml_document_free(ptr::null_mut()) };
    }

    #[test]
    fn malformed_utf8_is_rejected() {
        let bytes: [u8; 3] = [0xff, 0xfe, 0xfd];
        let mut error = TptYamlErrorCode::Ok;
        let doc = unsafe { tpt_yaml_parse(bytes.as_ptr(), bytes.len(), &mut error) };
        assert!(doc.is_null());
        assert_eq!(error, TptYamlErrorCode::InvalidUtf8);
    }

    #[test]
    fn malformed_yaml_reports_parse_error_and_message() {
        // An unterminated flow mapping is a real parse error, not just an unusual-but-valid
        // shape.
        let source = "key: [1, 2";
        let mut error = TptYamlErrorCode::Ok;
        let doc = unsafe { tpt_yaml_parse(source.as_ptr(), source.len(), &mut error) };
        assert!(doc.is_null());
        assert_eq!(error, TptYamlErrorCode::ParseError);
        let message = tpt_yaml_last_error_message();
        assert!(!message.is_null());
    }

    #[test]
    fn out_of_bounds_node_id_is_reported() {
        let doc = parse_ok("a: 1\n");
        let mut kind = TptYamlNodeKind::Scalar;
        assert_eq!(
            unsafe { tpt_yaml_node_kind(doc, 9_999, &mut kind) },
            TptYamlErrorCode::IndexOutOfBounds
        );
        unsafe { tpt_yaml_document_free(doc) };
    }

    #[test]
    fn type_mismatch_is_reported_for_wrong_accessor() {
        let doc = parse_ok("42\n");
        let mut root = 0u32;
        assert_eq!(unsafe { tpt_yaml_document_root(doc, &mut root) }, TptYamlErrorCode::Ok);
        let mut len = 0usize;
        assert_eq!(
            unsafe { tpt_yaml_mapping_len(doc, root, &mut len) },
            TptYamlErrorCode::TypeMismatch
        );
        let mut bool_out = false;
        assert_eq!(
            unsafe { tpt_yaml_scalar_as_bool(doc, root, &mut bool_out) },
            TptYamlErrorCode::TypeMismatch
        );
        unsafe { tpt_yaml_document_free(doc) };
    }
}
