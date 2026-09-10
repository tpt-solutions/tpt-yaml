# tpt-yaml-edit

Lossless YAML editing over [`tpt-yaml-core`](../tpt-yaml-core)'s arena: change a value, and
[`EditableDocument::render`] re-renders only the parts of the tree an edit actually touched —
everything else is copied byte-for-byte from the original source, comments and formatting
included.

## Quick start

```rust
use tpt_yaml_core::ScalarValue;
use tpt_yaml_edit::{EditValue, EditableDocument, Path};

let mut doc = EditableDocument::parse("a: 1\nb: 2 # keep me\n")?;
doc.set(&Path::root().field("b"), EditValue::Scalar(ScalarValue::Int(99)))?;
let rendered = doc.render();
// `a: 1` is byte-identical; only `b`'s line is synthesized.
assert!(rendered.contains("a: 1\n"));
# Ok::<(), tpt_yaml_edit::Error>(())
```

## API

- [`EditableDocument::parse`](EditableDocument) — parse source into the shared core arena.
- [`set`](EditableDocument::set) — replace/upsert the value at a [`Path`]
  (`Path::root().field("a").index(0)`). Replacing the whole document: `set(Path::root(), …)`.
- [`remove`](EditableDocument::remove) / [`push`](EditableDocument::push) — delete an entry, or
  append to a sequence.
- [`resolve_path`](resolve_path) / [`resolve_trail`](resolve_trail) — the shared `Path`→`NodeId`
  helpers (also reused by the CLI's structural `diff`).
- [`EditValue`] — content to write: `Scalar`/`Sequence`/`Mapping`, or `EditValue::typed(&value)?`
  (the `typed` feature) for any `T: serde::Serialize`, routed through `tpt-yaml-serde`.

## How render works

An edit doesn't mutate a node in place; it splices a fresh node (no source span) into the parent
and marks every ancestor "dirty". `render()` walks the tree: a node with a source span that isn't
dirty is blitted verbatim from the original source; a dirty node is reconstructed by recursing
into its children with the same rule. The one trade-off: a container that becomes dirty loses its
directly-attached trivia (core's trivia model is per-container, not per-entry), so comments
immediately around edited entries aren't re-inserted. Nested clean subtrees are unaffected.

## Known limitations

- Comments directly attached to a dirty container aren't preserved (see above); comments around
  untouched siblings are.
- Edits to aliases and anchors on synthesized subtrees are not yet expressible (`EditValue` has
  no alias/anchor spelling); existing aliases in untouched regions blit verbatim.
