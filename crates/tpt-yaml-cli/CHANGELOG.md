# Changelog

All notable changes to `tpt-yaml-cli` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- `tpt-yaml` binary with `check`, `fmt`, `convert`, and `diff` subcommands, parsed via a
  hand-rolled `std::env::args()` loop (no external arg-parsing crate).
- `check <FILE>...`: parse errors with file/line/column, plus optional `--schema <FILE>`
  validation reporting dotted-path issues.
- `fmt <FILE>...`: re-render through `tpt-yaml-core`'s pretty-printer, with `--check`/`--write`
  modes.
- `convert <FILE> --to json|yaml [-o <FILE>]`: YAML↔JSON conversion via `tpt-yaml-serde::Value`.
- `diff <A> <B>`: structural diff (added/removed/changed by dotted path, using
  `tpt-yaml-edit`'s path machinery) with a `--text` line-based fallback.
- Distinct process exit codes for success, generic errors, parse errors, "found a problem"
  results, and usage errors (see the README's exit-code table).
- `--yaml-version 1.1|1.2|auto` flag on `check`, threaded through to
  `tpt_yaml_core::ParserOptions`.

### Known limitations

- `--strict-version` is accepted and wired through, but the underlying ambiguity-detection
  behavior in `tpt-yaml-core` isn't implemented yet.
- `convert --to json` only converts the first document of a multi-document stream.
- Structural `diff` compares only the first document of each file and dereferences aliases
  before comparing.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-cli
