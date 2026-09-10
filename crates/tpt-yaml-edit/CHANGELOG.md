# Changelog

All notable changes to `tpt-yaml-edit` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- `EditableDocument::parse`/`render`: lossless editing over `tpt-yaml-core`'s arena — an edit
  splices in a fresh, dirty subtree, and `render` blits every untouched node verbatim from the
  original source.
- `set`/`remove`/`push` mutation operations addressed by `Path` (`Path::root().field("a")
  .index(0)`).
- `resolve_path`/`resolve_trail`, the shared `Path` → `NodeId` helpers also used by
  `tpt-yaml-cli`'s structural `diff`.
- `EditValue`, content to write: `Scalar`/`Sequence`/`Mapping`, or `EditValue::typed(&value)?`
  (the `typed` feature) for any `T: serde::Serialize`, routed through `tpt-yaml-serde`.
- `no_std` + `alloc` + `std` (default) feature ladder, forwarding to `tpt-yaml-core`'s.

### Known limitations

- Comments directly attached to a dirty container aren't preserved (core's trivia model is
  per-container, not per-entry); comments around untouched siblings are.
- Edits to aliases/anchors on synthesized subtrees are not yet expressible (`EditValue` has no
  alias/anchor spelling); existing aliases in untouched regions blit verbatim.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-edit
