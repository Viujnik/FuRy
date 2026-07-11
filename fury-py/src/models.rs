use crate::errors::fury_error_to_pyerr;
use crate::get_global_registry;
use chrono::{Datelike, Timelike};
use fury_core::encoder::buffer::BinaryRecord;
use fury_core::encoder::value::ValueType;
use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyType};
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
    fn __init_subclass__(_cls: Bound<'_, PyType>) -> PyResult<()> {
        let name: String = _cls.getattr("__name__")?.extract()?;

        let annotations: Bound<'_, PyDict> = match _cls.getattr("__annotations__") {
            Ok(ann) => ann.cast_into::<PyDict>()?,
            Err(_) => PyDict::new(_cls.py()),
        };

        let mut fields = Vec::with_capacity(annotations.len());

        for (field_name, py_type) in annotations.iter() {
            let field_name: String = field_name.extract()?;
            let field_type = python_type_to_field_type(py_type.to_owned())?;
            fields.push(FieldSchema::new(field_name, field_type, true));
        }

        let schema = ModelSchema::new(name, fields);
        get_global_registry().register(schema);

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

/// Converts a Python type annotation to a FuRy FieldType.
///
/// Supports: int, str, bool, float, bytes.
///
/// # Errors
///
/// Returns `TypeError` if the Python type is not supported.
fn python_type_to_field_type(py_type: Bound<'_, PyAny>) -> PyResult<FieldType> {
    let type_name: String = py_type.getattr("__name__")?.extract()?;
    match type_name.as_str() {
        "int" => Ok(FieldType::Int64),
        "str" => Ok(FieldType::String),
        "bool" => Ok(FieldType::Bool),
        "float" => Ok(FieldType::Float64),
        "bytes" => Ok(FieldType::Bytes),
        _ => Err(pyo3::exceptions::PyTypeError::new_err(format!(
            "Unsupported Python type: '{}' for FuRy serialization.",
            type_name
        ))),
    }
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
