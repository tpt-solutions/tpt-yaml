# Changelog

All notable changes to `tpt-yaml-ffi` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- C ABI surface over `tpt-yaml-core`'s parser and node arena: `tpt_yaml_parse`/
  `tpt_yaml_document_free`, document/node introspection (root, version, structural kind),
  mapping/sequence walking, scalar reads, and alias following.
- Per-call `TptYamlErrorCode` results, plus `tpt_yaml_last_error_message()` for a
  human-readable message on the calling thread.
- `build.rs`-driven header generation via `cbindgen`, writing and checking in
  `include/tpt_yaml.h`.
- Panic safety: every `extern "C"` function body runs inside `std::panic::catch_unwind`,
  converting any caught panic into `TPT_YAML_ERROR_CODE_PANIC_CAUGHT`.
- Null-pointer checks on every function taking a pointer argument, returning
  `TPT_YAML_ERROR_CODE_NULL_POINTER` rather than dereferencing.
- `typed`/`edit` Cargo features reserving `tpt-yaml-serde`/`tpt-yaml-edit` as optional
  dependencies for a future FFI surface (not yet exposed by any function).

### Known limitations

- No `extern "C"` function yet uses the `typed`/`edit` features.
- No streaming/incremental parse API.
- `tpt_yaml_mapping_get`'s "no matching key" case reuses `IndexOutOfBounds` rather than a
  dedicated not-found code.
- No CI job regenerates and diffs `include/tpt_yaml.h` against the checked-in copy yet.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-ffi
