//! External `$ref` resolution (the `std` feature): [`SchemaDocument::from_yaml_file`]'s
//! implementation. Filesystem-only — there is no HTTP(S)/`file://` URI support, no `$anchor`/
//! base-URI tracking, and an external ref is namespaced by the literal path spelling written in
//! the referencing schema rather than by canonical path (documented in the README's "Known
//! limitations").
//!
//! Design: a `$ref` naming another file (anything not starting with `#`) is resolved eagerly at
//! compile time by loading and compiling that file (relative to the *referencing* file's own
//! directory), then flattening its root schema and every `$defs` entry into the caller's `defs`
//! map under namespaced keys (`"other.yaml"` for the whole-file root, `"other.yaml#/$defs/name"`
//! for a named def). Internal `$ref`s found inside the loaded file are rewritten to those same
//! namespaced keys so they keep resolving correctly once merged into a different document's
//! `defs` map. A [`Loader`] tracks which files are mid-load (cycle detection: a circular chain of
//! external `$ref`s is a compile error, not a hang) and which are already fully loaded (so the
//! same file referenced twice isn't parsed/compiled twice).

use crate::{compile_root, Additional, BTreeMap, Error, RefResolver, Schema, String, ToString};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Tracks in-flight and completed external-file loads for one [`SchemaDocument::from_yaml_file`]
/// call (shared across every nested external file that call transitively loads).
struct Loader {
    /// Canonical paths currently being loaded (for cycle detection).
    loading: BTreeSet<PathBuf>,
    /// Canonical paths already fully loaded and merged into the running `defs` map.
    loaded: BTreeSet<PathBuf>,
}

impl Loader {
    fn new() -> Self {
        Self { loading: BTreeSet::new(), loaded: BTreeSet::new() }
    }
}

/// The [`RefResolver`] used by [`SchemaDocument::from_yaml_file`][crate::SchemaDocument::from_yaml_file]:
/// an internal `#/...` ref behaves exactly as [`crate::NoExternal`]; anything else is resolved by
/// loading the named file relative to `base_dir`.
struct FileResolver<'a> {
    base_dir: PathBuf,
    loader: &'a mut Loader,
}

impl RefResolver for FileResolver<'_> {
    fn resolve(
        &mut self,
        raw: &str,
        defs: &mut BTreeMap<String, Schema>,
        is_root: bool,
    ) -> Result<String, Error> {
        if let Some(internal) = raw.strip_prefix('#') {
            let key = internal
                .strip_prefix("/$defs/")
                .or_else(|| internal.strip_prefix('/'))
                .ok_or_else(|| Error::InvalidSchema(alloc::format!("unsupported $ref: {raw}")))?;
            if is_root && !defs.contains_key(key) {
                return Err(Error::InvalidSchema(alloc::format!("unknown $ref target: {raw}")));
            }
            return Ok(raw.to_string());
        }
        self.resolve_external(raw, defs)
    }
}

impl FileResolver<'_> {
    fn resolve_external(
        &mut self,
        raw: &str,
        defs: &mut BTreeMap<String, Schema>,
    ) -> Result<String, Error> {
        let (file_part, fragment) = match raw.split_once('#') {
            Some((f, frag)) => (f, Some(frag)),
            None => (raw, None),
        };
        if file_part.is_empty() {
            return Err(Error::InvalidSchema(alloc::format!(
                "external $ref must name a file: {raw}"
            )));
        }
        let full_path = self.base_dir.join(file_part);
        let canon = full_path.canonicalize().unwrap_or_else(|_| full_path.clone());
        let file_key = file_part.to_string();

        if !self.loader.loaded.contains(&canon) {
            let (mut root_schema, sub_defs) = load_and_compile(&full_path, self.loader)?;
            rewrite_internal_refs(&mut root_schema, &file_key);
            defs.insert(file_key.clone(), root_schema);
            for (name, mut schema) in sub_defs {
                rewrite_internal_refs(&mut schema, &file_key);
                defs.insert(alloc::format!("{file_key}#/$defs/{name}"), schema);
            }
        }

        let key = match fragment {
            None => file_key,
            Some(frag) => {
                let name = frag
                    .strip_prefix("/$defs/")
                    .or_else(|| frag.strip_prefix("/definitions/"))
                    .or_else(|| frag.strip_prefix('/'))
                    .unwrap_or(frag);
                alloc::format!("{file_key}#/$defs/{name}")
            }
        };
        if !defs.contains_key(&key) {
            return Err(Error::InvalidSchema(alloc::format!(
                "unknown external $ref target: {raw}"
            )));
        }
        Ok(key)
    }
}

