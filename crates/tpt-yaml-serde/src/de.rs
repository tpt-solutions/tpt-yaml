use crate::error::Error;
use serde::de::{
    self, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};
use serde::forward_to_deserialize_any;
use tpt_yaml_core::{Document, NodeId, NodeKind, ScalarValue};

/// Deserializes directly from an already-parsed [`Document`] arena — no re-parsing. Borrows
/// scalar strings straight out of the document's own storage.
pub struct Deserializer<'de> {
    doc: &'de Document,
    node: NodeId,
}

impl<'de> Deserializer<'de> {
    pub fn from_document(doc: &'de Document, node: NodeId) -> Self {
        Self { doc, node }
    }

    /// Follows an alias chain to its final non-alias target. Cycle-free by construction (see
    /// `tpt_yaml_core::parser`'s anchor-registration ordering).
    fn resolved(&self) -> NodeId {
        let mut id = self.node;
        let mut steps = 0usize;
        while let Some(node) = self.doc.node(id) {
            if let NodeKind::Alias(target) = node.kind {
                id = target;
                steps += 1;
                if steps > self.doc.nodes.len() {
                    break;
                }
            } else {
                break;
            }
        }
        id
    }
}

struct SeqDeserializer<'de> {
    doc: &'de Document,
    items: core::slice::Iter<'de, NodeId>,
}

impl<'de> SeqAccess<'de> for SeqDeserializer<'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        match self.items.next() {
            Some(&id) => seed.deserialize(Deserializer::from_document(self.doc, id)).map(Some),
            None => Ok(None),
        }
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len())
    }
}

struct MapDeserializer<'de> {
    doc: &'de Document,
    entries: core::slice::Iter<'de, (NodeId, NodeId)>,
    value: Option<NodeId>,
}

impl<'de> MapAccess<'de> for MapDeserializer<'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        match self.entries.next() {
            Some(&(k, v)) => {
                self.value = Some(v);
                seed.deserialize(Deserializer::from_document(self.doc, k)).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let value = self.value.take().ok_or_else(|| Error::msg("value requested before key"))?;
        seed.deserialize(Deserializer::from_document(self.doc, value))
    }

    fn size_hint(&self) -> Option<usize> {
        let (lower, upper) = self.entries.size_hint();
        upper.or(Some(lower))
    }
}

struct EnumDeserializer<'de> {
    doc: &'de Document,
    variant: NodeId,
    value: NodeId,
}

impl<'de> EnumAccess<'de> for EnumDeserializer<'de> {
    type Error = Error;
    type Variant = VariantDeserializer<'de>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        let variant = seed.deserialize(Deserializer::from_document(self.doc, self.variant))?;
        Ok((variant, VariantDeserializer { doc: self.doc, value: self.value }))
    }
}

struct VariantDeserializer<'de> {
    doc: &'de Document,
    value: NodeId,
}

impl<'de> VariantAccess<'de> for VariantDeserializer<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(Deserializer::from_document(self.doc, self.value))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_seq(
            Deserializer::from_document(self.doc, self.value),
            visitor,
        )
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_map(
            Deserializer::from_document(self.doc, self.value),
            visitor,
        )
    }
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let id = self.resolved();
        let node = self.doc.node(id).ok_or_else(|| Error::msg("dangling node id"))?;
        match &node.kind {
            NodeKind::Scalar(scalar) => match &scalar.value {
                ScalarValue::Null => visitor.visit_unit(),
                ScalarValue::Bool(b) => visitor.visit_bool(*b),
                ScalarValue::Int(i) => visitor.visit_i64(*i),
                ScalarValue::Float(f) => visitor.visit_f64(*f),
                ScalarValue::String(s) | ScalarValue::Timestamp(s) => {
                    visitor.visit_borrowed_str(s.as_str())
                }
            },
            NodeKind::Sequence(items) => {
                visitor.visit_seq(SeqDeserializer { doc: self.doc, items: items.iter() })
            }
            NodeKind::Mapping(entries) => visitor.visit_map(MapDeserializer {
                doc: self.doc,
                entries: entries.iter(),
                value: None,
            }),
            NodeKind::Alias(_) => unreachable!("resolved() dereferences aliases"),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let id = self.resolved();
        let is_null = matches!(
            self.doc.node(id).map(|n| &n.kind),
            Some(NodeKind::Scalar(scalar)) if scalar.value == ScalarValue::Null
        );
        if is_null {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let id = self.resolved();
        match self.doc.node(id).map(|n| &n.kind) {
            Some(NodeKind::Scalar(scalar)) => match &scalar.value {
                ScalarValue::String(s) => visitor.visit_enum(s.as_str().into_deserializer()),
                _ => Err(Error::msg("expected a string for a unit enum variant")),
            },
            Some(NodeKind::Mapping(entries)) if entries.len() == 1 => {
                let (variant, value) = entries[0];
                visitor.visit_enum(EnumDeserializer { doc: self.doc, variant, value })
            }
            _ => Err(Error::msg("expected a string or single-entry mapping for an enum")),
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}
