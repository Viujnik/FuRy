use crate::cli::LangTo;
use crate::errors::CliError;
use fury_core::schema::registry::ModelSchema;
use std::path::Path;
use std::sync::Arc;

pub mod typescript;

/// Generates code for all schemas in the specified target language.
pub fn generate_all(
    schemas: &[Arc<ModelSchema>],
    dst_dir: &Path,
    lang: LangTo,
) -> Result<(), CliError> {
    match lang {
        LangTo::Ts => typescript::generate_all(schemas, dst_dir),
    }
}
