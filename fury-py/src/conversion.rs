use fury_core::db::executor::SqlValue;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyFloat, PyInt, PyString};

pub fn python_to_sql_value(obj: &Bound<'_, PyAny>) -> PyResult<SqlValue> {
    if obj.is_none() {
        return Ok(SqlValue::Null);
    }

    if let Ok(py_str) = obj.cast::<PyString>() {
        let s: String = py_str.extract()?;
        return Ok(SqlValue::String(s));
    }

    if let Ok(py_int) = obj.cast::<PyInt>() {
        let val: i64 = py_int.extract()?;
        return Ok(SqlValue::Int64(val));
    }

    if let Ok(py_bool) = obj.cast::<PyBool>() {
        let val: bool = py_bool.extract()?;
        return Ok(SqlValue::Bool(val));
    }

    if let Ok(py_float) = obj.cast::<PyFloat>() {
        let val: f64 = py_float.extract()?;
        return Ok(SqlValue::Float64(val));
    }

    if let Ok(py_bytes) = obj.cast::<PyBytes>() {
        let val: Vec<u8> = py_bytes.extract()?;
        return Ok(SqlValue::Bytes(val));
    }

    let type_name: String = obj.getattr("__class__")?.getattr("__name__")?.extract()?;
    if type_name == "UUID" {
        let uuid_str: String = obj.call_method0("__str__")?.extract()?;
        return Ok(SqlValue::String(uuid_str));
    }

    Err(pyo3::exceptions::PyTypeError::new_err(format!(
        "Unsupported SQL argument type from Python: '{}'",
        type_name
    )))
}
