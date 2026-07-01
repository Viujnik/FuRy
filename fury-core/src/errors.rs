use thiserror::Error;

#[derive(Error, Debug)]
pub enum FuryError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Encoding error: {0}")]
    Encoding(String),

    #[error("Schema not found: {0}")]
    SchemaNotFound(String),

    #[error("Field '{field}' not found in schema '{schema}'")]
    FieldNotFound { field: String, schema: String },

    #[error("Type mismatch for field '{field}': expected {expected}, got {actual}")]
    TypeMismatch {
        field: String,
        expected: String,
        actual: String,
    },

    #[error("Buffer not finished")]
    BufferNotFinished,

    #[error("Buffer already finished")]
    BufferAlreadyFinished,

    #[error("Invalid schema {0}")]
    InvalidSchema(String),
}

pub type Result<T> = std::result::Result<T, FuryError>;
