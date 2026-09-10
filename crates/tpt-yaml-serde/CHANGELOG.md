# Changelog

All notable changes to `tpt-yaml-serde` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- `Deserializer` borrowing a parsed `tpt_yaml_core::Document` directly, plus `from_str`/
  `from_slice` convenience wrappers for `T: DeserializeOwned`.
- `Serializer`, building a fresh `tpt_yaml_core::Document` and rendering it through
  `tpt-yaml-core`'s shared pretty-printer; `to_string` convenience wrapper.
- `Value`, a dynamic type for documents without a fixed shape, produced via `Value::from_node`
  or by deserializing through this crate's own `Deserializer`.
- Enum variant serialization matching `serde_yaml`/`serde_json` convention: unit variants as a
  plain string, newtype/tuple/struct variants as a single-entry mapping.
- `no_std` + `alloc` + `std` (default) feature ladder, forwarding to `tpt-yaml-core`'s.

### Known limitations

- `from_str`/`from_slice` only deserialize the first document of a multi-document stream.
- The `streaming` feature (constant-memory decode via `Deserializer::from_events`) is not yet
  implemented.
- `f64::NAN`/`f64::INFINITY` don't round-trip through `to_string`/`from_str`.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-serde
