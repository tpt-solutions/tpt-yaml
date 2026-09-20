//! `serde` support for `tpt-yaml-core`: a zero-copy [`de::Deserializer`] over an already-parsed
//! [`Document`](tpt_yaml_core::Document), a [`ser::Serializer`] that builds one via the same
//! arena/pretty-printer `tpt-yaml-core` uses, and a dynamic [`Value`] type for untyped YAML.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod de;
pub mod error;
pub mod ser;
#[cfg(feature = "streaming")]
pub mod stream;
pub mod value;

pub use de::Deserializer;
pub use error::Error;
pub use ser::Serializer;
pub use value::Value;

use alloc::string::String;
use serde::Deserialize;

/// Parses `source` and deserializes its first document into `T`.
///
/// This owns the parsed [`Document`] locally, so `T` must not borrow from the input (use
/// [`Deserializer::from_document`] directly against a `Document` you keep alive yourself for a
/// zero-copy, no-re-parsing deserialize).
pub fn from_str<T>(source: &str) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    let document = tpt_yaml_core::parse(source)?;
    let root = document.root().ok_or_else(|| Error::msg("document has no root node"))?;
    T::deserialize(Deserializer::from_document(&document, root))
}

/// Parses `bytes` as UTF-8 YAML and deserializes its first document into `T`.
pub fn from_slice<T>(bytes: &[u8]) -> Result<T, Error>
where
    T: for<'de> Deserialize<'de>,
{
    let source = core::str::from_utf8(bytes).map_err(|e| Error::msg(alloc::format!("{e}")))?;
    from_str(source)
}

/// Serializes `value` to a YAML string via the shared `tpt-yaml-core` pretty-printer.
pub fn to_string<T>(value: &T) -> Result<String, Error>
where
    T: serde::Serialize + ?Sized,
{
    let mut serializer = Serializer::new();
    let root = value.serialize(&mut serializer)?;
    let (document, root) = serializer.into_document(root);
    Ok(tpt_yaml_core::pretty_print(root, &document))
}

/// Serializes `value` as YAML to `writer`.
#[cfg(feature = "std")]
pub fn to_writer<W, T>(mut writer: W, value: &T) -> Result<(), Error>
where
    W: std::io::Write,
    T: serde::Serialize + ?Sized,
{
    let rendered = to_string(value)?;
    writer.write_all(rendered.as_bytes()).map_err(|e| Error::msg(alloc::format!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;
    use alloc::vec::Vec;
    use serde::{Deserialize, Serialize};

    #[test]
    fn placeholder() {
        assert!(from_str::<Value>("ok: true").is_ok());
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Config {
        name: String,
        count: i32,
        enabled: bool,
        tags: Vec<String>,
        nested: Option<Nested>,
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Nested {
        value: f64,
    }

    #[test]
    fn deserializes_a_typed_struct() {
        let source =
            "name: demo\ncount: 3\nenabled: true\ntags:\n  - a\n  - b\nnested:\n  value: 1.5\n";
        let config: Config = from_str(source).unwrap();
        assert_eq!(
            config,
            Config {
                name: "demo".to_string(),
                count: 3,
                enabled: true,
                tags: vec!["a".to_string(), "b".to_string()],
                nested: Some(Nested { value: 1.5 }),
            }
        );
    }

    #[test]
    fn deserializes_none_from_null() {
        let config: Config =
            from_str("name: demo\ncount: 0\nenabled: false\ntags: []\nnested: null\n").unwrap();
        assert_eq!(config.nested, None);
    }

    #[test]
    fn round_trips_a_typed_struct_through_to_string() {
        let config = Config {
            name: "demo".to_string(),
            count: 3,
            enabled: true,
            tags: vec!["a".to_string(), "b".to_string()],
            nested: Some(Nested { value: 1.5 }),
        };
        let rendered = to_string(&config).unwrap();
        let parsed: Config = from_str(&rendered).unwrap();
        assert_eq!(parsed, config);
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    enum Shape {
        Unit,
        Newtype(i32),
        Tuple(i32, i32),
        Struct { x: i32, y: i32 },
    }

    #[test]
    fn round_trips_every_enum_variant_shape() {
        for shape in
            [Shape::Unit, Shape::Newtype(1), Shape::Tuple(1, 2), Shape::Struct { x: 1, y: 2 }]
        {
            let rendered = to_string(&shape).unwrap();
            let parsed: Shape = from_str(&rendered).unwrap();
            assert_eq!(parsed, shape, "round-trip mismatch via {rendered:?}");
        }
    }

    #[test]
    fn value_from_node_matches_deserialized_value() {
        let source = "a: 1\nb:\n  - true\n  - null\n  - 3.5\n";
        let document = tpt_yaml_core::parse(source).unwrap();
        let root = document.root().unwrap();
        let via_from_node = Value::from_node(&document, root);
        let via_deserialize: Value = from_str(source).unwrap();
        assert_eq!(via_from_node, via_deserialize);
    }
}
