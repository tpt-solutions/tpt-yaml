//! `tpt-yaml`: a small hand-rolled CLI over the `tpt-yaml-*` crate family.
//!
//! No external arg-parsing crate is used (see the workspace's `AGENTS.md`/`todo.md`: `clap` is
//! explicitly deferred) — this is a plain `match`-driven loop over `std::env::args()`.
//!
//! ## Exit codes
//!
//! - `0` — success (and, for `check`/`fmt --check`/`diff`, "nothing wrong found")
//! - `1` — generic error (I/O failure, unreadable file, internal error)
//! - `2` — YAML parse error
//! - `3` — the operation completed but found a problem: a schema validation failure, a file
//!   that `fmt --check` would reformat, or a `diff` that found differences
//! - `4` — usage error: bad/missing arguments, unknown subcommand or flag

use std::fs;
use std::io::Write as _;

use tpt_yaml_core::{Document, NodeId, NodeKind, ParserOptions, ScalarValue, YamlVersion};
use tpt_yaml_edit::Key;

const EXIT_OK: i32 = 0;
const EXIT_ERROR: i32 = 1;
const EXIT_PARSE: i32 = 2;
const EXIT_CHECK_FAILED: i32 = 3;
const EXIT_USAGE: i32 = 4;

fn main() {
    let mut args = std::env::args();
    args.next(); // skip argv[0]
    let code = run(args);
    std::process::exit(code);
}

fn run(mut args: impl Iterator<Item = String>) -> i32 {
    match args.next().as_deref() {
        None | Some("-h") | Some("--help") => {
            print_usage();
            EXIT_OK
        }
        Some("check") => cmd_check(args),
        Some("fmt") => cmd_fmt(args),
        Some("convert") => cmd_convert(args),
        Some("diff") => cmd_diff(args),
        Some(other) => {
            eprintln!("tpt-yaml: unknown subcommand '{other}'\n");
            print_usage();
            EXIT_USAGE
        }
    }
}

fn print_usage() {
    println!(
        r#"tpt-yaml — a small CLI over the tpt-yaml-* crate family

USAGE:
    tpt-yaml <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
    check <FILE>...
        Parse each FILE, reporting parse errors with file/line/column.
        --schema <FILE>            Validate each file's root against a JSON-Schema-subset
                                    document loaded from FILE (YAML).
        --yaml-version <V>         Force resolution under 1.1, 1.2, or auto (default: auto —
                                    honor a %YAML directive, else 1.2).
        --strict-version           Reserved for future ambiguity diagnostics; currently just
                                    wires the flag through (no behavior yet).

    fmt <FILE>...
        Re-render each FILE through the canonical pretty-printer.
        --check                    Exit non-zero if formatting would change a file, without
                                    writing; lists which files would change.
        --write                    Overwrite each file in place with the formatted output.
        (neither flag)             Print the formatted output to stdout.

    convert <FILE> --to json|yaml [-o <FILE>]
        Parse FILE as YAML and convert it to the target format.
        --to <FORMAT>              "json" or "yaml" (required).
        -o <FILE>                  Write output to FILE instead of stdout.

    diff <A> <B>
        Structural diff of two YAML files by dotted path (added/removed/changed).
        --text                     Fall back to a literal line-based diff of the raw text.

EXIT CODES:
    0   success (or: nothing wrong found)
    1   generic error (I/O failure, unreadable file, internal error)
    2   YAML parse error
    3   the operation found a problem (schema validation failure, fmt --check would
        reformat, or diff found differences)
    4   usage error (bad/missing arguments, unknown subcommand or flag)
"#
    );
}

// ---------------------------------------------------------------------------------------------
// check
// ---------------------------------------------------------------------------------------------

