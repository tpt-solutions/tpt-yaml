# Contributing to tpt-yaml

Thanks for looking at this repo. It's a workspace of six core crates
(`tpt-yaml-core`, `-serde`, `-edit`, `-schema`, `-cli`, `-ffi`) plus two
binding sub-crates (`tpt-yaml-python`, `tpt-yaml-wasm`). Read `AGENTS.md`
first — it documents publish order, the `no_std`/`alloc`/`std` feature-split
policy, and the "no `tpt-io-*` dependency" rule that apply to every change
here.

## Building

```sh
cargo build --workspace
```

`tpt-yaml-python` and `tpt-yaml-wasm` have their own toolchain requirements
(`pyo3`/maturin, `wasm-bindgen`) beyond the workspace MSRV — see their
READMEs if you're working on those specifically.

## Running tests

```sh
cargo test --workspace --all-features
```

Most crates also carry proptests (panic-freedom, round-trip, and
cross-checking invariants between independent code paths, e.g. the
event-driven `stream::EventParser` against the arena `parser`) that run as
part of the normal test suite — no separate invocation needed.

## Conformance suite

`tpt-yaml-core` can run against the [yaml-test-suite][suite], the de facto
YAML conformance corpus. It isn't checked into this repo (licensing), and
the harness test is `#[ignore]`d and corpus-presence-gated so its absence
never fails a normal test run. To fetch and run it:

```sh
git clone --depth 1 -b data-2022-01-17 https://github.com/yaml/yaml-test-suite.git \
  crates/tpt-yaml-core/tests/conformance/yaml-test-suite/src

cargo test -p tpt-yaml-core --test conformance -- --ignored
```

See `crates/tpt-yaml-core/tests/conformance/README.md` for the full
details (why the `data-*` branch layout specifically, and current
pass/fail status).

[suite]: https://github.com/yaml/yaml-test-suite

## Fuzzing

Every crate with a `fuzz/` directory (`tpt-yaml-core`, `-serde`, `-edit`,
`-schema`, `-ffi`) uses standard `cargo-fuzz`/libFuzzer layout, opted out of
the main workspace so they don't affect a normal `cargo build --workspace`.

```sh
cargo install cargo-fuzz
cd crates/<crate>/fuzz
cargo +nightly fuzz run <target>
```

**Windows note:** `cargo-fuzz`/libFuzzer's sanitizer runtime isn't shipped
for the native MSVC target, so fuzzing doesn't work directly in a native
Windows shell. Run it under WSL instead (a normal Linux `cargo-fuzz`
install works there).

## MSRV

`rust-version = "1.75"`, set in the workspace `Cargo.toml` and mirrored in
`clippy.toml`'s `msrv`. `tpt-yaml-cli` and `tpt-yaml-ffi` are std-only by
design (see `AGENTS.md`) but still honor the same MSRV. `tpt-yaml-python`
and `tpt-yaml-wasm` are excluded from the MSRV commitment (their own
dependencies, `pyo3`/`wasm-bindgen`, require newer toolchains).

Bumping the MSRV requires updating both the workspace `Cargo.toml` and
`clippy.toml`, plus the `msrv` CI job in `.github/workflows/ci.yml`.

## PR checklist

Before opening a PR, please run:

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] Update the `CHANGELOG.md` of every crate you changed (under
      `crates/<crate>/CHANGELOG.md`, "Unreleased" section — see the root
      `CHANGELOG.md` for how they're aggregated)

If your change touches `tpt-yaml-ffi`'s public C ABI, also run `cargo build
-p tpt-yaml-ffi` locally and check `include/tpt_yaml.h` for drift (CI's
`ffi-header` job does this too, but it's faster to catch locally).
