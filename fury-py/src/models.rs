use chrono::{Datelike, Timelike};
use fury_core::encoder::buffer::FlatBufferBuilder;
use fury_core::encoder::value::ValueType;
use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyType};
use std::collections::HashMap;

use crate::get_global_registry;

#[pyclass(subclass, skip_from_py_object)]
pub struct BaseModel {
    buffer: Option<Vec<u8>>,
    fields: HashMap<String, Vec<u8>>,
    model_name: Option<String>,
}

// Ручная реализация Debug, т.к. FlatBufferBuilder не Debug
impl std::fmt::Debug for BaseModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BaseModel")
            .field("model_name", &self.model_name)
            .field("buffer_len", &self.buffer.as_ref().map(|b| b.len()))
            .field("fields_count", &self.fields.len())
            .finish()
    }
}

impl BaseModel {
    pub fn set_buffer(
        &mut self,
        bytes: Vec<u8>,
        fields: HashMap<String, Vec<u8>>,
        model_name: String,
    ) {
        self.buffer = Some(bytes);
        self.fields = fields;
        self.model_name = Some(model_name);
    }
}

#[pymethods]
impl BaseModel {
    #[new]
    fn new() -> Self {
        Self {
            buffer: None,
            fields: HashMap::new(),
            model_name: None,
        }
    }

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

    /// Возвращает бинарный буфер
    fn to_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let buffer = self.buffer.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Buffer not initialized")
        })?;
        Ok(PyBytes::new(py, buffer))
    }

    /// Ленивое чтение полей
    fn __getattr__(&self, name: &str, py: Python) -> PyResult<Py<PyAny>> {
        let field_bytes = self.fields.get(name).ok_or_else(|| {
            pyo3::exceptions::PyAttributeError::new_err(format!("Field '{}' not found", name))
        })?;

        let model_name = self.model_name.as_ref().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Model name not set")
        })?;

        let schema = get_global_registry().get(model_name).ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Schema not found: {}", model_name))
        })?;

        let field = schema.fields().iter().find(|f| f.name() == name).ok_or_else(|| {
            pyo3::exceptions::PyAttributeError::new_err(format!(
                "Field '{}' not found in schema",
                name
            ))
        })?;

        let value = ValueType::from_bytes(field_bytes, field.field_type()).ok_or_else(|| {
            pyo3::exceptions::PyValueError::new_err(format!("Failed to parse field '{}'", name))
        })?;

        value_to_python(py, &value)
    }
}

pub fn create_py_model_instance(
    model_class: Bound<'_, PyAny>,
    schema: &ModelSchema,
    builder: &FlatBufferBuilder,
) -> PyResult<Py<PyAny>> {
    let instance = model_class.call0()?;

    // Собираем байты полей в HashMap
    let mut fields_map = HashMap::new();
    for field in schema.fields() {
        if let Some(bytes) = builder.get_field_bytes(field.name()) {
            fields_map.insert(field.name().to_string(), bytes.to_vec());
        }
    }

    // Получаем финальные байты буфера
    let buffer_bytes = builder.as_bytes().map_err(|e| {
        pyo3::exceptions::PyRuntimeError::new_err(format!("Failed to get buffer bytes: {}", e))
    })?.to_vec();

    // Устанавливаем буфер через обычный Rust метод
    let mut base_model: PyRefMut<BaseModel> = instance.extract()?;
    base_model.set_buffer(buffer_bytes, fields_map, schema.name().to_string());
    drop(base_model);

    Ok(instance.unbind())
}

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
            let datetime_mod = py.import("datetime")?;
            let date_cls = datetime_mod.getattr("date")?;
            let py_date = date_cls.call1((d.year(), d.month(), d.day()))?;
            Ok(py_date.unbind())
        }
        ValueType::DateTime(dt) => {
            let datetime_mod = py.import("datetime")?;
            let datetime_cls = datetime_mod.getattr("datetime")?;
            let py_dt = datetime_cls.call1((
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
            let list = PyList::empty(py);
            for item in items {
                let py_item = value_to_python(py, item)?;
                list.append(py_item.bind(py))?;
            }
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