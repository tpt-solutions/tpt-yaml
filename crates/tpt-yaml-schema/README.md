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
- **`$ref` resolution — internal and external.** `SchemaDocument::from_yaml_str` resolves only
  internal references: `#/$defs/<name>` or `#/<name>`, against the schema document's own `$defs`.
  `SchemaDocument::from_yaml_file` (the `std` feature) additionally resolves external references —
  `$ref: "other-schema.yaml#/$defs/foo"`, `$ref: "other-schema.yaml#/foo"`, or a bare
  `$ref: "other-schema.yaml"` referencing that file's root schema — relative to each file's own
  directory. External files are loaded and flattened into the returned `SchemaDocument`'s `defs`
  map under namespaced keys the moment they're referenced, with a visited-path set guarding
  against circular external `$ref` chains (a compile error, not a hang). See "Known limitations"
  below for exactly what this does and doesn't cover. Internal-ref resolution (whichever loader is
  used) happens at validation time (`Schema::Ref` looks up `defs` each visit), so `$ref`s may point
  at schemas defined anywhere in `$defs`, including ones not yet compiled at the point of
  reference. Runtime recursion (through self-referential `$ref`s or deeply nested combinators) is
  capped by `ValidationSettings::max_depth`, reported as `IssueKind::DepthExceeded` past the limit.
- **`format` validation.** A hand-rolled validator per format (no `regex`/`chrono`/`url`
  dependency, the same "pragmatic subset" approach as `pattern.rs`), covering `email`, `date-time`,
  `date`, `uri`, `ipv4`, `ipv6`, and `uuid` (see `src/format.rs`). A `format` value this crate
  doesn't recognize is silently ignored, matching the crate's existing unknown-keyword convention.
- **`ValidationSettings` builder.** `ValidationSettings::new().only(&["types", "objects"])` gates
  which check groups run (`types`, `enums`, `strings`, `numbers`, `arrays`, `objects`,
  `combinators`, `refs`, `formats`), each defaulting to on; `.with_max_depth(n)` adjusts the
  recursion cap. Pass a settings value to `validate_with` instead of `validate`'s default.
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

- **`$ref` is filesystem-only; no HTTP(S)/base-URI resolution.** `SchemaDocument::from_yaml_str`
  supports only internal `#/$defs/<name>` / `#/<name>` references. `SchemaDocument::from_yaml_file`
  (the `std` feature) additionally resolves a `$ref` naming another file, but strictly as a local
  filesystem path relative to the referencing file's own directory — there is no `http://`/
  `https://`/`file://` URI fetching, no `$id`-based base-URI remapping, and no
  `$anchor`/`$dynamicRef`/`$dynamicAnchor` support. External files are namespaced in the compiled
  `defs` map by the *literal path spelling* written in the `$ref` (e.g. `"../shared/port.yaml"`),
  not by canonical/absolute path — two different on-disk files reached via the same relative
  spelling from two different referencing files are not distinguished and will collide in the
  `defs` map. A circular chain of external `$ref`s (file A refs file B refs file A) is rejected as
  a compile error rather than causing infinite recursion. JSON schema source
  (`SchemaDocument::from_json_str`, the `json-schema` feature) supports only internal `$ref`;
  external `$ref` resolution is YAML-only.
- **`enum`/`const` are scalar-only.** Both keywords accept only scalar values (`null`, booleans,
  numbers, strings); a compound (array/object) `enum` member or `const` value fails to compile
  with `Error::InvalidSchema`.
- **`format` covers a small subset, not the full JSON Schema format vocabulary.** Only `email`,
  `date-time`, `date`, `uri`, `ipv4`, `ipv6`, and `uuid` are validated (`src/format.rs`); every
  other `format` value (`hostname`, `regex`, `duration`, `json-pointer`, `iri`, …) is silently
  ignored, same as before this was added. None of the seven implemented validators claim full RFC
  conformance — they're hand-rolled, dependency-free checks tuned for the common case: `email`
  doesn't handle RFC 5322 quoted-string/comment forms, `uri` checks for a scheme plus a non-empty
  remainder rather than the full RFC 3986 grammar, and `ipv6` doesn't accept the IPv4-mapped tail
  form (`::ffff:192.0.2.1`).
- **Regex subset, not full ECMA-262.** `pattern` supports `^`/`$` anchors, `.`, `*`/`+`/`?`
  quantifiers, character classes `[...]` (with ranges and `^` negation), and the shorthand
  escapes `\d \w \s \D \W \S` (inside or outside a class). Anything outside that subset either
  fails to compile (`Error::InvalidPattern`) or, for unrecognized escapes, is matched literally
  rather than rejected — check `src/pattern.rs` before relying on more exotic regex syntax.
- **No `unevaluatedProperties`/`unevaluatedItems`, `if`/`then`/`else`, `contains`, or
  `propertyNames`.** The supported keyword set is exactly the one listed in `src/lib.rs`'s module
  doc comment: `type`, `enum`, `const`, `pattern`/`minLength`/`maxLength`/`format`,
  `minimum`/`maximum`/`exclusiveMinimum`/`exclusiveMaximum`/`multipleOf`,
  `items`/`minItems`/`maxItems`/`uniqueItems`,
  `properties`/`required`/`additionalProperties`/`minProperties`/`maxProperties`,
  `oneOf`/`anyOf`/`allOf`/`not`, and `$ref` (internal always; external via `from_yaml_file`).
- **`integer` is treated as `number`.** There's no separate integer-vs-float type check; JSON
  Schema's `type: integer` and `type: number` both compile to `JsonType::Number`, matching either
  an `Int` or `Float` scalar.