/// Loads and compiles the schema at `path`, returning its root schema and `$defs` map (not yet
/// namespaced — the caller namespaces and merges them).
fn load_and_compile(
    path: &Path,
    loader: &mut Loader,
) -> Result<(Schema, BTreeMap<String, Schema>), Error> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if loader.loading.contains(&canon) {
        return Err(Error::InvalidSchema(alloc::format!(
            "circular external $ref involving {}",
            path.display()
        )));
    }
    loader.loading.insert(canon.clone());

    let source = std::fs::read_to_string(path).map_err(|e| {
        Error::InvalidSchema(alloc::format!("failed to read {}: {e}", path.display()))
    })?;
    let document = tpt_yaml_core::parse(&source)?;
    let root = document.root().ok_or_else(|| {
        Error::InvalidSchema(alloc::format!("{}: empty document", path.display()))
    })?;
    let base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();

    let mut resolver = FileResolver { base_dir, loader: &mut *loader };
    let result = compile_root(&document, root, &mut resolver)?;

    loader.loading.remove(&canon);
    loader.loaded.insert(canon);
    Ok(result)
}

/// Rewrites every internal-style `Schema::Ref` (`"#/$defs/name"` / `"#/name"`) found anywhere in
/// `schema` to the namespaced external key (`"{file_key}#/$defs/name"`) it now lives under, so
/// refs inside a merged-in external file keep resolving against the caller's `defs` map. Already-
/// external refs (a ref this file's own `$ref`s resolved to another file) are left untouched —
/// they're already namespaced against the right target.
fn rewrite_internal_refs(schema: &mut Schema, file_key: &str) {
    match schema {
        Schema::Ref(name) => {
            if let Some(stripped) = name.strip_prefix('#') {
                let normalized = stripped
                    .strip_prefix("/$defs/")
                    .or_else(|| stripped.strip_prefix('/'))
                    .unwrap_or(stripped);
                *name = alloc::format!("{file_key}#/$defs/{normalized}");
            }
        }
        Schema::Array { items, .. } => {
            if let Some(items) = items {
                rewrite_internal_refs(items, file_key);
            }
        }
        Schema::Object { properties, additional, .. } => {
            for (_, prop_schema) in properties.iter_mut() {
                rewrite_internal_refs(prop_schema, file_key);
            }
            if let Additional::Schema(inner) = additional {
                rewrite_internal_refs(inner, file_key);
            }
        }
        Schema::Not(inner) => rewrite_internal_refs(inner, file_key),
        Schema::OneOf(items) | Schema::AnyOf(items) | Schema::AllOf(items) => {
            for item in items.iter_mut() {
                rewrite_internal_refs(item, file_key);
            }
        }
        Schema::Constant(_)
        | Schema::Type(_)
        | Schema::Enum(_)
        | Schema::Const(_)
        | Schema::String { .. }
        | Schema::Number { .. } => {}
    }
}

/// [`crate::SchemaDocument::from_yaml_file`]'s implementation.
pub(crate) fn from_yaml_file(path: &Path) -> Result<crate::SchemaDocument, Error> {
    let mut loader = Loader::new();
    let (root, defs) = load_and_compile(path, &mut loader)?;
    Ok(crate::SchemaDocument { root, defs })
}
