//! Minimal YAML language-server example
//!
//! Shows how `tpt-yaml-core`'s per-node [`Span`] information combined with
//! `tpt-yaml-schema`'s [`ValidationReport`] cover the two most-requested
//! YAML language-server capabilities:
//!
//! 1. **Hover** — given a cursor byte offset, find the innermost node whose
//!    span contains it and describe its type and value.
//! 2. **Diagnostics** — validate the document against a JSON Schema and emit
//!    LSP-style `Diagnostic` objects with 0-based line/column positions.
//!
//! Run from this directory:
//! ```
//! cargo run
//! ```
//!
//! A real language server would:
//! - Add a JSON-RPC framing layer (e.g. the `lsp-server` crate) and wire
//!   `textDocument/hover` + `textDocument/publishDiagnostics` to these two
//!   functions.
//! - Keep a per-document cache of `(source, Document, SchemaDocument)` and
//!   re-parse incrementally on `textDocument/didChange`.
//! - Use `tpt-yaml-edit`'s `EditableDocument::set`/`remove`/`render()` to
//!   power code actions (e.g. "Add required field") via
//!   `textDocument/codeAction`.

use tpt_yaml_core::{parse, Document, NodeId, NodeKind, ScalarValue, Span};
use tpt_yaml_schema::SchemaDocument;

// ---------------------------------------------------------------------------
// Hover: find the innermost node whose span contains the cursor
// ---------------------------------------------------------------------------

/// Walk the arena depth-first and return the deepest node whose span
/// contains `cursor_byte`. Falls back to `node` itself if no child matches.
fn find_node_at(doc: &Document, node: NodeId, cursor_byte: usize) -> NodeId {
    let data = match doc.node(node) {
        Some(d) => d,
        None => return node,
    };

    let contains = |id: NodeId| -> bool {
        doc.span(id)
            .map_or(false, |s| s.start <= cursor_byte && cursor_byte < s.end)
    };

    match &data.kind {
        NodeKind::Mapping(entries) => {
            for &(k, v) in entries {
                if contains(k) {
                    return find_node_at(doc, k, cursor_byte);
                }
                if contains(v) {
                    return find_node_at(doc, v, cursor_byte);
                }
            }
        }
        NodeKind::Sequence(items) => {
            for &item in items {
                if contains(item) {
                    return find_node_at(doc, item, cursor_byte);
                }
            }
        }
        NodeKind::Alias(target) => {
            // The cursor is over the *alias text* (`*anchor`), not the
            // definition site — don't recurse into the target's span.
            let _ = target;
        }
        NodeKind::Scalar(_) => {}
    }

    node
}

struct HoverInfo {
    yaml_type: &'static str,
    value_preview: String,
    span: Option<Span>,
}

fn hover(doc: &Document, cursor_byte: usize) -> HoverInfo {
    let root = match doc.root() {
        Some(r) => r,
        None => {
            return HoverInfo {
                yaml_type: "empty",
                value_preview: "(empty document)".into(),
                span: None,
            }
        }
    };

    let node = find_node_at(doc, root, cursor_byte);
    let data = doc.node(node).expect("node from this document");

    let (yaml_type, value_preview) = match &data.kind {
        NodeKind::Scalar(s) => {
            let t = match &s.value {
                ScalarValue::Null => "null",
                ScalarValue::Bool(_) => "boolean",
                ScalarValue::Int(_) => "integer",
                ScalarValue::Float(_) => "float",
                ScalarValue::String(_) => "string",
                ScalarValue::Timestamp(_) => "timestamp",
            };
            (t, format!("{:?}", s.value))
        }
        NodeKind::Mapping(entries) => ("mapping", format!("{{{} entries}}", entries.len())),
        NodeKind::Sequence(items) => ("sequence", format!("[{} items]", items.len())),
        NodeKind::Alias(target) => ("alias", format!("*alias → node #{}", target.get())),
    };

    HoverInfo {
        yaml_type,
        value_preview,
        span: doc.span(node),
    }
}

// ---------------------------------------------------------------------------
// Diagnostics: map ValidationIssues to LSP-style Diagnostic objects
// ---------------------------------------------------------------------------

