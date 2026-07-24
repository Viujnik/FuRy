use crate::cli::LangFrom;
use crate::errors::CliError;

pub mod python;

/// Extracts class names from source code based on the language.
pub fn extract_model_names(content: &str, lang: LangFrom) -> Result<Vec<String>, CliError> {
    match lang {
        LangFrom::Python => python::extract_model_names(content),
    }
}