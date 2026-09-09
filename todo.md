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
Next step: run the fetched `yaml-test-suite` corpus against the conformance
harness and fix whatever it finds (harness runs but hasn't been run against
the real corpus yet — no network fetch was done this session); then move on
to implementing `tpt-yaml-serde`'s `Deserializer`/`Serializer` (currently a
placeholder stub).

---

## 0. Workspace-level

- [x] Root `Cargo.toml` workspace with `[workspace.package]` shared metadata (license, edition, rust-version, repository)
- [~] `crates/` scaffolded — 5 of 6 present (`tpt-yaml-core`, `-serde`, `-edit`, `-schema`, `-cli` as `--bin`); `tpt-yaml-ffi` not yet created and not in workspace `members`
- [x] `LICENSE-MIT` and `LICENSE-APACHE` at repo root
- [x] Root `README.md` (family overview, crate table, quick-start example)
- [x] `.gitignore` (`/target`, `*.rs.bk`/`*.pdb`, plus the gitignored `tests/conformance/yaml-test-suite/` corpus dir)
- [x] `git init` + first commit (workspace `Cargo.toml`/`Cargo.lock`/`crates/` still untracked — need a follow-up commit)
- [x] `.github/workflows/ci.yml` — fmt/clippy(default+all-features)/test(default+all-features)/msrv(1.75) jobs; no `windows-com`-equivalent job; **not yet run in real CI**, only sanity-checked by running the equivalent commands locally
- [x] `rustfmt.toml` (`edition = "2021"`, `max_width = 100`, `use_small_heuristics = "Max"`) / `clippy.toml` (`msrv = "1.75"`) — whole workspace reformatted to match (it wasn't rustfmt-clean before)
- [x] `AGENTS.md` documenting publish order, feature-split policy, `tpt-yaml-cli` is-std-only exception, no-`tpt-io-*`-dependency rule
- [x] Decide & document MSRV policy — `rust-version = "1.75"` set in workspace `Cargo.toml`, narrated in `AGENTS.md`

## 1. `tpt-yaml-core` (foundation — do this first, sets the pattern for all others)

- [x] `Cargo.toml`: metadata, `no_std` + `alloc` + `std` (default) features, **zero external dependencies**
- [x] `src/lib.rs` module layout: `lexer`, `parser`, `event`, `node`, `span`, `version`, `resolve`, `diagnostics`
- [x] Local `YamlError`/`ErrorContext`/`ErrorKind`/`ParseError`-equivalent types: byte offset, captured byte-window context — modeled on but independent of `tpt-io-core`'s shape
- [x] Lexer: indentation/tab-detection, block/flow scalar indicators, plain/single/double-quoted scalars, directives drafted (389 lines) — compiles and tests pass; directive lexing fixed to capture full directive text (was truncating to just the keyword), `!!`-style secondary tag handles now lex correctly
- [x] Parser: column/indentation-aware arena-building recursive-descent parser (`crates/tpt-yaml-core/src/parser.rs`) — implicit `key: value` block mappings (previously entirely unhandled — only explicit `?`-key mappings worked), block sequences, flow mappings/sequences, the "sequence value may align with its mapping key" exception, nested structures via token-column comparison
- [x] Multi-document stream support (`---` / `...`) — bare first document (no leading `---`) plus explicit `---`-separated subsequent documents; stray content without a `---` separator is a `TrailingContent` error
- [x] Anchors (`&name`) and aliases (`*name`) — populated by the parser; alias-cycle detection is structural (an anchor is only registered after its value is fully parsed, so a cycle is unconstructible) rather than a runtime check
- [x] Merge key (`<<`) support — `ParserOptions.merge_keys: bool`, default `true`; explicit keys win over merged ones, earlier merge sources win over later ones, sequence-of-mappings merge values supported
- [x] Comment/blank-line trivia attachment — populated via `Parser::skip_trivia`/`attach_trivia`
- [x] Span tracking (`span: Option<Span>`) — populated on every node the parser creates
- [x] `YamlVersion`-parameterized `resolve.rs`: bool/int/float/null/timestamp tag-resolution tables; fixed a real bug where quoted/block scalars (e.g. `"true"`, `'42'`) were being implicit-tag-resolved like plain scalars — only plain scalars undergo implicit resolution now
- [x] Norway-problem boolean table (1.1 `y/n/on/off/...` vs. 1.2 `true/false` only)
- [x] Octal sigil handling (1.1 bare `0755` vs. 1.2 `0o755`)
- [x] Sexagesimal ints/floats (1.1-only, e.g. `1:20:30` → `Int(4830)`, `1:20:30.5` → `Float`) — distinct int vs. float paths, gated to `YamlVersion::Version11` only (1.2 no longer misparses colon-containing strings)
- [~] `document.version()` always reports resolved version — now updated from a `%YAML` directive when present (unless `ParserOptions.yaml_version` was explicit); the `--strict-version` ambiguity-detection hook itself is still unimplemented (`ParserOptions.strict_version` field exists but is inert)
- [x] Shared pretty-printer function for synthesized subtrees — drafted in `node.rs` (`pretty_print`/`render_node`/`render_scalar`), compiles and works
- [x] Unit tests for lexer/parser edge cases — 24 tests in `tpt-yaml-core` covering implicit/explicit mappings, nested block structures, flow collections, anchors/aliases, merge keys, multi-doc streams, sexagesimal, quoted-scalar type safety, trailing-content errors
- [x] Golden fixtures in `tests/samples/*.yaml` (block/flow styles, anchors/aliases, merge keys, comments in every syntactic position); `tests/golden.rs` asserts every fixture parses and re-renders without panicking. Multi-doc streams are covered by `multi_doc.yaml` but not yet cross-checked against `tests/samples/*.yaml` from a *second* independent implementation — this is a smoke test, not a byte-exact golden-output check (no `.expected` files yet)
- [x] `yaml-test-suite` conformance harness: `tests/conformance/README.md` (fetch instructions) + `tests/conformance.rs` runner, corpus gitignored, `#[ignore]`d and dir-gated so absence never fails `cargo test --workspace`. **Not yet run against the real corpus** (no network fetch this session) — the runner checks per-case `in.yaml`/`error` pass-fail but doesn't yet single out 1.1-vs-1.2-divergent cases to run under both versions explicitly
- [x] Proptest: panic-freedom over arbitrary bytes (`tests/proptest_parser.rs::never_panics_on_arbitrary_bytes`); span-nesting invariant (child span ⊆ parent span) over a generated "plausible YAML" tree (`span_nesting_holds_over_generated_yaml`) — this exercise is what surfaced the container-span-extent bug fixed above
- [x] Fuzz targets: `lexer`, `parser` scaffolded under `fuzz/` (standard `cargo-fuzz` layout, opted out of the main workspace). Typechecks clean under `cargo +nightly check` in `fuzz/`, but **not actually fuzz-run** — `cargo-fuzz` isn't installed in this environment, so no corpus/crash data exists yet
- [x] Doc comments + crate-level docs pass (`cargo doc --no-deps` clean) — added a crate-level `//!` doc comment to `lib.rs`; no warnings either before or after (the crate doesn't opt into `#![warn(missing_docs)]`)
- [x] `cargo publish --dry-run -p tpt-yaml-core` clean — verified passing

## 2. `tpt-yaml-serde`

- [x] `Cargo.toml` + `serde` dependency; `std`/`alloc`/`streaming` features
- [ ] `Deserializer<'de>` borrowing `&'de Document` + `NodeId` cursor directly (no re-parsing) — `lib.rs` is currently just a placeholder `parse()` stub that always errors
- [ ] `Serializer` building fresh `NodeBuilder` output via `tpt-yaml-core`'s shared printer
- [ ] `Value` dynamic type (`Null/Bool/Int/Float/String/Sequence/Mapping/Tagged`), produced only via `Value::from_node(&Document, NodeId)` (single conversion boundary)
- [ ] `from_str` / `from_slice` / `to_string` / `to_writer` (std)
- [ ] `streaming` feature: `Deserializer::from_events` for constant-memory decode of large documents
- [ ] Round-trip proptest: `from_str(to_string(v)?)? == v` over an arbitrary `Value` strategy
- [ ] Fuzz target: `deserialize` (arbitrary bytes → `from_slice::<Value>`, no panics)
- [ ] README + docs, `cargo publish --dry-run`

## 3. `tpt-yaml-edit`

- [x] `Cargo.toml` + optional `tpt-yaml-serde` dep behind `typed` feature
- [ ] `EditableDocument` wrapping the same core arena by value (not a second tree) — `lib.rs` is currently just a placeholder `parse()` stub that always errors
- [ ] `Path`/`Key::{Field,Index}` types + shared `resolve_path` helper (reusable by CLI `diff`)
- [ ] `edit`/`remove` mutation: replace/insert/remove `NodeId` links, mark replaced node `span: None`, flag ancestors "has synthesized descendant"
- [ ] `render()`: blit `source[span]` byte-for-byte for untouched nodes, pretty-print only synthesized subtrees via the shared core printer
- [ ] `EditValue::{Scalar,Sequence,Mapping}` + `EditValue::Typed` (routes `T: Serialize` through `tpt-yaml-serde`'s `Serializer` under `typed` feature)
- [ ] Flagship proptest: parse → random edit sequence → assert `render()` always re-parses, typed content matches applying edits directly, and untouched byte regions stay byte-identical
- [ ] Fuzz target: `roundtrip_edit` (highest priority in the family — span/trivia splicing is the riskiest new code path)
- [ ] README + docs, `cargo publish --dry-run`

## 4. `tpt-yaml-schema`

- [x] `Cargo.toml` + optional `serde`/`serde_json` deps; `typed`/`json-schema` features
- [ ] JSON Schema 2020-12 subset validation IR (type/enum/pattern/required/properties/items/oneOf/anyOf/allOf/`$ref`) — `lib.rs` is currently just a placeholder `parse()` stub that always errors
- [ ] `Schema::from_yaml_str` (dogfoods `tpt_yaml_core::parse`) / `from_json_str` (behind `json-schema`) loaders
- [ ] `validate(doc, schema) -> ValidationReport` + `ValidationIssue` (spans via `document.span(node)`)
- [ ] Local syntactic/semantic `ValidationSettings`-shaped builder idiom (own type, not imported)
- [ ] `typed` feature: validate a `T: Serialize` value directly
- [ ] Proptest: schema IR + matching/non-matching documents, panic-freedom and determinism
- [ ] Fuzz target: `validate` (arbitrary bytes as a schema document, graceful failure)
- [ ] README + docs, `cargo publish --dry-run`

## 5. `tpt-yaml-cli`

- [~] `Cargo.toml`: **std-only** dependency wiring in place (`serde_json`, `tpt-yaml-core`/`-edit`/`-schema` path deps); hand-rolled arg parser not started — `main.rs` is a one-line placeholder (`eprintln!("tpt-yaml is being initialized")`)
- [ ] `check <FILE>...` subcommand (`--schema <FILE>`, `--yaml-version 1.1|1.2|auto`, `--strict-version`)
- [ ] `fmt <FILE>...` subcommand (`--check`, `--write`)
- [ ] `convert <FILE> --to json|yaml [-o <FILE>]` subcommand
- [ ] `diff <A> <B>` subcommand (structural diff by default via `tpt-yaml-edit`'s `Path`/tree; `--text` for literal diff)
- [ ] Exit-code conventions (0 success, non-zero per failure kind)
- [ ] README + docs

## 6. `tpt-yaml-ffi` (language bindings — C ABI foundation, then Python/JS)

- [ ] Not yet scaffolded — no `crates/tpt-yaml-ffi` directory and not listed in workspace `Cargo.toml` `members`
- [ ] `Cargo.toml`: `crate-type = ["cdylib", "rlib"]`, depends on `tpt-yaml-core` (+ `tpt-yaml-serde`/`tpt-yaml-edit` for typed/edit surface); **std-only**, no `no_std` ladder (documented as intentional, same exception class as `tpt-yaml-cli`)
- [ ] Confirm `NodeId`/`Document` stay FFI-friendly before this crate starts (plain index-based arena, no Rust-only generics in the public shape) — audit `tpt-yaml-core`'s public API for anything that would force a redesign here
- [ ] C ABI layer: opaque handle types (`TptYamlDocument*`, `TptYamlNode*`), `#[no_mangle] extern "C"` parse/query/free functions, error codes (no panics across the FFI boundary — catch and convert)
- [ ] Generated C header (`cbindgen`) checked into the crate and verified up to date in CI
- [ ] Python bindings: `pyo3` feature/sub-crate exposing `parse`/`Document`/`Node` with a Pythonic API (dict/list conversion mirroring `tpt-yaml-serde`'s `Value`); packaged via `maturin`
- [ ] JS/WASM bindings: `wasm-bindgen` feature/sub-crate exposing `parse`/`to_json`-equivalent for browser/Node use; packaged via `wasm-pack`
- [ ] Memory-safety fuzz/test pass across the FFI boundary specifically (use-after-free, double-free, null-handle handling)
- [ ] README per binding target (C, Python, JS) with install + quick-start; `cargo publish --dry-run` for the Rust crate, plus PyPI/npm dry-run packaging checks
- [ ] Versioning policy: bindings track `tpt-yaml-core`'s semver, released only after core is stable (not gated on `tpt-yaml-edit`/`tpt-yaml-schema`)

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

## Deferred / explicitly out of scope

- TOML support (no crate in this family) — smaller ecosystem gap given mature `toml`/`toml_edit`; revisit only if a concrete need emerges.
- Migrating any `tpt-io-standards` crate onto `tpt-yaml-*` — not applicable (that workspace has no YAML-based domain crate today).
- `clap`-based CLI ergonomics — noted as the natural upgrade path if richer `--help`/completions become a real ask; not needed for v1.
