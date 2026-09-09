use crate::error::Error;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::Serialize;
use tpt_yaml_core::{
    Document, NodeData, NodeId, NodeKind, Scalar, ScalarStyle, ScalarValue, YamlVersion,
};

/// Builds a fresh [`Document`] from a `T: Serialize` value, reusing `tpt_yaml_core`'s node model
/// (and, for rendering, its shared pretty-printer) rather than composing YAML text directly.
pub struct Serializer {
    pub(crate) document: Document,
}

impl Serializer {
    pub fn new() -> Self {
        Self { document: Document::new(YamlVersion::Version12) }
    }

    pub fn into_document(self, root: NodeId) -> (Document, NodeId) {
        (self.document, root)
    }

    fn scalar(&mut self, value: ScalarValue, style: ScalarStyle, raw: String) -> NodeId {
        self.document.add_node(NodeData::new(
            NodeId(0),
            NodeKind::Scalar(Scalar { value, style, raw, tag: None }),
            None,
        ))
    }

    fn mapping(&mut self, entries: Vec<(NodeId, NodeId)>) -> NodeId {
        self.document.add_node(NodeData::new(NodeId(0), NodeKind::Mapping(entries), None))
    }

    fn sequence(&mut self, items: Vec<NodeId>) -> NodeId {
        self.document.add_node(NodeData::new(NodeId(0), NodeKind::Sequence(items), None))
    }

    fn key(&mut self, name: &str) -> NodeId {
        self.scalar(ScalarValue::String(name.to_string()), ScalarStyle::Plain, name.to_string())
    }
}

impl Default for Serializer {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SeqSerializer<'a> {
    ser: &'a mut Serializer,
    items: Vec<NodeId>,
}

impl<'a> serde::ser::SerializeSeq for SeqSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(value.serialize(&mut *self.ser)?);
        Ok(())
    }

    fn end(self) -> Result<NodeId, Error> {
        Ok(self.ser.sequence(self.items))
    }
}

impl<'a> serde::ser::SerializeTuple for SeqSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<NodeId, Error> {
        serde::ser::SerializeSeq::end(self)
    }
}

impl<'a> serde::ser::SerializeTupleStruct for SeqSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        serde::ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<NodeId, Error> {
        serde::ser::SerializeSeq::end(self)
    }
}

pub struct TupleVariantSerializer<'a> {
    ser: &'a mut Serializer,
    variant: &'static str,
    items: Vec<NodeId>,
}

impl<'a> serde::ser::SerializeTupleVariant for TupleVariantSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.items.push(value.serialize(&mut *self.ser)?);
        Ok(())
    }

    fn end(self) -> Result<NodeId, Error> {
        let seq = self.ser.sequence(self.items);
        let key = self.ser.key(self.variant);
        Ok(self.ser.mapping(alloc::vec![(key, seq)]))
    }
}

pub struct MapSerializer<'a> {
    ser: &'a mut Serializer,
    entries: Vec<(NodeId, NodeId)>,
    pending_key: Option<NodeId>,
}

impl<'a> serde::ser::SerializeMap for MapSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.pending_key = Some(key.serialize(&mut *self.ser)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .pending_key
            .take()
            .ok_or_else(|| Error::msg("serialize_value before serialize_key"))?;
        let value = value.serialize(&mut *self.ser)?;
        self.entries.push((key, value));
        Ok(())
    }

    fn end(self) -> Result<NodeId, Error> {
        Ok(self.ser.mapping(self.entries))
    }
}

impl<'a> serde::ser::SerializeStruct for MapSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let key = self.ser.key(name);
        let value = value.serialize(&mut *self.ser)?;
        self.entries.push((key, value));
        Ok(())
    }

    fn end(self) -> Result<NodeId, Error> {
        Ok(self.ser.mapping(self.entries))
    }
}

pub struct StructVariantSerializer<'a> {
    ser: &'a mut Serializer,
    variant: &'static str,
    entries: Vec<(NodeId, NodeId)>,
}

impl<'a> serde::ser::SerializeStructVariant for StructVariantSerializer<'a> {
    type Ok = NodeId;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        let key = self.ser.key(name);
        let value = value.serialize(&mut *self.ser)?;
        self.entries.push((key, value));
        Ok(())
    }

    fn end(self) -> Result<NodeId, Error> {
        let inner = self.ser.mapping(self.entries);
        let key = self.ser.key(self.variant);
        Ok(self.ser.mapping(alloc::vec![(key, inner)]))
    }
}

