# tpt-yaml-core

The foundation crate for the [`tpt-yaml`](..) family: a lexer, a column/indentation-aware
recursive-descent parser, and an arena-based node model for YAML 1.1/1.2. Zero external
dependencies, `no_std` + `alloc` + `std` (default).

## Quick start

```rust
let doc = tpt_yaml_core::parse("key: value\nlist:\n  - a\n  - b\n")?;
let root = doc.root().unwrap();
assert_eq!(doc.version(), tpt_yaml_core::YamlVersion::Version12);
# Ok::<(), tpt_yaml_core::YamlError>(())
```

`parse` returns a [`Document`]: an arena of [`NodeData`] indexed by [`NodeId`]. Look up a node
via [`Document::node`], its source span via [`Document::span`], and re-render a synthesized
subtree via [`pretty_print`].

## What's supported

- **Block and flow styles**: implicit and explicit (`?`-key) block mappings, block sequences,
  flow mappings/sequences, and the "a sequence value may align with its mapping key" exception.
- **Multi-document streams**: bare first document plus `---`-separated subsequent ones, `...`
  end markers, and a `TrailingContent` error for stray content without a separator.
- **Anchors and aliases** (`&name` / `*name`), with alias-cycle detection that's structural (an
  anchor is only registered once its value is fully parsed, so a cycle can't be constructed).
- **Merge keys** (`<<`) via `ParserOptions::merge_keys` (default `true`): explicit keys win over
  merged ones, earlier merge sources win over later ones, and sequence-of-mappings merge values
  are supported.
- **Comment and blank-line trivia**, attached to the nodes they surround.
- **Source spans** on every node the parser creates, for editor/diagnostic tooling built on top
  (see [`tpt-yaml-edit`](../tpt-yaml-edit) and [`tpt-yaml-schema`](../tpt-yaml-schema)).
- **YAML 1.1 vs. 1.2 implicit-scalar resolution** ([`YamlVersion`], via `resolve.rs`): the
  "Norway problem" boolean table (1.1 `y`/`n`/`on`/`off`/... vs. 1.2 `true`/`false` only), octal
  sigils (1.1 bare `0755` vs. 1.2 `0o755`), and 1.1-only sexagesimal ints/floats
  (`1:20:30` → `Int(4830)`). [`ParserOptions::yaml_version`] forces a version; leaving it `None`
  auto-detects from a `%YAML` directive, defaulting to 1.2.

## Design

- **Arena, not a tree of `Box`es.** [`Document`] owns a flat `Vec<NodeData>`; every reference
  between nodes ([`NodeId`]) is a plain arena index. This is what lets
  [`tpt-yaml-edit`](../tpt-yaml-edit) splice in synthesized nodes without an owned/borrowed
  lifetime tangle, and what lets [`tpt-yaml-ffi`](../tpt-yaml-ffi) expose nodes as opaque
  `uint32_t` handles across a C ABI.
- **Local diagnostics types.** [`YamlError`]/[`ErrorContext`]/[`ErrorKind`] are modeled on
  `tpt-io-standards`'s shape for familiarity but owned in this crate — this repo has no
  dependency on any `tpt-io-*` crate (see the workspace `AGENTS.md`).
- **`no_std` ladder.** `default = ["std"]`; `alloc` builds without `std` (`extern crate alloc`)
  for `no_std + alloc` targets. This is the only crate in the family with zero dependencies of
  its own — every other crate depends on it with `default-features = false, features =
  ["alloc"]` and forwards its own `std`/`alloc` features to match.

## Known limitations

- `ParserOptions::strict_version` is a reserved field with no behavior yet: `document.version()`
  reports the resolved version (honoring a `%YAML` directive), but there is no ambiguity
  diagnostic when a document mixes version-specific syntax without a directive.
- The `yaml-test-suite` conformance harness (`tests/conformance.rs`) exists but is `#[ignore]`d
  and gated on a gitignored corpus directory — it has not yet been run against the real upstream
  corpus in this environment.
- Golden fixture tests (`tests/golden.rs`) assert every sample under `tests/samples/*.yaml`
  parses and re-renders without panicking; they are a smoke test, not a byte-exact
  golden-output check against a second independent YAML implementation.
