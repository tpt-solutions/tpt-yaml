# tpt-yaml-schema

A JSON Schema 2020-12 *subset* validator for [`tpt-yaml-core`](../tpt-yaml-core) documents:
load a schema from YAML (or JSON, via the `json-schema` feature), validate a parsed document
against it, and get back a deterministic, ordered list of issues — each carrying the offending
node's source span for editor surfacing.

## Quick start

```rust
use tpt_yaml_schema::SchemaDocument;

let schema = SchemaDocument::from_yaml_str(
    "type: object\nrequired: [name, ports]\nproperties:\n  name: {type: string}\n  ports: {type: array, items: {type: number, minimum: 1, maximum: 65535}}\n",
)?;

let doc = tpt_yaml_core::parse("name: alpha\nports: [80, 443]\n")?;
let report = schema.validate(&doc, doc.root().unwrap());
assert!(report.is_valid());
# Ok::<(), Box<dyn std::error::Error>>(())
```

A failing document gets a `ValidationReport` of `ValidationIssue`s, each with a dotted `path`
(e.g. `ports.0`), an `IssueKind`, a human-readable `message`, and the node's `span` (via
`document.span(issue.node)`) for pointing an editor at the exact offending text.

## Design

- **Compiled IR.** `SchemaDocument::from_yaml_str`/`from_json_str` compile schema source once
  into the `Schema` enum (`Type`, `String`, `Number`, `Array`, `Object`, `Enum`, `Const`,
  `OneOf`/`AnyOf`/`AllOf`/`Not`, `Ref`, and the `{}`-shaped `Constant(true)`) plus a `$defs` map;
  `validate`/`validate_with` then walk a `Document` node against that IR without re-parsing the
  schema.
- **`$ref` resolution.** Only internal references are supported: `#/$defs/<name>` or `#/<name>`,
  resolved against the schema document's own `$defs`. Resolution happens at validation time
  (`Schema::Ref` looks up `defs` each visit), so `$ref`s may point at schemas defined anywhere in
  `$defs`, including ones not yet compiled at the point of reference. Runtime recursion (through
  self-referential `$ref`s or deeply nested combinators) is capped by
  `ValidationSettings::max_depth`, reported as `IssueKind::DepthExceeded` past the limit.
- **`ValidationSettings` builder.** `ValidationSettings::new().only(&["types", "objects"])` gates
  which check groups run (`types`, `enums`, `strings`, `numbers`, `arrays`, `objects`,
  `combinators`, `refs`), each defaulting to on; `.with_max_depth(n)` adjusts the recursion cap.
  Pass a settings value to `validate_with` instead of `validate`'s default.
- **Typed validation.** The `typed` feature adds `SchemaDocument::validate_typed<T:
  Serialize>(&self, value: &T)`, which routes `value` through `tpt-yaml-serde`'s `Serializer`
  into the same arena the rest of the family uses, then validates it exactly like any other
  parsed document — no separate typed-vs-untyped validation logic to keep in sync.
- **Determinism.** `ValidationReport` sorts its issues by `(path, kind)` before returning
  (`ValidationReport::deterministic`), so two validations of the same document against the same
  schema always compare equal — asserted by the `validation_is_deterministic` unit test and the
  `tests/proptest_schema.rs` property tests.
- **Pattern subset.** `pattern`/`patternProperties` use a hand-rolled regex-subset matcher
  (`src/pattern.rs`) rather than a full regex engine, keeping the crate dependency-light; see
  "Known limitations" for exactly what's supported.

## Known limitations

- **`$ref` is internal-only.** Only `#/$defs/<name>` and `#/<name>` are recognized; there is no
  remote/external `$ref` resolution (no fetching another document, no base-URI tracking), and no
  `$anchor`/`$dynamicRef`/`$dynamicAnchor` support.
- **`enum`/`const` are scalar-only.** Both keywords accept only scalar values (`null`, booleans,
  numbers, strings); a compound (array/object) `enum` member or `const` value fails to compile
  with `Error::InvalidSchema`.
- **No format keyword.** `format` (e.g. `date-time`, `email`, `uri`) is not validated; it is
  silently ignored if present in schema source.
- **Regex subset, not full ECMA-262.** `pattern` supports `^`/`$` anchors, `.`, `*`/`+`/`?`
  quantifiers, character classes `[...]` (with ranges and `^` negation), and the shorthand
  escapes `\d \w \s \D \W \S` (inside or outside a class). Anything outside that subset either
  fails to compile (`Error::InvalidPattern`) or, for unrecognized escapes, is matched literally
  rather than rejected — check `src/pattern.rs` before relying on more exotic regex syntax.
- **No `unevaluatedProperties`/`unevaluatedItems`, `if`/`then`/`else`, `contains`, or
  `propertyNames`.** The supported keyword set is exactly the one listed in `src/lib.rs`'s module
  doc comment: `type`, `enum`, `const`, `pattern`/`minLength`/`maxLength`,
  `minimum`/`maximum`/`exclusiveMinimum`/`exclusiveMaximum`/`multipleOf`,
  `items`/`minItems`/`maxItems`/`uniqueItems`,
  `properties`/`required`/`additionalProperties`/`minProperties`/`maxProperties`,
  `oneOf`/`anyOf`/`allOf`/`not`, and internal `$ref`.
- **`integer` is treated as `number`.** There's no separate integer-vs-float type check; JSON
  Schema's `type: integer` and `type: number` both compile to `JsonType::Number`, matching either
  an `Int` or `Float` scalar.
