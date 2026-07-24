use crate::cli::{LangFrom, LangTo};
use crate::errors::CliError;
use crate::extractors;
use crate::generators;
use crate::get_global_registry;
use std::fs;
use std::path::PathBuf;

/// Handles the end-to-end flow of schema extraction and code generation.
pub fn handle_generate(
    lang_from: Option<LangFrom>,
    lang_to: LangTo,
    src: String,
    dst: String,
) -> Result<(), CliError> {
    let registry = get_global_registry();
    let mut schemas_to_generate = Vec::new();

    if src.eq_ignore_ascii_case("all") {
        schemas_to_generate = registry.get_all_schemas();
    } else {
        let lang = lang_from.ok_or_else(|| {
            CliError::UnsupportedSourceLanguage(
                "unspecified (required when --src is a file)".to_string(),
            )
        })?;

        let src_path = PathBuf::from(&src);
        let content = fs::read_to_string(&src_path).map_err(|e| CliError::Io {
            path: src_path.clone(),
            source: e,
        })?;

        let model_names = extractors::extract_model_names(&content, lang)?;

        for name in model_names {
            if let Some(schema) = registry.get(&name) {
                schemas_to_generate.push(schema);
            } else {
                return Err(CliError::SchemaNotFoundInCore(name));
            }
        }
    }

    let dst_path = PathBuf::from(dst);
    match lang_to {
        LangTo::Ts => {
            generators::typescript::generate_all(&schemas_to_generate, &dst_path)?;
        }
    }

    Ok(())
}
