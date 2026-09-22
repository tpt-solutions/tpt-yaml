# Changelog

This repo is a workspace of multiple crates, each with its own
`CHANGELOG.md` (following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
format). This root file is a pointer to those, not a duplicate of their
content.

**Status: nothing has been published to crates.io/PyPI/npm yet.** Every
crate below is at `[Unreleased]`. See `AGENTS.md` for the required publish
order once a release does happen.

## Per-crate changelogs

| Crate | Changelog |
| ----- | --------- |
| `tpt-yaml-core` | [`crates/tpt-yaml-core/CHANGELOG.md`](crates/tpt-yaml-core/CHANGELOG.md) |
| `tpt-yaml-serde` | [`crates/tpt-yaml-serde/CHANGELOG.md`](crates/tpt-yaml-serde/CHANGELOG.md) |
| `tpt-yaml-edit` | [`crates/tpt-yaml-edit/CHANGELOG.md`](crates/tpt-yaml-edit/CHANGELOG.md) |
| `tpt-yaml-schema` | [`crates/tpt-yaml-schema/CHANGELOG.md`](crates/tpt-yaml-schema/CHANGELOG.md) |
| `tpt-yaml-cli` | [`crates/tpt-yaml-cli/CHANGELOG.md`](crates/tpt-yaml-cli/CHANGELOG.md) |
| `tpt-yaml-ffi` | [`crates/tpt-yaml-ffi/CHANGELOG.md`](crates/tpt-yaml-ffi/CHANGELOG.md) |
| `tpt-yaml-python` | [`crates/tpt-yaml-python/CHANGELOG.md`](crates/tpt-yaml-python/CHANGELOG.md) |
| `tpt-yaml-wasm` | [`crates/tpt-yaml-wasm/CHANGELOG.md`](crates/tpt-yaml-wasm/CHANGELOG.md) |

## [Unreleased]

Workspace-level changes not specific to one crate:

### Added

- `CONTRIBUTING.md` — build/test/conformance/fuzz instructions, MSRV, PR
  checklist.
- `docs/migrating-from-serde-yaml.md` — a side-by-side migration guide for
  `serde_yaml` users (deprecated upstream).
- `templates/` — small, realistic YAML snippets (Kubernetes manifest,
  Docker Compose, CI config, OpenAPI) for demoing `tpt-yaml-cli`'s
  `check`/`convert` subcommands end-to-end.
- This root `CHANGELOG.md`.

For substantive per-crate changes (parser features, bug fixes, new
subcommands, etc.), see the individual crate changelogs linked above —
this file intentionally stays thin so there's only one place to look for
"what actually changed in crate X."
