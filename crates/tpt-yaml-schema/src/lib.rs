//! A JSON Schema 2020-12 *subset* validator for YAML/JSON documents, built directly on
//! [`tpt_yaml_core`]'s arena. Schemas load from YAML (dogfooding [`tpt_yaml_core::parse`]) or
//! JSON (the `json-schema` feature), documents validate against the compiled IR, and every
//! [`ValidationIssue`] carries the offending node's span for editor surfacing.
//!
//! Supported keywords: `type`, `enum`, `const`, `pattern`/`minLength`/`maxLength` (strings),
//! `minimum`/`maximum`/`exclusiveMinimum`/`exclusiveMaximum`/`multipleOf` (numbers),
//! `items`/`minItems`/`maxItems`/`uniqueItems` (arrays), `properties`/`required`/
//! `additionalProperties`/`minProperties`/`maxProperties` (objects), `oneOf`/`anyOf`/`allOf`/
//! `not`, and internal `$ref` (`#/$defs/<name>` or `#/<name>`).

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod pattern;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use tpt_yaml_core::{Document, NodeId, NodeKind, Span};

/// The error type for schema loading and compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The schema source failed to parse as YAML.
    Parse(String),
    /// The schema parsed but isn't a valid schema document (wrong shape, unknown `$ref`, …).
    InvalidSchema(String),
    /// The `pattern` value used regex syntax outside the supported subset.
    InvalidPattern(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "schema parse error: {msg}"),
            Self::InvalidSchema(msg) => write!(f, "invalid schema: {msg}"),
            Self::InvalidPattern(msg) => write!(f, "invalid pattern: {msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl From<tpt_yaml_core::YamlError> for Error {
    fn from(e: tpt_yaml_core::YamlError) -> Self {
        Self::Parse(e.to_string())
    }
}

/// The JSON types a schema can constrain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonType {
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

impl JsonType {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "null" => Self::Null,
            "boolean" => Self::Boolean,
            "number" | "integer" => Self::Number,
            "string" => Self::String,
            "array" => Self::Array,
            "object" => Self::Object,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Boolean => "boolean",
            Self::Number => "number",
            Self::String => "string",
            Self::Array => "array",
            Self::Object => "object",
        }
    }

    /// The JSON type of a document node (an `integer` is a `Number`).
    fn of_node(document: &Document, id: NodeId) -> Option<Self> {
        Some(match &document.node(id)?.kind {
            NodeKind::Scalar(scalar) => match scalar.value {
                tpt_yaml_core::ScalarValue::Null => Self::Null,
                tpt_yaml_core::ScalarValue::Bool(_) => Self::Boolean,
                tpt_yaml_core::ScalarValue::Int(_) | tpt_yaml_core::ScalarValue::Float(_) => {
                    Self::Number
                }
                tpt_yaml_core::ScalarValue::String(_) | tpt_yaml_core::ScalarValue::Timestamp(_) => {
                    Self::String
                }
            },
            NodeKind::Mapping(_) => Self::Object,
            NodeKind::Sequence(_) => Self::Array,
            NodeKind::Alias(target) => return JsonType::of_node(document, *target),
        })
    }
}

/// The compiled schema IR.
#[derive(Clone, Debug, PartialEq)]
pub enum Schema {
    /// The `true`/`false` schema: `false` rejects everything, `true` accepts everything.
    Constant(bool),
    Type(JsonType),
    Enum(Vec<EnumValue>),
    Const(EnumValue),
    String {
        min_length: Option<usize>,
        max_length: Option<usize>,
        pattern: Option<pattern::Pattern>,
    },
    Number {
        minimum: Option<f64>,
        maximum: Option<f64>,
        exclusive_minimum: Option<f64>,
        exclusive_maximum: Option<f64>,
        multiple_of: Option<f64>,
    },
    Array {
        items: Option<Box<Schema>>,
        min_items: Option<usize>,
        max_items: Option<usize>,
        unique_items: bool,
    },
    Object {
        properties: Vec<(String, Schema)>,
        required: Vec<String>,
        additional: Additional,
        min_properties: Option<usize>,
        max_properties: Option<usize>,
    },
    Not(Box<Schema>),
    OneOf(Vec<Schema>),
    AnyOf(Vec<Schema>),
    AllOf(Vec<Schema>),
    /// An internal reference: `#/$defs/<name>` or `#/<name>`.
    Ref(String),
}

/// How an object treats properties not listed in `properties`.
#[derive(Clone, Debug, PartialEq)]
pub enum Additional {
    Allow,
    Forbid,
    Schema(Box<Schema>),
}

/// A value usable in `enum`/`const` — the compiled form of a scalar (or a stringified compound
/// value, since this subset constrains enums to scalars).
#[derive(Clone, Debug, PartialEq)]
pub enum EnumValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl EnumValue {
    fn from_node(document: &Document, id: NodeId) -> Option<Self> {
        Some(match &document.node(id)?.kind {
            NodeKind::Scalar(scalar) => match &scalar.value {
                tpt_yaml_core::ScalarValue::Null => Self::Null,
                tpt_yaml_core::ScalarValue::Bool(b) => Self::Bool(*b),
                tpt_yaml_core::ScalarValue::Int(i) => Self::Number(*i as f64),
                tpt_yaml_core::ScalarValue::Float(f) => Self::Number(*f),
                tpt_yaml_core::ScalarValue::String(s) | tpt_yaml_core::ScalarValue::Timestamp(s) => {
                    Self::String(s.clone())
                }
            },
            _ => None?,
        })
    }

