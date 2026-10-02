use crate::get_global_registry;
use fury_core::encoder::buffer::BinaryRecord;
use fury_core::encoder::value::ValueType;
use fury_core::schema::field::FieldType;
use fury_core::schema::registry::FieldSchema;
use wasm_bindgen::prelude::*;

/// Represents a binary record mapped to a specific schema in WebAssembly.
///
/// Provides zero-copy or minimal-allocation access to binary data,
/// acting as a thin, high-performance wrapper around `fury_core::BinaryRecord`.
#[wasm_bindgen]
pub struct WasmRecord {
    inner: BinaryRecord,
}

#[wasm_bindgen]
impl WasmRecord {
    /// Creates a new `WasmRecord` from an optional binary buffer.
    ///
    /// # Arguments
    /// * `schema_name` - The name of the schema to validate against.
    /// * `buffer` - Optional raw byte array. If `None`, an empty record is created.
    ///
    /// # Errors
    /// Returns a `JsValue` error if the schema is not found in the global registry
    /// or if the binary data fails to parse.
    #[wasm_bindgen(constructor)]
    pub fn new(schema_name: &str, buffer: Option<Vec<u8>>) -> Result<WasmRecord, JsValue> {
        let schema = get_global_registry().get(schema_name).ok_or_else(|| {
            JsValue::from_str(&format!(
                "Schema '{schema_name}' absent from registry — call loadRegistry() first"
            ))
        })?;

        let record = match buffer {
            Some(data) => BinaryRecord::from_raw_bytes(data, &schema)
                .map_err(|e| JsValue::from_str(&format!("Binary stream parsing violation: {e}")))?,
            None => BinaryRecord::new(schema),
        };

        Ok(WasmRecord { inner: record })
    }

    #[inline]
    fn field_schema(&self, field_name: &str) -> Result<&FieldSchema, JsValue> {
        self.inner
            .schema()
            .find_field(field_name)
            .ok_or_else(|| JsValue::from_str(&format!("Field '{field_name}' not in schema")))
    }

    #[inline]
    fn decode(&self, field_name: &str) -> Result<ValueType, JsValue> {
        let field = self.field_schema(field_name)?;
        let bytes = self
            .inner
            .get_field_bytes(field_name)
            .ok_or_else(|| JsValue::from_str(&format!("Field '{field_name}' was never set")))?;

        ValueType::from_bytes(bytes, field.field_type())
            .ok_or_else(|| JsValue::from_str(&format!("Corrupt bytes in field '{field_name}'")))
    }

    /// Retrieves a numeric field value as an `f64` (JavaScript's native number type).
    #[wasm_bindgen(js_name = getNumber)]
    pub fn get_number(&self, field_name: &str) -> Result<JsValue, JsValue> {
        match self.decode(field_name)? {
            ValueType::None => Ok(JsValue::NULL),
            ValueType::Int8(v) => Ok((v as f64).into()),
            ValueType::Int16(v) => Ok((v as f64).into()),
            ValueType::Int32(v) => Ok((v as f64).into()),
            ValueType::Int64(v) => Ok((v as f64).into()),
            ValueType::UInt8(v) => Ok((v as f64).into()),
            ValueType::UInt16(v) => Ok((v as f64).into()),
            ValueType::UInt32(v) => Ok((v as f64).into()),
            ValueType::UInt64(v) => Ok((v as f64).into()),
            ValueType::Float32(v) => Ok((v as f64).into()),
            ValueType::Float64(v) => Ok(v.into()),
            other => Err(JsValue::from_str(&format!(
                "Field '{field_name}' holds {other:?}, not a number"
            ))),
        }
    }

