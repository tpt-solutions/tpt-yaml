use crate::{Document, NodeId, NodeKind};

/// Emit a compact event representation of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    DocumentStart,
    DocumentEnd,
    Node(NodeId),
    Alias(NodeId),
}

pub fn events(document: &Document) -> Vec<Event> {
    document
        .documents
        .iter()
        .flat_map(|root| {
            let mut out = Vec::new();
            out.push(Event::DocumentStart);
            collect(*root, document, &mut out);
            out.push(Event::DocumentEnd);
            out
        })
        .collect()
}

fn collect(id: NodeId, document: &Document, out: &mut Vec<Event>) {
    if let Some(node) = document.node(id) {
        out.push(match &node.kind {
            NodeKind::Alias(target) => Event::Alias(*target),
            _ => Event::Node(id),
        });
        match &node.kind {
            NodeKind::Mapping(entries) => {
                for (key, value) in entries {
                    collect(*key, document, out);
                    collect(*value, document, out);
                }
            }
            NodeKind::Sequence(items) => {
                for item in items {
                    collect(*item, document, out);
                }
            }
            _ => {}
        }
    }
}
