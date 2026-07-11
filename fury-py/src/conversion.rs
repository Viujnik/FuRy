use fury_core::db::executor::SqlValue;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyFloat, PyInt, PyString};

/// Converts a Python object to a SQL-compatible `SqlValue`.
///
/// Supports the following Python types:
/// - `None` → `SqlValue::Null`
/// - `str` → `SqlValue::String`
/// - `int` → `SqlValue::Int64`
/// - `bool` → `SqlValue::Bool`
/// - `float` → `SqlValue::Float64`
/// - `bytes` → `SqlValue::Bytes`
/// - `uuid.UUID` → `SqlValue::String` (as UUID string representation)
///
/// # Errors
///
/// Returns `TypeError` if the Python type is not supported.
pub fn python_to_sql_value(obj: &Bound<'_, PyAny>) -> PyResult<SqlValue> {
    if obj.is_none() {
        return Ok(SqlValue::Null);
    }

    // bool MUST be checked before int (bool is a subclass of int in Python)
    if let Ok(py_bool) = obj.cast::<PyBool>() {
        return Ok(SqlValue::Bool(py_bool.extract()?));
    }

    if let Ok(py_str) = obj.cast::<PyString>() {
        let s: String = py_str.extract()?;
        return Ok(SqlValue::String(s));
    }

    if let Ok(py_int) = obj.cast::<PyInt>() {
        let val: i64 = py_int.extract()?;
        return Ok(SqlValue::Int64(val));
    }

    if let Ok(py_float) = obj.cast::<PyFloat>() {
        let val: f64 = py_float.extract()?;
        return Ok(SqlValue::Float64(val));
    }

    if let Ok(py_bytes) = obj.cast::<PyBytes>() {
        let val: Vec<u8> = py_bytes.extract()?;
        return Ok(SqlValue::Bytes(val));
    }

    // uuid.UUID → String (SQLx Any doesn't support UUID natively)
    let type_name: String = obj.get_type().name()?.extract()?;
    if type_name == "UUID" {
        let uuid_str: String = obj.call_method0("__str__")?.extract()?;
        return Ok(SqlValue::String(uuid_str));
    }

    Err(pyo3::exceptions::PyTypeError::new_err(format!(
        "Unsupported Python type for SQL argument: '{}'. \
         Supported types: None, bool, str, int, float, bytes, uuid.UUID",
        type_name
    )))
}
