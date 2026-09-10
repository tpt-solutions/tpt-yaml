use crate::Span;
use alloc::string::{String, ToString};

/// An index into a [`Document`] node arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    pub const ROOT: Self = Self(0);

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Textual trivia associated with a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TriviaKind {
    Comment(String),
    BlankLine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub span: Span,
}

/// The quoting/block style used for a scalar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarStyle {
    Plain,
    SingleQuoted,
    DoubleQuoted,
    BlockLiteral,
    BlockChomped,
}

/// The typed representation of a resolved YAML scalar.
#[derive(Clone, Debug, PartialEq)]
pub enum ScalarValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Timestamp(String),
}

impl ScalarValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) | Self::Timestamp(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scalar {
    pub value: ScalarValue,
    pub style: ScalarStyle,
    /// The source spelling of the scalar, without surrounding quotes.
    pub raw: String,
    pub tag: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Scalar(Scalar),
    Mapping(Vec<(NodeId, NodeId)>),
    Sequence(Vec<NodeId>),
    Alias(NodeId),
}

pub type Node = NodeData;

/// One node in a parse document.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeData {
    pub id: NodeId,
    pub kind: NodeKind,
    pub span: Option<Span>,
    /// Trivia appearing immediately before this node in source order.
    pub trivia: Vec<Trivia>,
    pub anchor: Option<String>,
    pub tag: Option<String>,
}

impl NodeData {
    pub fn new(id: NodeId, kind: NodeKind, span: Option<Span>) -> Self {
        Self { id, kind, span, trivia: Vec::new(), anchor: None, tag: None }
    }
}

/// A parsed YAML document stream.
#[derive(Clone, Debug, PartialEq)]
pub struct Document {
    /// The version selected for the stream.
    pub version: crate::YamlVersion,
    /// Root node ids in document order.
    pub documents: Vec<NodeId>,
    /// Nodes are stored in an arena; IDs are indices into this vector.
    pub nodes: Vec<NodeData>,
}

impl Document {
    pub fn new(version: crate::YamlVersion) -> Self {
        Self { version, documents: Vec::new(), nodes: Vec::new() }
    }

    pub fn add_node(&mut self, node: NodeData) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.nodes.push(NodeData { id, ..node });
        id
    }

    pub fn root(&self) -> Option<NodeId> {
        self.documents.first().copied().filter(|id| self.nodes.get(id.get() as usize).is_some())
    }

    pub fn node(&self, id: NodeId) -> Option<&NodeData> {
        self.nodes.get(id.get() as usize)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut NodeData> {
        self.nodes.get_mut(id.get() as usize)
    }

    pub fn span(&self, id: NodeId) -> Option<Span> {
        self.nodes.get(id.get() as usize).and_then(|node| node.span)
    }

    pub fn is_alias(&self, id: NodeId) -> bool {
        matches!(self.nodes.get(id.get() as usize), Some(NodeData { kind: NodeKind::Alias(_), .. }))
    }

    pub fn version(&self) -> crate::YamlVersion {
        self.version
    }
}

/// Render a node in canonical block YAML.
pub fn pretty_print(root: NodeId, document: &Document) -> String {
    let mut output = String::new();
    render_node(root, document, 0, &mut output);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output
}

fn render_node(id: NodeId, document: &Document, indent: usize, output: &mut String) {
    let Some(node) = document.node(id) else {
        return;
    };
    let prefix = " ".repeat(indent);
    match &node.kind {
        NodeKind::Scalar(scalar) => {
            if matches!(scalar.style, ScalarStyle::BlockLiteral | ScalarStyle::BlockChomped) {
                let indicator = if scalar.style == ScalarStyle::BlockLiteral { "|" } else { ">" };
                output.push_str(&prefix);
                output.push_str(indicator);
                output.push('\n');
                for line in scalar.raw.lines() {
                    output.push_str(&prefix);
                    output.push(' ');
                    output.push_str(line);
                    output.push('\n');
                }
                return;
            }
            output.push_str(&prefix);
            output.push_str(&render_scalar(scalar));
            output.push('\n');
        }
        NodeKind::Mapping(entries) => {
            if entries.is_empty() {
                output.push_str(&prefix);
                output.push_str("{}\n");
                return;
            }
            for (key_id, value_id) in entries {
                let key = document
                    .node(*key_id)
                    .and_then(|key| match &key.kind {
                        NodeKind::Scalar(scalar) => Some(render_scalar(scalar)),
                        _ => None,
                    })
                    .unwrap_or_default();
                let Some(value) = document.node(*value_id) else {
                    output.push_str(&prefix);
                    output.push_str(&key);
                    output.push_str(":\n");
                    continue;
                };
                if is_nonempty_collection(value) {
                    output.push_str(&prefix);
                    output.push_str(&key);
                    output.push_str(":\n");
                    render_node(*value_id, document, indent + 2, output);
                } else {
                    output.push_str(&prefix);
                    output.push_str(&key);
                    output.push_str(": ");
                    output.push_str(&render_inline(value, document));
                    output.push('\n');
                }
            }
        }
        NodeKind::Sequence(items) => {
            if items.is_empty() {
                output.push_str(&prefix);
                output.push_str("[]\n");
                return;
            }
            for item in items {
                let Some(item_node) = document.node(*item) else {
                    continue;
                };
                if is_nonempty_collection(item_node) {
                    output.push_str(&prefix);
                    output.push('-');
                    output.push('\n');
                    render_node(*item, document, indent + 2, output);
                } else {
                    output.push_str(&prefix);
                    output.push_str("- ");
                    output.push_str(&render_inline(item_node, document));
                    output.push('\n');
                }
            }
        }
        NodeKind::Alias(alias) => {
            output.push_str(&prefix);
            output.push('*');
            output
                .push_str(&anchor_for(*alias, document).unwrap_or_else(|| "<unknown>".to_string()));
            output.push('\n');
        }
    }
}

