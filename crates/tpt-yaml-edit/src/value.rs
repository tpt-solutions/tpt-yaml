use alloc::string::String;
use alloc::vec::Vec;
use tpt_yaml_core::ScalarValue;

/// A value to write into a [`crate::EditableDocument`] via [`crate::EditableDocument::set`] or
/// [`crate::EditableDocument::push`]. Building one of these creates fresh, synthesized nodes
/// (no source span) — [`crate::EditableDocument::render`] pretty-prints them via
/// `tpt_yaml_core`'s shared printer rather than blitting source text, since there is none.
#[derive(Clone, Debug, PartialEq)]
pub enum EditValue {
    Scalar(ScalarValue),
    Sequence(Vec<EditValue>),
    Mapping(Vec<(String, EditValue)>),
    /// Typed content routed through `tpt-yaml-serde` (requires the `typed` feature). The
    /// `T: serde::Serialize` value is converted once, at construction time, into the dynamic
    /// [`tpt_yaml_serde::Value`] carried here; the arena build then treats it exactly like
    /// hand-built content.
    #[cfg(feature = "typed")]
    Typed(tpt_yaml_serde::Value),
}

impl EditValue {
    /// Builds an [`EditValue`] from any `T: serde::Serialize` value, routing it through
    /// `tpt-yaml-serde`'s [`tpt_yaml_serde::Serializer`] (the same arena/pretty-printer the
    /// rest of the family uses). Only available with the `typed` feature.
    #[cfg(feature = "typed")]
    pub fn typed<T: serde::Serialize + ?Sized>(value: &T) -> Result<Self, tpt_yaml_serde::Error> {
        let mut serializer = tpt_yaml_serde::Serializer::new();
        let root = value.serialize(&mut serializer)?;
        let (document, root) = serializer.into_document(root);
        Ok(Self::Typed(tpt_yaml_serde::Value::from_node(&document, root)))
    }

    /// Flattens a `Typed` payload into the plain variants, so the arena build doesn't need to
    /// know about the serde surface at all.
    #[cfg(feature = "typed")]
    pub(crate) fn into_plain(self) -> Self {
        match self {
            Self::Scalar(_) | Self::Sequence(_) | Self::Mapping(_) => self,
            Self::Typed(v) => Self::from_serde_value(&v),
        }
    }

    #[cfg(feature = "typed")]
    fn from_serde_value(v: &tpt_yaml_serde::Value) -> Self {
        match v {
            tpt_yaml_serde::Value::Null => Self::Scalar(ScalarValue::Null),
            tpt_yaml_serde::Value::Bool(b) => Self::Scalar(ScalarValue::Bool(*b)),
            tpt_yaml_serde::Value::Int(i) => Self::Scalar(ScalarValue::Int(*i)),
            tpt_yaml_serde::Value::Float(f) => Self::Scalar(ScalarValue::Float(*f)),
            tpt_yaml_serde::Value::String(s) => Self::Scalar(ScalarValue::String(s.clone())),
            tpt_yaml_serde::Value::Tagged(_, inner) => Self::from_serde_value(inner),
            tpt_yaml_serde::Value::Sequence(items) => {
                Self::Sequence(items.iter().map(Self::from_serde_value).collect())
            }
            tpt_yaml_serde::Value::Mapping(entries) => Self::Mapping(
                entries.iter().map(|(k, v)| (serde_key(k), Self::from_serde_value(v))).collect(),
            ),
        }
    }
}

/// Stringifies a serde [`tpt_yaml_serde::Value`] used as a mapping key, the way YAML mapping
/// keys are spelled: scalars keep their spelling; compound keys fall back to the shared
/// pretty-printer (which quotes as needed).
#[cfg(feature = "typed")]
fn serde_key(k: &tpt_yaml_serde::Value) -> String {
    match k {
        tpt_yaml_serde::Value::Null => String::new(),
        tpt_yaml_serde::Value::Bool(b) => b.to_string(),
        tpt_yaml_serde::Value::Int(i) => i.to_string(),
        tpt_yaml_serde::Value::Float(f) => f.to_string(),
        tpt_yaml_serde::Value::String(s) => s.clone(),
        tpt_yaml_serde::Value::Tagged(_, inner) => serde_key(inner),
        tpt_yaml_serde::Value::Sequence(_) | tpt_yaml_serde::Value::Mapping(_) => {
            tpt_yaml_serde::to_string(k).unwrap_or_default()
        }
    }
}
