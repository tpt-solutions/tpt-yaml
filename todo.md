# tpt-yaml — Build Checklist

Tracks every crate from scaffold → full implementation → crates.io publish.
Order follows the approved plan (`tpt-yaml-core` → `tpt-yaml-serde` →
`tpt-yaml-edit` → `tpt-yaml-schema` → `tpt-yaml-cli`). Check items off as
completed.

Legend: `[ ]` todo · `[~]` in progress · `[x]` done

Standalone repo — no dependency on any `tpt-io-*` crate. Local diagnostics
types (`ParseError`/`ErrorContext`/`ErrorKind`) are modeled on
`tpt-io-standards`'s design but owned here, not imported.

---

## 0. Workspace-level

- [ ] Root `Cargo.toml` workspace with `[workspace.package]` shared metadata (license, edition, rust-version, repository)
- [ ] `crates/` scaffolded for all 5 crates (`cargo new --lib`, `tpt-yaml-cli` is `--bin`)
- [ ] `LICENSE-MIT` and `LICENSE-APACHE` at repo root
- [ ] Root `README.md` (family overview, crate table, quick-start example)
- [ ] `.gitignore` (`/target`, plus gitignored conformance-corpus dirs once added)
- [ ] `git init` + first commit
- [ ] `.github/workflows/ci.yml` — adapted from `tpt-io-standards`: fmt/clippy(default+all-features)/test(default+all-features)/msrv jobs; no `windows-com`-equivalent job
- [ ] `rustfmt.toml` (`edition = "2021"`, `max_width = 100`, `use_small_heuristics = "Max"`) / `clippy.toml` (`msrv = "1.75"`)
- [ ] `AGENTS.md` documenting publish order, feature-split policy, `tpt-yaml-cli` is-std-only exception, no-`tpt-io-*`-dependency rule
- [ ] Decide & document MSRV policy (1.75, matching tpt-io-standards)

## 1. `tpt-yaml-core` (foundation — do this first, sets the pattern for all others)

- [ ] `Cargo.toml`: metadata, `no_std` + `alloc` + `std` (default) features, **zero external dependencies**
- [ ] `src/lib.rs` module layout: `lexer`, `parser`, `event`, `node`, `span`, `version`, `resolve`, `diagnostics`
- [ ] Local `YamlError`/`ErrorContext`/`ErrorKind`/`ParseError`-equivalent types: byte offset, captured byte-window context, spec-note field — modeled on but independent of `tpt-io-core`'s shape
- [ ] Lexer: indentation tracking, tab-detection, block scalar indicators (`|`, `>`, chomping `+`/`-`), flow scalar indicators, plain/single/double-quoted scalars, directives (`%YAML`, `%TAG`)
- [ ] Parser: builds `NodeId(u32)` arena → `NodeData` (`Document`, `Node`, `NodeKind::{Scalar,Mapping,Sequence,Alias}`)
- [ ] Multi-document stream support (`---` / `...`)
- [ ] Anchors (`&name`) and aliases (`*name`), with alias-cycle detection
- [ ] Merge key (`<<`) support — `ParserOptions.merge_keys: bool`, default `true` regardless of `YamlVersion`
- [ ] Comment/blank-line trivia attachment in every syntactic position (needed later by `tpt-yaml-edit`)
- [ ] Span tracking (`span: Option<Span>`) for every node
- [ ] `YamlVersion`-parameterized `resolve.rs`: bool/int/float/null/timestamp tag-resolution tables
- [ ] Norway-problem boolean table (1.1 `y/n/on/off/...` vs. 1.2 `true/false` only)
- [ ] Octal sigil handling (1.1 bare `0755` vs. 1.2 `0o755`)
- [ ] Sexagesimal ints/floats (1.1-only, e.g. `1:20:30`)
- [ ] `document.version()` always reports resolved version (never a silent guess); ambiguity-detection hook for `--strict-version` consumers
- [ ] Shared pretty-printer function for synthesized subtrees (reused later by `tpt-yaml-edit`/`tpt-yaml-serde`)
- [ ] Unit tests for lexer/parser edge cases
- [ ] Golden fixtures in `tests/samples/*.yaml` (block/flow styles, anchors/aliases, merge keys, multi-doc streams, every scalar style, comments in every syntactic position)
- [ ] `yaml-test-suite` conformance harness: `tests/conformance/README.md` (fetch instructions), corpus gitignored pending licensing, tests `#[ignore]`/env-gated so absence never fails `cargo test --workspace`; every 1.1-vs-1.2-divergent case run under both versions explicitly
- [ ] Proptest: panic-freedom over arbitrary bytes; span-nesting invariants (child span ⊆ parent span) over a generated "plausible YAML" tree
- [ ] Fuzz targets: `lexer`, `parser`
- [ ] Doc comments + crate-level docs pass (`cargo doc --no-deps` clean)
- [ ] `cargo publish --dry-run -p tpt-yaml-core` clean

## 2. `tpt-yaml-serde`

