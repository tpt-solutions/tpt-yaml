//! `wasm-bindgen` bindings for the [`tpt-yaml`](..) family: exposes [`parse`], [`to_json`], and
//! [`stringify`] for use from JavaScript/TypeScript in the browser or Node.
//!
//! This crate is a thin marshaling layer only — all real parsing/rendering logic lives in
//! `tpt-yaml-core` (the parser/pretty-printer) and `tpt-yaml-serde` (the dynamic [`Value`] type
//! and its `Serialize`/`Deserialize` impls). It does not route through `tpt-yaml-ffi`'s C ABI:
//! `wasm-bindgen` has its own JS-value marshaling, so this crate depends directly on
//! `tpt-yaml-core`/`tpt-yaml-serde` and converts `Value` to/from `JsValue` via
//! `serde-wasm-bindgen`, which works because `Value` already has a hand-written
//! `Serialize`/`Deserialize` impl (see `tpt-yaml-serde`'s README).
//!
//! Like `tpt-yaml-ffi` and `tpt-yaml-cli`, this is a foreign-interface bindings crate with no
//! reason to run in `no_std` environments, so it depends on `tpt-yaml-core`/`tpt-yaml-serde`
//! with their default (`std`) features rather than carrying a `no_std` ladder of its own.
//!
//! ## Anchors/aliases
//!
//! `tpt_yaml_serde::Value::from_node` (used internally here) already resolves aliases to their
//! target's plain value while converting a parsed `Document` into a `Value` — there is no
//! separate resolution step in this crate. JavaScript has no reference-preserving equivalent of
//! a YAML anchor/alias pair short of building one, so an aliased node round-trips as a plain
//! *copy* of its anchor's value, not a shared reference. See the crate README for more detail.

use tpt_yaml_serde::Value;
use wasm_bindgen::prelude::*;

/// Parses `source` as YAML and returns the first document as a native JavaScript value
/// (object/array/string/number/boolean/`null`).
///
/// Anchors/aliases resolve to plain copies of their target's value (see the crate-level docs).
/// On a parse error, returns a rejected `Err` carrying a JS `Error` with the parser's message.
#[wasm_bindgen]
pub fn parse(source: &str) -> Result<JsValue, JsValue> {
    let value = parse_to_value(source)?;
    serde_wasm_bindgen::to_value(&value).map_err(|err| js_error(&err.to_string()))
}

/// Parses `source` as YAML and re-serializes it as a JSON string.
///
/// This is the `to_json`-equivalent named by the JS/WASM bindings task: useful when the caller
/// wants a JSON string directly (e.g. to hand to `JSON.parse` themselves, or to send over the
/// wire) rather than a live `JsValue`.
#[wasm_bindgen(js_name = toJson)]
pub fn to_json(source: &str) -> Result<String, JsValue> {
    let value = parse_to_value(source)?;
    serde_json::to_string(&value).map_err(|err| js_error(&err.to_string()))
}

/// Converts a native JavaScript value into a YAML string.
///
/// This is the natural complement to [`parse`]: it accepts anything `serde-wasm-bindgen` can
/// decode into `tpt_yaml_serde::Value` (objects, arrays, strings, numbers, booleans, `null`/
/// `undefined`) and renders it via `tpt-yaml-serde`'s `Serializer`/pretty-printer, the same path
/// `tpt_yaml_serde::to_string` uses for any `T: Serialize`.
#[wasm_bindgen]
pub fn stringify(value: JsValue) -> Result<String, JsValue> {
    let value: Value =
        serde_wasm_bindgen::from_value(value).map_err(|err| js_error(&err.to_string()))?;
    tpt_yaml_serde::to_string(&value).map_err(|err| js_error(&err.to_string()))
}

/// Shared parse step for [`parse`] and [`to_json`]: parse via `tpt_yaml_core::parse`, then
/// convert the root node into a `Value` via the "direct conversion boundary"
/// (`Value::from_node`) per `tpt-yaml-serde`'s README.
fn parse_to_value(source: &str) -> Result<Value, JsValue> {
    let document = tpt_yaml_core::parse(source).map_err(|err| js_error(&err.to_string()))?;
    let root = document.root().ok_or_else(|| js_error("document has no root node"))?;
    Ok(Value::from_node(&document, root))
}

fn js_error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    // Real behavioral tests for this crate need a `wasm32` target + a JS engine
    // (`wasm-bindgen-test` + `wasm-pack test`), which this native dev environment can't run.
    // See the crate README for how to run them once `wasm-pack`/a browser or Node runtime is
    // available.
}
