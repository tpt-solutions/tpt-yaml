//! Regenerates `include/tpt_yaml.h` from this crate's `extern "C"` surface via `cbindgen`.
//!
//! The generated header is also checked into the crate (not gitignored) so that consumers who
//! can't or don't want to run `cbindgen` themselves (e.g. building from a `cargo publish`ed
//! tarball without network access to fetch its transitive deps) still have a header to compile
//! against — this script just keeps it in sync during development. A build failure here does
//! not fail the crate build: cbindgen has known constraints around unusual crate layouts, and a
//! stale-but-present header is preferable to a hard build failure for a bindings crate. CI
//! should additionally run `cbindgen` and diff the result against the checked-in header to catch
//! drift (not yet wired up — see this crate's README).

use std::env;
use std::path::PathBuf;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let out_path = PathBuf::from(&crate_dir).join("include").join("tpt_yaml.h");

    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=cbindgen.toml");

    let config = cbindgen::Config::from_root_or_default(&crate_dir);

    match cbindgen::Builder::new().with_crate(&crate_dir).with_config(config).generate() {
        Ok(bindings) => {
            bindings.write_to_file(&out_path);
        }
        Err(err) => {
            // Don't fail the build over header generation: warn loudly and leave the
            // already-checked-in header in place.
            println!(
                "cargo:warning=tpt-yaml-ffi: cbindgen header generation failed ({err}); leaving \
                 the existing include/tpt_yaml.h in place"
            );
        }
    }
}