    /// Updates a numeric field, automatically repacking the underlying binary layout.
    #[wasm_bindgen(js_name = setNumber)]
    pub fn set_number(&mut self, field_name: &str, value: f64) -> Result<(), JsValue> {
        let field = self.field_schema(field_name)?;
        let typed = match field.field_type() {
            FieldType::Int8 => ValueType::Int8(value as i8),
            FieldType::Int16 => ValueType::Int16(value as i16),
            FieldType::Int32 => ValueType::Int32(value as i32),
            FieldType::Int64 => ValueType::Int64(value as i64),
            FieldType::UInt8 => ValueType::UInt8(value as u8),
            FieldType::UInt16 => ValueType::UInt16(value as u16),
            FieldType::UInt32 => ValueType::UInt32(value as u32),
            FieldType::UInt64 => ValueType::UInt64(value as u64),
            FieldType::Float32 => ValueType::Float32(value as f32),
            FieldType::Float64 => ValueType::Float64(value),
            other => {
                return Err(JsValue::from_str(&format!(
                    "Cannot write number into '{field_name}' of type {other:?}"
                )));
            }
        };
        self.set_field(field_name, typed)
    }

    /// Extracts a UTF-8 string from the binary payload.
    /// Note: Allocation is required here as JS strings are UTF-16.
    #[wasm_bindgen(js_name = getString)]
    pub fn get_string(&self, field_name: &str) -> Result<JsValue, JsValue> {
        match self.decode(field_name)? {
            ValueType::None => Ok(JsValue::NULL),
            ValueType::String(s) => Ok(s.into()),
            other => Err(JsValue::from_str(&format!(
                "Field '{field_name}' holds {other:?}, not a string"
            ))),
        }
    }

    /// Re-allocates and writes a new string value, shifting subsequent field offsets.
    #[wasm_bindgen(js_name = setString)]
    pub fn set_string(&mut self, field_name: &str, value: &str) -> Result<(), JsValue> {
        self.set_field(field_name, ValueType::String(value.to_owned()))
    }

    /// Reads a single byte to evaluate a native boolean value.
    #[wasm_bindgen(js_name = getBool)]
    pub fn get_bool(&self, field_name: &str) -> Result<JsValue, JsValue> {
        match self.decode(field_name)? {
            ValueType::None => Ok(JsValue::NULL),
            ValueType::Bool(b) => Ok(b.into()),
            other => Err(JsValue::from_str(&format!(
                "Field '{field_name}' holds {other:?}, not a bool"
            ))),
        }
    }

    /// Encodes a boolean flag directly into the binary stream.
    #[wasm_bindgen(js_name = setBool)]
    pub fn set_bool(&mut self, field_name: &str, value: bool) -> Result<(), JsValue> {
        self.set_field(field_name, ValueType::Bool(value))
    }

    /// Instantiates a native JavaScript `Date` object from a serialized ISO string.
    #[wasm_bindgen(js_name = getDate)]
    pub fn get_date(&self, field_name: &str) -> Result<JsValue, JsValue> {
        match self.decode(field_name)? {
            ValueType::None => Ok(JsValue::NULL),
            ValueType::DateTime(dt) => Ok(js_sys::Date::new(&JsValue::from_f64(
                dt.and_utc().timestamp_millis() as f64,
            ))
            .into()),
            ValueType::Date(d) => Ok(js_sys::Date::new(&JsValue::from_f64(
                d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64,
            ))
            .into()),
            other => Err(JsValue::from_str(&format!(
                "'{field_name}' holds {other:?}, not a date"
            ))),
        }
    }

    /// Converts a JavaScript `Date` instance into an ISO string for binary storage.
    #[wasm_bindgen(js_name = setDate)]
    pub fn set_date(&mut self, field_name: &str, value: &js_sys::Date) -> Result<(), JsValue> {
        let ms = value.get_time();
        if ms.is_nan() {
            return Err(JsValue::from_str("Invalid Date"));
        }
        let field = self.field_schema(field_name)?;
        let typed = match field.field_type() {
            FieldType::DateTime => {
                let dt = chrono::DateTime::from_timestamp_millis(ms as i64)
                    .ok_or_else(|| JsValue::from_str("Date out of range"))?
                    .naive_utc();
                ValueType::DateTime(dt)
            }
            FieldType::Date => {
                let days = (ms as i64).div_euclid(86_400_000) as i32;
                let date = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                    .and_then(|e| e.checked_add_signed(chrono::Duration::days(days as i64)))
                    .ok_or_else(|| JsValue::from_str("Date out of range"))?;
                ValueType::Date(date)
            }
            other => {
                return Err(JsValue::from_str(&format!(
                    "Cannot write date into {other:?}"
                )));
            }
        };
        self.set_field(field_name, typed)
    }

