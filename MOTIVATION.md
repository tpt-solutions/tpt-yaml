# Why tpt-yaml exists

## The gap it fills

The Rust YAML ecosystem has historically been dominated by two things:

- **`serde_yaml`** — the crate most Rust projects reached for. It's
  [deprecated by its maintainer](https://github.com/dtolnay/serde-yaml) with
  no successor named, leaving a large, active userbase looking for
  somewhere to land.
- **`yaml-rust`/`yaml-rust2`** — a parser producing a single dynamic `Yaml`
  enum. Useful for reading a document as a generic value, but it isn't a
  `serde` backend, doesn't support editing without destroying formatting,
  and doesn't validate.

Neither covers what a real YAML workflow tends to need beyond "parse it
into a struct": preserving a human's formatting/comments when a tool edits
a config file, validating structure before acting on it, or embedding a
parser in a `no_std` environment. Projects that need those things end up
pulling in several unrelated crates (a parser, a schema validator ported
from JSON tooling, a hand-rolled comment-preserving editor) that weren't
designed to share a data model, and pay for it in duplicated parsing and
inconsistent YAML-version handling between them.

`tpt-yaml` treats parsing, serde integration, lossless editing, schema
validation, a CLI, and language bindings as one family built on a single
shared core, rather than six unrelated projects that happen to all touch
YAML.

## What it solves, concretely

**One core, multiple consumers.** `tpt-yaml-core`'s arena and span model is
the thing every other crate builds on: `tpt-yaml-serde` deserializes
straight from arena nodes (no re-parsing), `tpt-yaml-edit` mutates the same
arena and blits untouched byte spans back out verbatim, and
`tpt-yaml-schema` validates against it directly. A bug fixed once in
resolution or span tracking is fixed everywhere; there's no drift between
"the parser's opinion of this document" and "the editor's opinion of it."

**Lossless editing is a first-class crate, not a bolt-on.** `tpt-yaml-edit`
re-renders only the subtree you actually changed and blits the rest of the
source byte-for-byte — comments, formatting, and key order on untouched
regions survive a round trip. This is verified by a proptest (parse →
random edit sequence → render always re-parses, untouched regions stay
byte-identical), not just asserted in docs.

**Schema validation against the same document you parsed**, not a
translation to/from JSON first. `tpt-yaml-schema` implements a JSON Schema
2020-12 subset directly over `tpt-yaml-core`'s node model, so validation
errors carry real spans back into the original YAML source.

**Explicit, tested YAML 1.1/1.2 version handling.** The "Norway problem"
(`no`/`off`/`on` resolving to booleans instead of strings) and similar
1.1-vs-1.2 divergences are handled with an explicit `YamlVersion`
parameter and an opt-in `--strict-version` mode that rejects scalars whose
resolution actually differs between the two versions, rather than quietly
picking one behavior and hoping it matches the document's author's intent.

**A real `no_std` ladder where it matters.** `tpt-yaml-core` has zero
external dependencies and supports `no_std` + `alloc`, so it can run in
embedded or constrained environments; every crate built on it re-exposes
that ladder rather than silently requiring `std`. (The CLI and FFI/bindings
layers are the deliberate, documented exception — see `AGENTS.md`.)

**A constant-memory path for large documents.** `tpt-yaml-core::stream`
and `tpt-yaml-serde`'s `streaming` feature parse and deserialize as a
pull-based event stream (O(nesting depth) memory) instead of always
materializing a full arena, for cases where a document is too large to
hold entirely in memory. This is a real scope trade-off, not a free
feature: anchors/aliases are rejected on that path, since replaying them
correctly would require buffering the exact memory the streaming path
exists to avoid.

**Bindings that don't force a lowest-common-denominator API.** Python
(`pyo3`) and WASM (`wasm-bindgen`) bindings marshal directly against
`tpt-yaml-serde`'s `Value` rather than routing through the C ABI, since
each already does its own marshaling — avoiding a redundant, more
constrained shared layer. The C ABI itself exists independently for
languages that need it, with panics caught at the boundary so they can
never cross into caller code.

## Why a standalone repo

This family intentionally has **no dependency on any `tpt-io-*` crate**.
Its diagnostics types (`YamlError`/`ErrorContext`/`ErrorKind`) are modeled
on `tpt-io-standards`'s shape for familiarity to anyone who's used that
family, but are owned and maintained here — so `tpt-yaml` can be adopted,
versioned, and published on its own, without pulling in an unrelated
ecosystem's dependency tree.

## What this is not

- Not a claim of full YAML 1.1/1.2 spec conformance yet — an initial run of
  the `yaml-test-suite` conformance corpus (data-2022-01-17) passes 190/333
  cases; the gaps are tracked in `todo.md`.
- Fuzzed, but only for short bring-up sessions so far, not the "ran for
  hours and built a crash corpus" sense yet. Initial runs (~1.4M execs
  total across `lexer`/`parser`/`roundtrip_edit`/`validate`/
  `ffi_parse_roundtrip`) found and fixed one real bug (a global-buffer-
  overflow in the `tpt-yaml-ffi` fuzz harness's own hardcoded byte-string
  length, not in the crate under test) and no others; longer soak runs are
  still needed before calling any target clean.
- Not published to crates.io/PyPI/npm yet. See `todo.md` §7 for the
  dependency-ordered publish plan.
