# tpt-yaml — Build Checklist

Tracks every crate from scaffold → full implementation → crates.io publish.
Order follows the approved plan (`tpt-yaml-core` → `tpt-yaml-serde` →
`tpt-yaml-edit` → `tpt-yaml-schema` → `tpt-yaml-cli` → `tpt-yaml-ffi`). Check
items off as completed.

Legend: `[ ]` todo · `[~]` in progress · `[x]` done

Standalone repo — no dependency on any `tpt-io-*` crate. Local diagnostics
types (`ParseError`/`ErrorContext`/`ErrorKind`) are modeled on
`tpt-io-standards`'s design but owned here, not imported.

**Status snapshot (2026-09-09, updated):** workspace scaffolding is well
underway (5 of 6 crates created, `Cargo.toml` metadata/features wired
correctly across the family). `cargo build --workspace`, `cargo test
--workspace`, and `cargo publish --dry-run -p tpt-yaml-core` all **succeed**.
The `tpt-yaml-core` parser is now a real column/indentation-aware
arena-building parser (previously it compiled to a stub that only handled
explicit `?`-prefixed mapping keys and `---`-prefixed streams — plain `key:
value` mappings, the single most common YAML shape, silently failed to
parse). Implicit block mappings/sequences, multi-document streams (bare
first document + `---`-separated subsequent ones), anchors/aliases, `<<`
merge keys, `%YAML` directive version resolution, and 1.1 sexagesimal
int/float resolution are implemented and covered by unit tests (24 passing
in `tpt-yaml-core`). Quoted/block scalars are no longer incorrectly
type-resolved (a real bug: `"true"` used to resolve to `Bool(true)` instead
of `String("true")`).
Golden fixtures, a `yaml-test-suite` conformance harness scaffold, proptest
(panic-freedom + span-nesting invariant), and fuzz target scaffolding are now
also in place for `tpt-yaml-core` (see §1), along with repo hygiene (§0):
LICENSE files, root README, CI workflow, rustfmt/clippy config, AGENTS.md.
Fixing the span-nesting proptest surfaced a real gap: container node spans
(mapping/sequence) previously covered only their *first token*, not their
full byte extent — fixed, since `tpt-yaml-edit`'s planned "blit
`source[span]` for untouched nodes" design depends on spans covering a
node's whole range. Also fixed along the way: plain scalars with internal
spaces (`name: John Doe`) were completely unparseable (lexer stopped at the
first space) — this would have broken an enormous fraction of real-world
YAML.
`tpt-yaml-serde` is now fully implemented (see §2): a zero-copy
`Deserializer<'de>`, a `Serializer`, and a dynamic `Value` type, all with
real test coverage (6 unit tests including full struct/enum/`Option`
round-trips, plus a round-trip proptest). Getting the proptest to pass
surfaced two more real `tpt-yaml-core` bugs, now fixed: the pretty-printer
rendered empty mappings/sequences as nothing at all (silently becoming
`null` on re-parse) instead of flow `{}`/`[]`, and rendered an alias used as
a mapping value/sequence item as the literal text `null` instead of
`*anchor_name`. Also fixed in `resolve.rs`: `+42` was incorrectly resolving
to `-42`, and `i64::MIN` was silently falling back to `Float`.
`tpt-yaml-edit` (§3) is now fully implemented: `EditableDocument` wraps the
core arena by value, `set`/`remove`/`push` mutate via `NodeId` splicing with
ancestor-dirty tracking, and `render()` blits untouched source spans
byte-for-byte while pretty-printing only dirty subtrees — verified by a
flagship proptest (parse → random edit sequence → `render()` always
re-parses, untouched regions stay byte-identical) plus 7 unit/integration
tests. `tpt-yaml-schema` (§4) is also fully implemented: a JSON Schema
2020-12 subset IR/compiler/validator with `ValidationSettings`, typed
validation, determinism-sorted `ValidationReport`s, and its own proptest +
fuzz scaffold. Fixed one real bug found along the way: `tpt-yaml-schema`'s
`validates_a_typed_value` unit test asserted a negative port number was
invalid against a schema with no `minimum` constraint — added `minimum: 0`
so the test actually exercises what it claims to. `tpt-yaml-cli` (§5) is now
fully implemented as a hand-rolled (no `clap`) arg parser with `check`/
`fmt`/`convert`/`diff` subcommands and a documented exit-code scheme.
Both crates' `README.md`s are written; `cargo publish --dry-run` for both
is (expectedly) blocked on `tpt-yaml-core`/`tpt-yaml-serde` not being on
crates.io yet, same as `tpt-yaml-serde`'s existing state (§7). Neither
crate's fuzz target has actually been fuzz-run (`cargo-fuzz` isn't
installed in this environment) — typechecks clean only.
`tpt-yaml-ffi` (§6) is now scaffolded and its C ABI foundation is fully
implemented: opaque `TptYamlDocument` handle, plain-`u32` node ids (no
heap-allocated node handles needed — `tpt_yaml_core::NodeId` is already a
`Copy` newtype), a `TptYamlErrorCode` enum, every `extern "C" fn` wrapped in
`catch_unwind` so no panic can cross the FFI boundary, and a `cbindgen`-
generated `include/tpt_yaml.h` checked in (no CI job wired yet to keep it
verified-up-to-date). Python bindings (`tpt-yaml-python`, a `pyo3`/maturin
sub-crate — deliberately *not* built on top of the C ABI, since pyo3 does
its own marshaling) expose `loads`/`dumps`/`TptYamlError`, and were verified
end-to-end with a real `maturin develop --release` + Python session in this
environment. JS/WASM bindings (`tpt-yaml-wasm`, a `wasm-bindgen` sub-crate,
likewise independent of the C ABI) expose `parse`/`toJson`/`stringify`,
typecheck cleanly against the real `wasm32-unknown-unknown` target (which
was available in this environment), but have no `wasm-bindgen-test`
behavioral coverage yet (needs a real JS engine). All three new crates'
anchor/alias handling independently converged on the same simple choice:
aliases resolve to a plain copy of their target value, no shared-identity
structure. Remaining §6 gaps: no CI job regenerating/diffing the C header,
no PyPI/npm dry-run packaging checks, and the FFI-boundary-specific fuzz
target typechecks but hasn't been fuzz-run (same `cargo-fuzz`-not-installed
situation as every other fuzz target in this workspace).
Next step: the `yaml-test-suite` conformance corpus still hasn't been
fetched/run against `tpt-yaml-core`'s harness (§1); no crate's fuzz target
has actually been fuzz-run in this environment; and §7's release-readiness
items (tag `v0.1.0`, real `cargo publish` in dependency order, docs.rs
verification, CI job for the FFI header) are still outstanding.

