use crate::errors::CliError;
use regex::Regex;
use std::sync::LazyLock;

/// Cached regular expression. Compiled exactly once upon first invocation.
static PYTHON_MODEL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(\s*[^)]*?(?:BaseModel|FuryBaseModel|Model)[^)]*?\)\s*:").unwrap()
});

/// Extracts class names inheriting from BaseModel variants.
///
/// This function does not validate the language. It is called exclusively
/// from `extractors::mod.rs` after the language has already been confirmed as Python.
pub fn extract_model_names(content: &str) -> Result<Vec<String>, CliError> {
    let mut names = Vec::new();

    for cap in PYTHON_MODEL_REGEX.captures_iter(content) {
        names.push(cap[1].to_string());
    }

    Ok(names)
}
