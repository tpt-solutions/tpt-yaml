# Changelog

All notable changes to `tpt-yaml-core` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- Lexer covering indentation/tab detection, block/flow scalar indicators, and
  plain/single/double-quoted scalars.
- Column/indentation-aware recursive-descent parser: implicit and explicit (`?`-key) block
  mappings, block sequences, flow mappings/sequences, and multi-document streams (`---`/`...`).
- Anchors (`&name`) and aliases (`*name`), with structural alias-cycle prevention.
- Merge key (`<<`) support, gated by `ParserOptions::merge_keys` (default `true`).
- Comment and blank-line trivia attachment, and source spans on every parsed node.
- `YamlVersion`-parameterized implicit-scalar resolution: the 1.1/1.2 boolean table, octal
  sigils, and 1.1-only sexagesimal ints/floats.
- A shared pretty-printer (`pretty_print`) for rendering synthesized subtrees, used by
  `tpt-yaml-serde` and `tpt-yaml-edit`.
- `no_std` + `alloc` + `std` (default) feature ladder, with zero external dependencies.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-core
