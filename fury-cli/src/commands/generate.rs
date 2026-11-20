use crate::cli::{LangFrom, LangTo};
use crate::errors::CliError;
use crate::extractors;
use crate::generators;
use fury_core::schema::registry::SchemaRegistry;
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
        let registry = SchemaRegistry::load_from_file(".fury_registry.bin")
            .map_err(|e| CliError::ParseError(e.to_string()))?;
        schemas_to_generate = registry.get_all_schemas();
    } else {
        let lang = lang_from.ok_or_else(|| {
            CliError::UnsupportedSourceLanguage(
                "unspecified (required when --src is a file)".into(),
            )
        })?;

        let src_path = PathBuf::from(&src);

        let mut model_names = extractors::extract_model_names(&src_path, lang)?;
        model_names.sort();
        model_names.dedup();

        if model_names.is_empty() {
            return Err(CliError::ParseError(format!("No models found in '{src}'")));
        }

        let registry = SchemaRegistry::load_from_file(".fury_registry.bin")
            .map_err(|e| CliError::ParseError(e.to_string()))?;

        for name in model_names {
            schemas_to_generate.push(
                registry
                    .get(&name)
                    .ok_or(CliError::SchemaNotFoundInCore(name))?,
            );
        }
    }

    let registry = SchemaRegistry::encode_schemas(schemas_to_generate.iter().map(AsRef::as_ref))
        .map_err(|e| CliError::ParseError(e.to_string()))?;
    let dst_path = PathBuf::from(dst);
    generators::generate_all(&schemas_to_generate, &dst_path, lang_to)?;

    let canonical_dst = dst_path.canonicalize().map_err(|source| CliError::Io {
        path: dst_path.clone(),
        source,
    })?;
    let cwd = PathBuf::from(".")
        .canonicalize()
        .map_err(|source| CliError::Io {
            path: ".".into(),
            source,
        })?;
    // When generating in-place the cache is also the input registry.
    if canonical_dst == cwd {
        return Ok(());
    }
    let path = dst_path.join(".fury_registry.bin");
    std::fs::write(&path, registry).map_err(|source| CliError::Io { path, source })?;
    Ok(())
}