    /// Whether the document node's value equals this enum value (`Number` compares by value so
    /// `Int(3)` and `Float(3.0)` match; `String`/`Timestamp` compare textually).
    fn node_matches(&self, document: &Document, id: NodeId) -> bool {
        let Some(node) = document.node(id) else { return false };
        match (&node.kind, self) {
            (NodeKind::Scalar(scalar), Self::Null) => {
                matches!(scalar.value, tpt_yaml_core::ScalarValue::Null)
            }
            (NodeKind::Scalar(scalar), Self::Bool(b)) => scalar.value.as_bool() == Some(*b),
            (NodeKind::Scalar(scalar), Self::Number(n)) => {
                match scalar.value {
                    tpt_yaml_core::ScalarValue::Int(i) => i as f64 == *n,
                    tpt_yaml_core::ScalarValue::Float(f) => f == *n,
                    _ => false,
                }
            }
            (NodeKind::Scalar(scalar), Self::String(s)) => scalar.value.as_str() == Some(s.as_str()),
            _ => false,
        }
    }
}

/// A loaded schema document: the compiled root schema plus any `$defs`.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaDocument {
    pub root: Schema,
    pub defs: BTreeMap<String, Schema>,
}

impl SchemaDocument {
    /// Loads a schema from YAML source, dogfooding [`tpt_yaml_core::parse`] for the parse.
    pub fn from_yaml_str(source: &str) -> Result<Self, Error> {
        let document = tpt_yaml_core::parse(source)?;
        let root = document.root().ok_or_else(|| Error::InvalidSchema("empty document".to_string()))?;
        let (root, defs) = compile_root(&document, root)?;
        Ok(Self { root, defs })
    }

    /// Loads a schema from JSON source (the `json-schema` feature, backed by `serde_json`).
    #[cfg(feature = "json-schema")]
    pub fn from_json_str(source: &str) -> Result<Self, Error> {
        let value: serde_json::Value =
            serde_json::from_str(source).map_err(|e| Error::Parse(format!("{e}")))?;
        let (root, defs) = compile_json(&value)?;
        Ok(Self { root, defs })
    }

    /// Validates `root` of `document` with default [`ValidationSettings`].
    pub fn validate(&self, document: &Document, root: NodeId) -> ValidationReport {
        self.validate_with(document, root, &ValidationSettings::default())
    }

    /// Validates `root` of `document`, gating which checks run via `settings`.
    pub fn validate_with(
        &self,
        document: &Document,
        root: NodeId,
        settings: &ValidationSettings,
    ) -> ValidationReport {
        let mut report = ValidationReport::default();
        let mut path = String::new();
        validate_node(
            document,
            root,
            &self.root,
            &self.defs,
            settings,
            0,
            &mut path,
            &mut report,
        );
        report.deterministic();
        report
    }

    /// Validates a `T: serde::Serialize` value directly (the `typed` feature): the value is
    /// routed through `tpt-yaml-serde`'s serializer into the same arena the rest of the family
    /// uses, then validated as an ordinary document.
    #[cfg(feature = "typed")]
    pub fn validate_typed<T: serde::Serialize + ?Sized>(
        &self,
        value: &T,
    ) -> Result<ValidationReport, Error> {
        let mut serializer = tpt_yaml_serde::Serializer::new();
        let node = value
            .serialize(&mut serializer)
            .map_err(|e| Error::InvalidSchema(format!("serialization failed: {e}")))?;
        let (document, node) = serializer.into_document(node);
        Ok(self.validate(&document, node))
    }
}

/// The string-keyed shape a `properties` entry needs.
fn entry_name(name: &str) -> String {
    name.to_string()
}

#[cfg(feature = "json-schema")]
fn compile_json(value: &serde_json::Value) -> Result<(Schema, BTreeMap<String, Schema>), Error> {
    let Some(obj) = value.as_object() else {
        return Err(Error::InvalidSchema("schema root must be an object".to_string()));
    };
    let mut defs = BTreeMap::new();
    if let Some(defs_value) = obj.get("$defs") {
        let Some(defs_obj) = defs_value.as_object() else {
            return Err(Error::InvalidSchema("$defs must be an object".to_string()));
        };
        for (name, def) in defs_obj {
            defs.insert(name.clone(), json_to_schema(def)?);
        }
    }
    Ok((json_mapping_entries(obj, &defs, true)?, defs))
}

#[cfg(feature = "json-schema")]
fn json_to_schema(value: &serde_json::Value) -> Result<Schema, Error> {
    let Some(obj) = value.as_object() else {
        return Err(Error::InvalidSchema("schema must be an object".to_string()));
    };
    json_mapping_entries(obj, &BTreeMap::new(), false)
}

