# Working in this repo

## Publish order

Crates must be published in dependency order — each one only after every
crate it depends on is already on crates.io:

1. `tpt-yaml-core` (no internal dependencies)
2. `tpt-yaml-serde` (depends on `tpt-yaml-core`)
3. `tpt-yaml-edit` (depends on `tpt-yaml-core`, optionally `tpt-yaml-serde`)
4. `tpt-yaml-schema` (depends on `tpt-yaml-core`, optionally `tpt-yaml-serde`)
5. `tpt-yaml-cli` (depends on `tpt-yaml-core`/`-edit`/`-schema`)
6. `tpt-yaml-ffi` (depends on `tpt-yaml-core`, optionally `-serde`/`-edit`); its
   own bindings track `tpt-yaml-core`'s semver and can release independently
   of `tpt-yaml-edit`/`tpt-yaml-schema`.

## Feature-split policy

`tpt-yaml-core` is the only crate with a real `no_std` ladder:
`default = ["std"]`, plus `alloc` for `no_std + alloc` environments. It has
**zero external dependencies** — keep it that way.

Every other crate in the family depends on `tpt-yaml-core` with
`default-features = false, features = ["alloc"]` and re-exposes its own
`std`/`alloc` features that forward to `tpt-yaml-core`'s, so a consumer can
build the whole stack `no_std` if they want to.

`typed` features (on `tpt-yaml-edit` and `tpt-yaml-schema`) pull in
`tpt-yaml-serde` as an optional dependency to route `T: Serialize` values
through its `Serializer`/`Deserializer`.

### Exception: `tpt-yaml-cli` and `tpt-yaml-ffi` are std-only

Both `tpt-yaml-cli` (a binary) and `tpt-yaml-ffi` (a C ABI / Python / JS
bindings layer) depend on `std` unconditionally and do not carry a `no_std`
ladder. This is intentional: a CLI binary and a foreign-language bindings
layer have no reason to run in `no_std` environments, and adding the ladder
there would only add maintenance cost for no real consumer.

## No `tpt-io-*` dependency

This is a standalone repo. Nothing in `tpt-yaml-*` depends on any
`tpt-io-*` crate. Local diagnostics types (`YamlError`/`ErrorContext`/
`ErrorKind`, in `tpt-yaml-core::diagnostics`) are modeled on
`tpt-io-standards`'s shape for familiarity, but are owned here — don't
"fix" this by importing from `tpt-io-standards`.

## MSRV

`rust-version = "1.75"` (set in the workspace `Cargo.toml` and mirrored in
`clippy.toml`'s `msrv`). Bumping it requires updating both places plus the
`msrv` CI job.
