# tpt-yaml-ffi

The C ABI foundation crate for the [`tpt-yaml`](..) family: a stable, panic-free
`extern "C"` surface over `tpt-yaml-core`'s parser and node arena, for embedding this YAML
toolkit in non-Rust code.

This crate currently implements **only the C ABI**. Python (`pyo3`) and JS/WASM
(`wasm-bindgen`) bindings are planned to sit on top of this crate but are not part of it —
see `todo.md` §6 in the workspace root; this README's C section will get Python/JS
counterparts once those crates exist.

## What's exposed

- Parse YAML source into an opaque document handle (`tpt_yaml_parse`) and free it
  (`tpt_yaml_document_free`).
- Query the document: root node, resolved YAML version, a node's structural kind
  (scalar/mapping/sequence/alias).
- Walk mappings (length, entry-at-index, get-by-string-key) and sequences (length,
  item-at-index).
- Read scalar values (bool/int/float/string, plus a resolved type tag) and follow aliases.
- Every call returns a `TptYamlErrorCode`; `tpt_yaml_last_error_message()` gives a
  human-readable message for the most recent failure on the calling thread.

See `include/tpt_yaml.h` for the exact, authoritative function signatures — it's
generated from the Rust `extern "C"` functions in `src/lib.rs` and kept in sync by
`build.rs` (see [Header generation](#header-generation) below).

## Build

```sh
cargo build --release -p tpt-yaml-ffi
```

This produces both a `cdylib` (a shared library — `target/release/tpt_yaml_ffi.dll` on
Windows, `libtpt_yaml_ffi.so` on Linux, `libtpt_yaml_ffi.dylib` on macOS) and an `rlib` (for
Rust consumers). The C header lives at `crates/tpt-yaml-ffi/include/tpt_yaml.h`, checked
into the crate — see below.

## C usage example

```c
#include "tpt_yaml.h"
#include <stdio.h>
#include <string.h>

int main(void) {
    const char *source = "name: Ada\ncount: 3\n";
    TptYamlErrorCode error = TPT_YAML_ERROR_CODE_OK;

    TptYamlDocument *doc = tpt_yaml_parse((const uint8_t *)source, strlen(source), &error);
    if (doc == NULL) {
        fprintf(stderr, "parse failed: %s\n", tpt_yaml_last_error_message());
        return 1;
    }

    TptYamlNodeId root;
    tpt_yaml_document_root(doc, &root);

    TptYamlNodeId name_value;
    const char *key = "name";
    if (tpt_yaml_mapping_get(doc, root, (const uint8_t *)key, strlen(key), &name_value)
        == TPT_YAML_ERROR_CODE_OK) {
        const char *ptr;
        size_t len;
        tpt_yaml_scalar_string(doc, name_value, &ptr, &len);
        printf("name = %.*s\n", (int)len, ptr);
    }

    tpt_yaml_document_free(doc);
    return 0;
}
```

Compile against `include/tpt_yaml.h` and link against the built `cdylib`
(e.g. `cc example.c -I crates/tpt-yaml-ffi/include -L target/release -ltpt_yaml_ffi`).

## Memory-safety contract

- **Ownership**: `tpt_yaml_parse` transfers ownership of a new `TptYamlDocument*` to the
  caller. Free it exactly once with `tpt_yaml_document_free`.
- **Double-free**: calling `tpt_yaml_document_free` twice on the same pointer is undefined
  behavior. This crate does not detect it (a plain C ABI has no way to reach back into the
  caller's variable to null it out), so it's the caller's responsibility not to do so.
- **Use-after-free**: using a document pointer, a node id obtained from it, or a borrowed
  string pointer (see below) after the document has been freed is undefined behavior.
- **Borrowed strings**: `tpt_yaml_scalar_string`'s `(ptr, len)` output points into memory
  owned by the document — not null-terminated, not caller-owned, valid only until the
  document is freed. `tpt_yaml_last_error_message()`'s return value is similar but owned by
  thread-local storage instead: valid only until the next `tpt_yaml_*` call on the same
  thread.
- **Null handles**: every function checks its pointer arguments against null before
  dereferencing and returns `TPT_YAML_ERROR_CODE_NULL_POINTER` rather than crashing.
  `tpt_yaml_document_free(NULL)` is an explicit, documented no-op.
- **No panics cross the FFI boundary**: every function body runs inside
  `std::panic::catch_unwind` and converts any caught Rust panic into
  `TPT_YAML_ERROR_CODE_PANIC_CAUGHT` (an unwind across an FFI boundary is undefined
  behavior in Rust, so this crate never lets one happen). If you ever observe this code,
  it's a bug in this crate or in `tpt-yaml-core` — please report it.

Node handles (`TptYamlNodeId`) are plain `uint32_t` arena indices, not pointers — there's
nothing to free for a node, and an out-of-range or stale node id (e.g. from a different,
already-freed document) is safely rejected as `TPT_YAML_ERROR_CODE_INDEX_OUT_OF_BOUNDS`
rather than causing memory unsafety, since it's just checked against the target document's
arena length.

## Header generation

`build.rs` runs [`cbindgen`](https://github.com/mozilla/cbindgen) against this crate's
`extern "C"` surface on every build (unless built with `--no-default-features`, see below) and
(re)writes `include/tpt_yaml.h`, configured via `cbindgen.toml`. The header is also checked into
the repository (not gitignored) so it's available even without running the generator — e.g.
reading the API on GitHub, or building from an environment that can't fetch `cbindgen`'s
dependencies.

Header generation lives behind the `generate-header` feature, which is on by default. It can be
turned off (`--no-default-features`) to skip `cbindgen` entirely, which is otherwise an
unconditional build-dependency: `cbindgen` 0.29's own dependency chain has drifted past this
workspace's MSRV (1.75), and Cargo resolves build-dependencies for the whole build graph, so
leaving it unconditional would break the `msrv` CI job for every crate in the workspace, not
just this one. The `msrv` job builds this crate with `--no-default-features`; the checked-in
header ships either way.

The `ffi-header` CI job additionally rebuilds the crate and runs `git diff --exit-code`
against the checked-in header, so a hand-edit or a `cbindgen.toml`/signature change that
wasn't followed by a rebuild fails CI instead of silently drifting.

## Versioning policy

This crate's bindings track `tpt-yaml-core`'s semver directly and can be released
independently of `tpt-yaml-edit`/`tpt-yaml-schema` (see `AGENTS.md`'s publish-order
section). Like the rest of the family, it won't see a real `cargo publish` until
`tpt-yaml-core` is stable and itself published.

## Known limitations / not-yet-implemented

- No typed/edit surface yet: the `typed` (`tpt-yaml-serde`) and `edit` (`tpt-yaml-edit`)
  Cargo features exist and pull in their dependency, but no `extern "C"` function currently
  uses them — they're reserved for a future FFI surface over serde-typed values and
  document mutation/rendering.
- No streaming/incremental parse API — `tpt_yaml_parse` parses the whole input up front,
  mirroring `tpt_yaml_core::parse`.
- `tpt_yaml_mapping_get`'s "no matching key" case reuses `IndexOutOfBounds` rather than a
  dedicated not-found code; see that function's doc comment in `src/lib.rs`.
