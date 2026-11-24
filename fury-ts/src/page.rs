use crate::get_global_registry;
use crate::record::MAX_SAFE_INTEGER;
use fury_core::encoder::DecodeLimits;
use fury_core::encoder::frame::FrameReader;
use fury_core::encoder::value::ValueType;
use fury_core::schema::registry::ModelSchema;
use js_sys::{Array, BigInt, Date, JsString, Object, PropertyDescriptor, Reflect, Uint8Array};
use std::sync::Arc;
use wasm_bindgen::prelude::*;

/// Materializes a registered SQL projection in one call. Each result is an ordinary JS object.
/// Exact integers outside the safe Number range become bigint.
#[wasm_bindgen(js_name = decodePage)]
pub fn decode_page(schema_name: &str, bytes: &Uint8Array, max_rows: f64) -> Result<Array, JsValue> {
    let schema = get_global_registry()
        .get(schema_name)
        .ok_or_else(|| JsValue::from_str("unknown page schema"))?;
    decode_with_schema(&schema, bytes, max_rows, false)
}

pub(crate) fn decode_with_schema(
    schema: &Arc<ModelSchema>,
    bytes: &Uint8Array,
    max_rows: f64,
    exact: bool,
) -> Result<Array, JsValue> {
    let mut limits = DecodeLimits::default();
    if max_rows.fract() != 0.0 || !(0.0..=limits.max_collection_items as f64).contains(&max_rows) {
        return Err(JsValue::from_str("invalid page row limit"));
    }
    if bytes.length() as usize > limits.max_frame_bytes {
        return Err(JsValue::from_str("page exceeds byte limit"));
    }
    if schema
        .fields()
        .iter()
        .any(|f| f.field_type().is_collection())
    {
        return Err(JsValue::from_str(
            "page decoder requires a scalar projection",
        ));
    }
    limits.max_collection_items =
        (max_rows as usize).min(limits.max_total_values / schema.fields_count().max(1));
    let prototype = Object::get_prototype_of(&Object::new());
    let keys = schema
        .fields()
        .iter()
        .map(|field| {
            let key: JsString = field.name().into();
            let inherited = Reflect::has_str(&prototype, &key)?;
            Ok((key, inherited))
        })
        .collect::<Result<Vec<_>, JsValue>>()?;
    let descriptor = PropertyDescriptor::new();
    descriptor.set_enumerable(true);
    descriptor.set_writable(true);
    descriptor.set_configurable(true);
    let rows = Array::new();
    let input = bytes.to_vec();
    let mut rest = input.as_slice();
    let mut remaining_fields = limits.max_total_values;
    while !rest.is_empty() {
        if rows.length() as usize >= limits.max_collection_items {
            return Err(JsValue::from_str("page exceeds row limit"));
        }
        let size = rest
            .first_chunk::<4>()
            .ok_or_else(|| JsValue::from_str("truncated frame length"))?;
        let size = u32::from_le_bytes(*size) as usize;
        let (frame, tail) = rest
            .split_at_checked(size)
            .ok_or_else(|| JsValue::from_str("invalid frame length"))?;
        let error = |err: fury_core::error::FuryError| JsValue::from_str(&err.to_string());
        let mut reader = FrameReader::new(frame, &limits).map_err(error)?;
        let row = Object::new();
        while let Some(field) = reader.next_field(schema).map_err(error)? {
            remaining_fields = remaining_fields
                .checked_sub(1)
                .ok_or_else(|| JsValue::from_str("page exceeds field limit"))?;
            if let Some(index) = field.index {
                let ty = schema.fields()[index].field_type();
                let value = if let Some(text) = field.text {
                    JsValue::from_str(text)
                } else {
                    let decoded = ValueType::from_bytes(field.payload, ty)
                        .ok_or_else(|| JsValue::from_str("invalid field"))?;
                    scalar_value(decoded, exact)?
                };
                let (key, inherited) = &keys[index];
                set_field(&row, key, *inherited, &value, &descriptor)?;
            }
        }
        rows.push(&row);
        rest = tail;
    }
    Ok(rows)
}

fn set_field(
    row: &Object,
    key: &JsString,
    inherited: bool,
    value: &JsValue,
    descriptor: &PropertyDescriptor,
) -> Result<(), JsValue> {
    // Write an own property for an inherited name, bypassing inherited setters.
    if inherited {
        descriptor.set_value(value);
        Object::define_property_str(row, key, descriptor)?;
    } else if !Reflect::set_str(row, key, value)? {
        return Err(JsValue::from_str("could not set page field"));
    }
    Ok(())
}

fn signed_integer(n: i64, exact: bool) -> JsValue {
    if !exact && n.unsigned_abs() <= MAX_SAFE_INTEGER as u64 {
        (n as f64).into()
    } else {
        BigInt::from(n).into()
    }
}

fn scalar_value(value: ValueType, exact: bool) -> Result<JsValue, JsValue> {
    Ok(match value {
        ValueType::None => JsValue::NULL,
        ValueType::Int8(v) => v.into(),
        ValueType::Int16(v) => v.into(),
        ValueType::Int32(v) => v.into(),
        ValueType::Int64(v) => signed_integer(v, exact),
        ValueType::UInt8(v) => v.into(),
        ValueType::UInt16(v) => v.into(),
        ValueType::UInt32(v) => v.into(),
        ValueType::UInt64(v) if !exact && v <= MAX_SAFE_INTEGER as u64 => (v as f64).into(),
        ValueType::UInt64(v) => BigInt::from(v).into(),
        ValueType::Float32(v) => v.into(),
        ValueType::Float64(v) => v.into(),
        ValueType::Bool(v) => v.into(),
        ValueType::String(s) => s.into(),
        ValueType::Bytes(b) => Uint8Array::from(b.as_slice()).into(),
        ValueType::Uuid(u) => u.to_string().into(),
        ValueType::DateTime(d) => Date::new(&(d.and_utc().timestamp_millis() as f64).into()).into(),
        ValueType::Date(d) => Date::new(
            &(d.and_time(chrono::NaiveTime::MIN)
                .and_utc()
                .timestamp_millis() as f64)
                .into(),
        )
        .into(),
        _ => {
            return Err(JsValue::from_str(
                "page decoder requires a scalar projection",
            ));
        }
    })
}
