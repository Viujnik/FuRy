use std::path::PathBuf;
use thiserror::Error;

/// Represents all possible errors that can occur during CLI execution.
#[derive(Error, Debug)]
pub enum CliError {
    /// Triggered when reading from or writing to the file system fails.
    #[error("File system error at '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Triggered when the user requests code generation for an unknown language.
    #[error("Target language '{0}' is not supported.")]
    UnsupportedTargetLanguage(String),

    /// Triggered when the user provides an unknown source language for parsing.
    #[error("Source language '{0}' is not supported for parsing.")]
    UnsupportedSourceLanguage(String),

    /// Triggered when a model is found in the source file, but its schema is not registered in Fury Core.
    #[error("Schema '{0}' was found in source, but is missing from the Core registry. Did you register it?")]
    SchemaNotFoundInCore(String),

    /// Triggered when the source file cannot be parsed correctly.
    #[error("Failed to parse source file: {0}")]
    ParseError(String),
}