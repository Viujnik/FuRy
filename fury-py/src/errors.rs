use fury_core::error::FuryError;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;

/// Converts a FuRy core error into a Python exception.
///
/// Maps each `FuryError` variant to the most appropriate Python exception type:
/// - Database errors → `RuntimeError`
/// - Encoding/buffer errors → `RuntimeError`
/// - Schema/field errors → `ValueError`
/// - Row constraints mismatches → `ValueError`
/// - Type mismatches → `TypeError`
///
/// # Examples
///
/// ```rust
/// use fury_core::error::FuryError;
///
/// let err = FuryError::QueryResultRowMismatch { expected: 1, actual: 0 };
/// let py_err = fury_error_to_pyerr(err);
/// // Raises: ValueError: Query returned 0 rows, expected exact 1
/// ```
pub fn fury_error_to_pyerr(err: FuryError) -> PyErr {
    match err {
        FuryError::Database(msg) => PyRuntimeError::new_err(format!("Database failure: {}", msg)),
        FuryError::QueryResultRowMismatch { expected, actual } => PyValueError::new_err(format!(
            "Query returned {actual} rows, expected exact {expected}"
        )),
        FuryError::Encoding(msg) => {
            PyValueError::new_err(format!("FuRy binary encoding failure: {}", msg))
        }
        FuryError::SchemaNotFound(name) => {
            PyValueError::new_err(format!("Schema registry error: model '{}' not found", name))
        }
        FuryError::FieldNotFound { field, schema } => PyValueError::new_err(format!(
            "Field '{}' is missing in schema '{}'",
            field, schema
        )),
        FuryError::TypeMismatch {
            field,
            expected,
            actual,
        } => PyTypeError::new_err(format!(
            "Type mismatch on field '{}': expected {}, but database returned {}",
            field, expected, actual
        )),
        FuryError::BufferNotFinished => {
            PyRuntimeError::new_err("Attempted to read from an incomplete FlatBuffer builder")
        }
        FuryError::BufferAlreadyFinished => PyRuntimeError::new_err(
            "FlatBuffer builder is already finalized and cannot be modified",
        ),
        FuryError::InvalidSchema(msg) => {
            PyValueError::new_err(format!("Invalid metadata structure: {}", msg))
        }
    }
}