#[cfg(feature = "json-schema")]
fn json_mapping_entries(
    obj: &serde_json::Map<String, serde_json::Value>,
    defs: &BTreeMap<String, Schema>,
    is_root: bool,
) -> Result<Schema, Error> {
    if let Some(serde_json::Value::String(name)) = obj.get("$ref") {
        let internal = name
            .strip_prefix("#/$defs/")
            .or_else(|| name.strip_prefix("#/"))
            .ok_or_else(|| Error::InvalidSchema(format!("only internal $refs are supported: {name}")))?;
        if !is_root && !defs.contains_key(internal) {
            return Err(Error::InvalidSchema(format!("unknown $ref target: {name}")));
        }
        return Ok(Schema::Ref(name.clone()));
    }
    if let Some(serde_json::Value::String(name)) = obj.get("type") {
        let json_type = JsonType::from_name(name)
            .ok_or_else(|| Error::InvalidSchema(format!("unknown type: {name}")))?;
        let opt_usize = |key: &str| obj.get(key).and_then(|v| v.as_u64()).map(|v| v as usize);
        let opt_f64 = |key: &str| obj.get(key).and_then(|v| v.as_f64());
        return Ok(match json_type {
            JsonType::Null | JsonType::Boolean => Schema::Type(json_type),
            JsonType::String => Schema::String {
                min_length: opt_usize("minLength"),
                max_length: opt_usize("maxLength"),
                pattern: match obj.get("pattern") {
                    Some(serde_json::Value::String(source)) => pattern::compile(source)
                        .map(Some)
                        .map_err(|_| Error::InvalidPattern(source.clone()))?,
                    _ => None,
                },
            },
            JsonType::Number => Schema::Number {
                minimum: opt_f64("minimum"),
                maximum: opt_f64("maximum"),
                exclusive_minimum: opt_f64("exclusiveMinimum"),
                exclusive_maximum: opt_f64("exclusiveMaximum"),
                multiple_of: opt_f64("multipleOf"),
            },
            JsonType::Array => Schema::Array {
                items: match obj.get("items") {
                    Some(v) => Some(Box::new(json_to_schema(v)?)),
                    None => None,
                },
                min_items: opt_usize("minItems"),
                max_items: opt_usize("maxItems"),
                unique_items: obj.get("uniqueItems").and_then(|v| v.as_bool()).unwrap_or(false),
            },
            JsonType::Object => Schema::Object {
                properties: match obj.get("properties") {
                    Some(serde_json::Value::Object(props)) => props
                        .iter()
                        .map(|(name, v)| Ok::<_, Error>((name.clone(), json_to_schema(v)?)))
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(_) => return Err(Error::InvalidSchema("properties must be an object".to_string())),
                    None => Vec::new(),
                },
                required: match obj.get("required") {
                    Some(serde_json::Value::Array(items)) => items
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => Ok::<_, Error>(s.clone()),
                            _ => Err(Error::InvalidSchema("required entries must be strings".to_string())),
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(_) => return Err(Error::InvalidSchema("required must be an array".to_string())),
                    None => Vec::new(),
                },
                additional: match obj.get("additionalProperties") {
                    Some(serde_json::Value::Bool(false)) => Additional::Forbid,
                    Some(serde_json::Value::Bool(true)) | None => Additional::Allow,
                    Some(other) => Additional::Schema(Box::new(json_to_schema(other)?)),
                },
                min_properties: opt_usize("minProperties"),
                max_properties: opt_usize("maxProperties"),
            },
        });
    }
    for name in ["oneOf", "anyOf", "allOf"] {
        if let Some(serde_json::Value::Array(items)) = obj.get(name) {
            let schemas = items.iter().map(json_to_schema).collect::<Result<Vec<_>, _>>()?;
            return Ok(match name {
                "oneOf" => Schema::OneOf(schemas),
                "anyOf" => Schema::AnyOf(schemas),
                _ => Schema::AllOf(schemas),
            });
        }
    }
    if let Some(inner) = obj.get("not") {
        return Ok(Schema::Not(Box::new(json_to_schema(inner)?)));
    }
    Ok(Schema::Constant(true))
}
fn compile_root(
    document: &Document,
    root: NodeId,
) -> Result<(Schema, BTreeMap<String, Schema>), Error> {
    let Some(NodeKind::Mapping(entries)) = document.node(root).map(|n| &n.kind) else {
        return Err(Error::InvalidSchema("schema root must be a mapping".to_string()));
    };
    let mut defs = BTreeMap::new();
    // `$defs` compiles first so internal `$ref`s resolve.
    for (key_id, value_id) in entries {
        let key = scalar_key(document, *key_id);
        if key.as_deref() == Some("$defs") {
            if let Some(NodeKind::Mapping(def_entries)) = document.node(*value_id).map(|n| &n.kind)
            {
                for (def_key_id, def_value_id) in def_entries {
                    let def_name = scalar_key(document, *def_key_id)
                        .ok_or_else(|| Error::InvalidSchema("$defs keys must be strings".to_string()))?;
                    let schema = compile_schema(document, *def_value_id)?;
                    defs.insert(def_name, schema);
                }
            }
        }
    }
    // Compile the root schema *without* re-entering `$defs` (already done above).
    let root_schema = compile_mapping_entries(document, entries, &defs, true)?;
    Ok((root_schema, defs))
}

fn scalar_key(document: &Document, id: NodeId) -> Option<String> {
    match &document.node(id)?.kind {
        NodeKind::Scalar(scalar) => scalar.value.as_str().map(|s| s.to_string()),
        _ => None,
    }
}

fn compile_schema(document: &Document, id: NodeId) -> Result<Schema, Error> {
    let Some(NodeKind::Mapping(entries)) = document.node(id).map(|n| &n.kind) else {
        return Err(Error::InvalidSchema("schema must be a mapping".to_string()));
    };
    compile_mapping_entries(document, entries, &BTreeMap::new(), false)
}

