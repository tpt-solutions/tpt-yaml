# tpt-yaml-serde

`serde` support for [`tpt-yaml-core`](../tpt-yaml-core): deserialize into your own
`#[derive(Deserialize)]` types, serialize them back out, or work with a dynamic
[`Value`] when you don't have (or want) a fixed shape.

## Quick start

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    name: String,
    retries: u32,
    tags: Vec<String>,
}

let config: Config = tpt_yaml_serde::from_str(
    "name: demo\nretries: 3\ntags:\n  - a\n  - b\n",
)?;

let rendered = tpt_yaml_serde::to_string(&config)?;
# Ok::<(), tpt_yaml_serde::Error>(())
```

Or work with an untyped document via [`Value`]:

```rust
use tpt_yaml_serde::Value;

let value: Value = tpt_yaml_serde::from_str("a: 1\nb: [true, null]\n")?;
# Ok::<(), tpt_yaml_serde::Error>(())
```

## Design

- [`Deserializer`] borrows an already-parsed `&tpt_yaml_core::Document` directly — no
  re-parsing. `from_str`/`from_slice` are convenience wrappers that parse into an
  owned `Document` internally, so they require `T: DeserializeOwned`-shaped types;
  for a zero-copy deserialize (borrowing `&str` fields straight out of the
  document's own storage), parse yourself and construct `Deserializer::from_document`
  against a `Document` you keep alive.
- [`Serializer`] builds a fresh `tpt_yaml_core::Document` and renders it via that
  crate's shared pretty-printer, rather than composing YAML text directly.
- Enum variants follow the same convention serde_yaml/serde_json use: a unit
  variant serializes as a plain string, and newtype/tuple/struct variants
  serialize as a single-entry mapping (`Variant: value`).
- [`Value`] is produced either via [`Value::from_node`] (a direct, serde-free
  conversion from an existing `Document` node — the single conversion boundary)
  or by deserializing through this crate's own `Deserializer`.

## Known limitations

- Multi-document streams: `from_str`/`from_slice` only deserialize the first
  document (via `Document::root()`), matching `serde_yaml`'s behavior.
- The `streaming` feature (constant-memory decode of large documents via
  `Deserializer::from_events`) is not yet implemented.
- `f64::NAN`/`f64::INFINITY` don't round-trip through `to_string`/`from_str`
  (`tpt-yaml-core`'s implicit-scalar resolution doesn't special-case
  `.nan`/`.inf` yet), so the round-trip proptest restricts itself to finite
  floats.