impl<'a> serde::Serializer for &'a mut Serializer {
    type Ok = NodeId;
    type Error = Error;
    type SerializeSeq = SeqSerializer<'a>;
    type SerializeTuple = SeqSerializer<'a>;
    type SerializeTupleStruct = SeqSerializer<'a>;
    type SerializeTupleVariant = TupleVariantSerializer<'a>;
    type SerializeMap = MapSerializer<'a>;
    type SerializeStruct = MapSerializer<'a>;
    type SerializeStructVariant = StructVariantSerializer<'a>;

    fn serialize_bool(self, v: bool) -> Result<NodeId, Error> {
        Ok(self.scalar(ScalarValue::Bool(v), ScalarStyle::Plain, v.to_string()))
    }

    fn serialize_i8(self, v: i8) -> Result<NodeId, Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i16(self, v: i16) -> Result<NodeId, Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i32(self, v: i32) -> Result<NodeId, Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i64(self, v: i64) -> Result<NodeId, Error> {
        Ok(self.scalar(ScalarValue::Int(v), ScalarStyle::Plain, v.to_string()))
    }
    fn serialize_i128(self, v: i128) -> Result<NodeId, Error> {
        i64::try_from(v)
            .map_err(|_| Error::msg("i128 value out of i64 range"))
            .and_then(|v| self.serialize_i64(v))
    }

    fn serialize_u8(self, v: u8) -> Result<NodeId, Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_u16(self, v: u16) -> Result<NodeId, Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_u32(self, v: u32) -> Result<NodeId, Error> {
        self.serialize_i64(v as i64)
    }
    fn serialize_u64(self, v: u64) -> Result<NodeId, Error> {
        i64::try_from(v)
            .map_err(|_| Error::msg("u64 value out of i64 range"))
            .and_then(|v| self.serialize_i64(v))
    }
    fn serialize_u128(self, v: u128) -> Result<NodeId, Error> {
        i64::try_from(v)
            .map_err(|_| Error::msg("u128 value out of i64 range"))
            .and_then(|v| self.serialize_i64(v))
    }

    fn serialize_f32(self, v: f32) -> Result<NodeId, Error> {
        self.serialize_f64(v as f64)
    }
    fn serialize_f64(self, v: f64) -> Result<NodeId, Error> {
        Ok(self.scalar(ScalarValue::Float(v), ScalarStyle::Plain, v.to_string()))
    }

    fn serialize_char(self, v: char) -> Result<NodeId, Error> {
        let mut buf = [0u8; 4];
        self.serialize_str(v.encode_utf8(&mut buf))
    }

    fn serialize_str(self, v: &str) -> Result<NodeId, Error> {
        Ok(self.scalar(
            ScalarValue::String(v.to_string()),
            ScalarStyle::DoubleQuoted,
            v.to_string(),
        ))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<NodeId, Error> {
        let items = v
            .iter()
            .map(|&b| self.scalar(ScalarValue::Int(b as i64), ScalarStyle::Plain, b.to_string()))
            .collect();
        Ok(self.sequence(items))
    }

    fn serialize_none(self) -> Result<NodeId, Error> {
        Ok(self.scalar(ScalarValue::Null, ScalarStyle::Plain, String::new()))
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<NodeId, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<NodeId, Error> {
        Ok(self.scalar(ScalarValue::Null, ScalarStyle::Plain, String::new()))
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<NodeId, Error> {
        self.serialize_unit()
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<NodeId, Error> {
        Ok(self.scalar(
            ScalarValue::String(variant.to_string()),
            ScalarStyle::Plain,
            variant.to_string(),
        ))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<NodeId, Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<NodeId, Error> {
        let value = value.serialize(&mut *self)?;
        let key = self.key(variant);
        Ok(self.mapping(alloc::vec![(key, value)]))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer<'a>, Error> {
        Ok(SeqSerializer { ser: self, items: Vec::with_capacity(len.unwrap_or(0)) })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer<'a>, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqSerializer<'a>, Error> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<TupleVariantSerializer<'a>, Error> {
        Ok(TupleVariantSerializer { ser: self, variant, items: Vec::with_capacity(len) })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<MapSerializer<'a>, Error> {
        Ok(MapSerializer { ser: self, entries: Vec::new(), pending_key: None })
    }
    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<MapSerializer<'a>, Error> {
        Ok(MapSerializer { ser: self, entries: Vec::with_capacity(len), pending_key: None })
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<StructVariantSerializer<'a>, Error> {
        Ok(StructVariantSerializer { ser: self, variant, entries: Vec::with_capacity(len) })
    }
}
