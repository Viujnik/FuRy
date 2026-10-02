pub mod python;
pub mod python_import;

use crate::cli::LangFrom;
use crate::errors::CliError;
use std::fs;
use std::path::PathBuf;

/// Extracts model names from source files based on the language.
///
/// For Python, this function:
/// 1. Spawns a subprocess to import the module (triggering __init_subclass__)
/// 2. Reads the source file
/// 3. Uses regex to find class definitions inheriting from BaseModel
///
/// # Arguments
///
/// * `src_path` - Path to the source file
/// * `lang` - Source language (currently only Python supported)
///
/// # Returns
///
/// Vector of model names found in the source file
///
/// # Example
///
/// ```rust
/// let path = PathBuf::from("models/user.py");
/// let names = extract_model_names(&path, LangFrom::Python)?;
/// // Returns: ["User", "UserCreate", "UserUpdate", ...]
/// ```
pub fn extract_model_names(src_path: &PathBuf, lang: LangFrom) -> Result<Vec<String>, CliError> {
    match lang {
        LangFrom::Python => {
            let emitted = python_import::import_python_module(src_path)?;
            if !emitted.is_empty() {
                return Ok(emitted);
            }

            let content = fs::read_to_string(src_path).map_err(|e| CliError::Io {
                path: src_path.clone(),
                source: e,
            })?;
            python::extract_model_names(&content)
        }
    }
}
