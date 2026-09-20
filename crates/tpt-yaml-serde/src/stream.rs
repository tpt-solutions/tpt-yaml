//! Constant-memory decode straight from source, built on [`tpt_yaml_core::stream::EventParser`].
//!
//! [`crate::Deserializer`] borrows an already-parsed [`tpt_yaml_core::Document`], which is
//! zero-copy but requires the whole document to be materialized as an arena first. This module's
//! [`Deserializer`] instead pulls [`Event`]s lazily from a live `EventParser`, so a caller can
//! decode a YAML stream containing many documents through a fixed amount of memory (proportional
//! to nesting depth, not to the number of documents or nodes already consumed) via [`Documents`].
//!
//! ```
//! use tpt_yaml_serde::stream::Documents;
//!
//! let source = "a: 1\n---\na: 2\n---\na: 3\n";
//! let values: Vec<Value> = Documents::new(source).collect::<Result<_, _>>().unwrap();
//! assert_eq!(values.len(), 3);
//!
//! #[derive(serde::Deserialize, PartialEq, Debug)]
//! struct Value {
//!     a: i32,
//! }
//! ```
//!
//! # Limitations vs. [`crate::Deserializer`]
//!
//! - No zero-copy: every scalar the underlying `EventParser` produces is already an owned
//!   `String`, so this deserializer always visits owned data (`visit_string`, never
//!   `visit_borrowed_str`).
//! - Anchors/aliases are rejected. Replaying an alias requires the anchored subtree's events to
//!   have been buffered in memory, which would silently reintroduce the O(document size) memory
//!   use this module exists to avoid; documents with anchors should use [`crate::from_str`]
//!   instead.
//! - Merge keys (`<<`) are rejected by the underlying `EventParser` for the same reason (see
//!   [`tpt_yaml_core::stream`]'s module docs), unless `ParserOptions::merge_keys` is `false`.

use crate::error::Error;
use alloc::format;
use core::marker::PhantomData;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;
use tpt_yaml_core::stream::{Event, EventParser, ScalarEvent};
use tpt_yaml_core::{ParserOptions, ScalarValue};

/// Deserializes one value by pulling [`Event`]s lazily from a live [`EventParser`], never
/// materializing a [`tpt_yaml_core::Document`].
pub struct Deserializer<'p, 'src> {
    parser: &'p mut EventParser<'src>,
    peeked: Option<Event>,
}

impl<'p, 'src> Deserializer<'p, 'src> {
    pub fn from_events(parser: &'p mut EventParser<'src>) -> Self {
        Self { parser, peeked: None }
    }

    fn peek(&mut self) -> Result<&Event, Error> {
        if self.peeked.is_none() {
            let event = self
                .parser
                .next_event()?
                .ok_or_else(|| Error::msg("unexpected end of event stream"))?;
            self.peeked = Some(event);
        }
        Ok(self.peeked.as_ref().expect("just filled"))
    }

    fn next(&mut self) -> Result<Event, Error> {
        if let Some(event) = self.peeked.take() {
            return Ok(event);
        }
        self.parser.next_event()?.ok_or_else(|| Error::msg("unexpected end of event stream"))
    }

    fn expect_mapping_end(&mut self) -> Result<(), Error> {
        match self.next()? {
            Event::MappingEnd => Ok(()),
            other => Err(Error::msg(format!("expected end of mapping, found {other:?}"))),
        }
    }

    fn expect_sequence_end(&mut self) -> Result<(), Error> {
        match self.next()? {
            Event::SequenceEnd => Ok(()),
            other => Err(Error::msg(format!("expected end of sequence, found {other:?}"))),
        }
    }
}

fn visit_scalar<'de, V: Visitor<'de>>(scalar: ScalarEvent, visitor: V) -> Result<V::Value, Error> {
    match scalar.value {
        ScalarValue::Null => visitor.visit_unit(),
        ScalarValue::Bool(b) => visitor.visit_bool(b),
        ScalarValue::Int(i) => visitor.visit_i64(i),
        ScalarValue::Float(f) => visitor.visit_f64(f),
        ScalarValue::String(s) | ScalarValue::Timestamp(s) => visitor.visit_string(s),
    }
}

struct SeqEvents<'p, 'src, 'a> {
    de: &'a mut Deserializer<'p, 'src>,
}

impl<'p, 'src, 'a, 'de> SeqAccess<'de> for SeqEvents<'p, 'src, 'a> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        // Peeks and leaves `SequenceEnd` unconsumed: the caller (`deserialize_any`) always
        // consumes it after `visit_seq` returns, whether or not the visitor drained every
        // element itself (a fixed-length tuple visitor stops after its known arity).
        let at_end = matches!(self.de.peek()?, Event::SequenceEnd);
        if at_end {
            return Ok(None);
        }
        seed.deserialize(&mut *self.de).map(Some)
    }
}

