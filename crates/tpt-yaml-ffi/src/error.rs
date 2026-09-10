//! Error codes and thread-local last-error message plumbing for the C ABI.
//!
//! Every `extern "C"` function in this crate returns (or writes via an out-parameter) a
//! [`TptYamlErrorCode`]. On any non-`Ok` code, a human-readable message describing the failure
//! is stashed in a thread-local slot retrievable via [`tpt_yaml_last_error_message`] — callers
//! that don't care about the message can ignore it entirely.

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::c_char;
use std::ptr;

/// Result/status code returned by every `extern "C"` function in this crate.
///
/// `Ok` is always `0` so a caller can treat any nonzero return as failure without inspecting
/// the exact variant if they don't need to.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TptYamlErrorCode {
    /// The call succeeded.
    Ok = 0,
    /// `tpt_yaml_parse` failed to parse the given source as YAML. See
    /// [`tpt_yaml_last_error_message`] for the underlying [`tpt_yaml_core::YamlError`]'s
    /// rendered message.
    ParseError = 1,
    /// A required pointer argument was null.
    NullPointer = 2,
    /// A byte slice argument was not valid UTF-8.
    InvalidUtf8 = 3,
    /// A node id, mapping index, or sequence index was out of bounds for the document.
    IndexOutOfBounds = 4,
    /// The requested operation doesn't apply to the node's actual kind (e.g. asking for the
    /// sequence length of a scalar node).
    TypeMismatch = 5,
    /// A Rust panic was caught at the FFI boundary and converted into this error code rather
    /// than being allowed to unwind across it (which is undefined behavior). This indicates a
    /// bug in this crate or in `tpt-yaml-core` — please report it.
    PanicCaught = 6,
}

thread_local! {
    static LAST_ERROR: RefCell<Option<CString>> = const { RefCell::new(None) };
}

/// Store `message` as the current thread's last-error message, replacing any previous one.
pub(crate) fn set_last_error(message: impl Into<Vec<u8>>) {
    // A `CString::new` failure means `message` contained an interior NUL byte; fall back to a
    // fixed message rather than silently dropping the error entirely.
    let message =
        CString::new(message).unwrap_or_else(|_| CString::new("<error message contained NUL>").expect("no NUL"));
    LAST_ERROR.with(|slot| *slot.borrow_mut() = Some(message));
}

pub(crate) fn clear_last_error() {
    LAST_ERROR.with(|slot| *slot.borrow_mut() = None);
}

/// Returns a pointer to the current thread's last-error message, or null if none is set (either
/// no error has occurred yet on this thread, or the most recent call succeeded).
///
/// # Safety / lifetime contract
///
/// The returned pointer is owned by this crate's internal thread-local storage. It is valid
/// only until the next call into any `tpt_yaml_*` function *on the same thread*, which may
/// overwrite or clear it — copy the string out (e.g. with `strdup`) before making another call
/// if you need it to outlive that. Never call `free`/`tpt_yaml_string_free` on it.
#[no_mangle]
pub extern "C" fn tpt_yaml_last_error_message() -> *const c_char {
    let result = std::panic::catch_unwind(|| {
        LAST_ERROR.with(|slot| match slot.borrow().as_ref() {
            Some(message) => message.as_ptr(),
            None => ptr::null(),
        })
    });
    result.unwrap_or(ptr::null())
}

/// Run `f`, catching any Rust panic and converting it into `PanicCaught` + a last-error message
/// rather than letting it unwind across the FFI boundary (which is undefined behavior). `on_err`
/// builds the sentinel return value for the panic case (e.g. `null`, `-1`, or
/// `TptYamlErrorCode::PanicCaught` itself).
pub(crate) fn guard<R>(on_err: impl FnOnce() -> R, f: impl FnOnce() -> R + std::panic::UnwindSafe) -> R {
    match std::panic::catch_unwind(f) {
        Ok(value) => value,
        Err(payload) => {
            let message = panic_message(&payload);
            set_last_error(format!("panic caught at FFI boundary: {message}"));
            on_err()
        }
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}
