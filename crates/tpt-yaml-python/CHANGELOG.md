# Changelog

All notable changes to `tpt-yaml-python` are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Nothing has been
published to crates.io or PyPI yet — see the workspace root's `AGENTS.md` for the publish order.

## [Unreleased]

### Added

- Full implementation: `tpt_yaml.loads`/`tpt_yaml.dumps` (Python `loads`/`dumps` naming
  convention) converting between YAML text and native Python objects via `tpt-yaml-serde`'s
  `Value`, a `tpt_yaml.TptYamlError` exception, and the `#[pymodule] fn tpt_yaml` registering
  both. `Cargo.toml`: `pyo3` dependency, `cdylib`/`rlib` lib target, an `auto-initialize`
  dev-dependency for running `cargo test` without a `maturin` build (see the crate README's
  "Testing without maturin" section for why `extension-module` is deliberately not a default or
  crate-level feature here). `pyproject.toml` configured for `maturin` (`bindings = "pyo3"`,
  `module-name = "tpt_yaml"`). Added to the workspace `Cargo.toml` `members` list.
- 5 Rust unit tests (`Python::with_gil`-based, via `auto-initialize`): `loads` round-tripping a
  mapping, `dumps` rendering and round-tripping a Python `dict`, a `loads` parse-error case, a
  `dumps` unsupported-type case, and a `bool`-vs-`int` disambiguation case.

### Verified in this environment

- `cargo build -p tpt-yaml-python` / `--all-features`, `cargo test -p tpt-yaml-python`,
  `cargo build --workspace --all-features`, `cargo test --workspace --all-features` — all pass.
- `maturin develop --release` (Python 3.13, maturin 1.14.1) built and installed the extension;
  `import tpt_yaml` plus `loads`/`dumps`/`TptYamlError` all worked from a real interpreter.

### Known limitations

- Anchors/aliases resolve to independent Python values (no shared-identity round-trip); explicit
  YAML tags other than `!!str` are dropped, keeping only the inner value. See the README's
  "Design decisions" section for the rationale.

[Unreleased]: https://github.com/tpt-solutions/tpt-yaml/commits/master/crates/tpt-yaml-python
