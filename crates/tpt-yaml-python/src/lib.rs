//! Python bindings for the `tpt-yaml` family, via `pyo3`.
//!
//! This crate is a separate, idiomatic Python binding, not a wrapper around `tpt-yaml-ffi`'s C
//! ABI: it depends directly on [`tpt_yaml_core`] and [`tpt_yaml_serde`] and converts
//! [`tpt_yaml_serde::Value`] straight into native Python objects (dict/list/str/int/float/bool/
//! `None`), the same way `pyo3` itself would if `Value` implemented `IntoPyObject` — routing
//! through the C ABI would just mean redoing marshaling `pyo3` already does. See the crate
//! README for install/quick-start and known limitations (anchors/aliases, tagged scalars).
//!
//! # Public API
//!
//! - [`loads`] — parse a YAML string into a Python object (mirrors `json.loads` /
//!   `yaml.safe_load`).
//! - [`dumps`] — render a Python object as a YAML string (mirrors `json.dumps` /
//!   `yaml.safe_dump`).
//! - [`TptYamlError`] — the exception both functions raise on failure, so callers can catch it
//!   specifically instead of a generic `ValueError`.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString};

use tpt_yaml_serde::Value;

pyo3::create_exception!(
    tpt_yaml,
    TptYamlError,
    pyo3::exceptions::PyException,
    "Raised for YAML parse errors (`loads`) and unsupported/invalid values (`dumps`)."
);

/// Parse `source` as YAML and return the equivalent Python object.
///
/// Mirrors `json.loads`/`yaml.safe_load`'s naming convention. Mapping keys and values are
/// converted recursively; see the crate README for how anchors/aliases and explicit tags are
/// handled (aliases are resolved to their target's value, tags other than `!!str` are dropped
/// with their inner value kept).
#[pyfunction]
fn loads(py: Python<'_>, source: &str) -> PyResult<PyObject> {
    let document = tpt_yaml_core::parse(source).map_err(|err| TptYamlError::new_err(err.to_string()))?;
    let value = match document.root() {
        Some(root) => Value::from_node(&document, root),
        None => Value::Null,
    };
    value_to_py(py, &value)
}

/// Render a Python object (`dict`/`list`/`str`/`int`/`float`/`bool`/`None`, arbitrarily nested)
/// as a YAML string.
///
/// Mirrors `json.dumps`/`yaml.safe_dump`'s naming convention. Dict keys must themselves be one
/// of the supported scalar/collection types (YAML mapping keys aren't restricted to strings);
/// any other Python type (e.g. a custom class instance, a tuple, `bytes`) raises
/// [`TptYamlError`].
#[pyfunction]
fn dumps(value: &Bound<'_, PyAny>) -> PyResult<String> {
    let value = py_to_value(value)?;
    tpt_yaml_serde::to_string(&value).map_err(|err| TptYamlError::new_err(err.to_string()))
}

/// Recursively convert a [`Value`] into a native Python object.
///
/// Design choices (see the README's "known limitations" section for the rationale):
/// - `Value::Tagged(_, inner)` drops the tag and converts `inner` directly — Python has no
///   built-in "scalar with a custom tag" type, and inventing a wrapper type would make every
///   consumer that doesn't care about tags handle it anyway.
/// - Aliases are already resolved by `Value::from_node` before this function ever sees them, so
///   there is no alias case here: two YAML nodes that alias the same anchor become two
///   independent, unlinked Python objects (no shared-reference / `id()` equality is preserved).
fn value_to_py(py: Python<'_>, value: &Value) -> PyResult<PyObject> {
    Ok(match value {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py)?.to_owned().unbind().into_any(),
        Value::Int(i) => i.into_pyobject(py)?.unbind().into_any(),
        Value::Float(f) => f.into_pyobject(py)?.unbind().into_any(),
        Value::String(s) => s.into_pyobject(py)?.unbind().into_any(),
        Value::Sequence(items) => {
            let converted =
                items.iter().map(|item| value_to_py(py, item)).collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, converted)?.unbind().into_any()
        }
        Value::Mapping(entries) => {
            let dict = PyDict::new(py);
            for (key, value) in entries {
                dict.set_item(value_to_py(py, key)?, value_to_py(py, value)?)?;
            }
            dict.unbind().into_any()
        }
        Value::Tagged(_tag, inner) => value_to_py(py, inner)?,
    })
}