fn cmd_check(args: impl Iterator<Item = String>) -> i32 {
    let mut schema_path: Option<String> = None;
    let mut yaml_version: Option<String> = None;
    let mut strict_version = false;
    let mut files = Vec::new();

    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--schema" => match args.next() {
                Some(v) => schema_path = Some(v),
                None => return usage_error("--schema requires a <FILE> argument"),
            },
            "--yaml-version" => match args.next() {
                Some(v) => yaml_version = Some(v),
                None => return usage_error("--yaml-version requires 1.1, 1.2, or auto"),
            },
            "--strict-version" => strict_version = true,
            other if other.starts_with('-') => {
                return usage_error(&format!("unknown flag '{other}' for check"))
            }
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        return usage_error("check requires at least one <FILE>");
    }

    let version = match yaml_version.as_deref() {
        None | Some("auto") => None,
        Some("1.1") => Some(YamlVersion::Version11),
        Some("1.2") => Some(YamlVersion::Version12),
        Some(other) => {
            return usage_error(&format!(
                "invalid --yaml-version '{other}' (expected 1.1, 1.2, or auto)"
            ))
        }
    };
    let options = ParserOptions { yaml_version: version, strict_version, ..ParserOptions::default() };

    let schema = match schema_path {
        Some(path) => match fs::read_to_string(&path) {
            Ok(source) => match tpt_yaml_schema::SchemaDocument::from_yaml_str(&source) {
                Ok(schema) => Some(schema),
                Err(e) => {
                    eprintln!("tpt-yaml: failed to load schema '{path}': {e}");
                    return EXIT_ERROR;
                }
            },
            Err(e) => {
                eprintln!("tpt-yaml: failed to read schema '{path}': {e}");
                return EXIT_ERROR;
            }
        },
        None => None,
    };

    let mut worst = EXIT_OK;
    for file in &files {
        let source = match fs::read_to_string(file) {
            Ok(source) => source,
            Err(e) => {
                eprintln!("{file}: error reading file: {e}");
                worst = worst.max(EXIT_ERROR);
                continue;
            }
        };
        let document = match tpt_yaml_core::parse_with_options(&source, &options) {
            Ok(document) => document,
            Err(e) => {
                eprintln!("{file}: {e}");
                worst = worst.max(EXIT_PARSE);
                continue;
            }
        };

        if let Some(schema) = &schema {
            let Some(root) = document.root() else {
                println!("{file}: OK (empty document, nothing to validate)");
                continue;
            };
            let report = schema.validate(&document, root);
            if report.is_valid() {
                println!("{file}: OK (schema valid)");
            } else {
                for issue in report.issues() {
                    let where_ = if issue.path.is_empty() { "<root>" } else { issue.path.as_str() };
                    match issue.span {
                        Some(span) => eprintln!(
                            "{file}:{}:{}: {where_}: {} ({})",
                            span.line, span.column, issue.message, issue.kind
                        ),
                        None => eprintln!("{file}: {where_}: {} ({})", issue.message, issue.kind),
                    }
                }
                worst = worst.max(EXIT_CHECK_FAILED);
            }
        } else {
            println!("{file}: OK");
        }
    }
    worst
}

// ---------------------------------------------------------------------------------------------
// fmt
// ---------------------------------------------------------------------------------------------

fn render_document(document: &Document) -> String {
    let mut out = String::new();
    for (i, &root) in document.documents.iter().enumerate() {
        if i > 0 {
            out.push_str("---\n");
        }
        out.push_str(&tpt_yaml_core::pretty_print(root, document));
    }
    out
}

