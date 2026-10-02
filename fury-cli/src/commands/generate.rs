use crate::cli::{LangFrom, LangTo};
use crate::errors::CliError;
use crate::extractors;
use crate::generators;
use crate::get_global_registry;
use std::path::PathBuf;

/// Handles the end-to-end flow of schema extraction and code generation.
pub fn handle_generate(
    lang_from: Option<LangFrom>,
    lang_to: LangTo,
    src: String,
    dst: String,
) -> Result<(), CliError> {
    let mut schemas_to_generate = Vec::new();

    if src.eq_ignore_ascii_case("all") {
        let registry = get_global_registry();
        schemas_to_generate = registry.get_all_schemas();
    } else {
        let lang = lang_from.ok_or_else(|| {
            CliError::UnsupportedSourceLanguage(
                "unspecified (required when --src is a file)".into(),
            )
        })?;

        let src_path = PathBuf::from(&src);

        let model_names = extractors::extract_model_names(&src_path, lang)?;

        if model_names.is_empty() {
            return Err(CliError::ParseError(format!("No models found in '{src}'")));
        }

        let registry = get_global_registry();

        for name in model_names {
            schemas_to_generate.push(
                registry
                    .get(&name)
                    .ok_or(CliError::SchemaNotFoundInCore(name))?,
            );
        }
    }

    let dst_path = PathBuf::from(dst);
    generators::generate_all(&schemas_to_generate, &dst_path, lang_to)?;

    // Copy the registry next to generated TS so the frontend can import it as an asset.
    let registry_src = PathBuf::from(".fury_registry.bin");
    if registry_src.exists() {
        let _ = std::fs::copy(registry_src, dst_path.join(".fury_registry.bin"));
    }

    Ok(())
}
