use crate::errors::CliError;
use std::path::PathBuf;
use std::process::Command;

/// Imports a Python module using the user's interpreter to trigger `__init_subclass__`.
///
/// # Purpose
///
/// When a Python module defines Fury models (e.g., `class User(BaseModel)`), the
/// `__init_subclass__` hook automatically registers those models into `.fury_registry.bin`.
/// This function spawns the user's Python interpreter as a subprocess to execute the import,
/// causing registration to happen as a side effect.
///
/// # Arguments
///
/// * `src_path` - Path to the Python source file (e.g., `models/user.py`)
///
/// # Returns
///
/// * `Ok(())` - Import succeeded, `.fury_registry.bin` was updated
/// * `Err(CliError::ParseError)` - Import failed or interpreter not found
///
/// # Example
///
/// ```rust
/// let path = PathBuf::from("models/user.py");
/// import_python_module(&path)?;
/// // Now .fury_registry.bin contains User, UserCreate, etc.
/// ```
pub fn import_python_module(src_path: &PathBuf) -> Result<Vec<String>, CliError> {
    // ─── Extract directory containing the source file ───────────────────────
    // For "models/user.py", parent_dir = "models"
    // For "user.py", parent_dir = "."
    let parent_dir = src_path
        .parent()                    // Option<&Path>: Some("models") or None
        .and_then(|p| p.to_str())    // Option<&str>: Some("models") or None
        .filter(|s| !s.is_empty())   // Reject empty strings
        .unwrap_or(".");             // Default to current directory

    // ─── Extract module name from filename ──────────────────────────────────
    // "models/user.py" → "user"
    // Python will execute: `import user`
    let module_name = src_path
        .file_stem()                 // Option<&OsStr>: Some("user")
        .and_then(|s| s.to_str())    // Option<&str>: Some("user")
        .ok_or_else(|| CliError::ParseError("Invalid source file name".into()))?;

    // ─── Resolve which Python interpreter to use ────────────────────────────
    let python = resolve_python()?;

    // ─── Build PYTHONPATH environment variable ──────────────────────────────
    // We need Python to find `user.py` in the `models/` directory.
    // PYTHONPATH="models:/old/path" tells Python to search `models/` first.
    let mut pythonpath = parent_dir.to_string();
    if let Ok(existing) = std::env::var("PYTHONPATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        pythonpath.push(sep);
        pythonpath.push_str(&existing);
    }

    // ─── Spawn Python subprocess ────────────────────────────────────────────
    // Command::new("python")
    //   .arg("-c")                      // Execute command from string
    //   .arg("import user")             // The command to execute
    //   .env("PYTHONPATH", "models:...") // Set environment variable
    //   .status()                       // Run and wait for exit
    let output = Command::new(&python)
        .arg("-c")
        .arg(format!("import {module_name}"))
        .env("PYTHONPATH", pythonpath)
        .env("FURY_EMIT_MODELS", "1")
        .output()
        .map_err(|e| CliError::ParseError(format!("Failed to spawn {python}: {e}")))?;

    if !output.status.success() {
        return Err(CliError::ParseError(format!(
            "{python} -c \"import {module_name}\" failed:\n{}",
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