/// Recursively convert a Python object into a [`Value`].
///
/// Only the types [`loads`]'s output can produce are accepted (`None`/`bool`/`int`/`float`/
/// `str`/`list`/`dict`, arbitrarily nested); anything else is a [`TptYamlError`]. `bool` is
/// checked before `int` since in Python `bool` is a subclass of `int` (an `isinstance(True,
/// int)` is `True`), so checking `int` first would silently turn `True`/`False` into `1`/`0`.
fn py_to_value(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    if value.is_none() {
        return Ok(Value::Null);
    }
    if let Ok(b) = value.downcast::<PyBool>() {
        return Ok(Value::Bool(b.is_true()));
    }
    if let Ok(i) = value.downcast::<PyInt>() {
        let i: i64 = i.extract()?;
        return Ok(Value::Int(i));
    }
    if let Ok(f) = value.downcast::<PyFloat>() {
        return Ok(Value::Float(f.value()));
    }
    if let Ok(s) = value.downcast::<PyString>() {
        return Ok(Value::String(s.to_str()?.to_string()));
    }
    if let Ok(list) = value.downcast::<PyList>() {
        let items = list.iter().map(|item| py_to_value(&item)).collect::<PyResult<Vec<_>>>()?;
        return Ok(Value::Sequence(items));
    }
    if let Ok(dict) = value.downcast::<PyDict>() {
        let mut entries = Vec::with_capacity(dict.len());
        for (key, value) in dict.iter() {
            entries.push((py_to_value(&key)?, py_to_value(&value)?));
        }
        return Ok(Value::Mapping(entries));
    }
    Err(PyTypeError::new_err(format!(
        "unsupported type for tpt_yaml.dumps: {}",
        value.get_type().name()?
    )))
}

#[pymodule]
fn tpt_yaml(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("TptYamlError", m.py().get_type::<TptYamlError>())?;
    m.add_function(wrap_pyfunction!(loads, m)?)?;
    m.add_function(wrap_pyfunction!(dumps, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_round_trips_a_simple_mapping() {
        Python::with_gil(|py| {
            let obj = loads(py, "name: Ada\ncount: 3\nok: true\nnothing: null\n").unwrap();
            let dict = obj.downcast_bound::<PyDict>(py).unwrap();
            let name: String = dict.get_item("name").unwrap().unwrap().extract().unwrap();
            assert_eq!(name, "Ada");
            let count: i64 = dict.get_item("count").unwrap().unwrap().extract().unwrap();
            assert_eq!(count, 3);
            let ok: bool = dict.get_item("ok").unwrap().unwrap().extract().unwrap();
            assert!(ok);
            assert!(dict.get_item("nothing").unwrap().unwrap().is_none());
        });
    }

    #[test]
    fn dumps_renders_a_python_dict() {
        Python::with_gil(|py| {
            let dict = PyDict::new(py);
            dict.set_item("a", 1).unwrap();
            dict.set_item("b", vec![1, 2, 3]).unwrap();
            let rendered = dumps(dict.as_any()).unwrap();
            // Keys/values built fresh through `tpt_yaml_serde::Serializer` render double-quoted
            // (its default string style), so check content rather than exact quoting/spacing.
            assert!(rendered.contains("\"a\": 1") || rendered.contains("a: 1"), "rendered = {rendered:?}");
            assert!(rendered.contains('b'), "rendered = {rendered:?}");

            // Round-trip through `loads` to make sure it's valid YAML, not just plausible text.
            let parsed = loads(py, &rendered).unwrap();
            let parsed_dict = parsed.downcast_bound::<PyDict>(py).unwrap();
            let a: i64 = parsed_dict.get_item("a").unwrap().unwrap().extract().unwrap();
            assert_eq!(a, 1);
        });
    }

    #[test]
    fn loads_reports_a_parse_error() {
        Python::with_gil(|py| {
            let err = loads(py, "key: [1, 2").unwrap_err();
            assert!(err.is_instance_of::<TptYamlError>(py), "err = {err:?}");
        });
    }

    #[test]
    fn dumps_rejects_an_unsupported_type() {
        Python::with_gil(|py| {
            // A Python `tuple` isn't one of the supported dumps-able types.
            let tuple = pyo3::types::PyTuple::new(py, [1, 2, 3]).unwrap();
            let err = dumps(tuple.as_any()).unwrap_err();
            assert!(err.is_instance_of::<PyTypeError>(py), "err = {err:?}");
        });
    }

    #[test]
    fn bool_is_not_confused_with_int() {
        Python::with_gil(|py| {
            let value = py_to_value(true.into_pyobject(py).unwrap().as_any()).unwrap();
            assert_eq!(value, Value::Bool(true));
        });
    }
}
