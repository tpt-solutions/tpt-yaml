# yaml-test-suite conformance harness

This directory runs `tpt-yaml-core` against the [yaml-test-suite][suite], the
de facto conformance corpus for YAML parsers. The corpus itself is **not**
checked into this repo (its license doesn't clearly permit redistribution;
revisit if that changes) — fetch it locally to run the harness.

[suite]: https://github.com/yaml/yaml-test-suite

## Fetching the corpus

The harness expects the per-case `data-YYYY-MM-DD` release layout
(`<id>/in.yaml`, `<id>/error`), not the consolidated `src/*.yaml` format on
the suite's `main` branch, so clone a `data-*` branch/tag — into a `src/`
subdirectory (that's just this harness's directory name, unrelated to the
suite's own branch naming):

```sh
git clone --depth 1 -b data-2022-01-17 https://github.com/yaml/yaml-test-suite.git \
  crates/tpt-yaml-core/tests/conformance/yaml-test-suite/src
```

The clone target (`yaml-test-suite/`) is gitignored, so this is safe to run
repeatedly and won't show up in `git status`.

## Running

The harness test is `#[ignore]`d by default and additionally checks for the
corpus directory at runtime, so its absence never fails `cargo test
--workspace`:

```sh
# skipped by a plain `cargo test` (both because it's #[ignore]d and because
# the corpus isn't present):
cargo test -p tpt-yaml-core

# after fetching the corpus above, run it explicitly:
cargo test -p tpt-yaml-core --test conformance -- --ignored
```

## What it checks

Each corpus case under `yaml-test-suite/src/<id>/in.yaml` is parsed and
compared against its expected outcome:

- Cases without an `error` marker must parse successfully.
- Cases with an `error` marker must fail to parse.

Every case whose expected behavior differs between YAML 1.1 and 1.2 (per the
suite's own `tags: fail-if-1.2`-style annotations, where present) is run
explicitly under **both** `YamlVersion::Version11` and `YamlVersion::Version12`
via `ParserOptions.yaml_version`, rather than relying on the crate's default.

## Status

Implemented and runs against the real corpus: as of the `data-2022-01-17`
release, 190/333 cases pass. See `../../../../todo.md` §1 for the tracked
gaps.
