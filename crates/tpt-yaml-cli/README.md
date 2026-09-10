# tpt-yaml-cli

A small command-line tool over the [`tpt-yaml`](..) crate family: parse/validate YAML,
reformat it, convert it to/from JSON, and diff two YAML files. Ships as the `tpt-yaml` binary.

No external arg-parsing crate is used — a plain hand-rolled loop over `std::env::args()`
(`clap` is an intentionally deferred upgrade path; see the workspace's `AGENTS.md`).

## Install / run

```sh
cargo install --path crates/tpt-yaml-cli
tpt-yaml --help
```

or, from a checkout, `cargo run -p tpt-yaml-cli -- <subcommand> ...`.

## Subcommands

### `check <FILE>...`

Parses each file and reports parse errors with file/line/column.

```sh
tpt-yaml check config.yaml
# config.yaml: OK
```

Optionally validates each file's root value against a schema:

```sh
tpt-yaml check --schema schema.yaml config.yaml
# config.yaml: OK (schema valid)
```

`schema.yaml` is loaded via `tpt_yaml_schema::SchemaDocument::from_yaml_str` (a JSON Schema
2020-12 subset — see [`tpt-yaml-schema`](../tpt-yaml-schema)'s README for the supported
keywords). On a validation failure, every `ValidationIssue` is printed with its dotted path,
message, and source line/column when available:

```sh
tpt-yaml check --schema schema.yaml bad-config.yaml
# bad-config.yaml:2:8: count: -1 below 0 (below minimum)
```

Flags:

- `--schema <FILE>` — validate against this schema (YAML).
- `--yaml-version 1.1|1.2|auto` — force implicit-scalar resolution under a specific YAML
  version; `auto` (the default) honors a `%YAML` directive in the file, falling back to 1.2.
- `--strict-version` — reserved for future YAML-version-ambiguity diagnostics. The flag is
  wired through to `tpt_yaml_core::ParserOptions::strict_version`, but that hook is not yet
  implemented upstream in `tpt-yaml-core`, so passing it currently has no observable effect.

### `fmt <FILE>...`

Re-renders each file through `tpt-yaml-core`'s canonical pretty-printer.

```sh
tpt-yaml fmt config.yaml          # prints the formatted result to stdout
tpt-yaml fmt --check config.yaml  # exits non-zero and lists files that would change
tpt-yaml fmt --write config.yaml  # reformats the file in place
```

`--check` and `--write` are mutually exclusive; with neither, the formatted output goes to
stdout (each input file prefixed with a `# <file>` comment line when more than one is given).

### `convert <FILE> --to json|yaml [-o <FILE>]`

Parses `<FILE>` as YAML and converts it.

```sh
tpt-yaml convert config.yaml --to json
tpt-yaml convert config.yaml --to json -o config.json
tpt-yaml convert config.yaml --to yaml   # equivalent to `fmt`
```

`--to json` goes through `tpt_yaml_serde::Value` and a small hand-written
`Value -> serde_json::Value` mapper, then `serde_json::to_string_pretty`. `--to yaml` re-renders
the parsed document the same way `fmt` does. `-o <FILE>` writes to a file instead of stdout.

### `diff <A> <B>`

Structural diff by default: parses both files and walks their trees (via
[`tpt-yaml-edit`](../tpt-yaml-edit)'s `Key`/path machinery) comparing node-by-node, reporting
added (`+`), removed (`-`), and changed (`~`) values by dotted path.

```sh
tpt-yaml diff a.yaml b.yaml
# ~ count: 3 -> 4
# ~ tags.1: b -> c
# + extra: true
```

`--text` falls back to a literal line-based diff of the two files' raw text (an LCS-based
`-`/`+` line diff — not YAML-aware, useful when one file doesn't parse or you want to see
whitespace/comment/formatting changes the structural diff ignores).

## Exit codes

| Code | Meaning |
| ---- | ------- |
| `0`  | Success (or, for `check`/`fmt --check`/`diff`: nothing wrong found) |
| `1`  | Generic error — I/O failure, unreadable file, internal error |
| `2`  | YAML parse error |
| `3`  | The operation completed but found a problem: a schema validation failure, a file `fmt --check` would reformat, or a `diff` that found differences |
| `4`  | Usage error — bad/missing arguments, unknown subcommand or flag |

## Known limitations

- `--strict-version` is accepted and threaded through to `ParserOptions`, but the underlying
  ambiguity-detection behavior in `tpt-yaml-core` isn't implemented yet (see that crate's
  `todo.md` entry) — passing it is a no-op today.
- `convert --to json` only converts the first document of a multi-document YAML stream (JSON
  has no multi-document concept); `fmt`/`convert --to yaml` render every document, separated by
  `---`.
- The structural `diff` compares only the first document of each file, dereferences aliases
  before comparing (so an alias and its expansion compare equal), and reports mapping-key
  order-independently; it is not a general tree-edit-distance diff, just an added/removed/changed
  report by path.