struct MapEvents<'p, 'src, 'a> {
    de: &'a mut Deserializer<'p, 'src>,
}

impl<'p, 'src, 'a, 'de> MapAccess<'de> for MapEvents<'p, 'src, 'a> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        // See the matching comment on `SeqEvents::next_element_seed`: `MappingEnd` is left
        // unconsumed for `deserialize_any` to pick up after `visit_map` returns.
        let at_end = matches!(self.de.peek()?, Event::MappingEnd);
        if at_end {
            return Ok(None);
        }
        seed.deserialize(&mut *self.de).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(&mut *self.de)
    }
}

struct EnumEvents<'p, 'src, 'a> {
    de: &'a mut Deserializer<'p, 'src>,
}

impl<'p, 'src, 'a, 'de> EnumAccess<'de> for EnumEvents<'p, 'src, 'a> {
    type Error = Error;
    type Variant = VariantEvents<'p, 'src, 'a>;

    fn variant_seed<V: DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Error> {
        let variant = seed.deserialize(&mut *self.de)?;
        Ok((variant, VariantEvents { de: self.de }))
    }
}

struct VariantEvents<'p, 'src, 'a> {
    de: &'a mut Deserializer<'p, 'src>,
}

impl<'p, 'src, 'a, 'de> VariantAccess<'de> for VariantEvents<'p, 'src, 'a> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.de.next()? {
            Event::Scalar(scalar) if scalar.value == ScalarValue::Null => Ok(()),
            other => Err(Error::msg(format!(
                "expected null for a unit enum variant's value, found {other:?}"
            ))),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(&mut *self.de)
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_seq(&mut *self.de, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        de::Deserializer::deserialize_map(&mut *self.de, visitor)
    }
}

impl<'p, 'src, 'de> de::Deserializer<'de> for &mut Deserializer<'p, 'src> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.next()? {
            Event::Scalar(scalar) => visit_scalar(scalar, visitor),
            Event::MappingStart(_) => {
                let value = visitor.visit_map(MapEvents { de: &mut *self })?;
                self.expect_mapping_end()?;
                Ok(value)
            }
            Event::SequenceStart(_) => {
                let value = visitor.visit_seq(SeqEvents { de: &mut *self })?;
                self.expect_sequence_end()?;
                Ok(value)
            }
            Event::Alias(alias) => Err(Error::msg(format!(
                "aliases are not supported by the streaming deserializer (anchor `{}`); use \
                 `tpt_yaml_serde::from_str`/`from_document` for documents with anchors/aliases",
                alias.name
            ))),
            other => Err(Error::msg(format!("unexpected event {other:?} in value position"))),
        }
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let is_null = matches!(self.peek()?, Event::Scalar(s) if s.value == ScalarValue::Null);
        if is_null {
            self.next()?;
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
        let is_mapping = matches!(self.peek()?, Event::MappingStart(_));
        if is_mapping {
            self.next()?;
            let value = visitor.visit_enum(EnumEvents { de: self })?;
            self.expect_mapping_end()?;
            Ok(value)
        } else {
            match self.next()? {
                Event::Scalar(scalar) => match scalar.value {
                    ScalarValue::String(s) => visitor.visit_enum(s.into_deserializer()),
                    _ => Err(Error::msg("expected a string for a unit enum variant")),
                },
                other => Err(Error::msg(format!(
                    "expected a string or single-entry mapping for an enum, found {other:?}"
                ))),
            }
        }
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }
}

/// Deserializes one value from a live [`EventParser`], pulling events lazily. Leaves the parser
/// positioned right after the value's events (its `DocumentStart`/`DocumentEnd` are the caller's
/// responsibility — see [`Documents`] for a convenience that handles those too).
pub fn from_events<'src, T>(parser: &mut EventParser<'src>) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let mut de = Deserializer::from_events(parser);
    T::deserialize(&mut de)
}

/// Iterates the documents of a `---`-separated YAML stream, deserializing each into `T` without
/// ever materializing a full [`tpt_yaml_core::Document`] arena for any of them.
///
/// Memory use per document is whatever `T`'s own `Deserialize` impl uses (e.g. a `Vec<Item>`
/// field still buffers all its items) — the constant-memory property is across documents, not
/// within one. A caller that wants sub-document constant memory needs a custom `Visitor` that
/// consumes and drops elements one at a time instead of collecting them.
pub struct Documents<'src, T> {
    parser: EventParser<'src>,
    stream_started: bool,
    finished: bool,
    _marker: PhantomData<fn() -> T>,
}

impl<'src, T> Documents<'src, T> {
    pub fn new(source: &'src str) -> Self {
        Self::with_options(source, &ParserOptions::default())
    }

    pub fn with_options(source: &'src str, options: &ParserOptions) -> Self {
        Self {
            parser: EventParser::new(source, options),
            stream_started: false,
            finished: false,
            _marker: PhantomData,
        }
    }

