use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum FuryError {
    #[error("Database error: {0}")]
    Database(Arc<str>),

    #[error("Encoding error: {0}")]
    Encoding(Box<str>),

    #[error("Schema not found: {0}")]
    SchemaNotFound(Arc<str>),

    #[error("Field '{field}' not found in schema '{schema}'")]
    FieldNotFound { field: Arc<str>, schema: Arc<str> },

    #[error("Type mismatch for field '{field}': expected {expected}, got {actual}")]
    TypeMismatch {
        field: Arc<str>,
        expected: Box<str>,
        actual: Box<str>,
    },

    #[error("Buffer not finished")]
    BufferNotFinished,

    #[error("Buffer already finished")]
    BufferAlreadyFinished,

    #[error("Invalid schema {0}")]
    InvalidSchema(Box<str>),
}

impl From<sqlx::Error> for FuryError {
    fn from(err: sqlx::Error) -> Self {
        Self::Database(Arc::from(err.to_string()))
    }
}

pub type Result<T> = std::result::Result<T, FuryError>;
