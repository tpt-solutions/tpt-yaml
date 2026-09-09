use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use tpt_yaml_core::{Document, NodeId, NodeKind, ScalarValue};

/// A dynamic YAML value, produced only via [`Value::from_node`] (the single conversion boundary
/// from a parsed [`Document`]) or by deserializing through this crate's `Deserializer`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Sequence(Vec<Value>),
    Mapping(Vec<(Value, Value)>),
    /// A value carrying an explicit YAML tag that isn't the core `!!str` string override.
    Tagged(String, Box<Value>),
}

impl Value {
    /// Converts one node of a parsed [`Document`] into an owned [`Value`], resolving aliases
    /// transparently (structurally cycle-free — see `tpt_yaml_core::parser`'s anchor handling).
    pub fn from_node(document: &Document, id: NodeId) -> Value {
        let Some(node) = document.node(id) else {
            return Value::Null;
        };
        let base = match &node.kind {
            NodeKind::Scalar(scalar) => match &scalar.value {
                ScalarValue::Null => Value::Null,
                ScalarValue::Bool(b) => Value::Bool(*b),
                ScalarValue::Int(i) => Value::Int(*i),
                ScalarValue::Float(f) => Value::Float(*f),
                ScalarValue::String(s) | ScalarValue::Timestamp(s) => Value::String(s.clone()),
            },
            NodeKind::Sequence(items) => Value::Sequence(
                items.iter().map(|&item| Value::from_node(document, item)).collect(),
            ),
            NodeKind::Mapping(entries) => Value::Mapping(
                entries
                    .iter()
                    .map(|&(k, v)| (Value::from_node(document, k), Value::from_node(document, v)))
                    .collect(),
            ),
            NodeKind::Alias(target) => return Value::from_node(document, *target),
        };
        match &node.tag {
            Some(tag) if tag != "!str" => Value::Tagged(tag.clone(), alloc::boxed::Box::new(base)),
            _ => base,
        }
    }
}

impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_none(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Int(i) => serializer.serialize_i64(*i),
            Value::Float(f) => serializer.serialize_f64(*f),
            Value::String(s) => serializer.serialize_str(s),
            Value::Sequence(items) => {
                use serde::ser::SerializeSeq;
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Value::Mapping(entries) => {
                use serde::ser::SerializeMap;
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (k, v) in entries {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
            Value::Tagged(_, inner) => inner.serialize(serializer),
        }
    }
}

struct ValueVisitor;

impl<'de> serde::de::Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("any valid YAML value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Int(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Int(v as i64))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
        Ok(Value::Float(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        serde::Deserialize::deserialize(deserializer)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::new();
        while let Some(v) = seq.next_element()? {
            items.push(v);
        }
        Ok(Value::Sequence(items))
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = map.next_entry()? {
            entries.push(entry);
        }
        Ok(Value::Mapping(entries))
    }
}

impl<'de> serde::Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}