    fn ensure_stream_started(&mut self) -> Result<(), Error> {
        if self.stream_started {
            return Ok(());
        }
        match self.parser.next_event()? {
            Some(Event::StreamStart) => {
                self.stream_started = true;
                Ok(())
            }
            other => Err(Error::msg(format!("expected StreamStart, found {other:?}"))),
        }
    }
}

impl<'src, T: DeserializeOwned> Iterator for Documents<'src, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        if let Err(e) = self.ensure_stream_started() {
            self.finished = true;
            return Some(Err(e));
        }
        match self.parser.next_event() {
            Ok(Some(Event::DocumentStart)) => {}
            Ok(Some(Event::StreamEnd)) | Ok(None) => {
                self.finished = true;
                return None;
            }
            Ok(Some(other)) => {
                self.finished = true;
                return Some(Err(Error::msg(format!("expected DocumentStart, found {other:?}"))));
            }
            Err(e) => {
                self.finished = true;
                return Some(Err(e.into()));
            }
        }

        let value = match from_events::<T>(&mut self.parser) {
            Ok(value) => value,
            Err(e) => {
                self.finished = true;
                return Some(Err(e));
            }
        };

        match self.parser.next_event() {
            Ok(Some(Event::DocumentEnd)) => Some(Ok(value)),
            Ok(Some(other)) => {
                self.finished = true;
                Some(Err(Error::msg(format!("expected DocumentEnd, found {other:?}"))))
            }
            Ok(None) => {
                self.finished = true;
                Some(Err(Error::msg("unexpected end of event stream after document value")))
            }
            Err(e) => {
                self.finished = true;
                Some(Err(e.into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    struct Config {
        name: String,
        count: i32,
        enabled: bool,
        tags: Vec<String>,
        nested: Option<Nested>,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    struct Nested {
        value: f64,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    enum Shape {
        Unit,
        Newtype(i32),
        Tuple(i32, i32),
        Struct { x: i32, y: i32 },
    }

    #[test]
    fn deserializes_a_single_document_scalar() {
        let mut parser = EventParser::new("42", &ParserOptions::default());
        parser.next_event().unwrap(); // StreamStart
        parser.next_event().unwrap(); // DocumentStart
        let value: i32 = from_events(&mut parser).unwrap();
        assert_eq!(value, 42);
    }

    #[test]
    fn deserializes_a_typed_struct() {
        let source =
            "name: demo\ncount: 3\nenabled: true\ntags:\n  - a\n  - b\nnested:\n  value: 1.5\n";
        let mut docs = Documents::<Config>::new(source);
        let config = docs.next().unwrap().unwrap();
        assert_eq!(
            config,
            Config {
                name: "demo".into(),
                count: 3,
                enabled: true,
                tags: vec!["a".into(), "b".into()],
                nested: Some(Nested { value: 1.5 }),
            }
        );
        assert!(docs.next().is_none());
    }

    #[test]
    fn deserializes_none_from_null() {
        let source = "name: demo\ncount: 0\nenabled: false\ntags: []\nnested: null\n";
        let config = Documents::<Config>::new(source).next().unwrap().unwrap();
        assert_eq!(config.nested, None);
    }

    #[test]
    fn iterates_every_document_in_a_stream() {
        let source = "a: 1\n---\na: 2\n---\na: 3\n";
        #[derive(Debug, Clone, PartialEq, Deserialize)]
        struct Doc {
            a: i32,
        }
        let values: Vec<Doc> = Documents::new(source).collect::<Result<_, _>>().unwrap();
        assert_eq!(values, vec![Doc { a: 1 }, Doc { a: 2 }, Doc { a: 3 }]);
    }

    #[test]
    fn round_trips_every_enum_variant_shape_via_from_str_rendering() {
        let cases = [
            ("Unit\n", Shape::Unit),
            ("Newtype: 1\n", Shape::Newtype(1)),
            ("Tuple:\n  - 1\n  - 2\n", Shape::Tuple(1, 2)),
            ("Struct:\n  x: 1\n  y: 2\n", Shape::Struct { x: 1, y: 2 }),
        ];
        for (source, expected) in cases {
            let parsed = Documents::<Shape>::new(source).next().unwrap().unwrap();
            assert_eq!(parsed, expected, "mismatch for {source:?}");
        }
    }

    #[test]
    fn rejects_aliases() {
        let source = "a: &x 1\nb: *x\n";
        let err = Documents::<crate::Value>::new(source).next().unwrap().unwrap_err();
        assert!(err.to_string().contains("alias"), "unexpected error: {err}");
    }

    #[test]
    fn rejects_a_document_that_is_not_the_expected_shape() {
        let source = "42\n";
        let err = Documents::<Config>::new(source).next().unwrap().unwrap_err();
        assert!(!err.to_string().is_empty());
    }
}
