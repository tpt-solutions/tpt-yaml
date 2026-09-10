# Changelog

All notable changes to `tpt-yaml-schema` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- `SchemaDocument::from_yaml_str` (and, with the `json-schema` feature, `from_json_str`),
  compiling schema source once into a `Schema` IR plus a `$defs` map.
- `validate`/`validate_with`, producing a deterministic, sorted `ValidationReport` of
  `ValidationIssue`s, each carrying a dotted path, an `IssueKind`, a message, and the offending
  node's source span.
- Supported keyword set: `type`, `enum`, `const`, `pattern`/`minLength`/`maxLength`,
  `minimum`/`maximum`/`exclusiveMinimum`/`exclusiveMaximum`/`multipleOf`,
  `items`/`minItems`/`maxItems`/`uniqueItems`,
  `properties`/`required`/`additionalProperties`/`minProperties`/`maxProperties`,
  `oneOf`/`anyOf`/`allOf`/`not`, and internal `$ref` (`#/$defs/<name>` or `#/<name>`).
- `ValidationSettings` builder: `.only(&[...])` to gate check groups, `.with_max_depth(n)` to
  adjust the recursion cap (`IssueKind::DepthExceeded` past the limit).
- `typed` feature: `SchemaDocument::validate_typed<T: Serialize>`, routing a value through
  `tpt-yaml-serde`'s `Serializer` before validating it like any other parsed document.
- A hand-rolled regex-subset matcher (`src/pattern.rs`) for `pattern`/`patternProperties`,
  keeping the crate dependency-light.
- `no_std` + `alloc` + `std` (default) feature ladder, forwarding to `tpt-yaml-core`'s.

### Known limitations

- `$ref` is internal-only: no remote/external resolution, no `$anchor`/`$dynamicRef`/
  `$dynamicAnchor`.
- `enum`/`const` accept only scalar values.
- `format` is recognized but not validated.
- `pattern` supports a documented regex subset, not full ECMA-262.
- No `unevaluatedProperties`/`unevaluatedItems`, `if`/`then`/`else`, `contains`, or
  `propertyNames`.
- `type: integer` and `type: number` both compile to the same numeric check.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-schema
