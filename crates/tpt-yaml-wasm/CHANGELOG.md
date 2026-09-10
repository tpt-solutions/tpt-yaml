# Changelog

All notable changes to `tpt-yaml-wasm` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io or npm yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- `parse(source): unknown` — parse YAML into a native JS value.
- `toJson(source): string` — parse YAML and re-serialize as a JSON string.
- `stringify(value): string` — convert a JS value into a YAML string.
- `wasm-bindgen`/`serde-wasm-bindgen` marshaling directly from `tpt-yaml-serde::Value`, without
  routing through `tpt-yaml-ffi`'s C ABI.
- Errors surfaced as JS `Error` objects (rejected promises in async contexts) with descriptive
  messages.

### Known limitations

- Anchors/aliases collapse to independent copies of their anchor's value — JavaScript has no
  built-in equivalent of a YAML anchor/alias pair.
- `stringify` always renders through `tpt-yaml-core`'s pretty-printer, so comments, original
  scalar style, and key-ordering quirks from hand-written source are not preserved.
- `NaN`/`±Infinity` floats don't round-trip.
- Explicit non-`!!str` tags are dropped: `Value`'s `Serialize` impl serializes only the inner
  value.
- Packaged (`wasm-pack build`) output has not been built/tested in this native Windows
  development environment; only `cargo build`/`cargo check --target wasm32-unknown-unknown` have
  been verified.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-wasm
