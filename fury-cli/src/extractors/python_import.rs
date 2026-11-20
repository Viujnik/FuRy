use crate::errors::CliError;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Executes the selected trusted Python file to collect its registered model names.
pub fn import_python_module(src_path: &Path) -> Result<Vec<String>, CliError> {
    let python = resolve_python()?;
    let source = src_path.canonicalize().map_err(|source| CliError::Io {
        path: src_path.to_path_buf(),
        source,
    })?;
    // Source paths are arguments. The requested source takes precedence over
    // identically named files in the caller's working directory.
    let script = r#"
import importlib.util
from pathlib import Path
import sys

source = Path(sys.argv[1])
sys.path.insert(0, str(source.parent))
spec = importlib.util.spec_from_file_location(source.stem, source)
if spec is None or spec.loader is None:
    raise ImportError("cannot import Fury schema source")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)
"#;
    let output = Command::new(&python)
        .args(["-c", script])
        .arg(&source)
        .env("FURY_EMIT_MODELS", "1")
        .output()
        .map_err(|e| CliError::ParseError(format!("Failed to spawn {python}: {e}")))?;

    if !output.status.success() {
        return Err(CliError::ParseError(format!(
            "{python} failed to load {}:\n{}",
            source.display(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let names: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("__FURY_MODEL__:").map(str::to_string))
        .collect();

    Ok(names)
}

/// Resolves the Python interpreter to use based on environment variables.
///
/// # Resolution Order
///
/// 1. `$FURY_PYTHON` - Explicit override (for CI or custom setups)
/// 2. `$VIRTUAL_ENV` - Active virtualenv (venv, uv, pipenv)
/// 3. `$CONDA_PREFIX` - Active conda environment
/// 4. `python3` / `python` - Fallback to system Python
///
/// # Returns
///
/// * `Ok(String)` - Path to Python interpreter or command name
/// * `Err(CliError::ParseError)` - No interpreter found
///
/// # Example
///
/// ```rust
/// // With VIRTUAL_ENV=/path/to/venv set:
/// let python = resolve_python()?;  // Returns "/path/to/venv/bin/python"
///
/// // Without any env vars:
/// let python = resolve_python()?;  // Returns "python3" (from PATH)
/// ```
fn resolve_python() -> Result<String, CliError> {
    if let Ok(custom) = std::env::var("FURY_PYTHON") {
        return Ok(custom);
    }

    if let Ok(venv) = std::env::var("VIRTUAL_ENV") {
        let v = PathBuf::from(venv);

        // Try Unix path first
        let unix_path = v.join("bin/python");
        if unix_path.exists() {
            return Ok(unix_path.to_string_lossy().into_owned());
        }

        // Try Windows path
        let windows_path = v.join("Scripts/python.exe");
        if windows_path.exists() {
            return Ok(windows_path.to_string_lossy().into_owned());
        }
    }

    if let Ok(prefix) = std::env::var("CONDA_PREFIX") {
        let p = PathBuf::from(prefix);

        let unix_path = p.join("bin/python");
        if unix_path.exists() {
            return Ok(unix_path.to_string_lossy().into_owned());
        }

        let windows_path = p.join("python.exe");
        if windows_path.exists() {
            return Ok(windows_path.to_string_lossy().into_owned());
        }
    }

    for cand in ["python3", "python"] {
        if Command::new(cand)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok(cand.into());
        }
    }

    Err(CliError::ParseError("No Python interpreter found".into()))
}
