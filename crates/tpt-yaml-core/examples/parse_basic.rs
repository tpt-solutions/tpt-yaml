//! Parse a small YAML document and walk the resulting arena, printing each node's shape.
//!
//! Run with: `cargo run --example parse_basic -p tpt-yaml-core`

use tpt_yaml_core::{Document, NodeId, NodeKind};

fn main() {
    let source = "\
name: tpt-yaml
version: 1
keywords:
  - yaml
  - parser
  - rust
metadata:
  stable: true
";

    let document = tpt_yaml_core::parse(source).expect("valid YAML");

    println!("resolved version: {:?}", document.version());

    let root = document.root().expect("document has a root node");
    walk(&document, root, 0);
}

/// Recursively print a node and its children, indented by depth.
fn walk(document: &Document, id: NodeId, depth: usize) {
    let indent = "  ".repeat(depth);
    let node = document.node(id).expect("node id from this document is valid");

    match &node.kind {
        NodeKind::Scalar(scalar) => {
            println!("{indent}Scalar({:?})", scalar.value);
        }
        NodeKind::Mapping(entries) => {
            println!("{indent}Mapping ({} entries)", entries.len());
            for (key, value) in entries {
                println!("{indent}  key:");
                walk(document, *key, depth + 2);
                println!("{indent}  value:");
                walk(document, *value, depth + 2);
            }
        }
        NodeKind::Sequence(items) => {
            println!("{indent}Sequence ({} items)", items.len());
            for item in items {
                walk(document, *item, depth + 1);
            }
        }
        NodeKind::Alias(target) => {
            println!("{indent}Alias -> {target:?}");
            walk(document, *target, depth + 1);
        }
    }
}
