use crate::errors::fury_error_to_pyerr;
use crate::get_global_registry;
use chrono::{Datelike, Timelike};
use fury_core::encoder::buffer::BinaryRecord;
use fury_core::encoder::value::ValueType;
use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes, PyDict, PyList, PyType};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;

/// Cached reference to Python's `datetime.date` class for fast instantiation.
static DATE_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

/// Cached reference to Python's `datetime.datetime` class for fast instantiation.
static DATETIME_CLS: OnceLock<Py<PyAny>> = OnceLock::new();

/// Base model layout for FuRy high-performance binary serialization.
///
/// Wraps sequential binary record payloads and provides direct, zero-copy field
/// access by mapping lookups into pre-calculated boundaries.
#[pyclass(subclass, skip_from_py_object)]
pub struct BaseModel {
    buffer: Vec<u8>,
    field_offsets: HashMap<String, (usize, usize)>,
    schema: Arc<ModelSchema>,
}

impl BaseModel {
    /// Eagerly injects underlying record byte buffers and compiled offset metadata map layouts.
    pub fn set_data(
        &mut self,
        buffer: Vec<u8>,
        field_offsets: HashMap<String, (usize, usize)>,
        schema: Arc<ModelSchema>,
    ) {
        self.buffer = buffer;
        self.field_offsets = field_offsets;
        self.schema = schema;
    }
}

#[pymethods]
impl BaseModel {
    /// Creates a new empty BaseModel instance.
    #[new]
    fn new() -> Self {
        Self {
            buffer: Vec::new(),
            field_offsets: HashMap::new(),
            schema: Arc::new(ModelSchema::new("Unknown", vec![])),
        }
    }

    /// Registers the model schema when a subclass is defined.
    ///
    /// This is called automatically by Python when a class inherits from BaseModel.
    /// It extracts type annotations and registers the schema in the global registry.
    #[classmethod]
    #[pyo3(signature = (**_kwargs))]
    fn __init_subclass__<'py>(
        _cls: &Bound<'py, PyType>,
        _kwargs: Option<&Bound<'py, PyDict>>,
    ) -> PyResult<()> {
        _cls.py();
        let name: String = _cls.getattr("__name__")?.extract()?;

        if name == "BaseModel" || name == "FuryBaseModel" {
            // Hardcored
            return Ok(());
        }

        let mro_result: PyResult<Vec<Bound<'py, PyType>>> = _cls
            .mro()
            .iter()
            .map(|x| x.cast_into::<PyType>().map_err(|e| e.into()))
            .collect();

        let mro: Vec<Bound<'py, PyType>> = mro_result?;

        let mut ordered: Vec<(String, Bound<'py, PyAny>)> = Vec::new();
        for base_class in mro.iter().rev() {
            if let Ok(annotations_obj) = base_class.getattr("__annotations__")
                && let Ok(dict) = annotations_obj.cast::<PyDict>()
            {
                for (key, value) in dict.iter() {
                    let k: String = key.extract()?;
                    match ordered.iter_mut().find(|(name, _)| *name == k) {
                        Some(slot) => slot.1 = value,
                        None => ordered.push((k, value)),
                    }
                }
            }
        }
        let mut fields: Vec<FieldSchema> = Vec::with_capacity(ordered.len());
        for (field_name, py_type) in ordered {
            let (field_type, is_nullable) = parse_python_type(&py_type)?;
            fields.push(FieldSchema::new(field_name, field_type, is_nullable));
        }

        let schema = ModelSchema::new(name.clone(), fields);
        let emit = std::env::var_os("FURY_EMIT_MODELS").is_some();
        get_global_registry()
            .register_with_cache(schema, emit)
            .map_err(fury_error_to_pyerr)?;

        if emit {
            println!("__FURY_MODEL__:{name}");
        }

        Ok(())
    }

    /// Returns the raw binary buffer as Python bytes.
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        if self.buffer.is_empty() {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "Buffer is uninitialized",
            ));
        }
        Ok(PyBytes::new(py, &self.buffer))
    }

    /// Gets a field value by name with zero-copy buffer access.
    ///
    /// This is called automatically when accessing attributes on the model.
    /// It performs O(1) lookup in field_offsets and schema for maximum performance.
    fn __getattr__(&self, name: &str, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let &(offset, len) = self.field_offsets.get(name).ok_or_else(|| {
            pyo3::exceptions::PyAttributeError::new_err(format!("Field '{name}' not found"))
        })?;

        let field = self.schema.find_field(name).ok_or_else(|| {
            pyo3::exceptions::PyAttributeError::new_err(format!(
                "Field '{name}' not found in schema"
            ))
        })?;

        let field_bytes = &self.buffer[offset..(offset + len)];

        let value = ValueType::from_bytes(field_bytes, field.field_type()).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to parse field '{name}'"))
        })?;

        value_to_python(py, &value)
    }
}

