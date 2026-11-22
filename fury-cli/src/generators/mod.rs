use crate::cli::LangTo;
use crate::errors::CliError;
use fury_core::schema::registry::ModelSchema;
use std::sync::Arc;

pub mod typescript;

/// Renders the output for each schema in the specified target language.
pub fn generate_all(
    schemas: &[Arc<ModelSchema>],
    lang: LangTo,
) -> Result<Vec<(String, String)>, CliError> {
    match lang {
        LangTo::Ts => typescript::generate_all(schemas),
    }
}
