# tpt-yaml

A dependency-light YAML toolkit for Rust, split into small crates so you only
pay for what you use. No dependency on any `tpt-io-*` crate — this family is
standalone.

| Crate | Status | Description |
| --- | --- | --- |
| [`tpt-yaml-core`](crates/tpt-yaml-core) | implemented, unreleased | Lexer, parser, arena-based node model, event-driven streaming parser, `no_std` + `alloc` + `std`, zero external dependencies |
| [`tpt-yaml-serde`](crates/tpt-yaml-serde) | implemented, unreleased | `serde` `Deserializer`/`Serializer` over `tpt-yaml-core`'s arena, a dynamic `Value` type, and a constant-memory streaming `Deserializer` |
| [`tpt-yaml-edit`](crates/tpt-yaml-edit) | implemented, unreleased | Lossless, span-preserving edits: change a value, re-render only the touched subtree |
| [`tpt-yaml-schema`](crates/tpt-yaml-schema) | implemented, unreleased | JSON Schema (2020-12 subset) validation for parsed YAML documents |
| [`tpt-yaml-cli`](crates/tpt-yaml-cli) | implemented, unreleased | `tpt-yaml` binary: `check`, `fmt`, `convert`, `diff` subcommands |
| [`tpt-yaml-ffi`](crates/tpt-yaml-ffi) | implemented, unreleased | C ABI foundation for language bindings |
| [`tpt-yaml-python`](crates/tpt-yaml-python) | implemented, unreleased | `pyo3` bindings: `loads`/`dumps`/`TptYamlError`, verified with `maturin develop` |
| [`tpt-yaml-wasm`](crates/tpt-yaml-wasm) | implemented, unreleased | `wasm-bindgen` bindings for the browser/Node.js: `parse`/`toJson`/`stringify` |

Each crate has its own `README.md` (usage, design, known limitations) and `CHANGELOG.md`.
None have been published to crates.io/PyPI/npm yet — see [todo.md](todo.md) §7 for
release-readiness status and publish order.

## Quick start

```rust
let doc = tpt_yaml_core::parse("key: value\nlist:\n  - a\n  - b\n")?;
let root = doc.root().unwrap();
```

## Status

This is pre-1.0, under active development. See [todo.md](todo.md) for the
detailed build checklist and current status snapshot.

## Publish order

Crates depend on each other in this order, so that's the order they'll be
published in:

1. `tpt-yaml-core`
2. `tpt-yaml-serde`
3. `tpt-yaml-edit`
4. `tpt-yaml-schema`
5. `tpt-yaml-cli`
6. `tpt-yaml-ffi`

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
