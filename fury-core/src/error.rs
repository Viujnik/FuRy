use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur in the FuRy serialization engine.
///
/// This enum represents all possible error conditions in FuRy, including
/// database operations, serialization/deserialization, and schema validation.
/// ```
#[derive(Error, Debug, Clone)]
pub enum FuryError {
    /// Database operation failed (connection error, query error, etc.).
    ///
    /// Contains a human-readable error message from the database driver.
    #[error("Database error: {0}")]
    Database(Arc<str>),

    /// Database query returned an unexpected number of rows.
    ///
    /// Occurs when strict row constraints are violated (e.g., `fetch_one`
    /// expects exactly 1 row, but the query results are empty or duplicated).
    #[error("Query rows mismatch: expected {expected}, but database returned {actual}")]
    QueryResultRowMismatch {
        /// The exact number of rows the execution engine expected to process.
        expected: usize,
        /// The actual number of rows returned by the SQL statement at runtime.
        actual: usize,
    },

    /// Serialization or deserialization error.
    ///
    /// Occurs when data cannot be encoded to or decoded from binary format.
    #[error("Encoding error: {0}")]
    Encoding(Box<str>),

    /// Requested schema is not registered in the schema registry.
    ///
    /// Contains the name of the missing schema.
    #[error("Schema not found: {0}")]
    SchemaNotFound(Arc<str>),

    /// Requested field does not exist in the model schema.
    ///
    /// Contains the field name and schema name for debugging.
    #[error("Field '{field}' not found in schema '{schema}'")]
    FieldNotFound {
        /// Name of the missing field.
        field: Arc<str>,
        /// Name of the schema where the field was expected.
        schema: Arc<str>,
    },

    /// Type mismatch between expected and actual data types.
    ///
    /// Occurs when a field value cannot be converted to the expected type.
    #[error("Type mismatch for field '{field}': expected {expected}, got {actual}")]
    TypeMismatch {
        /// Name of the field with type mismatch.
        field: Arc<str>,
        /// Expected type name.
        expected: Box<str>,
        /// Actual type name or error description.
        actual: Box<str>,
    },

    /// Attempted to read from a buffer that has not been finalized.
    ///
    /// Call `finish()` on the buffer before reading.
    #[error("Buffer not finished")]
    BufferNotFinished,

    /// Attempted to modify a buffer that has already been finalized.
    ///
    /// The buffer is immutable after `finish()` is called.
    #[error("Buffer already finished")]
    BufferAlreadyFinished,

    /// Schema definition is invalid or malformed.
    ///
    /// Contains a description of what is wrong with the schema.
    #[error("Invalid schema: {0}")]
    InvalidSchema(Box<str>),
}

impl From<sqlx::Error> for FuryError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(Arc::from(err.to_string()))
    }
}

/// Result type alias for FuRy operations.
pub type Result<T> = std::result::Result<T, FuryError>;
