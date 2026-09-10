# tpt-yaml

A dependency-light YAML toolkit for Rust, split into small crates so you only
pay for what you use. No dependency on any `tpt-io-*` crate — this family is
standalone.

| Crate | Status | Description |
| --- | --- | --- |
| [`tpt-yaml-core`](crates/tpt-yaml-core) | in progress | Lexer, parser, arena-based node model, `no_std` + `alloc` + `std`, zero external dependencies |
| [`tpt-yaml-serde`](crates/tpt-yaml-serde) | stub | `serde` `Deserializer`/`Serializer` over `tpt-yaml-core`'s arena, plus a dynamic `Value` type |
| [`tpt-yaml-edit`](crates/tpt-yaml-edit) | stub | Lossless, span-preserving edits: change a value, re-render only the touched subtree |
| [`tpt-yaml-schema`](crates/tpt-yaml-schema) | stub | JSON Schema (2020-12 subset) validation for parsed YAML documents |
| [`tpt-yaml-cli`](crates/tpt-yaml-cli) | stub | `tpt-yaml` binary: `check`, `fmt`, `convert`, `diff` subcommands |
| [`tpt-yaml-ffi`](crates/tpt-yaml-ffi) | in progress | C ABI foundation for language bindings |
| [`tpt-yaml-wasm`](crates/tpt-yaml-wasm) | stub | `wasm-bindgen` bindings for the browser/Node.js |
| [`tpt-yaml-python`](crates/tpt-yaml-python) | not yet implemented | Planned `pyo3` bindings (scaffold only) |

Each crate has its own `README.md` (usage, design, known limitations) and `CHANGELOG.md`
(unreleased so far — nothing has been published).

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