fn compile_mapping_entries(
    document: &Document,
    entries: &[(NodeId, NodeId)],
    defs: &BTreeMap<String, Schema>,
    is_root: bool,
) -> Result<Schema, Error> {
    let by_key = |name: &str| {
        entries
            .iter()
            .find(|&&(k, _)| scalar_key(document, k).as_deref() == Some(name))
            .map(|&(_, v)| v)
    };

    if let Some(ref_id) = by_key("$ref") {
        let name = scalar_key(document, ref_id)
            .ok_or_else(|| Error::InvalidSchema("$ref must be a string".to_string()))?;
        let internal = name
            .strip_prefix("#/$defs/")
            .or_else(|| name.strip_prefix("#/"))
            .ok_or_else(|| {
                Error::InvalidSchema(format!("only internal $refs are supported: {name}"))
            })?;
        if !is_root && !defs.contains_key(internal) && !entries_is_def_of_self(defs, internal) {
            return Err(Error::InvalidSchema(format!("unknown $ref target: {name}")));
        }
        return Ok(Schema::Ref(name));
    }

    if let Some(type_id) = by_key("type") {
        let name = scalar_key(document, type_id)
            .ok_or_else(|| Error::InvalidSchema("type must be a string".to_string()))?;
        let json_type = JsonType::from_name(&name)
            .ok_or_else(|| Error::InvalidSchema(format!("unknown type: {name}")))?;
        return compile_typed(document, entries, json_type);
    }

    if let Some(enum_id) = by_key("enum") {
        let Some(NodeKind::Sequence(items)) = document.node(enum_id).map(|n| &n.kind) else {
            return Err(Error::InvalidSchema("enum must be a sequence".to_string()));
        };
        let values = items
            .iter()
            .map(|&item| EnumValue::from_node(document, item))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| Error::InvalidSchema("enum values must be scalars".to_string()))?;
        return Ok(Schema::Enum(values));
    }

    if let Some(const_id) = by_key("const") {
        let value = EnumValue::from_node(document, const_id)
            .ok_or_else(|| Error::InvalidSchema("const must be a scalar".to_string()))?;
        return Ok(Schema::Const(value));
    }

    for (name, variant) in [
        ("oneOf", 0),
        ("anyOf", 1),
        ("allOf", 2),
    ] {
        if let Some(seq_id) = by_key(name) {
            let Some(NodeKind::Sequence(items)) = document.node(seq_id).map(|n| &n.kind) else {
                return Err(Error::InvalidSchema(format!("{name} must be a sequence")));
            };
            let schemas = items
                .iter()
                .map(|&item| compile_schema(document, item))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(match variant {
                0 => Schema::OneOf(schemas),
                1 => Schema::AnyOf(schemas),
                _ => Schema::AllOf(schemas),
            });
        }
    }

    if let Some(not_id) = by_key("not") {
        return Ok(Schema::Not(Box::new(compile_schema(document, not_id)?)));
    }

    Ok(Schema::Constant(true))
}

/// Whether the `$ref` target could be this schema itself (a `def` of itself) — used only to let
/// recursive-looking refs compile; runtime cycle detection still caps recursion.
fn entries_is_def_of_self(defs: &BTreeMap<String, Schema>, _name: &str) -> bool {
    defs.is_empty()
}

