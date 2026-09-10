# tpt-yaml-wasm

`wasm-bindgen` bindings for the [`tpt-yaml`](..) family: parse YAML and convert to/from
native JavaScript values in the browser or Node.js.

This crate depends directly on `tpt-yaml-core` (parsing/pretty-printing) and
`tpt-yaml-serde` (the dynamic `Value` type and its `Serialize`/`Deserialize` impls) — it
does **not** route through `tpt-yaml-ffi`'s C ABI. `wasm-bindgen` already has its own
JS-value marshaling (`serde-wasm-bindgen` converts `Value` to/from `JsValue` directly), so
going via the C ABI's opaque handles/`extern "C"` functions would just add an unnecessary
layer. `tpt-yaml-ffi` (C ABI) and the planned `tpt-yaml-python` (`pyo3`) crate are siblings
of this one, not dependencies of it.

## What's exposed

- `parse(source: string): unknown` — parses YAML and returns the first document as a
  native JS value (object/array/string/number/boolean/`null`).
- `toJson(source: string): string` — parses YAML and returns it re-serialized as a JSON
  string.
- `stringify(value: unknown): string` — the complement of `parse`: converts a JS value
  into a YAML string.

All three throw (reject, in Promise-returning contexts) a JS `Error` with a descriptive
message on failure — a YAML parse error, or a `value` `stringify` can't decode.

## Build

This targets `wasm32-unknown-unknown` and is packaged with
[`wasm-pack`](https://rustwasm.github.io/wasm-pack/). Two build targets matter, depending
on where the package is consumed:

```sh
# Browser (ES module, `fetch`-based .wasm loading via the generated `init()`)
wasm-pack build --target web crates/tpt-yaml-wasm

# Node.js (CommonJS, synchronous .wasm loading — no init() call needed)
wasm-pack build --target nodejs crates/tpt-yaml-wasm
```

Both produce a `pkg/` directory next to `Cargo.toml` with the compiled `.wasm`, a
generated `.js` glue module, and a `.d.ts` type declaration file.

**Note on this development environment**: this is a native Windows dev environment
without `wasm-pack` installed, so the packaged output above hasn't actually been built or
tested here. What *has* been verified: `cargo build -p tpt-yaml-wasm` (native target)
compiles cleanly, and `cargo check -p tpt-yaml-wasm --target wasm32-unknown-unknown`
(the `wasm32-unknown-unknown` target is installed in this environment via `rustup`)
typechecks cleanly with no warnings. Real behavioral testing (via
`wasm-bindgen-test`/`wasm-pack test`, in an actual JS engine) is still open work.

## JS usage example

Browser (`--target web`):

```js
import init, { parse, toJson, stringify } from './pkg/tpt_yaml_wasm.js';

await init();

const value = parse('name: Ada\ncount: 3\ntags: [a, b]\n');
// value = { name: "Ada", count: 3, tags: ["a", "b"] }

const json = toJson('name: Ada\ncount: 3\n');
// json = '{"name":"Ada","count":3}'

const yaml = stringify({ name: "Ada", count: 3 });
// yaml = "name: Ada\ncount: 3\n"
```

Node.js (`--target nodejs`):

```js
const { parse, toJson, stringify } = require('./pkg/tpt_yaml_wasm.js');

const value = parse('name: Ada\ncount: 3\n');
```

## Known limitations

- **Anchors/aliases collapse to plain values.** `tpt_yaml_serde::Value::from_node`
  resolves an alias to its anchor's value while converting the parsed document into a
  `Value` — there is no separate resolution step in this crate. JavaScript objects/arrays
  have no built-in equivalent of a YAML anchor/alias pair (short of deliberately building
  a shared-reference graph, which this binding does not attempt), so an aliased node comes
  back as an independent *copy* of its anchor's value, not a shared reference. Mutating one
  copy in JS has no effect on the other. This is the same reasonable simplification the
  sibling Python bindings independently make for the same underlying reason.
- **Round-tripping through `stringify` is not always byte-identical to hand-written
  source.** Like `tpt_yaml_serde::to_string`, `stringify` always renders through the
  shared `tpt-yaml-core` pretty-printer, so comments, original scalar quoting/style, and
  key ordering quirks from a hand-written YAML file are not preserved — only the
  structural fact of the value is. (`tpt-yaml-edit` is the crate to use, from Rust, when
  preserving original formatting matters; that surface is not currently exposed to JS.)
- **`NaN`/`±Infinity` floats do not round-trip**, matching `tpt-yaml-core`/`tpt-yaml-serde`
  (see their READMEs) — the underlying resolver/printer doesn't special-case them.
- **Tagged values**: a YAML node with a non-`!!str` explicit tag becomes
  `Value::Tagged(tag, inner)` in Rust, but `Value`'s `Serialize` impl (used for both
  `parse` and `toJson`) just serializes the inner value and drops the tag — so tag
  information does not reach JavaScript. `stringify` likewise has no way to attach a tag
  from a plain JS value.