struct Diagnostic {
    path: String,
    message: String,
    /// 0-based line (LSP convention)
    line: usize,
    /// 0-based column (LSP convention)
    column: usize,
}

fn lsp_diagnostics(doc: &Document, schema: &SchemaDocument) -> Vec<Diagnostic> {
    let root = match doc.root() {
        Some(r) => r,
        None => return vec![],
    };
    let report = schema.validate(doc, root);
    report
        .issues()
        .iter()
        .map(|issue| Diagnostic {
            path: issue.path.clone(),
            message: issue.message.clone(),
            // tpt-yaml-core spans are 1-based; LSP is 0-based
            line: issue.span.map_or(0, |s| s.line.saturating_sub(1)),
            column: issue.span.map_or(0, |s| s.column.saturating_sub(1)),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Demo
// ---------------------------------------------------------------------------

fn main() {
    let schema_src = r#"
type: object
required: [server, database]
properties:
  server:
    type: object
    required: [host, port]
    properties:
      host: {type: string}
      port: {type: integer, minimum: 1, maximum: 65535}
      tls:  {type: boolean}
  database:
    type: object
    required: [url]
    properties:
      url:             {type: string, format: uri}
      max_connections: {type: integer, minimum: 1}
  logging:
    type: object
    properties:
      level:  {type: string, enum: [debug, info, warn, error]}
      format: {type: string, enum: [text, json]}
"#;

    let schema = SchemaDocument::from_yaml_str(schema_src).expect("schema compiles");

    println!("=== tpt-yaml language-server example ===\n");

    // --- valid document ---
    let valid_source = "server:\n  host: localhost\n  port: 8080\n  tls: true\ndatabase:\n  url: postgres://db/myapp\n  max_connections: 25\nlogging:\n  level: info\n  format: json\n";
    let doc = parse(valid_source).expect("valid YAML");

    // Byte offsets into valid_source (0-based):
    //   0  = 's' of "server"         → the "server" key scalar
    //   9  = ' ' before "host"       → the server mapping value node
    //  10  = 'h' of "host"           → the "host" key scalar
    //  16  = 'l' of "localhost"      → the "localhost" value scalar
    //  51  = 'd' of "database"       → the "database" key scalar
    println!("Hover positions in valid document:");
    for (byte, label) in [
        (0, "'server' key"),
        (9, "server mapping (the value node)"),
        (10, "'host' key"),
        (16, "'localhost' value"),
        (51, "'database' key"),
    ] {
        let info = hover(&doc, byte);
        let loc = info
            .span
            .map(|s| format!(" [line {}, col {}]", s.line, s.column))
            .unwrap_or_default();
        println!(
            "  byte {:3}  ({label}): {ty} = {val}{loc}",
            byte,
            ty = info.yaml_type,
            val = info.value_preview,
            loc = loc,
        );
    }

    println!("\nDiagnostics for valid document:");
    let diags = lsp_diagnostics(&doc, &schema);
    if diags.is_empty() {
        println!("  (none — document is valid)");
    }

    // --- invalid document ---
    let invalid_source =
        "server:\n  host: 12345\n  port: 99999\ndatabase:\n  url: not-a-uri\nlogging:\n  level: VERBOSE\n";

    println!("\nDiagnostics for invalid document:");
    println!("  source: {:?}", invalid_source);
    let bad_doc = parse(invalid_source).expect("parses fine (validity is the schema's job)");
    let bad_diags = lsp_diagnostics(&bad_doc, &schema);
    for d in &bad_diags {
        println!(
            "  [{line}:{col}] {path}: {msg}",
            line = d.line,
            col = d.column,
            path = d.path,
            msg = d.message,
        );
    }
    if bad_diags.is_empty() {
        println!("  (none)");
    }

    println!();
    println!("What a real language server would do next:");
    println!("  - Wrap these functions in JSON-RPC handlers (e.g. via `lsp-server`)");
    println!("  - Wire `textDocument/hover` → hover()");
    println!("  - Wire `textDocument/publishDiagnostics` → lsp_diagnostics()");
    println!("  - Wire `textDocument/codeAction` → tpt_yaml_edit::EditableDocument::set()");
}
