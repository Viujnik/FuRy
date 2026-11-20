use fury_core::schema::registry::{FieldType, ModelSchema};
use std::fmt::Write;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::errors::CliError;

/// Iterates over validated schemas and writes the corresponding TypeScript classes to disk.
pub fn generate_all(schemas: &[Arc<ModelSchema>], dst_dir: &Path) -> Result<(), CliError> {
    let mut filenames = std::collections::HashSet::new();
    let mut generated = Vec::with_capacity(schemas.len());
    for schema in schemas {
        validate_identifier(schema.name())?;
        if ["$FuryWasmRecord", "globalThis"].contains(&schema.name()) {
            return Err(CliError::ParseError(
                "model name collides with generated imports".into(),
            ));
        }
        let filename = format!("{}.ts", schema.name().to_lowercase());
        if !filenames.insert(filename.clone()) {
            return Err(CliError::ParseError(
                "case-insensitive generated filename collision".into(),
            ));
        }
        for field in schema
            .fields()
            .iter()
            .filter(|f| ts_primitive_type(f.field_type()).is_some())
        {
            if ["constructor", "_internal", "toBytes", "$record"].contains(&field.name()) {
                return Err(CliError::ParseError(format!(
                    "field '{}.{}' collides with generated API",
                    schema.name(),
                    field.name()
                )));
            }
        }
        generated.push((filename, generate_class_string(schema)));
    }

    fs::create_dir_all(dst_dir).map_err(|source| CliError::Io {
        path: dst_dir.to_path_buf(),
        source,
    })?;
    for (filename, code) in generated {
        let path = dst_dir.join(filename);
        fs::write(&path, code).map_err(|source| CliError::Io { path, source })?;
    }
    Ok(())
}

/// Check whether a name can be emitted as a safe TypeScript identifier.
fn validate_identifier(name: &str) -> Result<(), CliError> {
    let mut chars = name.chars();
    let valid = chars
        .next()
        .is_some_and(|c| unicode_ident::is_xid_start(c) || c == '_' || c == '$')
        && chars.all(|c| unicode_ident::is_xid_continue(c) || c == '_' || c == '$');
    let reserved = [
        "break",
        "case",
        "catch",
        "class",
        "const",
        "continue",
        "debugger",
        "default",
        "delete",
        "do",
        "else",
        "enum",
        "export",
        "extends",
        "false",
        "finally",
        "for",
        "function",
        "if",
        "import",
        "in",
        "instanceof",
        "new",
        "null",
        "return",
        "super",
        "switch",
        "this",
        "throw",
        "true",
        "try",
        "typeof",
        "var",
        "void",
        "while",
        "with",
        "yield",
        "let",
        "static",
        "implements",
        "interface",
        "package",
        "private",
        "protected",
        "public",
        "await",
        "eval",
        "arguments",
        "any",
        "boolean",
        "number",
        "string",
        "symbol",
        "unknown",
        "never",
        "object",
        "bigint",
        "undefined",
        "intrinsic",
    ];
    if !valid || reserved.contains(&name) {
        return Err(CliError::ParseError(format!(
            "unsupported TypeScript identifier '{name}'"
        )));
    }
    Ok(())
}

/// Maps a field to its base TypeScript type.
///
/// Returns `None` for composite types (List/Map/Object): they are silently
/// skipped — no getters/setters, no TODO noise. Users who need them write
/// custom accessors on top of the raw `WasmRecord` themselves.
fn ts_primitive_type(field_type: &FieldType) -> Option<&'static str> {
    Some(match field_type {
        FieldType::Int8
        | FieldType::Int16
        | FieldType::Int32
        | FieldType::Int64
        | FieldType::UInt8
        | FieldType::UInt16
        | FieldType::UInt32
        | FieldType::UInt64
        | FieldType::Float32
        | FieldType::Float64 => "number",
        FieldType::Bool => "boolean",
        FieldType::String => "string",
        FieldType::Bytes => "globalThis.Uint8Array",
        FieldType::DateTime | FieldType::Date => "globalThis.Date",
        _ => return None,
    })
}

/// WASM boundary method pair for a primitive field type.
fn wasm_methods(field_type: &FieldType) -> (&'static str, &'static str) {
    match field_type {
        FieldType::Bool => ("getBool", "setBool"),
        FieldType::String => ("getString", "setString"),
        FieldType::Bytes => ("getBytes", "setBytes"),
        FieldType::DateTime | FieldType::Date => ("getDate", "setDate"),
        _ => ("getNumber", "setNumber"),
    }
}

/// Constructs the TypeScript class payload with getters and setters mapped to WASM boundary calls.
///
/// Only primitive fields get accessors. Nullable fields expose `T | null` on the
/// getter (the WASM layer returns JS `null` for `FLAG_NONE`). Composite fields
/// (List/Map/Object) are omitted entirely.
fn generate_class_string(schema: &ModelSchema) -> String {
    let mut code = String::with_capacity(2048);
    let name = schema.name();

    writeln!(code, "// AUTO-GENERATED BY FURY CLI. DO NOT EDIT.").unwrap();
    writeln!(
        code,
        "// Adjust the import path below to match your wasm-bindgen build output."
    )
    .unwrap();
    writeln!(
        code,
        "import {{ WasmRecord as $FuryWasmRecord }} from '@fury/wasm-core';\n"
    )
    .unwrap();

    writeln!(code, "export class {} {{", name).unwrap();
    writeln!(code, "    private readonly _internal: $FuryWasmRecord;").unwrap();

    writeln!(code, "\n    constructor(buffer?: globalThis.Uint8Array) {{").unwrap();
    writeln!(
        code,
        "        this._internal = new $FuryWasmRecord('{}', buffer);",
        name
    )
    .unwrap();
    writeln!(code, "    }}").unwrap();

    for field in schema.fields() {
        let field_name = field.name();

        // Composite types: skip silently, no TODO spam.
        let Some(base) = ts_primitive_type(field.field_type()) else {
            continue;
        };
        let (getter, setter) = wasm_methods(field.field_type());
        let getter_type = if field.is_nullable() {
            format!("{base} | null")
        } else {
            base.to_string()
        };

        let key = ts_string(field_name);
        writeln!(
            code,
            "\n    public get [{key}](): {getter_type} {{ return this._internal.{getter}({key}); }}"
        )
        .unwrap();
        let clear_null = if field.is_nullable() {
            format!("if (val === null) {{ this._internal.setNull({key}); return; }} ")
        } else {
            String::new()
        };
        writeln!(
            code,
            "    public set [{key}](val: {getter_type}) {{ {clear_null}this._internal.{setter}({key}, val); }}"
        )
        .unwrap();
    }

    writeln!(code, "\n    public toBytes(): globalThis.Uint8Array {{ return this._internal.toFinishedBytes(); }}").unwrap();
    writeln!(
        code,
        "    public get $record(): $FuryWasmRecord {{ return this._internal; }}"
    )
    .unwrap();
    writeln!(code, "}}").unwrap();

    code
}

/// Escape a field name in a TypeScript string literal.
fn ts_string(name: &str) -> String {
    let mut literal = String::from("'");
    for ch in name.chars() {
        match ch {
            '\'' => literal.push_str("\\'"),
            '\\' => literal.push_str("\\\\"),
            c if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') => {
                write!(literal, "\\u{:04x}", c as u32).unwrap();
            }
            c => literal.push(c),
        }
    }
    literal.push('\'');
    literal
}