fn cmd_fmt(args: impl Iterator<Item = String>) -> i32 {
    let mut check = false;
    let mut write = false;
    let mut files = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--check" => check = true,
            "--write" => write = true,
            other if other.starts_with('-') => {
                return usage_error(&format!("unknown flag '{other}' for fmt"))
            }
            other => files.push(other.to_string()),
        }
    }

    if files.is_empty() {
        return usage_error("fmt requires at least one <FILE>");
    }
    if check && write {
        return usage_error("fmt: pass only one of --check or --write");
    }

    let mut worst = EXIT_OK;
    for file in &files {
        let source = match fs::read_to_string(file) {
            Ok(source) => source,
            Err(e) => {
                eprintln!("{file}: error reading file: {e}");
                worst = worst.max(EXIT_ERROR);
                continue;
            }
        };
        let document = match tpt_yaml_core::parse(&source) {
            Ok(document) => document,
            Err(e) => {
                eprintln!("{file}: {e}");
                worst = worst.max(EXIT_PARSE);
                continue;
            }
        };
        let formatted = render_document(&document);
        let changed = formatted != source;

        if check {
            if changed {
                println!("would reformat: {file}");
                worst = worst.max(EXIT_CHECK_FAILED);
            }
        } else if write {
            if changed {
                if let Err(e) = fs::write(file, &formatted) {
                    eprintln!("{file}: error writing file: {e}");
                    worst = worst.max(EXIT_ERROR);
                    continue;
                }
                println!("formatted: {file}");
            }
        } else {
            if files.len() > 1 {
                println!("# {file}");
            }
            print!("{formatted}");
        }
    }
    worst
}

// ---------------------------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------------------------

fn value_to_json_key(value: &tpt_yaml_serde::Value) -> String {
    use tpt_yaml_serde::Value;
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.clone(),
        Value::Tagged(_, inner) => value_to_json_key(inner),
        Value::Sequence(_) | Value::Mapping(_) => {
            tpt_yaml_serde::to_string(value).unwrap_or_default()
        }
    }
}

fn value_to_json(value: &tpt_yaml_serde::Value) -> serde_json::Value {
    use tpt_yaml_serde::Value;
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::String(s) => serde_json::Value::String(s.clone()),
        Value::Sequence(items) => serde_json::Value::Array(items.iter().map(value_to_json).collect()),
        Value::Mapping(entries) => serde_json::Value::Object(
            entries.iter().map(|(k, v)| (value_to_json_key(k), value_to_json(v))).collect(),
        ),
        Value::Tagged(_, inner) => value_to_json(inner),
    }
}