/// Whether `node` is a mapping or sequence with at least one entry/item. Exposed (not just
/// `pub(crate)`) so `tpt-yaml-edit` can reuse the same block-vs-inline decision this
/// pretty-printer makes, rather than duplicating it.
pub fn is_nonempty_collection(node: &NodeData) -> bool {
    match &node.kind {
        NodeKind::Mapping(entries) => !entries.is_empty(),
        NodeKind::Sequence(items) => !items.is_empty(),
        _ => false,
    }
}

/// Renders a node that isn't a non-empty collection: a scalar, an empty mapping/sequence (as
/// flow `{}`/`[]`, since block style has no way to spell "empty"), or an alias (as `*name` —
/// previously this case fell through to the literal text `null`, silently losing the alias).
/// Exposed so `tpt-yaml-edit` can render individual synthesized leaves without duplicating this
/// logic (it mixes calls to this with byte-for-byte blitting of untouched spans).
pub fn render_inline(node: &NodeData, document: &Document) -> String {
    match &node.kind {
        NodeKind::Scalar(scalar) => render_scalar(scalar),
        NodeKind::Mapping(_) => "{}".to_string(),
        NodeKind::Sequence(_) => "[]".to_string(),
        NodeKind::Alias(target) => {
            let mut s = String::from("*");
            s.push_str(&anchor_for(*target, document).unwrap_or_else(|| "<unknown>".to_string()));
            s
        }
    }
}

fn render_scalar(scalar: &Scalar) -> String {
    if let ScalarValue::String(value) = &scalar.value {
        if scalar.style == ScalarStyle::DoubleQuoted {
            let mut escaped = String::from("\"");
            for ch in value.chars() {
                match ch {
                    '"' => escaped.push_str("\\\""),
                    '\\' => escaped.push_str("\\\\"),
                    '\n' => escaped.push_str("\\n"),
                    '\r' => escaped.push_str("\\r"),
                    '\t' => escaped.push_str("\\t"),
                    _ => escaped.push(ch),
                }
            }
            escaped.push('"');
            return escaped;
        }
        if value.is_empty()
            || value.chars().any(|ch| ": #[]{},&*!|>'\"%@`".contains(ch) || ch.is_whitespace())
            || matches!(
                scalar.value,
                ScalarValue::Null
                    | ScalarValue::Bool(_)
                    | ScalarValue::Int(_)
                    | ScalarValue::Float(_)
            )
        {
            let mut quoted = String::from("'");
            quoted.push_str(&value.replace('\'', "''"));
            quoted.push('\'');
            return quoted;
        }
    }
    match &scalar.value {
        ScalarValue::Null => "null".to_string(),
        ScalarValue::Bool(value) => value.to_string(),
        ScalarValue::Int(value) => value.to_string(),
        ScalarValue::Float(value) => value.to_string(),
        ScalarValue::String(value) | ScalarValue::Timestamp(value) => value.clone(),
    }
}

fn anchor_for(id: NodeId, document: &Document) -> Option<String> {
    let mut seen = Vec::new();
    let mut current = id;
    while !seen.contains(&current) {
        seen.push(current);
        let node = document.node(current)?;
        if let Some(anchor) = &node.anchor {
            return Some(anchor.clone());
        }
        if let NodeKind::Alias(target) = &node.kind {
            current = *target;
        } else {
            return None;
        }
    }
    None
}
