//! Property test for the `streaming` feature's [`tpt_yaml_serde::stream`] deserializer: for any
//! anchor/alias-free `Value`, decoding through the constant-memory event-based `Deserializer`
//! must agree with the zero-copy arena-based one.

#![cfg(feature = "streaming")]

use proptest::prelude::*;
use tpt_yaml_serde::stream::Documents;
use tpt_yaml_serde::{from_str, to_string, Value};

fn value_strategy() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Int),
        (-1.0e6f64..1.0e6f64).prop_map(Value::Float),
        "[a-zA-Z0-9 ]{0,12}".prop_map(Value::String),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Sequence),
            prop::collection::vec(("[a-z]{1,6}".prop_map(Value::String), inner), 0..4)
                .prop_map(Value::Mapping),
        ]
    })
}

proptest! {
    #[test]
    fn streaming_deserialize_agrees_with_the_arena_deserializer(value in value_strategy()) {
        let rendered = to_string(&value).unwrap_or_else(|e| panic!("to_string failed: {e}"));

        let via_arena: Value = from_str(&rendered)
            .unwrap_or_else(|e| panic!("from_str failed on {rendered:?}: {e}"));

        let via_stream: Value = Documents::new(&rendered)
            .next()
            .unwrap_or_else(|| panic!("no document in {rendered:?}"))
            .unwrap_or_else(|e| panic!("streaming deserialize failed on {rendered:?}: {e}"));

        prop_assert_eq!(&via_stream, &via_arena);
        prop_assert_eq!(via_arena, value);
    }
}
