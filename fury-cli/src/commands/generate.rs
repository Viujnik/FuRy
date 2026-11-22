use crate::cli::{LangFrom, LangTo};
use crate::errors::CliError;
use crate::extractors;
use crate::generators;
use fury_core::schema::registry::SchemaRegistry;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

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
    let generated = generators::generate_all(&schemas_to_generate, lang_to)?;
    let dst_path = PathBuf::from(dst);
    fs::create_dir_all(&dst_path).map_err(|source| CliError::Io {
        path: dst_path.clone(),
        source,
    })?;
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
    let mut files: Vec<_> = generated
        .into_iter()
        .map(|(name, code)| (canonical_dst.join(name), code.into_bytes()))
        .collect();
    // When generating in-place the cache is also the input registry.
    if canonical_dst != cwd {
        files.push((canonical_dst.join(".fury_registry.bin"), registry));
    }
    for (path, _) in &files {
        check_output_path(path).map_err(|source| CliError::Io {
            path: path.clone(),
            source,
        })?;
    }
    for (path, bytes) in files {
        write_output_file(&path, &bytes).map_err(|source| CliError::Io { path, source })?;
    }
    Ok(())
}

fn check_output_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "generated output path is not a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Replace a generated file only after its complete contents have been written.
fn write_output_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_file_name(format!(".fury-generated-{}-{id}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = written.and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
