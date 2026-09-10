#![no_main]

use libfuzzer_sys::fuzz_target;
use tpt_yaml_ffi::{
    tpt_yaml_document_free, tpt_yaml_document_root, tpt_yaml_mapping_get, tpt_yaml_mapping_len,
    tpt_yaml_node_kind, tpt_yaml_scalar_as_bool, tpt_yaml_scalar_as_float, tpt_yaml_scalar_as_int,
    tpt_yaml_scalar_string, tpt_yaml_scalar_type, tpt_yaml_sequence_item, tpt_yaml_sequence_len,
    tpt_yaml_parse, TptYamlErrorCode, TptYamlNodeKind,
};

// Drives the C ABI surface directly (not `tpt_yaml_core`'s Rust API) over arbitrary bytes: the
// interesting failure mode here isn't a parse error (those are expected and fine) but any
// panic escaping `catch_unwind`, any out-of-bounds read through the raw-pointer accessors, or a
// use-after-free/double-free in this crate's own glue code.
fuzz_target!(|data: &[u8]| {
    let mut error = TptYamlErrorCode::Ok;
    let doc = unsafe { tpt_yaml_parse(data.as_ptr(), data.len(), &mut error) };
    if doc.is_null() {
        // Parsing arbitrary bytes commonly fails (ParseError/InvalidUtf8) — that's expected and
        // not a bug; just make sure no document handle needs freeing.
        return;
    }

    let mut root = 0u32;
    if unsafe { tpt_yaml_document_root(doc, &mut root) } == TptYamlErrorCode::Ok {
        let mut kind = TptYamlNodeKind::Scalar;
        if unsafe { tpt_yaml_node_kind(doc, root, &mut kind) } == TptYamlErrorCode::Ok {
            match kind {
                TptYamlNodeKind::Scalar => {
                    let mut scalar_type = tpt_yaml_ffi::TptYamlScalarType::Null;
                    let _ = unsafe { tpt_yaml_scalar_type(doc, root, &mut scalar_type) };
                    let mut b = false;
                    let _ = unsafe { tpt_yaml_scalar_as_bool(doc, root, &mut b) };
                    let mut i = 0i64;
                    let _ = unsafe { tpt_yaml_scalar_as_int(doc, root, &mut i) };
                    let mut f = 0f64;
                    let _ = unsafe { tpt_yaml_scalar_as_float(doc, root, &mut f) };
                    let mut ptr = std::ptr::null();
                    let mut len = 0usize;
                    let _ = unsafe { tpt_yaml_scalar_string(doc, root, &mut ptr, &mut len) };
                }
                TptYamlNodeKind::Mapping => {
                    let mut len = 0usize;
                    if unsafe { tpt_yaml_mapping_len(doc, root, &mut len) } == TptYamlErrorCode::Ok
                    {
                        // Look up a key derived from the fuzz input itself, and one guaranteed
                        // to be absent, to exercise both the found and not-found paths.
                        let key = &data[..data.len().min(4)];
                        let mut out_node = 0u32;
                        let _ = unsafe {
                            tpt_yaml_mapping_get(doc, root, key.as_ptr(), key.len(), &mut out_node)
                        };
                        let _ = unsafe {
                            tpt_yaml_mapping_get(
                                doc,
                                root,
                                b"__definitely_absent_key__".as_ptr(),
                                26,
                                &mut out_node,
                            )
                        };
                    }
                }
                TptYamlNodeKind::Sequence => {
                    let mut len = 0usize;
                    if unsafe { tpt_yaml_sequence_len(doc, root, &mut len) } == TptYamlErrorCode::Ok
                    {
                        let mut item = 0u32;
                        // Index one past the end deliberately, to exercise the bounds check.
                        let _ = unsafe { tpt_yaml_sequence_item(doc, root, len, &mut item) };
                    }
                }
                TptYamlNodeKind::Alias => {}
            }
        }
    }

    // Also probe a node id that's almost certainly out of bounds for this document, to exercise
    // the arena bounds check on an arbitrary handle.
    let mut kind = TptYamlNodeKind::Scalar;
    let _ = unsafe { tpt_yaml_node_kind(doc, u32::MAX, &mut kind) };

    unsafe { tpt_yaml_document_free(doc) };
});