    #[wasm_bindgen(js_name = getBytes)]
    pub fn get_bytes(&self, field_name: &str) -> Result<JsValue, JsValue> {
        match self.decode(field_name)? {
            ValueType::None => Ok(JsValue::NULL),
            ValueType::Bytes(b) => Ok(js_sys::Uint8Array::from(b.as_slice()).into()),
            other => Err(JsValue::from_str(&format!(
                "Field '{field_name}' holds {other:?}, not bytes"
            ))),
        }
    }

    #[wasm_bindgen(js_name = setBytes)]
    pub fn set_bytes(&mut self, field_name: &str, value: &[u8]) -> Result<(), JsValue> {
        self.set_field(field_name, ValueType::Bytes(value.to_vec()))
    }

    /// Deserializes a list of nested objects, instantiating them via the provided JS constructor.
    #[wasm_bindgen(js_name = getObjectList)]
    pub fn get_object_list(
        &self,
        field_name: &str,
        constructor: js_sys::Function,
    ) -> Result<js_sys::Array, JsValue> {
        let items = match self.decode(field_name)? {
            ValueType::List(i) => i,
            ValueType::None => return Ok(js_sys::Array::new()),
            other => {
                return Err(JsValue::from_str(&format!(
                    "'{field_name}' is {other:?}, not a list"
                )));
            }
        };
        let out = js_sys::Array::new();
        for item in items {
            let child: Vec<u8> = match item {
                ValueType::Object(b) | ValueType::Bytes(b) => b,
                other => return Err(JsValue::from_str(&format!("list item is {other:?}"))),
            };
            let copy = js_sys::Uint8Array::from(child.as_slice());
            let args = js_sys::Array::new();
            args.push(&copy.into());
            out.push(&js_sys::Reflect::construct(&constructor, &args)?);
        }
        Ok(out)
    }

    /// Serializes an array of JavaScript model instances back into a raw byte layout.
    #[wasm_bindgen(js_name = setObjectList)]
    pub fn set_object_list(
        &mut self,
        field_name: &str,
        value: js_sys::Array,
    ) -> Result<(), JsValue> {
        let mut items = Vec::with_capacity(value.length() as usize);
        for i in 0..value.length() {
            let inst = value.get(i);
            let f = js_sys::Reflect::get(&inst, &"toBytes".into())?
                .dyn_into::<js_sys::Function>()
                .map_err(|_| JsValue::from_str("list items must expose toBytes()"))?;
            let bytes =
                js_sys::Uint8Array::from(js_sys::Reflect::apply(&f, &inst, &js_sys::Array::new())?)
                    .to_vec();
            items.push(ValueType::Object(bytes));
        }
        self.set_field(field_name, ValueType::List(items))
    }

    /// Finalizes the record; must be called before toBytes() after any setters.
    #[wasm_bindgen(js_name = finish)]
    pub fn finish(&mut self) -> Result<(), JsValue> {
        self.inner
            .finish()
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(())
    }

    /// Exports the mutated binary record as a native JavaScript `Uint8Array`.
    #[wasm_bindgen(js_name = toBytes)]
    pub fn to_bytes(&self) -> Result<js_sys::Uint8Array, JsValue> {
        self.inner
            .as_bytes()
            .map_err(|e| JsValue::from_str(&e.to_string()))
            .map(|b| js_sys::Uint8Array::from(b))
    }

    #[inline]
    fn set_field(&mut self, field_name: &str, value: ValueType) -> Result<(), JsValue> {
        if self.inner.is_finished() {
            self.inner.reopen();
        }
        self.inner
            .set_field(field_name, value)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