**Update (2026-09-20):** implemented `tpt-yaml-serde`'s `streaming` feature
(§2/§8), previously a documented no-op. `tpt-yaml-core` gained a new
`stream::EventParser` (§1) — a pull-based, source-driven parser reading
tokens lazily from a refactored line-buffered `Lexer::next_token` (the lexer
was previously eager, materializing its whole token `Vec` up front) — plus
`tpt-yaml-serde::stream::{Deserializer, Documents}` built on it. Both sides
have real test coverage, including proptests cross-checking the new
event-driven path against the existing, well-tested arena-based path
(`tpt-yaml-core/tests/proptest_stream.rs`,
`tpt-yaml-serde/tests/proptest_streaming.rs`); `cargo test --workspace
--all-features` and `cargo clippy --workspace --all-features` are both
clean. Scope was deliberately cut in one place: anchors/aliases are rejected
by the streaming path rather than supported, since replaying an alias
requires buffering the anchored subtree's events, which would silently
reintroduce the O(document size) memory use streaming exists to avoid —
documents with anchors should keep using `tpt_yaml_serde::from_str`. Other
§8 items (`--strict-version`, conformance corpus, fuzz runs, benches,
examples, `CONTRIBUTING.md`, etc.) are still outstanding.

**Update (2026-09-20, continued):** wired up the FFI header CI job and, while
doing so, found that the *existing* `msrv`/`clippy` CI jobs would not
actually have passed if run for real, despite the earlier "sanity-checked
locally" status:
- `cargo clippy -D warnings` failed on two pre-existing issues, now fixed:
  `tpt-yaml-edit/src/path.rs`'s `ok_or_else` → `ok_or` (pure lint, no
  behavior change) and `tpt-yaml-schema/src/pattern.rs`'s `Pattern::is_match`
  had a dead `let start = if self.anchored_start { 0 } else { 0 }` (both
  branches were `0`) left over from a refactor — the actual anchored/
  unanchored logic lives in the code right below it and was already correct,
  so this was inert cruft, not a live bug; removed it.
- `cargo build --workspace` under a real 1.75 toolchain failed outright:
  `tempfile` (pulled in as a transitive dependency by both `cbindgen`, a
  `tpt-yaml-ffi` build-dependency, and `proptest`'s `rusty-fork`) and
  `proptest` itself had both drifted to versions requiring a newer
  toolchain/Cargo (`edition2024` manifests, rustc 1.82+) than this
  workspace's declared 1.75 MSRV — the workspace `Cargo.lock` had simply
  never been resolved under anything but a modern stable toolchain before.
  Fixed by pinning `tempfile` in `Cargo.lock` (via `cargo update -p tempfile
  --precise 3.14.0`) and capping every crate's `proptest` dev-dependency to
  `>=1, <1.9` (1.8 is the newest release still supporting 1.75) so a future
  bare `cargo update` doesn't silently reintroduce the break.
- Separately, `tpt-yaml-ffi`'s `cbindgen` build-dependency was *itself*
  unconditionally past-MSRV (0.29's own `toml`/`indexmap`/`hashbrown` chain
  needs `edition2024`), independent of the tempfile pin above. Since
  `cbindgen` is a dev-time header-generation tool, not part of the crate's
  public API, it's now gated behind a new `generate-header` feature (on by
  default, so normal `cargo build` still regenerates the header exactly as
  before); the `msrv` CI job builds `tpt-yaml-ffi` with
  `--no-default-features` instead of as part of a blanket
  `--workspace` build.
