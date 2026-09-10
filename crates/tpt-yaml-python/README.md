# tpt-yaml-python

Python bindings (via [`pyo3`](https://pyo3.rs)) for the [`tpt-yaml`](..) family: parse YAML into
native Python objects (`dict`/`list`/`str`/`int`/`float`/`bool`/`None`) and serialize them back,
packaged as a `tpt_yaml` extension module via [`maturin`](https://www.maturin.rs/).

This is a separate, idiomatic Python binding crate — not a wrapper around
[`tpt-yaml-ffi`](../tpt-yaml-ffi)'s C ABI. It depends directly on `tpt-yaml-core` (parsing) and
`tpt-yaml-serde` (the dynamic `Value` type), since `pyo3` already does its own Python-value
marshaling and routing through the C ABI would just mean redoing that work. `tpt-yaml-ffi` and
the JS/WASM bindings are siblings of this crate, not dependencies of it.

## Install

```sh
pip install maturin
maturin develop            # builds the extension in-place and installs it into the active venv
# or, for a release build: maturin develop --release
# or, to build a wheel without installing it: maturin build --release
```

## Quick start

```python
import tpt_yaml

value = tpt_yaml.loads("name: Ada\ncount: 3\ntags: [a, b]\n")
# value == {"name": "Ada", "count": 3, "tags": ["a", "b"]}

yaml_text = tpt_yaml.dumps({"name": "Ada", "count": 3, "tags": ["a", "b"]})
# yaml_text == '"name": "Ada"\n"count": 3\n"tags":\n  - "a"\n  - "b"\n'

try:
    tpt_yaml.loads("key: [1, 2")
except tpt_yaml.TptYamlError as e:
    print(f"parse failed: {e}")
```

`loads`/`dumps` are named to match the convention `json.loads`/`json.dumps` and
`yaml.safe_load`/`yaml.safe_dump` (PyYAML) already use, rather than a `parse`/`stringify`
JS-flavored naming.

## Public API

- `tpt_yaml.loads(source: str) -> Any` — parse YAML `source` and return the equivalent Python
  object. Raises `tpt_yaml.TptYamlError` on a YAML parse error.
- `tpt_yaml.dumps(value) -> str` — render a Python object (`dict`/`list`/`str`/`int`/`float`/
  `bool`/`None`, arbitrarily nested) as a YAML string. Raises `TypeError` if `value` (or anything
  nested inside it) isn't one of those types — matching `json.dumps`'s behavior for unsupported
  types, since that failure is a Python-side "I don't know how to convert this object" error
  rather than a YAML-domain error; `tpt_yaml.TptYamlError` is reserved for failures that
  originate on the Rust/YAML side (a malformed document, `tpt-yaml-serde`'s own conversion
  errors).
- `tpt_yaml.TptYamlError` — exception type raised by `loads` (and `dumps`'s rendering step, if
  `tpt-yaml-serde` itself fails to render a value); a subclass of `Exception`, so callers who
  don't care about the distinction can still catch it as one.

## Design decisions

- **`loads`/`dumps`, not `parse`/`stringify`**: idiomatic Python YAML/JSON libraries
  (`json`, `PyYAML`, `ruamel.yaml`) use `loads`/`dumps` (or `load`/`dump` for streams); this
  crate follows that convention rather than inventing new names or mirroring a different host
  language's binding.
- **Anchors/aliases become independent values**: `Value::from_node` (the conversion boundary
  from a parsed `tpt_yaml_core::Document`) resolves aliases to their target's value before this
  crate ever sees them. Two YAML nodes that alias the same anchor come back as two separate,
  unlinked Python objects with no shared identity (`is`) — Python has no built-in
  reference-preserving structure equivalent of a YAML anchor/alias pair, and building one (e.g.
  tracking a `dict`/`list` of `id()`-keyed back-references) would be a much larger feature for a
  need that hasn't come up; this is documented here as the known, deliberate simplification.
- **Tagged scalars unwrap and drop the tag**: `Value::Tagged(tag, inner)` (any explicit YAML tag
  other than the core `!!str` string override) converts as just `inner`'s value, discarding
  `tag` entirely, rather than surfacing it as e.g. a `(tag, value)` tuple. Rationale: a tuple
  return would mean *every* caller has to special-case "is this value plain, or a
  `(tag, value)` pair?" even when they don't care about custom tags (the overwhelmingly common
  case), just to unwrap the common case back out. A caller that does need the tag can still get
  it by working with `tpt_yaml_core`/`tpt_yaml_serde` directly from Rust, or by driving
  `tpt-yaml-schema`/`tpt-yaml-edit` instead of this crate.
- **`bool` is checked before `int`**: in Python, `bool` is a subclass of `int`
  (`isinstance(True, int)` is `True`), so `dumps`'s Python → `Value` walk checks for `bool`
  first — checking `int` first would silently turn `True`/`False` into `1`/`0`.
- **No `tpt-yaml-ffi` dependency**: see the crate-level doc comment in `src/lib.rs` and the note
  at the top of this README — going through the C ABI would mean re-marshaling values `pyo3`
  already marshals directly.

## Testing without `maturin`

This crate's `src/lib.rs` unit tests (`cargo test -p tpt-yaml-python`) run as plain Rust tests
using `pyo3`'s `auto-initialize` dev-dependency feature, which boots an embedded Python
interpreter — no `maturin`/extension-module build needed. See the comments in `Cargo.toml` for
why `pyo3`'s `extension-module` feature (needed for the real `maturin`-built wheel) is
deliberately *not* enabled as this crate's own default or as a crate feature of its own: it's
incompatible with linking `cargo test`'s embedded-interpreter build in the same compilation, and
keeping it out of this crate's own `[features]` table means `cargo test --all-features` never
pulls the two together by accident. `maturin` enables it directly via
`pyproject.toml`'s `[tool.maturin] features = ["pyo3/extension-module"]`.

Verified in this environment: `cargo build -p tpt-yaml-python`, `cargo test -p tpt-yaml-python`,
`cargo build --workspace --all-features`, and `cargo test --workspace --all-features` all pass;
`maturin develop --release` (with Python 3.13 and `maturin` 1.14.1 available) built and
installed the extension successfully, and `import tpt_yaml; tpt_yaml.loads(...)` /
`tpt_yaml.dumps(...)` worked end-to-end from a real Python interpreter.

## Known limitations

- Anchors/aliases and explicit YAML tags don't round-trip identity/tag information — see
  [Design decisions](#design-decisions) above.
- `dumps` builds fresh YAML nodes through `tpt-yaml-serde`'s `Serializer`, so it inherits that
  crate's known limitations: no byte-identical formatting preservation of any *input* document
  (there is no input document here — `dumps` only ever renders fresh output) and `NaN`/`±Infinity`
  floats don't round-trip (`tpt-yaml-core` doesn't special-case them).
- Mapping keys are not restricted to strings (YAML allows arbitrary scalar/collection keys), so
  `dumps({1: "a", True: "b"})` is valid input as far as this binding is concerned — but note that
  in a Python `dict` literal, `1` and `True` collide as keys (same `hash`/`==`), which is a
  Python-level gotcha unrelated to this crate.
- No streaming/incremental API — `loads` parses the whole input up front, and `dumps` builds the
  whole output string in memory, mirroring `tpt_yaml_core::parse`/`tpt_yaml_serde::to_string`.