fn compile_typed(
    document: &Document,
    entries: &[(NodeId, NodeId)],
    json_type: JsonType,
) -> Result<Schema, Error> {
    let by_key = |name: &str| {
        entries
            .iter()
            .find(|&&(k, _)| scalar_key(document, k).as_deref() == Some(name))
            .map(|&(_, v)| v)
    };
    Ok(match json_type {
        JsonType::Null | JsonType::Boolean => Schema::Type(json_type),
        JsonType::String => Schema::String {
            min_length: optional_usize(document, by_key("minLength")),
            max_length: optional_usize(document, by_key("maxLength")),
            pattern: optional_pattern(document, by_key("pattern"))?,
        },
        JsonType::Number => Schema::Number {
            minimum: optional_f64(document, by_key("minimum")),
            maximum: optional_f64(document, by_key("maximum")),
            exclusive_minimum: optional_f64(document, by_key("exclusiveMinimum")),
            exclusive_maximum: optional_f64(document, by_key("exclusiveMaximum")),
            multiple_of: optional_f64(document, by_key("multipleOf")),
        },
        JsonType::Array => Schema::Array {
            items: by_key("items").map(|id| compile_schema(document, id)).transpose()?.map(Box::new),
            min_items: optional_usize(document, by_key("minItems")),
            max_items: optional_usize(document, by_key("maxItems")),
            unique_items: by_key("uniqueItems")
                .and_then(|id| scalar_key(document, id))
                .as_deref()
                == Some("true"),
        },
        JsonType::Object => Schema::Object {
            properties: match by_key("properties") {
                Some(id) => match document.node(id).map(|n| &n.kind) {
                    Some(NodeKind::Mapping(prop_entries)) => prop_entries
                        .iter()
                        .map(|&(k, v)| {
                            let name = scalar_key(document, k).ok_or_else(|| {
                                Error::InvalidSchema("property keys must be strings".to_string())
                            })?;
                            Ok::<(String, Schema), Error>((entry_name(&name), compile_schema(document, v)?))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(Error::InvalidSchema("properties must be a mapping".to_string())),
                },
                None => Vec::new(),
            },
            required: match by_key("required") {
                Some(id) => match document.node(id).map(|n| &n.kind) {
                    Some(NodeKind::Sequence(items)) => items
                        .iter()
                        .map(|&item| {
                            scalar_key(document, item).ok_or_else(|| {
                                Error::InvalidSchema("required entries must be strings".to_string())
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => return Err(Error::InvalidSchema("required must be a sequence".to_string())),
                },
                None => Vec::new(),
            },
            additional: match by_key("additionalProperties") {
                Some(id) => match document.node(id).map(|n| &n.kind) {
                    // `false`/`true` are resolved YAML booleans, not strings — match on the
                    // resolved value, not the raw text.
                    Some(NodeKind::Scalar(scalar)) => match scalar.value {
                        tpt_yaml_core::ScalarValue::Bool(false) => Additional::Forbid,
                        tpt_yaml_core::ScalarValue::Bool(true) => Additional::Allow,
                        _ => Additional::Schema(Box::new(compile_schema(document, id)?)),
                    },
                    _ => Additional::Schema(Box::new(compile_schema(document, id)?)),
                },
                None => Additional::Allow,
            },
            min_properties: optional_usize(document, by_key("minProperties")),
            max_properties: optional_usize(document, by_key("maxProperties")),
        },
    })
}

fn optional_usize(document: &Document, id: Option<NodeId>) -> Option<usize> {
    let id = id?;
    match &document.node(id)?.kind {
        NodeKind::Scalar(scalar) => scalar.value.as_i64().map(|v| v.max(0) as usize),
        _ => None,
    }
}

fn optional_f64(document: &Document, id: Option<NodeId>) -> Option<f64> {
    let id = id?;
    match &document.node(id)?.kind {
        NodeKind::Scalar(scalar) => match scalar.value {
            tpt_yaml_core::ScalarValue::Int(v) => Some(v as f64),
            tpt_yaml_core::ScalarValue::Float(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

fn optional_pattern(document: &Document, id: Option<NodeId>) -> Result<Option<pattern::Pattern>, Error> {
    let Some(id) = id else { return Ok(None) };
    let source = match &document.node(id).map(|n| &n.kind) {
        Some(NodeKind::Scalar(scalar)) => {
            scalar.value.as_str().map(|s| s.to_string()).ok_or_else(|| {
                Error::InvalidSchema("pattern must be a string".to_string())
            })?
        }
        _ => return Err(Error::InvalidSchema("pattern must be a string".to_string())),
    };
    pattern::compile(&source)
        .map(Some)
        .map_err(|_| Error::InvalidPattern(source))
}
/// The category of a [`ValidationIssue`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    TypeMismatch,
    EnumMismatch,
    ConstMismatch,
    PatternMismatch,
    MinLength,
    MaxLength,
    Minimum,
    Maximum,
    ExclusiveMinimum,
    ExclusiveMaximum,
    MultipleOf,
    MinItems,
    MaxItems,
    UniqueItems,
    RequiredMissing,
    AdditionalProperty,
    MinProperties,
    MaxProperties,
    OneOfMismatch,
    OneOfAmbiguous,
    AnyOfMismatch,
    AllOfMismatch,
    NotMatched,
    RefUnresolved,
    RefCycle,
    DepthExceeded,
}

impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let value = match self {
            Self::TypeMismatch => "type mismatch",
            Self::EnumMismatch => "enum mismatch",
            Self::ConstMismatch => "const mismatch",
            Self::PatternMismatch => "pattern mismatch",
            Self::MinLength => "below minLength",
            Self::MaxLength => "above maxLength",
            Self::Minimum => "below minimum",
            Self::Maximum => "above maximum",
            Self::ExclusiveMinimum => "at or below exclusiveMinimum",
            Self::ExclusiveMaximum => "at or above exclusiveMaximum",
            Self::MultipleOf => "not a multiple of multipleOf",
            Self::MinItems => "below minItems",
            Self::MaxItems => "above maxItems",
            Self::UniqueItems => "duplicate items with uniqueItems",
            Self::RequiredMissing => "required property missing",
            Self::AdditionalProperty => "additional property not allowed",
            Self::MinProperties => "below minProperties",
            Self::MaxProperties => "above maxProperties",
            Self::OneOfMismatch => "matched none of oneOf",
            Self::OneOfAmbiguous => "matched more than one of oneOf",
            Self::AnyOfMismatch => "matched none of anyOf",
            Self::AllOfMismatch => "failed one of allOf",
            Self::NotMatched => "matched `not`",
            Self::RefUnresolved => "$ref target not found",
            Self::RefCycle => "$ref recursion exceeded depth",
            Self::DepthExceeded => "validation depth exceeded",
        };
        f.write_str(value)
    }
}

/// One validation failure: where it happened (dotted path + node id + span) and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationIssue {
    /// Dotted path from the validated root, e.g. `config.tags.0` (empty at the root).
    pub path: String,
    pub kind: IssueKind,
    pub message: String,
    /// The node the issue was raised at, for span lookup via `document.span(node)`.
    pub node: NodeId,
    /// The node's source span, captured at issue time (editor-friendly).
    pub span: Option<Span>,
}

/// The result of validating a document: ordered, deterministic issue list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValidationReport {
    issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }

    pub fn issue_count(&self) -> usize {
        self.issues.len()
    }

    /// Sorts issues by (path, kind) so two runs over the same document compare equal — the
    /// determinism invariant the proptest asserts.
    fn deterministic(&mut self) {
        self.issues
            .sort_by(|a, b| (&a.path, a.kind as u32).cmp(&(&b.path, b.kind as u32)));
    }
}

/// Gates which schema checks run. The local builder idiom (own type, not imported).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationSettings {
    pub types: bool,
    pub enums: bool,
    pub strings: bool,
    pub numbers: bool,
    pub arrays: bool,
    pub objects: bool,
    pub combinators: bool,
    pub refs: bool,
    /// Cap on `$ref`/combinator recursion; `DepthExceeded` past this.
    pub max_depth: usize,
}

impl Default for ValidationSettings {
    fn default() -> Self {
        Self {
            types: true,
            enums: true,
            strings: true,
            numbers: true,
            arrays: true,
            objects: true,
            combinators: true,
            refs: true,
            max_depth: 64,
        }
    }
}

impl ValidationSettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables only the listed check groups (builder style; chains).
    pub fn only(mut self, groups: &[&str]) -> Self {
        self.types = groups.contains(&"types");
        self.enums = groups.contains(&"enums");
        self.strings = groups.contains(&"strings");
        self.numbers = groups.contains(&"numbers");
        self.arrays = groups.contains(&"arrays");
        self.objects = groups.contains(&"objects");
        self.combinators = groups.contains(&"combinators");
        self.refs = groups.contains(&"refs");
        self
    }

    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }
}

#[allow(clippy::too_many_arguments)]
fn validate_node(
    document: &Document,
    node: NodeId,
    schema: &Schema,
    defs: &BTreeMap<String, Schema>,
    settings: &ValidationSettings,
    depth: usize,
    path: &mut String,
    report: &mut ValidationReport,
) {
    if depth > settings.max_depth {
        issue(document, node, path, IssueKind::DepthExceeded, "recursion limit", report);
        return;
    }
    match schema {
        Schema::Constant(ok) => {
            if !ok {
                issue(document, node, path, IssueKind::NotMatched, "`false` schema", report);
            }
        }
        Schema::Type(json_type) => {
            if settings.types {
                let actual = JsonType::of_node(document, node);
                if actual.is_none() {
                    issue(document, node, path, IssueKind::RefCycle, "alias could not resolve", report);
                } else if actual != Some(*json_type) {
                    let actual_name = actual.map(|t| t.name()).unwrap_or("unknown");
                    issue(
                        document,
                        node,
                        path,
                        IssueKind::TypeMismatch,
                        format!("expected {}, found {actual_name}", json_type.name()),
                        report,
                    );
                }
            }
        }
        Schema::Enum(values) => {
            if settings.enums && !values.iter().any(|v| v.node_matches(document, node)) {
                issue(document, node, path, IssueKind::EnumMismatch, "value not in enum", report);
            }
        }
        Schema::Const(value) => {
            if settings.enums && !value.node_matches(document, node) {
                issue(document, node, path, IssueKind::ConstMismatch, "value not equal to const", report);
            }
        }
        Schema::String { min_length, max_length, pattern: pat } => {
            if settings.strings {
                // `type: string` compiles to this variant, so the arm carries the type check
                // itself: a non-string is a TypeMismatch and string constraints don't apply.
                if JsonType::of_node(document, node) != Some(JsonType::String) {
                    let actual = JsonType::of_node(document, node).map(|t| t.name()).unwrap_or("unknown");
                    issue(document, node, path, IssueKind::TypeMismatch, format!("expected string, found {actual}"), report);
                } else {
                    let Some(scalar) = scalar_str(document, node) else { return };
                    if let Some(min) = min_length {
                        if scalar.chars().count() < *min {
                            issue(document, node, path, IssueKind::MinLength, format!("shorter than {min}"), report);
                        }
                    }
                    if let Some(max) = max_length {
                        if scalar.chars().count() > *max {
                            issue(document, node, path, IssueKind::MaxLength, format!("longer than {max}"), report);
                        }
                    }
                    if let Some(pat) = pat {
                        if !pat.is_match(&scalar) {
                            issue(document, node, path, IssueKind::PatternMismatch, "string does not match pattern", report);
                        }
                    }
                }
            }
        }
        Schema::Number { minimum, maximum, exclusive_minimum, exclusive_maximum, multiple_of } => {
            if settings.numbers {
                let Some(value) = node_number(document, node) else {
                    let actual = JsonType::of_node(document, node).map(|t| t.name()).unwrap_or("unknown");
                    issue(document, node, path, IssueKind::TypeMismatch, format!("expected number, found {actual}"), report);
                    return;
                };
                    if let Some(bound) = minimum {
                        if value < *bound {
                            issue(document, node, path, IssueKind::Minimum, format!("{value} below {bound}"), report);
                        }
                    }
                    if let Some(bound) = maximum {
                        if value > *bound {
                            issue(document, node, path, IssueKind::Maximum, format!("{value} above {bound}"), report);
                        }
                    }
                    if let Some(bound) = exclusive_minimum {
                        if value <= *bound {
                            issue(document, node, path, IssueKind::ExclusiveMinimum, format!("{value} at or below {bound}"), report);
                        }
                    }
                    if let Some(bound) = exclusive_maximum {
                        if value >= *bound {
                            issue(document, node, path, IssueKind::ExclusiveMaximum, format!("{value} at or above {bound}"), report);
                        }
                    }
                    if let Some(step) = multiple_of {
                        if *step != 0.0 && (value / step).fract() != 0.0 {
                            issue(document, node, path, IssueKind::MultipleOf, format!("{value} not a multiple of {step}"), report);
                        }
                    }
            }
        }
        Schema::Array { items, min_items, max_items, unique_items } => {
            if settings.arrays {
                let Some(NodeKind::Sequence(elements)) = document.node(node).map(|n| &n.kind) else {
                    let actual = JsonType::of_node(document, node).map(|t| t.name()).unwrap_or("unknown");
                    issue(document, node, path, IssueKind::TypeMismatch, format!("expected array, found {actual}"), report);
                    return;
                };
                    if let Some(min) = min_items {
                        if elements.len() < *min {
                            issue(document, node, path, IssueKind::MinItems, format!("fewer than {min} items"), report);
                        }
                    }
                    if let Some(max) = max_items {
                        if elements.len() > *max {
                            issue(document, node, path, IssueKind::MaxItems, format!("more than {max} items"), report);
                        }
                    }
                    if *unique_items {
                        let mut dup_found = false;
                        for (i, a) in elements.iter().enumerate() {
                            for b in &elements[i + 1..] {
                                if nodes_equal(document, *a, *b) {
                                    issue(document, node, path, IssueKind::UniqueItems, "duplicate items", report);
                                    dup_found = true;
                                    break;
                                }
                            }
                            if dup_found {
                                break;
                            }
                        }
                    }
                    if let Some(items) = items {
                        for (i, &element) in elements.iter().enumerate() {
                            path.push_str(if path.is_empty() { "" } else { "." });
                            path.push_str(&i.to_string());
                            validate_node(document, element, items, defs, settings, depth + 1, path, report);
                            pop_path(path, i);
                        }
                    }
            }
        }
        Schema::Object { properties, required, additional, min_properties, max_properties } => {
            if settings.objects {
                let Some(NodeKind::Mapping(entries)) = document.node(node).map(|n| &n.kind) else {
                    let actual = JsonType::of_node(document, node).map(|t| t.name()).unwrap_or("unknown");
                    issue(document, node, path, IssueKind::TypeMismatch, format!("expected object, found {actual}"), report);
                    return;
                };
                    if let Some(min) = min_properties {
                        if entries.len() < *min {
                            issue(document, node, path, IssueKind::MinProperties, format!("fewer than {min} properties"), report);
                        }
                    }
                    if let Some(max) = max_properties {
                        if entries.len() > *max {
                            issue(document, node, path, IssueKind::MaxProperties, format!("more than {max} properties"), report);
                        }
                    }
                    for name in required {
                        if !entries.iter().any(|&(k, _)| scalar_key(document, k).as_deref() == Some(name.as_str())) {
                            issue(document, node, path, IssueKind::RequiredMissing, format!("required property `{name}` missing"), report);
                        }
                    }
                    for &(key_id, value_id) in entries {
                        let Some(name) = scalar_key(document, key_id) else { continue };
                        if let Some((_, prop_schema)) = properties.iter().find(|(prop, _)| *prop == name) {
                            path.push_str(if path.is_empty() { "" } else { "." });
                            path.push_str(&name);
                            validate_node(document, value_id, prop_schema, defs, settings, depth + 1, path, report);
                            pop_path(path, &name);
                        } else {
                            match additional {
                                Additional::Forbid => {
                                    issue(document, node, path, IssueKind::AdditionalProperty, format!("property `{name}` not allowed"), report);
                                }
                                Additional::Schema(prop_schema) => {
                                    path.push_str(if path.is_empty() { "" } else { "." });
                                    path.push_str(&name);
                                    validate_node(document, value_id, prop_schema, defs, settings, depth + 1, path, report);
                                    pop_path(path, &name);
                                }
                                Additional::Allow => {}
                            }
                        }
                }
            }
        }
        Schema::Not(inner) => {
            if settings.combinators {
                let mut sub = ValidationReport::default();
                validate_node(document, node, inner, defs, settings, depth + 1, path, &mut sub);
                if sub.is_valid() {
                    issue(document, node, path, IssueKind::NotMatched, "matched `not` schema", report);
                }
            }
        }
        Schema::OneOf(schemas) => {
            if settings.combinators {
                let matches = schemas
                    .iter()
                    .filter(|schema| {
                        let mut sub = ValidationReport::default();
                        validate_node(document, node, schema, defs, settings, depth + 1, path, &mut sub);
                        sub.is_valid()
                    })
                    .count();
                if matches == 0 {
                    issue(document, node, path, IssueKind::OneOfMismatch, "matched none of oneOf", report);
                } else if matches > 1 {
                    issue(document, node, path, IssueKind::OneOfAmbiguous, "matched several of oneOf", report);
                }
            }
        }
        Schema::AnyOf(schemas) => {
            if settings.combinators {
                let matches = schemas
                    .iter()
                    .filter(|schema| {
                        let mut sub = ValidationReport::default();
                        validate_node(document, node, schema, defs, settings, depth + 1, path, &mut sub);
                        sub.is_valid()
                    })
                    .count();
                if matches == 0 {
                    issue(document, node, path, IssueKind::AnyOfMismatch, "matched none of anyOf", report);
                }
            }
        }
        Schema::AllOf(schemas) => {
            if settings.combinators {
                for schema in schemas {
                    let mut sub = ValidationReport::default();
                    validate_node(document, node, schema, defs, settings, depth + 1, path, &mut sub);
                    for mut issue in sub.issues {
                        issue.node = node;
                        issue.span = document.span(node);
                        issue.path = path.clone();
                        report.issues.push(issue);
                    }
                }
            }
        }
        Schema::Ref(name) => {
            if settings.refs {
                // Compiled refs keep their full spelling ("#/$defs/flags"); strip it for lookup.
                let key = name
                    .strip_prefix("#/$defs/")
                    .or_else(|| name.strip_prefix("#/"))
                    .unwrap_or(name.as_str());
                let Some(target) = defs.get(key) else {
                    issue(document, node, path, IssueKind::RefUnresolved, format!("$ref `{name}` not in $defs"), report);
                    return;
                };
                validate_node(document, node, target, defs, settings, depth + 1, path, report);
            }
        }
    }
}

fn pop_path(path: &mut String, segment: impl ToString) {
    let segment = segment.to_string();
    let new_len = path.len().saturating_sub(segment.len() + usize::from(!path.is_empty() && path.len() > segment.len()));
    // Simplest correct approach: find the last `.`-separated segment and truncate it off.
    if let Some(pos) = path.rfind(&segment) {
        let mut cut = pos;
        if cut > 0 && path.as_bytes()[cut - 1] == b'.' {
            cut -= 1;
        }
        path.truncate(cut);
    } else {
        path.clear();
    }
    let _ = new_len;
}

fn issue(
    document: &Document,
    node: NodeId,
    path: &str,
    kind: IssueKind,
    message: impl Into<String>,
    report: &mut ValidationReport,
) {
    report.issues.push(ValidationIssue {
        path: path.to_string(),
        kind,
        message: message.into(),
        node,
        span: document.span(node),
    });
}

fn scalar_str(document: &Document, id: NodeId) -> Option<String> {
    match &document.node(id)?.kind {
        NodeKind::Scalar(scalar) => scalar.value.as_str().map(|s| s.to_string()),
        _ => None,
    }
}

fn node_number(document: &Document, id: NodeId) -> Option<f64> {
    match &document.node(id)?.kind {
        NodeKind::Scalar(scalar) => match scalar.value {
            tpt_yaml_core::ScalarValue::Int(v) => Some(v as f64),
            tpt_yaml_core::ScalarValue::Float(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

fn nodes_equal(document: &Document, a: NodeId, b: NodeId) -> bool {
    if a == b {
        return true;
    }
    match (document.node(a).map(|n| &n.kind), document.node(b).map(|n| &n.kind)) {
        (Some(NodeKind::Scalar(x)), Some(NodeKind::Scalar(y))) => x.value == y.value,
        (Some(NodeKind::Sequence(x)), Some(NodeKind::Sequence(y))) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(&x, &y)| nodes_equal(document, x, y))
        }
        (Some(NodeKind::Mapping(x)), Some(NodeKind::Mapping(y))) => {
            x.len() == y.len()
                && x.iter().all(|&(xk, xv)| {
                    y.iter().any(|&(yk, yv)| nodes_equal(document, xk, yk) && nodes_equal(document, xv, yv))
                })
        }
        _ => false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = r##"
type: object
required: [name, ports]
properties:
  name:
    type: string
    pattern: '^[A-Za-z]+$'
  ports:
    type: array
    minItems: 1
    maxItems: 4
    items:
      type: number
      minimum: 1
      maximum: 65535
  mode:
    enum: [strict, lenient]
  flags:
    $ref: "#/$defs/flags"
$defs:
  flags:
    type: object
    additionalProperties: false
    properties:
      verbose:
        type: boolean
"##;

    #[test]
    fn loads_a_schema_and_validates_a_matching_document() {
        let schema = SchemaDocument::from_yaml_str(SCHEMA).unwrap();
        let doc = tpt_yaml_core::parse(
            "name: alpha\nports:\n  - 80\n  - 443\nmode: strict\nflags:\n  verbose: true\n",
        )
        .unwrap();
        let report = schema.validate(&doc, doc.root().unwrap());
        assert!(report.is_valid(), "issues: {:?}", report.issues);
    }

    #[test]
    fn reports_type_required_and_enum_issues_with_spans() {
        let schema = SchemaDocument::from_yaml_str(SCHEMA).unwrap();
        let doc = tpt_yaml_core::parse(
            "name: 42\nports: []\nmode: turbo\nflags:\n  verbose: yes\n  extra: 1\n",
        )
        .unwrap();
        let report = schema.validate(&doc, doc.root().unwrap());
        assert!(!report.is_valid());
        let kinds: Vec<_> = report.issues().iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&IssueKind::TypeMismatch), "{kinds:?}");
        assert!(kinds.contains(&IssueKind::MinItems), "{kinds:?}");
        assert!(kinds.contains(&IssueKind::EnumMismatch), "{kinds:?}");
        assert!(kinds.contains(&IssueKind::AdditionalProperty), "{kinds:?}");
        // Spans are populated for editor surfacing.
        assert!(report.issues().iter().all(|i| i.span.is_some()), "{kinds:?}");
    }

    #[test]
    fn one_of_any_of_all_of_and_not() {
        let schema = SchemaDocument::from_yaml_str(
            "oneOf:\n  - type: string\n  - type: number\n",
        )
        .unwrap();
        let doc = tpt_yaml_core::parse("just a string").unwrap();
        assert!(schema.validate(&doc, doc.root().unwrap()).is_valid());
        // A scalar `"5"`... a plain `5` is a number in YAML: exactly one match.
        let doc = tpt_yaml_core::parse("5\n").unwrap();
        assert!(schema.validate(&doc, doc.root().unwrap()).is_valid());
        // Ambiguity: `true` matches neither string nor number.
        let doc = tpt_yaml_core::parse("true\n").unwrap();
        let report = schema.validate(&doc, doc.root().unwrap());
        assert_eq!(report.issues()[0].kind, IssueKind::OneOfMismatch);
    }

    #[test]
    fn settings_gate_which_checks_run() {
        let schema = SchemaDocument::from_yaml_str(
            "type: object\nrequired: [a]\nproperties:\n  a:\n    type: string\n",
        )
        .unwrap();
        let doc = tpt_yaml_core::parse("a: 1\nextra: 2\n").unwrap();
        let root = doc.root().unwrap();
        // With objects off, neither the type mismatch nor the missing `a` is reported.
        let report = schema.validate_with(&doc, root, &ValidationSettings::new().only(&["enums"]));
        assert!(report.is_valid());
    }

    #[test]
    fn validation_is_deterministic() {
        let schema = SchemaDocument::from_yaml_str(SCHEMA).unwrap();
        let doc = tpt_yaml_core::parse("name: 42\nports: []\nmode: turbo\n").unwrap();
        let root = doc.root().unwrap();
        let first = schema.validate(&doc, root);
        let second = schema.validate(&doc, root);
        assert_eq!(first, second);
    }

    #[cfg(feature = "typed")]
    #[test]
    fn validates_a_typed_value() {
        use serde::Serialize;
        #[derive(Serialize)]
        struct Config {
            name: &'static str,
            ports: Vec<i32>,
        }
        let schema = SchemaDocument::from_yaml_str(
            "type: object\nrequired: [name, ports]\nproperties:\n  name:\n    type: string\n  ports:\n    type: array\n    items:\n      type: number\n      minimum: 0\n",
        )
        .unwrap();
        let ok = Config { name: "a", ports: vec![1, 2] };
        assert!(schema.validate_typed(&ok).unwrap().is_valid());
        let bad = Config { name: "a", ports: vec![-1] };
        let report = schema.validate_typed(&bad).unwrap();
        assert!(!report.is_valid());
    }
}