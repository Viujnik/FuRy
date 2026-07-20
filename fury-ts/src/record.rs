use crate::get_global_registry;
use fury_core::encoder::buffer::BinaryRecord;
use fury_core::encoder::value::ValueType;
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
                "Compilation failed: schema '{}' absent from registry",
                schema_name
            ))
        })?;

        let record = BinaryRecord::from_raw_bytes(buffer.unwrap_or_default(), &schema)
            .map_err(|e| JsValue::from_str(&format!("Binary stream parsing violation: {}", e)))?;

        Ok(WasmRecord { inner: record })
    }

    /// Retrieves a numeric field value as an `f64` (JavaScript's native number type).
    #[wasm_bindgen(js_name = getNumber)]
    pub fn get_number(&self, field_name: &str) -> Result<f64, JsValue> {
        let (offset, length) = self.get_bounds(field_name)?;
        let bytes = &self.get_buffer_slice()?[offset..(offset + length)];

        match length {
            4 => {
                let array: [u8; 4] = bytes.try_into().map_err(|_| {
                    JsValue::from_str("Invalid slice boundaries size for Int32 mapping")
                })?;
                Ok(i32::from_le_bytes(array) as f64)
            }
            8 => {
                let array: [u8; 8] = bytes.try_into().map_err(|_| {
                    JsValue::from_str("Invalid slice boundaries size for 64-bit mapping")
                })?;

                let float_val = f64::from_le_bytes(array);
                if float_val.is_nan() || float_val.is_infinite() {
                    Ok(i64::from_le_bytes(array) as f64)
                } else {
                    Ok(float_val)
                }
            }
            _ => Err(JsValue::from_str(
                "Type mismatch violation: target boundary size is not a scalar number",
            )),
        }
    }

    /// Updates a numeric field, automatically repacking the underlying binary layout.
    #[wasm_bindgen(js_name = setNumber)]
    pub fn set_number(&mut self, field_name: &str, value: f64) -> Result<(), JsValue> {
        self.set_field(field_name, ValueType::Float64(value))
    }

    /// Extracts a UTF-8 string from the binary payload.
    /// Note: Allocation is required here as JS strings are UTF-16.
    #[wasm_bindgen(js_name = getString)]
    pub fn get_string(&self, field_name: &str) -> Result<String, JsValue> {
        let (offset, length) = self.get_bounds(field_name)?;
        let bytes = &self.get_buffer_slice()?[offset..(offset + length)];
        String::from_utf8(bytes.to_vec())
            .map_err(|e| JsValue::from_str(&format!("Malformed UTF-8 context parsed: {}", e)))
    }

    /// Re-allocates and writes a new string value, shifting subsequent field offsets.
    #[wasm_bindgen(js_name = setString)]
    pub fn set_string(&mut self, field_name: &str, value: &str) -> Result<(), JsValue> {
        self.set_field(field_name, ValueType::String(value.to_owned()))
    }

    /// Reads a single byte to evaluate a native boolean value.
    #[wasm_bindgen(js_name = getBool)]
    pub fn get_bool(&self, field_name: &str) -> Result<bool, JsValue> {
        let (offset, _) = self.get_bounds(field_name)?;
        let bytes = self.get_buffer_slice()?;
        Ok(bytes[offset] != 0)
    }

    /// Encodes a boolean flag directly into the binary stream.
    #[wasm_bindgen(js_name = setBool)]
    pub fn set_bool(&mut self, field_name: &str, value: bool) -> Result<(), JsValue> {
        self.set_field(field_name, ValueType::Bool(value))
    }

    /// Instantiates a native JavaScript `Date` object from a serialized ISO string.
    #[wasm_bindgen(js_name = getDate)]
    pub fn get_date(&self, field_name: &str) -> Result<js_sys::Date, JsValue> {
        let date_str = self.get_string(field_name)?;
        Ok(js_sys::Date::new(&JsValue::from_str(&date_str)))
    }

    /// Converts a JavaScript `Date` instance into an ISO string for binary storage.
    #[wasm_bindgen(js_name = setDate)]
    pub fn set_date(&mut self, field_name: &str, value: &js_sys::Date) -> Result<(), JsValue> {
        let iso = value.to_iso_string().as_string().ok_or_else(|| {
            JsValue::from_str("Invalid JavaScript Date object wrapper declaration")
        })?;

        let clean_iso: String = iso.chars().take(10).collect();
        self.set_field(field_name, ValueType::String(clean_iso))
    }

    /// Deserializes a list of nested objects, instantiating them via the provided JS constructor.
    #[wasm_bindgen(js_name = getObjectList)]
    pub fn get_object_list(
        &self,
        field_name: &str,
        constructor: js_sys::Function,
    ) -> Result<js_sys::Array, JsValue> {
        let (offset, length) = self.get_bounds(field_name)?;
        let list_bytes = &self.get_buffer_slice()?[offset..(offset + length)];

        let array_container = js_sys::Array::new();

        let mut cursor = 0;
        while cursor < list_bytes.len() {
            if cursor + 4 > list_bytes.len() {
                return Err(JsValue::from_str(
                    "Malformed nested list packet: unexpected EOF parsing element length header",
                ));
            }

            let item_len = u32::from_le_bytes([
                list_bytes[cursor],
                list_bytes[cursor + 1],
                list_bytes[cursor + 2],
                list_bytes[cursor + 3],
            ]) as usize;

            cursor += 4;

            if cursor + item_len > list_bytes.len() {
                return Err(JsValue::from_str(
                    "Out of bounds read detected during nested model record parsing iteration",
                ));
            }

            let child_model_bytes = &list_bytes[cursor..(cursor + item_len)];

            // Создаем zero-copy проекцию памяти для кучи JS
            let js_bytes_view = unsafe { js_sys::Uint8Array::view(child_model_bytes) };

            let constructor_args = js_sys::Array::new();
            constructor_args.push(&js_bytes_view.into());

            let instance = js_sys::Reflect::construct(&constructor, &constructor_args)?;
            array_container.push(&instance);

            cursor += item_len;
        }

        Ok(array_container)
    }

    /// Serializes an array of JavaScript model instances back into a raw byte layout.
    #[wasm_bindgen(js_name = setObjectList)]
    pub fn set_object_list(&mut self, field_name: &str, value: js_sys::Array) -> Result<(), JsValue> {
        let mut compiled_items = Vec::with_capacity(value.length() as usize);

        for i in 0..value.length() {
            let item_instance = value.get(i);

            let to_bytes_property = js_sys::Reflect::get(&item_instance, &JsValue::from_str("toBytes"))
                .map_err(|_| JsValue::from_str("Serialization error: list items must have a 'toBytes' method available"))?;

            let to_bytes_function = to_bytes_property.dyn_ref::<js_sys::Function>()
                .ok_or_else(|| JsValue::from_str("Type error: 'toBytes' attribute property binding is not a callable function"))?;

            let empty_args = js_sys::Array::new();
            let bytes_js = js_sys::Reflect::apply(to_bytes_function, &item_instance, &empty_args)?;
            let raw_child_bytes = js_sys::Uint8Array::from(bytes_js).to_vec();

            let child_len = raw_child_bytes.len() as u32;
            let mut packed_child_payload = child_len.to_le_bytes().to_vec();
            packed_child_payload.extend(raw_child_bytes);

            compiled_items.push(ValueType::Bytes(packed_child_payload));
        }

        self.set_field(field_name, ValueType::List(compiled_items))
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
    fn get_bounds(&self, field_name: &str) -> Result<(usize, usize), JsValue> {
        self.inner.get_field_offset(field_name).ok_or_else(|| {
            JsValue::from_str(&format!(
                "Requested field '{field_name}' absent from layout offsets"
            ))
        })
    }

    #[inline]
    fn get_buffer_slice(&self) -> Result<&[u8], JsValue> {
        self.inner
            .as_bytes()
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[inline]
    fn set_field(&mut self, field_name: &str, value: ValueType) -> Result<(), JsValue> {
        self.inner
            .set_field(field_name, value)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