fn cmd_convert(args: impl Iterator<Item = String>) -> i32 {
    let mut to: Option<String> = None;
    let mut output: Option<String> = None;
    let mut files = Vec::new();

    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--to" => match args.next() {
                Some(v) => to = Some(v),
                None => return usage_error("--to requires json or yaml"),
            },
            "-o" | "--output" => match args.next() {
                Some(v) => output = Some(v),
                None => return usage_error("-o requires a <FILE> argument"),
            },
            other if other.starts_with('-') => {
                return usage_error(&format!("unknown flag '{other}' for convert"))
            }
            other => files.push(other.to_string()),
        }
    }

    let file = match files.as_slice() {
        [file] => file.clone(),
        [] => return usage_error("convert requires exactly one <FILE>"),
        _ => return usage_error("convert takes exactly one <FILE>"),
    };
    let to = match to.as_deref() {
        Some("json") => "json",
        Some("yaml") => "yaml",
        Some(other) => return usage_error(&format!("invalid --to '{other}' (expected json or yaml)")),
        None => return usage_error("convert requires --to json|yaml"),
    };

    let source = match fs::read_to_string(&file) {
        Ok(source) => source,
        Err(e) => {
            eprintln!("{file}: error reading file: {e}");
            return EXIT_ERROR;
        }
    };
    let document = match tpt_yaml_core::parse(&source) {
        Ok(document) => document,
        Err(e) => {
            eprintln!("{file}: {e}");
            return EXIT_PARSE;
        }
    };

    let rendered = if to == "json" {
        let Some(root) = document.root() else {
            eprintln!("{file}: empty document, nothing to convert");
            return EXIT_ERROR;
        };
        let value = tpt_yaml_serde::Value::from_node(&document, root);
        let json = value_to_json(&value);
        match serde_json::to_string_pretty(&json) {
            Ok(mut s) => {
                s.push('\n');
                s
            }
            Err(e) => {
                eprintln!("{file}: failed to render JSON: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        render_document(&document)
    };

    match output {
        Some(path) => {
            if let Err(e) = fs::write(&path, &rendered) {
                eprintln!("{path}: error writing file: {e}");
                return EXIT_ERROR;
            }
        }
        None => {
            if let Err(e) = std::io::stdout().write_all(rendered.as_bytes()) {
                eprintln!("tpt-yaml: error writing to stdout: {e}");
                return EXIT_ERROR;
            }
        }
    }
    EXIT_OK
}

// ---------------------------------------------------------------------------------------------
// diff
// ---------------------------------------------------------------------------------------------

fn path_to_string(path: &[Key]) -> String {
    if path.is_empty() {
        return "<root>".to_string();
    }
    let mut out = String::new();
    for key in path {
        match key {
            Key::Field(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            Key::Index(i) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(&i.to_string());
            }
        }
    }
    out
}

/// Follows an alias chain to the node it ultimately refers to, for comparison purposes.
fn deref_alias(document: &Document, mut id: NodeId) -> NodeId {
    let mut hops = 0;
    while let Some(NodeKind::Alias(target)) = document.node(id).map(|n| &n.kind) {
        id = *target;
        hops += 1;
        if hops > 256 {
            break;
        }
    }
    id
}

fn scalar_display(value: &ScalarValue, raw: &str) -> String {
    match value {
        ScalarValue::Null if raw.is_empty() => "null".to_string(),
        _ => raw.to_string(),
    }
}

#[allow(clippy::too_many_arguments)]
fn diff_nodes(
    doc_a: &Document,
    id_a: NodeId,
    doc_b: &Document,
    id_b: NodeId,
    path: &mut Vec<Key>,
    out: &mut Vec<String>,
) {
    let id_a = deref_alias(doc_a, id_a);
    let id_b = deref_alias(doc_b, id_b);
    let (Some(node_a), Some(node_b)) = (doc_a.node(id_a), doc_b.node(id_b)) else {
        return;
    };

    match (&node_a.kind, &node_b.kind) {
        (NodeKind::Scalar(a), NodeKind::Scalar(b)) => {
            if a.value != b.value {
                out.push(format!(
                    "~ {}: {} -> {}",
                    path_to_string(path),
                    scalar_display(&a.value, &a.raw),
                    scalar_display(&b.value, &b.raw)
                ));
            }
        }
        (NodeKind::Sequence(a), NodeKind::Sequence(b)) => {
            let common = a.len().min(b.len());
            for i in 0..common {
                path.push(Key::Index(i));
                diff_nodes(doc_a, a[i], doc_b, b[i], path, out);
                path.pop();
            }
            for (i, &item) in a.iter().enumerate().skip(common) {
                path.push(Key::Index(i));
                out.push(format!("- {}: {}", path_to_string(path), describe(doc_a, item)));
                path.pop();
            }
            for (i, &item) in b.iter().enumerate().skip(common) {
                path.push(Key::Index(i));
                out.push(format!("+ {}: {}", path_to_string(path), describe(doc_b, item)));
                path.pop();
            }
        }
        (NodeKind::Mapping(a), NodeKind::Mapping(b)) => {
            let key_name = |document: &Document, id: NodeId| -> Option<String> {
                match &document.node(id)?.kind {
                    NodeKind::Scalar(s) => Some(s.raw.clone()),
                    _ => None,
                }
            };
            for &(key_id, value_id) in a {
                let Some(name) = key_name(doc_a, key_id) else { continue };
                match b.iter().find(|&&(k, _)| key_name(doc_b, k).as_deref() == Some(name.as_str())) {
                    Some(&(_, other_value)) => {
                        path.push(Key::Field(name));
                        diff_nodes(doc_a, value_id, doc_b, other_value, path, out);
                        path.pop();
                    }
                    None => {
                        path.push(Key::Field(name));
                        out.push(format!("- {}: {}", path_to_string(path), describe(doc_a, value_id)));
                        path.pop();
                    }
                }
            }
            for &(key_id, value_id) in b {
                let Some(name) = key_name(doc_b, key_id) else { continue };
                if !a.iter().any(|&(k, _)| key_name(doc_a, k).as_deref() == Some(name.as_str())) {
                    path.push(Key::Field(name));
                    out.push(format!("+ {}: {}", path_to_string(path), describe(doc_b, value_id)));
                    path.pop();
                }
            }
        }
        _ => {
            out.push(format!(
                "~ {}: {} -> {}",
                path_to_string(path),
                describe(doc_a, id_a),
                describe(doc_b, id_b)
            ));
        }
    }
}

/// A short one-line description of a node's kind/value, for added/removed/type-changed entries.
fn describe(document: &Document, id: NodeId) -> String {
    match document.node(id).map(|n| &n.kind) {
        Some(NodeKind::Scalar(s)) => scalar_display(&s.value, &s.raw),
        Some(NodeKind::Sequence(items)) => format!("[sequence, {} items]", items.len()),
        Some(NodeKind::Mapping(entries)) => format!("{{mapping, {} entries}}", entries.len()),
        Some(NodeKind::Alias(_)) => "*alias".to_string(),
        None => "<missing>".to_string(),
    }
}

fn text_diff(a: &str, b: &str) -> Vec<String> {
    let a_lines: Vec<&str> = a.lines().collect();
    let b_lines: Vec<&str> = b.lines().collect();
    // Classic O(n*m) LCS table — fine for the file sizes this CLI targets.
    let n = a_lines.len();
    let m = b_lines.len();
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a_lines[i] == b_lines[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a_lines[i] == b_lines[j] {
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(format!("-{}", a_lines[i]));
            i += 1;
        } else {
            out.push(format!("+{}", b_lines[j]));
            j += 1;
        }
    }
    while i < n {
        out.push(format!("-{}", a_lines[i]));
        i += 1;
    }
    while j < m {
        out.push(format!("+{}", b_lines[j]));
        j += 1;
    }
    out
}

fn cmd_diff(args: impl Iterator<Item = String>) -> i32 {
    let mut text = false;
    let mut files = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--text" => text = true,
            other if other.starts_with('-') => {
                return usage_error(&format!("unknown flag '{other}' for diff"))
            }
            other => files.push(other.to_string()),
        }
    }

    let (a, b) = match files.as_slice() {
        [a, b] => (a.clone(), b.clone()),
        _ => return usage_error("diff requires exactly two files: diff <A> <B>"),
    };

    let source_a = match fs::read_to_string(&a) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{a}: error reading file: {e}");
            return EXIT_ERROR;
        }
    };
    let source_b = match fs::read_to_string(&b) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{b}: error reading file: {e}");
            return EXIT_ERROR;
        }
    };

    if text {
        let lines = text_diff(&source_a, &source_b);
        if lines.is_empty() {
            println!("no differences");
            return EXIT_OK;
        }
        println!("--- {a}");
        println!("+++ {b}");
        for line in lines {
            println!("{line}");
        }
        return EXIT_CHECK_FAILED;
    }

    let document_a = match tpt_yaml_core::parse(&source_a) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{a}: {e}");
            return EXIT_PARSE;
        }
    };
    let document_b = match tpt_yaml_core::parse(&source_b) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{b}: {e}");
            return EXIT_PARSE;
        }
    };

    let (Some(root_a), Some(root_b)) = (document_a.root(), document_b.root()) else {
        println!("no differences");
        return EXIT_OK;
    };

    let mut path = Vec::new();
    let mut diffs = Vec::new();
    diff_nodes(&document_a, root_a, &document_b, root_b, &mut path, &mut diffs);

    if diffs.is_empty() {
        println!("no differences");
        return EXIT_OK;
    }
    for line in &diffs {
        println!("{line}");
    }
    EXIT_CHECK_FAILED
}

// ---------------------------------------------------------------------------------------------
// shared helpers
// ---------------------------------------------------------------------------------------------

fn usage_error(message: &str) -> i32 {
    eprintln!("tpt-yaml: {message}");
    EXIT_USAGE
}