impl std::fmt::Debug for BaseModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BaseModel")
            .field("schema_name", &self.schema.name())
            .field("buffer_len", &self.buffer.len())
            .field("fields_count", &self.field_offsets.len())
            .finish()
    }
}

/// Creates a Python model instance from a binary record.
///
/// Extracts field offsets (NOT copies!) from the buffer and initializes
/// the BaseModel with zero-copy access to the data.
/// The schema is shared via Arc, avoiding duplication across instances.
pub fn create_py_model_instance(
    model_class: Bound<'_, PyAny>,
    schema: &Arc<ModelSchema>,
    builder: &BinaryRecord,
) -> PyResult<Py<PyAny>> {
    let instance = model_class.call0()?;

    let mut field_offsets = HashMap::with_capacity(schema.fields_count());

    for field in schema.fields() {
        if let Some((offset, length)) = builder.get_field_offset(field.name()) {
            field_offsets.insert(field.name().to_owned(), (offset, length));
        }
    }

    let buffer_bytes = builder.as_bytes().map_err(fury_error_to_pyerr)?.to_vec();

    let mut base_model: PyRefMut<BaseModel> = instance.extract()?;
    base_model.set_data(buffer_bytes, field_offsets, Arc::clone(schema));
    drop(base_model);

    Ok(instance.unbind())
}

/// Global atomic cache container holding a pointer-assigned reference to the CPython `datetime.date` class metadata layout.
///
/// Thread-safe and initialized exactly once upon the first processed date field retrieval.
fn get_date_cls(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
    let cls = DATE_CLS.get_or_init(|| {
        py.import("datetime")
            .and_then(|m| m.getattr("date"))
            .map(|c| c.unbind())
            .expect("datetime module must be available")
    });
    Ok(cls.bind(py))
}

