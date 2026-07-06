use pyo3::prelude::*;
use pyo3::exceptions::{PyRuntimeError, PyValueError, PyTypeError};
use fury_core::error::FuryError;

pub fn fury_error_to_pyerr(err: FuryError) -> PyErr {
    match err {
        FuryError::Database(msg) => {
            PyRuntimeError::new_err(format!("Database failure: {}", msg))
        }
        FuryError::Encoding(msg) => {
            PyValueError::new_err(format!("FuRy binary encoding failure: {}", msg))
        }
        FuryError::SchemaNotFound(name) => {
            PyValueError::new_err(format!("Schema registry error: model '{}' not found", name))
        }
        FuryError::FieldNotFound { field, schema } => {
            PyValueError::new_err(format!("Field '{}' is missing in schema '{}'", field, schema))
        }
        FuryError::TypeMismatch { field, expected, actual } => {
            PyTypeError::new_err(format!(
                "Type mismatch on field '{}': expected {}, but database returned {}",
                field, expected, actual
            ))
        }
        FuryError::BufferNotFinished => {
            PyRuntimeError::new_err("Attempted to read from an incomplete FlatBuffer builder")
        }
        FuryError::BufferAlreadyFinished => {
            PyRuntimeError::new_err("FlatBuffer builder is already finalized and cannot be modified")
        }
        FuryError::InvalidSchema(msg) => {
            PyValueError::new_err(format!("Invalid metadata structure: {}", msg))
        }
    }
}