- The `msrv` job itself now builds/tests the six crates AGENTS.md's MSRV
  promise actually covers (`core`/`serde`/`edit`/`schema`/`cli`/`ffi`), not
  `--workspace` — `tpt-yaml-python`/`tpt-yaml-wasm` are bindings crates
  layered on top with their own much newer toolchain requirements
  (`pyo3`/`wasm-bindgen`) that were never part of the MSRV commitment.
- Added the `ffi-header` job itself: `cargo build -p tpt-yaml-ffi` (default
  features, so `cbindgen` runs) then `git diff --exit-code` on
  `include/tpt_yaml.h`, closing the §1/§8 "no CI job regenerating/diffing
  the header" gap.

All of the above was verified locally end-to-end: `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets [--all-features] -- -D warnings`,
`cargo test --workspace [--all-features]`, and (via `rustup run 1.75.0`) both
scoped MSRV build/test commands the `msrv` job now runs, all green. Real
GitHub Actions execution is still unverified (no push done from this
session) — that's the one piece of the "not yet run in real CI" status this
update doesn't close.

---

## 0. Workspace-level

- [x] Root `Cargo.toml` workspace with `[workspace.package]` shared metadata (license, edition, rust-version, repository)
- [x] `crates/` scaffolded — all 6 present (`tpt-yaml-core`, `-serde`, `-edit`, `-schema`, `-cli` as `--bin`, `-ffi` as C ABI cdylib+rlib), plus two additional binding sub-crates beyond the original 6-crate plan: `tpt-yaml-python` (pyo3) and `tpt-yaml-wasm` (wasm-bindgen), both in workspace `members`
- [x] `LICENSE-MIT` and `LICENSE-APACHE` at repo root
- [x] Root `README.md` (family overview, crate table, quick-start example)
- [x] `.gitignore` (`/target`, `*.rs.bk`/`*.pdb`, plus the gitignored `tests/conformance/yaml-test-suite/` corpus dir)
- [x] `git init` + first commit (workspace `Cargo.toml`/`Cargo.lock`/`crates/` still untracked — need a follow-up commit)
- [x] `.github/workflows/ci.yml` — fmt/clippy(default+all-features)/test(default+all-features)/msrv(1.75)/ffi-header jobs; no `windows-com`-equivalent job; the local sanity-check (running every job's commands by hand, including the `msrv` job under a real 1.75 toolchain via `rustup run 1.75.0`) now genuinely passes — see the 2026-09-20 status-snapshot update above for the real MSRV/clippy breakage it found and fixed along the way. **Still not yet run as real GitHub Actions**
- [x] `rustfmt.toml` (`edition = "2021"`, `max_width = 100`, `use_small_heuristics = "Max"`) / `clippy.toml` (`msrv = "1.75"`) — whole workspace reformatted to match (it wasn't rustfmt-clean before)
- [x] `AGENTS.md` documenting publish order, feature-split policy, `tpt-yaml-cli` is-std-only exception, no-`tpt-io-*`-dependency rule
- [x] Decide & document MSRV policy — `rust-version = "1.75"` set in workspace `Cargo.toml`, narrated in `AGENTS.md`

## 1. `tpt-yaml-core` (foundation — do this first, sets the pattern for all others)

- [x] `Cargo.toml`: metadata, `no_std` + `alloc` + `std` (default) features, **zero external dependencies**
- [x] `src/lib.rs` module layout: `lexer`, `parser`, `event`, `node`, `span`, `version`, `resolve`, `diagnostics`, `stream`
- [x] `stream::EventParser`: a pull-based, source-driven event parser (`src/stream.rs`) alongside the existing arena `parser`/replay-only `event` modules — reads tokens lazily from a new `Lexer::next_token` (the lexer itself was refactored from an eager `lex(self) -> Vec<Token>` into a line-buffered pull cursor so it never materializes the whole token stream either) and emits self-contained `Event`s without building a `Document`, so memory use is O(nesting depth) rather than O(node count). This is what `tpt-yaml-serde`'s `streaming` feature (§2) is built on. Differs from the arena parser in two documented ways: merge keys (`<<`) are rejected rather than expanded (expansion needs the anchored mapping held in memory), and a collection's span covers only its opening token rather than its full byte extent. Covered by unit tests (flat/nested/flow mappings, anchors/aliases, merge-key rejection with and without `ParserOptions.merge_keys`, multi-doc streams, empty input, resumability, `finish()`) plus a proptest cross-checking its output against the arena parser over generated YAML and a panic-freedom proptest over arbitrary bytes
- [x] Local `YamlError`/`ErrorContext`/`ErrorKind`/`ParseError`-equivalent types: byte offset, captured byte-window context — modeled on but independent of `tpt-io-core`'s shape
- [x] Lexer: indentation/tab-detection, block/flow scalar indicators, plain/single/double-quoted scalars, directives drafted (389 lines) — compiles and tests pass; directive lexing fixed to capture full directive text (was truncating to just the keyword), `!!`-style secondary tag handles now lex correctly
- [x] Parser: column/indentation-aware arena-building recursive-descent parser (`crates/tpt-yaml-core/src/parser.rs`) — implicit `key: value` block mappings (previously entirely unhandled — only explicit `?`-key mappings worked), block sequences, flow mappings/sequences, the "sequence value may align with its mapping key" exception, nested structures via token-column comparison
- [x] Multi-document stream support (`---` / `...`) — bare first document (no leading `---`) plus explicit `---`-separated subsequent documents; stray content without a `---` separator is a `TrailingContent` error
- [x] Anchors (`&name`) and aliases (`*name`) — populated by the parser; alias-cycle detection is structural (an anchor is only registered after its value is fully parsed, so a cycle is unconstructible) rather than a runtime check
- [x] Merge key (`<<`) support — `ParserOptions.merge_keys: bool`, default `true`; explicit keys win over merged ones, earlier merge sources win over later ones, sequence-of-mappings merge values supported
- [x] Comment/blank-line trivia attachment — populated via `Parser::skip_trivia`/`attach_trivia`
- [x] Span tracking (`span: Option<Span>`) — populated on every node the parser creates
- [x] `YamlVersion`-parameterized `resolve.rs`: bool/int/float/null/timestamp tag-resolution tables; fixed three real bugs: quoted/block scalars (e.g. `"true"`, `'42'`) were being implicit-tag-resolved like plain scalars (only plain scalars undergo implicit resolution now); `+42` was resolving to `-42` (any sign prefix triggered negation, not just `-`); `-9223372036854775808` (`i64::MIN`) silently fell back to `Float` since its magnitude (`2^63`) doesn't fit in `i64` as a positive number for the `try_from` check to pass
- [x] Norway-problem boolean table (1.1 `y/n/on/off/...` vs. 1.2 `true/false` only)
- [x] Octal sigil handling (1.1 bare `0755` vs. 1.2 `0o755`)
- [x] Sexagesimal ints/floats (1.1-only, e.g. `1:20:30` → `Int(4830)`, `1:20:30.5` → `Float`) — distinct int vs. float paths, gated to `YamlVersion::Version11` only (1.2 no longer misparses colon-containing strings)
- [~] `document.version()` always reports resolved version — now updated from a `%YAML` directive when present (unless `ParserOptions.yaml_version` was explicit); the `--strict-version` ambiguity-detection hook itself is still unimplemented (`ParserOptions.strict_version` field exists but is inert)
- [x] Shared pretty-printer function for synthesized subtrees — drafted in `node.rs` (`pretty_print`/`render_node`/`render_scalar`), compiles and works. Fixed two real bugs found while round-trip-testing `tpt-yaml-serde`: an empty mapping/sequence rendered as nothing at all in block style (silently becoming `null` on re-parse) — now rendered as flow `{}`/`[]`; an alias used as a mapping value or sequence item rendered as the literal text `null` instead of `*anchor_name` (only the top-level-node case handled aliases)
- [x] Unit tests for lexer/parser edge cases — 24 tests in `tpt-yaml-core` covering implicit/explicit mappings, nested block structures, flow collections, anchors/aliases, merge keys, multi-doc streams, sexagesimal, quoted-scalar type safety, trailing-content errors
- [x] Golden fixtures in `tests/samples/*.yaml` (block/flow styles, anchors/aliases, merge keys, comments in every syntactic position); `tests/golden.rs` asserts every fixture parses and re-renders without panicking. Multi-doc streams are covered by `multi_doc.yaml` but not yet cross-checked against `tests/samples/*.yaml` from a *second* independent implementation — this is a smoke test, not a byte-exact golden-output check (no `.expected` files yet)
- [x] `yaml-test-suite` conformance harness: `tests/conformance/README.md` (fetch instructions) + `tests/conformance.rs` runner, corpus gitignored, `#[ignore]`d and dir-gated so absence never fails `cargo test --workspace`. **Not yet run against the real corpus** (no network fetch this session) — the runner checks per-case `in.yaml`/`error` pass-fail but doesn't yet single out 1.1-vs-1.2-divergent cases to run under both versions explicitly
- [x] Proptest: panic-freedom over arbitrary bytes (`tests/proptest_parser.rs::never_panics_on_arbitrary_bytes`); span-nesting invariant (child span ⊆ parent span) over a generated "plausible YAML" tree (`span_nesting_holds_over_generated_yaml`) — this exercise is what surfaced the container-span-extent bug fixed above
- [x] Fuzz targets: `lexer`, `parser` scaffolded under `fuzz/` (standard `cargo-fuzz` layout, opted out of the main workspace). Typechecks clean under `cargo +nightly check` in `fuzz/`, but **not actually fuzz-run** — `cargo-fuzz` isn't installed in this environment, so no corpus/crash data exists yet
- [x] Doc comments + crate-level docs pass (`cargo doc --no-deps` clean) — added a crate-level `//!` doc comment to `lib.rs`; no warnings either before or after (the crate doesn't opt into `#![warn(missing_docs)]`)
- [x] `cargo publish --dry-run -p tpt-yaml-core` clean — verified passing

## 2. `tpt-yaml-serde`

- [x] `Cargo.toml` + `serde` dependency; `std`/`alloc`/`streaming` features; added `version = "0.1.0"` alongside every intra-workspace `path` dependency across all 4 dependent crates so `cargo publish` won't reject them once `tpt-yaml-core` is actually on crates.io (previously would have failed at real-publish time with "all dependencies must have a version requirement specified")
- [x] `Deserializer<'de>` borrowing `&'de Document` + `NodeId` cursor directly (no re-parsing) — `src/de.rs`: full `serde::de::Deserializer` impl (`deserialize_any`/`_option`/`_enum`/`_newtype_struct` custom, everything else via `forward_to_deserialize_any!`), `SeqAccess`/`MapAccess`/`EnumAccess`/`VariantAccess` impls, alias dereferencing
- [x] `Serializer` building fresh output via `tpt-yaml-core`'s shared printer — `src/ser.rs`: full `serde::Serializer` impl building a `Document` directly via `NodeData::add_node` (no separate `NodeBuilder` type exists in `tpt-yaml-core`, so this composes the arena API directly); unit enum variants → plain string, newtype/tuple/struct variants → single-entry mapping (matches `serde_yaml`/`serde_json` convention)
- [x] `Value` dynamic type (`Null/Bool/Int/Float/String/Sequence/Mapping/Tagged`) — `src/value.rs`; `Value::from_node(&Document, NodeId)` is the direct arena-conversion boundary, plus a hand-written `Serialize`/`Deserialize` (self-describing visitor) so `Value` also works as `T` in `from_str`/`to_string`
- [x] `from_str` / `from_slice` / `to_string` / `to_writer` (std) — `to_writer` is `std`-gated (needs `std::io::Write`); the others work under `alloc` alone
- [x] `streaming` feature: `Deserializer::from_events` for constant-memory decode of large documents — implemented on top of a new `tpt_yaml_core::stream::EventParser` (`crates/tpt-yaml-core/src/stream.rs`), a pull-based, source-driven parser that emits self-contained `Event`s without ever building a `Document` (memory use is O(nesting depth), not O(node count)); `tpt-yaml-serde/src/stream.rs` wraps it in a `serde::Deserializer` plus a `Documents<T>` iterator over `---`-separated streams. Known, documented scope cuts: no zero-copy (every scalar is already an owned `String` by the time it reaches the deserializer), and anchors/aliases are rejected (replaying an alias needs the anchored subtree's events buffered in memory, which would reintroduce the O(document size) cost this module exists to avoid) — same class of restriction as the pre-existing merge-key rejection. Covered by: `tpt-yaml-core`'s own unit tests + a proptest cross-checking `EventParser`'s output against the arena parser over generated YAML (`tests/proptest_stream.rs`), and `tpt-yaml-serde`'s unit tests + a proptest cross-checking the streaming `Deserializer` against the arena one over an arbitrary `Value` (`tests/proptest_streaming.rs`)
- [x] Round-trip proptest: `from_str(to_string(v)?)? == v` over an arbitrary `Value` strategy — `tests/roundtrip.rs`; finding and fixing its first failure (`Value::Sequence(vec![])` round-tripping to `Value::Null`) is what surfaced the empty-collection pretty-printer bug fixed above. Floats are restricted to a finite range (see README's "known limitations": `NaN`/`inf` don't round-trip since `tpt-yaml-core` doesn't special-case them)
- [x] Fuzz target: `deserialize` (arbitrary bytes → `from_slice::<Value>`, no panics) — scaffolded under `fuzz/` like `tpt-yaml-core`'s; typechecks on `cargo +nightly check` but not actually fuzz-run (`cargo-fuzz` not installed in this environment)
- [x] README + docs, `cargo publish --dry-run` — README written; `cargo publish --dry-run -p tpt-yaml-serde` correctly fails right now because `tpt-yaml-core` isn't on crates.io yet (not a manifest error — that was fixed by the `version =` addition above — this is the real, expected dependency-ordering block described in §7)

## 3. `tpt-yaml-edit`

- [x] `Cargo.toml` + optional `tpt-yaml-serde` dep behind `typed` feature
- [x] `EditableDocument` wrapping the same core arena by value (not a second tree) — `src/lib.rs`
- [x] `Path`/`Key::{Field,Index}` types + shared `resolve_path`/`resolve_trail` helpers (`src/path.rs`) — reused directly by `tpt-yaml-cli`'s `diff` subcommand
- [x] `edit`/`remove`/`push` mutation: replace/insert/remove `NodeId` links, dirty-set tracks ancestors with a synthesized/replaced descendant
- [x] `render()`: blit `source[span]` byte-for-byte for untouched nodes (indentation-normalized via `blit_indented`), pretty-print only dirty subtrees via the shared core printer/`render_inline`
- [x] `EditValue::{Scalar,Sequence,Mapping}` + `EditValue::Typed` (routes `T: Serialize` through `tpt-yaml-serde`'s `Serializer` under `typed` feature) — `src/value.rs`
- [x] Flagship proptest: parse → random edit sequence → assert `render()` always re-parses and untouched byte regions stay byte-identical — `tests/proptest_edit.rs`
- [x] Fuzz target: `roundtrip_edit` — scaffolded under `fuzz/` (cargo-fuzz layout, opted out of workspace); typechecks clean, not actually fuzz-run (`cargo-fuzz` not installed in this environment)
- [x] README + docs — `README.md` written; `cargo publish --dry-run` blocked on `tpt-yaml-core`/`tpt-yaml-serde` not being on crates.io yet (expected, per §7)

## 4. `tpt-yaml-schema`

- [x] `Cargo.toml` + optional `serde`/`serde_json` deps; `typed`/`json-schema` features
- [x] JSON Schema 2020-12 subset validation IR (type/enum/const/pattern/required/properties/items/minItems/maxItems/uniqueItems/minProperties/maxProperties/oneOf/anyOf/allOf/`not`/internal `$ref`) — `src/lib.rs`
- [x] `SchemaDocument::from_yaml_str` (dogfoods `tpt_yaml_core::parse`) / `from_json_str` (behind `json-schema`) loaders
- [x] `validate(doc, root) -> ValidationReport` + `ValidationIssue` (dotted path, `IssueKind`, spans via `document.span(node)`)
- [x] Local `ValidationSettings` builder idiom (own type, not imported) — gates per-keyword-group checks + `max_depth`
- [x] `typed` feature: `validate_typed<T: Serialize>` validates a value directly via `tpt-yaml-serde`'s `Serializer`
- [x] Proptest: schema IR + matching/non-matching documents, panic-freedom and determinism — `tests/proptest_schema.rs` (skips `$ref`/boolean-`false` generation as a deliberate simplification; doesn't weaken the properties tested)
- [x] Fuzz target: `validate` — scaffolded under `fuzz/` like `tpt-yaml-edit`; typechecks clean, not actually fuzz-run (`cargo-fuzz` not installed)
- [x] README + docs — `README.md` written; `cargo publish --dry-run` blocked on `tpt-yaml-core`/`tpt-yaml-serde` not being on crates.io yet (expected, per §7)

## 5. `tpt-yaml-cli`

- [x] `Cargo.toml`: **std-only** dependency wiring (`serde_json`, `tpt-yaml-core`/`-edit`/`-schema`/`-serde` path deps); hand-rolled arg parser (no `clap`) implemented in `src/main.rs`
- [x] `check <FILE>...` subcommand (`--schema <FILE>`, `--yaml-version 1.1|1.2|auto`, `--strict-version` — wired through to `ParserOptions.strict_version` but inert until the ambiguity-detection hook itself lands in `tpt-yaml-core`, per §1)
- [x] `fmt <FILE>...` subcommand (`--check`, `--write`, defaults to stdout)
- [x] `convert <FILE> --to json|yaml [-o <FILE>]` subcommand (hand-written `tpt_yaml_serde::Value` → `serde_json::Value` mapper)
- [x] `diff <A> <B>` subcommand (structural diff by default, recursive tree walk via `tpt-yaml-edit`'s `Path`/`Key`, alias-dereferencing; `--text` for a small LCS-based line diff)
- [x] Exit-code conventions: `0` success, `1` generic/IO error, `2` YAML parse error, `3` problem found (validation failure / `fmt --check` would reformat / `diff` found differences), `4` usage error — documented in `--help` and the README
- [x] README + docs

## 6. `tpt-yaml-ffi` (language bindings — C ABI foundation, then Python/JS)

- [x] Scaffolded — `crates/tpt-yaml-ffi` (C ABI), plus `crates/tpt-yaml-python` and `crates/tpt-yaml-wasm` as separate binding sub-crates (not `tpt-yaml-ffi` features — pyo3/wasm-bindgen each do their own marshaling, so routing through the C ABI would have meant redundant work); all three listed in workspace `Cargo.toml` `members`
- [x] `tpt-yaml-ffi/Cargo.toml`: `crate-type = ["cdylib", "rlib"]`, depends on `tpt-yaml-core`; **std-only**, no `no_std` ladder (same exception class as `tpt-yaml-cli`); `typed`/`edit` features reserved (wired but not yet consumed by any function)
- [x] Confirmed `NodeId`/`Document` are FFI-friendly — `NodeId(pub u32)` is a plain `Copy` newtype (no lifetimes/generics), `Document { version, documents: Vec<NodeId>, nodes: Vec<NodeData> }` is a straightforward arena; no redesign was needed, and node ids are passed by value rather than as heap-allocated handles
- [x] C ABI layer (`src/lib.rs`/`handle.rs`/`error.rs`): opaque `TptYamlDocument` handle, `#[no_mangle] extern "C"` parse/free/query functions (root, version, node kind, alias target, scalar type/bool/int/float/string, mapping len/key-at/value-at/get, sequence len/item, last-error message), `TptYamlErrorCode` enum, every function wrapped in `catch_unwind` so no panic crosses the FFI boundary, every raw pointer null-checked
- [x] Generated C header — `include/tpt_yaml.h` via a real `cbindgen` (0.29.4, fetched cleanly) `build.rs`, checked into the crate; **no CI job yet** regenerating/diffing it for drift (§7 gap)
- [x] Python bindings — `crates/tpt-yaml-python`, a standalone `pyo3` sub-crate (`loads`/`dumps`/`TptYamlError`) wrapping `tpt-yaml-serde`'s `Value` directly; packaged via `maturin` and verified end-to-end with a real `maturin develop --release` + Python 3.13 session in this environment (not just a typecheck)
- [x] JS/WASM bindings — `crates/tpt-yaml-wasm`, a standalone `wasm-bindgen` sub-crate (`parse`/`toJson`/`stringify`) also wrapping `Value` directly via `serde-wasm-bindgen`; typechecks cleanly against the real `wasm32-unknown-unknown` target (installed in this environment); no `wasm-bindgen-test` behavioral run yet (needs a JS engine) and no actual `wasm-pack build` was run
- [x] Memory-safety pass across the FFI boundary — documented contract (freeing a null handle is a no-op; double-free / use-after-free is documented UB, not detected, since a C ABI can't null the caller's pointer; borrowed strings are non-owned/non-null-terminated with documented lifetimes) plus 7 Rust unit tests exercising null pointers, invalid UTF-8, malformed YAML, out-of-bounds node ids, and type mismatches; a `fuzz/` target for `tpt-yaml-ffi` is scaffolded (typechecks under nightly) but not actually fuzz-run, same as every other fuzz target in this workspace
- [x] READMEs — one per crate (`tpt-yaml-ffi`/C, `tpt-yaml-python`, `tpt-yaml-wasm`) with install + quick-start; `cargo publish --dry-run` for `tpt-yaml-ffi` blocked the same way as every other dependent crate (§7); **no PyPI/npm dry-run packaging checks done** (a real `maturin develop` was run, which is more than a dry-run, but `maturin build`/`twine check`-style PyPI packaging validation and an npm `wasm-pack pack --dry-run`-equivalent were not attempted)
- [x] Versioning policy documented in `tpt-yaml-ffi/README.md`: bindings track `tpt-yaml-core`'s semver, released only after core is stable

## 7. Cross-cutting / release readiness

Publish order (respects dependency graph):

1. `tpt-yaml-core`
2. `tpt-yaml-serde`
3. `tpt-yaml-edit`
4. `tpt-yaml-schema`
5. `tpt-yaml-cli`
6. `tpt-yaml-ffi`

- [x] Publish order decided and documented above
- [x] `repository` URL confirmed in workspace `Cargo.toml` (`https://github.com/tpt-solutions/tpt-yaml`)
- [x] `cargo publish --dry-run` verified clean for `tpt-yaml-core`; other crates blocked until it's on crates.io (still awaiting golden fixtures/conformance/proptest/fuzz coverage per §1 before an actual publish, and licenses/README per §0)
- [ ] Tag `v0.1.0` release once all crates are publish-ready
- [ ] Actual `cargo publish` run per crate in dependency order (manual, not automated)
- [ ] Post-publish: verify docs.rs builds succeed for every crate (including feature-gated docs)

## 8. Platform review follow-ups (2026-09-20)

From a codebase-wide review for bugs/gaps, adoption friction, and innovation
opportunities. Not yet started unless marked otherwise.

### Correctness / inert-feature cleanup

- [x] Reconcile root `README.md`'s per-crate status table (currently says
      "stub"/"in progress" for several crates) with the actual fully-implemented
      state described in this file's status snapshot — currently contradictory
      and will confuse anyone landing on the repo — done: every crate now reads
      "implemented, unreleased" (all are fully implemented per this file; none
      are on crates.io/PyPI/npm yet), and `tpt-yaml-python`'s row was fixed from
      "not yet implemented (scaffold only)" to reflect that it's real and
      `maturin develop`-verified
- [x] Implement `tpt-yaml-serde`'s `streaming` feature (`Deserializer::from_events`
      constant-memory decode) or remove the feature flag until it's real — done,
      see §2/§1 for the `EventParser`/`stream::Deserializer` implementation and
      its test coverage
- [ ] Implement `ParserOptions.strict_version` / CLI `--strict-version` YAML
      1.1-vs-1.2 ambiguity detection, or remove the flag — currently wired
      end-to-end but inert (§1/§5 gap, restated here as an actionable item)
- [ ] Actually fetch and run the `yaml-test-suite` conformance corpus against
      `tpt-yaml-core`'s harness at least once, and fix whatever it finds —
      never run against the real corpus so far, only smoke-tested locally
- [ ] Run all scaffolded fuzz targets (core lexer/parser, serde deserialize,
      edit roundtrip, schema validate, ffi boundary) for real, not just
      typecheck them — install `cargo-fuzz` and let each run long enough to
      build a corpus before calling any of them "fuzzed"
- [x] Add a CI job that regenerates `tpt-yaml-ffi/include/tpt_yaml.h` via
      `cbindgen` and diffs it against the checked-in copy, failing on drift —
      `.github/workflows/ci.yml`'s new `ffi-header` job runs `cargo build -p
      tpt-yaml-ffi` (which drives the crate's existing `build.rs`/`cbindgen`
      regeneration) then `git diff --exit-code` on the header

### Missing capabilities

- [ ] Add `benches/` with `criterion` benchmarks for `tpt-yaml-core` parse and
      `tpt-yaml-serde` round-trip, including head-to-head numbers vs
      `serde_yaml`/`yaml-rust` — currently zero benchmarks exist anywhere in
      the workspace, which weakens both the correctness story and the adoption
      pitch
- [ ] Fix `tpt-yaml-edit`'s known comment-preservation gap: comments attached
      directly to a dirty container aren't preserved because core's trivia
      model is per-container, not per-entry (documented in
      `tpt-yaml-edit/README.md:40-45`) — risks undermining the "lossless
      editor" pitch for early adopters
- [ ] `tpt-yaml-schema`: add external `$ref` support (currently internal-only)
      and `format` validators (email/date-time/uri/etc.) — real-world JSON
      Schema usage leans heavily on both

### Adoption / usability

- [ ] Add an `examples/` directory to every crate with at least one runnable
      example: `tpt-yaml-core/examples/parse_basic.rs`,
      `tpt-yaml-edit/examples/round_trip_edit.rs` (edit a value, show comments
      survive), `tpt-yaml-schema/examples/validate_config.rs`,
      `tpt-yaml-serde/examples/derive_struct.rs`, plus one each for
      `tpt-yaml-python` and `tpt-yaml-wasm` — currently none exist anywhere,
      which is the single highest-friction gap for new users
- [ ] Add root-level `CONTRIBUTING.md` (build instructions, MSRV, how to run
      the conformance suite/fuzzers, PR checklist) — none exists anywhere in
      the repo today
- [ ] Add a root `CHANGELOG.md` aggregating/pointing at per-crate changelogs —
      currently only per-crate `CHANGELOG.md`s exist
- [ ] Get `.github/workflows/ci.yml` actually running (green) on GitHub, not
      just sanity-checked locally, before the `v0.1.0` publish push — the
      *local* sanity-check now genuinely passes end-to-end (see below); it
      hasn't run as real GitHub Actions yet, which is the only remaining gap
      here
- [ ] Write a "migrating from `serde_yaml`" guide/example — `serde_yaml` is
      deprecated upstream, so there's a live, active audience looking for a
      replacement right now; a concrete side-by-side API migration doc is a
      strong, timely adoption lever
- [ ] Consider a `templates/`/`cargo generate`-style set of common YAML shapes
      (k8s manifest, docker-compose, CI config, OpenAPI) to demo
      parse+schema+edit together end-to-end, rather than leaving it to docs
      alone

### Innovation / differentiation (exploratory, not committed)

- [ ] Explore a minimal YAML language-server example built on
      `tpt-yaml-schema` + `tpt-yaml-edit`'s span info (validation + hover) —
      most of the needed pieces already exist, would be a strong
      differentiator vs. plain parser crates
- [ ] Explore a static WASM playground page (parse/validate/edit YAML in
      browser) built on the existing `tpt-yaml-wasm` bindings — cheap given
      what's already implemented, doubles as a live README demo

## Deferred / explicitly out of scope

- TOML support (no crate in this family) — smaller ecosystem gap given mature `toml`/`toml_edit`; revisit only if a concrete need emerges.
- Migrating any `tpt-io-standards` crate onto `tpt-yaml-*` — not applicable (that workspace has no YAML-based domain crate today).
- `clap`-based CLI ergonomics — noted as the natural upgrade path if richer `--help`/completions become a real ask; not needed for v1.
