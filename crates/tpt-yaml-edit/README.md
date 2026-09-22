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
into its children with the same rule. Each node in `tpt_yaml_core`'s arena already carries its own
leading trivia (the comment/blank-line run immediately before it in source order — see
`tpt_yaml_core::node::NodeData::trivia`); when a container becomes dirty and has to be
reconstructed rather than blitted, `render()` re-emits every untouched child's own trivia (and the
container's own trailing trivia — a comment after the last entry with no specific entry to attach
to) as it walks the entries, so comments around edited siblings survive. Nested clean subtrees are
unaffected either way.

## Known limitations

- A comment attached to an entry that is itself edited/removed is dropped along with that entry —
  expected, since the entry's own node (and its trivia) no longer exists in the new tree.
- `tpt_yaml_core`'s trivia attachment has a couple of pre-existing quirks inherited here rather
  than fixed: a trailing same-line comment after a mapping's *last* entry (`b: 2 # trailing`, at
  end of block) is attached to the entry's *value* node, not necessarily where you'd naively
  expect it back if that entry is later edited; and a comment before the very first entry of a
  *nested* container can end up attached to a node one level down (inside that container) rather
  than to the container itself, which matters only if that inner node's line is specifically
  edited while its container stays otherwise untouched. Both are positional edge cases in
  `tpt_yaml_core::parser`'s trivia-to-node attribution, not data loss — the comment still survives
  `render()`, just not always re-anchored to the exact original line.
- Edits to aliases and anchors on synthesized subtrees are not yet expressible (`EditValue` has
  no alias/anchor spelling); existing aliases in untouched regions blit verbatim.