- [ ] `Cargo.toml` + `serde` dependency; `std`/`alloc`/`streaming` features
- [ ] `Deserializer<'de>` borrowing `&'de Document` + `NodeId` cursor directly (no re-parsing)
- [ ] `Serializer` building fresh `NodeBuilder` output via `tpt-yaml-core`'s shared printer
- [ ] `Value` dynamic type (`Null/Bool/Int/Float/String/Sequence/Mapping/Tagged`), produced only via `Value::from_node(&Document, NodeId)` (single conversion boundary)
- [ ] `from_str` / `from_slice` / `to_string` / `to_writer` (std)
- [ ] `streaming` feature: `Deserializer::from_events` for constant-memory decode of large documents
- [ ] Round-trip proptest: `from_str(to_string(v)?)? == v` over an arbitrary `Value` strategy
- [ ] Fuzz target: `deserialize` (arbitrary bytes → `from_slice::<Value>`, no panics)
- [ ] README + docs, `cargo publish --dry-run`

## 3. `tpt-yaml-edit`

- [ ] `Cargo.toml` + optional `tpt-yaml-serde` dep behind `typed` feature
- [ ] `EditableDocument` wrapping the same core arena by value (not a second tree)
- [ ] `Path`/`Key::{Field,Index}` types + shared `resolve_path` helper (reusable by CLI `diff`)
- [ ] `edit`/`remove` mutation: replace/insert/remove `NodeId` links, mark replaced node `span: None`, flag ancestors "has synthesized descendant"
- [ ] `render()`: blit `source[span]` byte-for-byte for untouched nodes, pretty-print only synthesized subtrees via the shared core printer
- [ ] `EditValue::{Scalar,Sequence,Mapping}` + `EditValue::Typed` (routes `T: Serialize` through `tpt-yaml-serde`'s `Serializer` under `typed` feature)
- [ ] Flagship proptest: parse → random edit sequence → assert `render()` always re-parses, typed content matches applying edits directly, and untouched byte regions stay byte-identical
- [ ] Fuzz target: `roundtrip_edit` (highest priority in the family — span/trivia splicing is the riskiest new code path)
- [ ] README + docs, `cargo publish --dry-run`

## 4. `tpt-yaml-schema`

- [ ] `Cargo.toml` + optional `serde`/`serde_json` deps; `typed`/`json-schema` features
- [ ] JSON Schema 2020-12 subset validation IR (type/enum/pattern/required/properties/items/oneOf/anyOf/allOf/`$ref`)
- [ ] `Schema::from_yaml_str` (dogfoods `tpt_yaml_core::parse`) / `from_json_str` (behind `json-schema`) loaders
- [ ] `validate(doc, schema) -> ValidationReport` + `ValidationIssue` (spans via `document.span(node)`)
- [ ] Local syntactic/semantic `ValidationSettings`-shaped builder idiom (own type, not imported)
- [ ] `typed` feature: validate a `T: Serialize` value directly
- [ ] Proptest: schema IR + matching/non-matching documents, panic-freedom and determinism
- [ ] Fuzz target: `validate` (arbitrary bytes as a schema document, graceful failure)
- [ ] README + docs, `cargo publish --dry-run`

## 5. `tpt-yaml-cli`

- [ ] `Cargo.toml`: **std-only** (first binary crate in the family, no `no_std`/`alloc` ladder — documented as intentional), hand-rolled zero-dependency arg parser, `serde_json` dep for `convert --to json`
- [ ] `check <FILE>...` subcommand (`--schema <FILE>`, `--yaml-version 1.1|1.2|auto`, `--strict-version`)
- [ ] `fmt <FILE>...` subcommand (`--check`, `--write`)
- [ ] `convert <FILE> --to json|yaml [-o <FILE>]` subcommand
- [ ] `diff <A> <B>` subcommand (structural diff by default via `tpt-yaml-edit`'s `Path`/tree; `--text` for literal diff)
- [ ] Exit-code conventions (0 success, non-zero per failure kind)
- [ ] README + docs

## 6. Cross-cutting / release readiness

Publish order (respects dependency graph):

1. `tpt-yaml-core`
2. `tpt-yaml-serde`
3. `tpt-yaml-edit`
4. `tpt-yaml-schema`
5. `tpt-yaml-cli`

- [ ] Publish order decided and documented above
- [ ] `repository` URL confirmed in workspace `Cargo.toml`
- [ ] `cargo publish --dry-run` verified clean for `tpt-yaml-core`; other crates blocked until it's on crates.io
- [ ] Tag `v0.1.0` release once all crates are publish-ready
- [ ] Actual `cargo publish` run per crate in dependency order (manual, not automated)
- [ ] Post-publish: verify docs.rs builds succeed for every crate (including feature-gated docs)

## Deferred / explicitly out of scope

- TOML support (no crate in this family) — smaller ecosystem gap given mature `toml`/`toml_edit`; revisit only if a concrete need emerges.
- Migrating any `tpt-io-standards` crate onto `tpt-yaml-*` — not applicable (that workspace has no YAML-based domain crate today).
- `clap`-based CLI ergonomics — noted as the natural upgrade path if richer `--help`/completions become a real ask; not needed for v1.