/// Global atomic cache container holding a pointer-assigned reference to the CPython `datetime.date` class metadata layout.
///
/// Thread-safe and initialized exactly once upon the first processed date field retrieval.
fn get_datetime_cls(py: Python<'_>) -> PyResult<&Bound<'_, PyAny>> {
    let cls = DATETIME_CLS.get_or_init(|| {
        py.import("datetime")
            .and_then(|m| m.getattr("datetime"))
            .map(|c| c.unbind())
            .expect("datetime.datetime module must be available")
    });
    Ok(cls.bind(py))
}

/// Converts a FuRy ValueType to a Python object.
///
/// Handles all supported types including primitives, strings, bytes,
/// dates, UUIDs, and nested collections.
fn value_to_python(py: Python<'_>, value: &ValueType) -> PyResult<Py<PyAny>> {
    match value {
        ValueType::None => Ok(py.None()),

        ValueType::Int8(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::Int16(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::Int32(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::Int64(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::UInt8(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::UInt16(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::UInt32(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::UInt64(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::Float32(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::Float64(v) => Ok(v.into_pyobject(py)?.into_any().unbind()),
        ValueType::Bool(v) => {
            let b: bool = *v;
            let py_bool = b.into_pyobject(py)?;
            Ok(py_bool.to_owned().into_any().unbind())
        }
        ValueType::String(v) => Ok(v.clone().into_pyobject(py)?.into_any().unbind()),
        ValueType::Bytes(v) => Ok(PyBytes::new(py, v).into_any().unbind()),

        ValueType::Date(d) => {
            let py_date = get_date_cls(py)?.call1((d.year(), d.month(), d.day()))?;
            Ok(py_date.unbind())
        }
        ValueType::DateTime(dt) => {
            let py_dt = get_datetime_cls(py)?.call1((
                dt.year(),
                dt.month(),
                dt.day(),
                dt.hour(),
                dt.minute(),
                dt.second(),
                dt.nanosecond() / 1000,
            ))?;
            Ok(py_dt.unbind())
        }
        ValueType::Uuid(u) => Ok(u.to_string().into_pyobject(py)?.into_any().unbind()),
        ValueType::List(items) => {
            let py_items = items
                .iter()
                .map(|item| value_to_python(py, item))
                .collect::<PyResult<Vec<_>>>()?;

            let list = PyList::new(py, py_items)?;
            Ok(list.into_any().unbind())
        }
        ValueType::Map(entries) => {
            let dict = PyDict::new(py);
            for (key, value) in entries {
                let py_key = value_to_python(py, key)?;
                let py_value = value_to_python(py, value)?;
                dict.set_item(py_key.bind(py), py_value.bind(py))?;
            }
            Ok(dict.into_any().unbind())
        }
        ValueType::Object(_) => Err(pyo3::exceptions::PyNotImplementedError::new_err(
            "Nested models serialization not implemented",
        )),
    }
}

fn parse_python_type(type_obj: &Bound<'_, PyAny>) -> PyResult<(FieldType, bool)> {
    let py = type_obj.py();
    let typing = py.import("typing")?;
    let get_origin = typing.getattr("get_origin")?;
    let get_args = typing.getattr("get_args")?;

    let origin = get_origin.call1((type_obj,))?;
    let mut nullable = false;
    let target = type_obj.clone();

    if !origin.is_none() {
        let args = get_args.call1((type_obj,))?;
        let args = args.cast::<pyo3::types::PyTuple>()?;
        let origin_name: String = origin
            .getattr("__name__")
            .and_then(|n| n.extract())
            .unwrap_or_default();

        match origin_name.as_str() {
            "list" => {
                let inner = args.get_item(0)?;
                let (inner_type, _) = parse_python_type(&inner)?;
                return Ok((FieldType::List(Box::new(inner_type)), nullable));
            }
            "dict" => {
                let key = args.get_item(0)?;
                let value = args.get_item(1)?;
                let (key_type, _) = parse_python_type(&key)?;
                let (val_type, _) = parse_python_type(&value)?;
                return Ok((
                    FieldType::Map(Box::new(key_type), Box::new(val_type)),
                    nullable,
                ));
            }
            "Union" | "UnionType" => {
                let mut inner: Option<Bound<'_, PyAny>> = None;
                for arg in args.iter() {
                    if arg.is(py.get_type::<pyo3::types::PyNone>()) {
                        nullable = true;
                    } else if inner.replace(arg).is_some() {
                        return Err(pyo3::exceptions::PyTypeError::new_err(
                            "only unions of one supported type with None are supported",
                        ));
                    }
                }
                if let Some(t) = inner {
                    let (ft, _) = parse_python_type(&t)?;
                    return Ok((ft, nullable));
                }
            }
            _ => {}
        }
    }

    let module: String = target.getattr("__module__")?.extract().unwrap_or_default();
    let name: String = target.getattr("__name__")?.extract().unwrap_or_default();

    let f_type = match (module.as_str(), name.as_str()) {
        ("builtins", "int") => FieldType::Int64,
        ("builtins", "str") => FieldType::String,
        ("builtins", "bool") => FieldType::Bool,
        ("builtins", "float") => FieldType::Float64,
        ("builtins", "bytes") | ("builtins", "bytearray") => FieldType::Bytes,
        ("datetime", "datetime") => FieldType::DateTime,
        ("datetime", "date") => FieldType::Date,
        ("uuid", "UUID") => FieldType::Uuid,
        _ => {
            return Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "Unsupported Fury annotation: {module}.{name}"
            )));
        }
    };

    Ok((f_type, nullable))
}
