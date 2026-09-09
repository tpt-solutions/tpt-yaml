# yaml-test-suite conformance harness

This directory runs `tpt-yaml-core` against the [yaml-test-suite][suite], the
de facto conformance corpus for YAML parsers. The corpus itself is **not**
checked into this repo (its license doesn't clearly permit redistribution;
revisit if that changes) — fetch it locally to run the harness.

[suite]: https://github.com/yaml/yaml-test-suite

## Fetching the corpus

```sh
git clone --depth 1 https://github.com/yaml/yaml-test-suite.git \
  crates/tpt-yaml-core/tests/conformance/yaml-test-suite
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

Each corpus case under `yaml-test-suite/src/*.yaml` (or the split
`in.yaml`/`error` layout used by newer versions of the suite) is parsed and
compared against its expected outcome:

- Cases without an `error` marker must parse successfully.
- Cases with an `error` marker must fail to parse.

Every case whose expected behavior differs between YAML 1.1 and 1.2 (per the
suite's own `tags: fail-if-1.2`-style annotations, where present) is run
explicitly under **both** `YamlVersion::Version11` and `YamlVersion::Version12`
via `ParserOptions.yaml_version`, rather than relying on the crate's default.

## Status

This harness is scaffolding: `conformance.rs` defines the runner and the
`#[ignore]`/env-gating described above, but the corpus-parsing and
per-case pass/fail bookkeeping (matching the suite's actual on-disk layout,
which has changed across its history) is not yet implemented. See
`../../../../todo.md` §1 for tracking.
