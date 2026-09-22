# Migrating from `serde_yaml`

[`serde_yaml`][serde_yaml] is [deprecated upstream][deprecation] and no
longer maintained. This guide walks through swapping it for
[`tpt-yaml-serde`](../crates/tpt-yaml-serde), the `serde` integration in
this crate family, covering the API surface most `serde_yaml` users
actually touch.

Every signature below was checked against the real source
(`crates/tpt-yaml-serde/src/{lib,de,ser,value,error}.rs`), not written from
memory — if something here doesn't compile against the version you're on,
that's a bug, please file it.

[serde_yaml]: https://docs.rs/serde_yaml
[deprecation]: https://github.com/dtolnay/serde-yaml

## `Cargo.toml`

```diff
 [dependencies]
-serde_yaml = "0.9"
+tpt-yaml-serde = "0.1"
 serde = { version = "1", features = ["derive"] }
```

`tpt-yaml-serde` is not yet published to crates.io (see the workspace
`AGENTS.md`'s publish order) — until then, depend on it via a `path` or
`git` dependency on this repo.

## Deserializing: `from_str`

```diff
- let config: Config = serde_yaml::from_str(&text)?;
+ let config: Config = tpt_yaml_serde::from_str(&text)?;
```

Same shape: a free function generic over `T: Deserialize<'de>`, taking
`&str`, returning `Result<T, Error>`. `tpt-yaml-serde` also has
`from_slice(&[u8])` (UTF-8 validated internally), matching `serde_yaml`'s
own `from_slice`.

One real difference: `from_str`/`from_slice` parse into an **owned**
`Document` internally, so `T` must not borrow from the input — same
restriction `serde_yaml` has via `serde::de::DeserializeOwned` in
practice. If you want a genuinely zero-copy deserialize (borrowing `&str`
fields straight out of parsed storage), parse first and construct the
deserializer yourself against a `Document` you keep alive:

```rust
let document = tpt_yaml_core::parse(&text)?;
let root = document.root().ok_or_else(|| /* ... */)?;
let config: Config = Config::deserialize(
    tpt_yaml_serde::Deserializer::from_document(&document, root),
)?;
```

`serde_yaml` had no equivalent of this — its `Deserializer` always
re-parsed from a `Read`/`&str`/`&[u8]` under the hood. This is a genuine
`tpt-yaml-serde` capability, not just an API-shape difference.

## Serializing: `to_string` / `to_writer`

```diff
- let text = serde_yaml::to_string(&config)?;
+ let text = tpt_yaml_serde::to_string(&config)?;
```

```diff
- serde_yaml::to_writer(file, &config)?;
+ tpt_yaml_serde::to_writer(file, &config)?;
```

Both match `serde_yaml`'s signatures (`to_writer` is gated behind
`tpt-yaml-serde`'s `std` feature, which is on by default). Internally,
`tpt-yaml-serde::Serializer` builds a `tpt_yaml_core::Document` via the
same arena API the parser uses, then renders it through `tpt-yaml-core`'s
shared pretty-printer — rather than composing YAML text field-by-field the
way `serde_yaml`'s serializer does. This shouldn't be visible from the
outside, but if you were relying on `serde_yaml`'s exact formatting
quirks (key ordering, quoting style, etc.), expect some cosmetic
differences.

## The dynamic `Value` type

```diff
- let value: serde_yaml::Value = serde_yaml::from_str(&text)?;
+ let value: tpt_yaml_serde::Value = tpt_yaml_serde::from_str(&text)?;
```

The variant set is similar but not identical. `tpt_yaml_serde::Value`:

```rust
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Sequence(Vec<Value>),
    Mapping(Vec<(Value, Value)>),
    Tagged(String, Box<Value>),
}
```

Differences from `serde_yaml::Value`:

- **`Int(i64)` instead of a `Number` enum covering both int and float.**
  `serde_yaml::Value::Number` wraps a single numeric type that can be
  asked "is this an int/uint/float"; `tpt_yaml_serde::Value` instead
  splits `Int(i64)`/`Float(f64)` up front at parse time, based on the
  source scalar's lexical form. There's no `u64`-specific variant — an
  out-of-`i64`-range unsigned value during *serialization* (not parsing)
  is a hard error rather than silently represented (see `Serializer`'s
  `serialize_u64`, which does `i64::try_from`).
- **`Mapping(Vec<(Value, Value)>)` instead of an ordered-map type.**
  `serde_yaml::Value::Mapping` wraps `serde_yaml::Mapping`, an
  `IndexMap`-like type with map-style lookup methods (`.get()`, etc.).
  `tpt_yaml_serde::Value::Mapping` is a plain `Vec` of key/value pairs —
  insertion-ordered, but no `O(1)` lookup helper; do a linear scan (or
  build your own `HashMap` from it) if you need keyed access.
- **`Tagged(String, Box<Value>)` is explicit and always present** for any
  node carrying a YAML tag that isn't the core `!!str` override — there's
  no separate `serde_yaml::value::TaggedValue` wrapper type to reach for;
  it's a `Value` variant.
- Conversion from an already-parsed `tpt_yaml_core::Document` is a
  first-class path: `Value::from_node(&document, node_id)`, useful if you
  already have a `Document` around for other reasons (e.g. from
  `tpt-yaml-edit` or `tpt-yaml-schema`) and don't want a redundant
  deserialize pass.

## Error handling

```diff
- fn load(text: &str) -> Result<Config, serde_yaml::Error> {
-     serde_yaml::from_str(text)
- }
+ fn load(text: &str) -> Result<Config, tpt_yaml_serde::Error> {
+     tpt_yaml_serde::from_str(text)
+ }
```

`tpt_yaml_serde::Error` is a simple newtype around a `String` message —
`impl Display`, `impl std::error::Error` (under the `std` feature), `impl
serde::de::Error`/`serde::ser::Error` (so `.custom()` works in a manual
`Deserialize`/`Serialize` impl). It's considerably less structured than
`serde_yaml::Error`, which carries a `Location` (line/column) you can
`.location()` out of.

If you need line/column context for a *parse* failure specifically (as
opposed to a `serde` type-mismatch failure), get it before converting to a
`tpt_yaml_serde::Error`: parse with `tpt_yaml_core::parse` directly and
inspect the returned `tpt_yaml_core::YamlError`, which carries a byte
offset and a captured source-context window via its
`ErrorContext`/`ErrorKind`. `tpt_yaml_serde::Error` converts from
`YamlError` via `Display` (`e.to_string()`), so that positional detail is
still in the message text even once it's an `Error`, just not available as
a separate structured field the way `serde_yaml::Location` is.

## Multi-document streams

Both crates deserialize only the **first** document out of a
`from_str`/`from_slice` call — `serde_yaml::from_str` never supported
multi-doc streams either (you needed `serde_yaml::Deserializer::from_str`
+ manual iteration), and `tpt_yaml_serde::from_str`/`from_slice` follow
the same convention deliberately, via `Document::root()`.

For genuinely large or multi-document streams, `tpt-yaml-serde` has a
`streaming` feature (`tpt_yaml_serde::stream`) built on a pull-based,
source-driven parser (`tpt_yaml_core::stream::EventParser`) rather than
building a full arena `Document` first — useful if `serde_yaml`'s
(nonexistent) answer to "large YAML file, don't hold it all in memory" was
a pain point for you. One documented scope cut: the streaming path
rejects anchors/aliases (replaying an alias needs the anchored subtree
buffered in memory, which would reintroduce the cost streaming exists to
avoid) — use `from_str`/`from_document` for documents that need those.

## What doesn't have a direct equivalent

- `serde_yaml::with::singleton_map` and friends (custom enum
  representation helpers) — not present. Enum representation is fixed:
  unit variants serialize as a plain string, newtype/tuple/struct variants
  as a single-entry mapping (`Variant: value`), matching the
  `serde_yaml`/`serde_json` default convention, not the `singleton_map`
  alternate one.
- `serde_yaml::Number`'s `.is_i64()`/`.is_f64()`/`.as_f64()` etc. helper
  methods — just match on the `Value::Int`/`Value::Float` variants
  directly instead.
