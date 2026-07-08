use chrono::{DateTime, NaiveDate, NaiveDateTime};
use uuid::Uuid;

use crate::schema::field::FieldType;

/// Null marker for optional fields.
const FLAG_NONE: u8 = 0x00;

/// Value-present marker for optional fields.
const FLAG_SOME: u8 = 0x01;

/// Maximum recursion depth for nested types (List, Map, Object).
/// Prevents stack overflow from malicious or deeply nested data.
const MAX_DEPTH: usize = 16;

/// Unix epoch (1970-01-01) as a compile-time constant.
const UNIX_EPOCH: NaiveDate = match NaiveDate::from_ymd_opt(1970, 1, 1) {
    Some(date) => date,
    None => unreachable!(),
};

/// Writes a primitive value (flag + fixed-size data) to the buffer.
macro_rules! write_primitive {
    ($buf:expr, $value:expr) => {{
        $buf.push(FLAG_SOME);
        $buf.extend_from_slice(&$value.to_le_bytes());
    }};
}

/// A dynamically-typed value that can be serialized to/from binary format.
///
/// Supports primitive types (integers, floats, bool), strings, bytes,
/// dates, UUIDs, and nested collections (List, Map, Object).
///
/// # Binary Format
///
/// Each value is prefixed with a flag byte:
/// - `0x00` (FLAG_NONE) — null value
/// - `0x01` (FLAG_SOME) — value present, followed by type-specific data
///
/// # Zero-Copy Access
///
/// When stored in `BinaryRecord`, field bytes can be accessed directly
/// via `get_field_bytes()` without deserialization, enabling ultra-fast
/// read operations for cases where only raw bytes are needed.
#[derive(Clone, Debug, PartialEq)]
pub enum ValueType {
    None,
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    Float32(f32),
    Float64(f64),
    Bool(bool),
    String(String),
    Bytes(Vec<u8>),
    DateTime(NaiveDateTime),
    Date(NaiveDate),
    Uuid(Uuid),
    List(Vec<ValueType>),
    Map(Vec<(ValueType, ValueType)>),
    Object(Vec<u8>),
}

impl ValueType {
    /// Calculates the exact byte size of this value when serialized.
    ///
    /// This is used for buffer pre-allocation to avoid reallocations.
    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            // do not forget about 1 byte for the flag
            Self::None => 1,
            Self::Int8(_) | Self::UInt8(_) | Self::Bool(_) => 2,
            Self::Int16(_) | Self::UInt16(_) => 3,
            Self::Int32(_) | Self::UInt32(_) | Self::Float32(_) | Self::Date(_) => 5,
            Self::Int64(_) | Self::UInt64(_) | Self::Float64(_) | Self::DateTime(_) => 9,
            Self::Uuid(_) => 17,
            // and 4 bytes for the length info
            Self::String(s) => 5 + s.as_bytes().len(),
            Self::Bytes(b) => 5 + b.len(),
            Self::List(items) => 5 + items.iter().map(|item| item.size()).sum::<usize>(),
            Self::Map(entries) => {
                5 + entries
                    .iter()
                    .map(|(k, v)| k.size() + v.size())
                    .sum::<usize>()
            }
            Self::Object(buffer) => 5 + buffer.len(),
        }
    }
    /// Serializes this value into a new byte vector.
    ///
    /// This is a convenience method that allocates a new vector.
    /// For better performance, use `write_to()` with a pre-allocated buffer.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.size());
        self.write_to(&mut buf);
        buf
    }

    /// Serializes this value into the provided buffer.
    ///
    /// This method appends bytes to the buffer without allocating a new vector.
    /// Used internally by `BinaryRecord` for zero-copy serialization.
    pub(crate) fn write_to(&self, buf: &mut Vec<u8>) {
        match self {
            Self::None => buf.push(FLAG_NONE),
            Self::Int8(v) => write_primitive!(buf, v),
            Self::UInt8(v) => write_primitive!(buf, v),
            Self::Int16(v) => write_primitive!(buf, v),
            Self::UInt16(v) => write_primitive!(buf, v),
            Self::Int32(v) => write_primitive!(buf, v),
            Self::UInt32(v) => write_primitive!(buf, v),
            Self::Float32(v) => write_primitive!(buf, v),
            Self::Int64(v) => write_primitive!(buf, v),
            Self::UInt64(v) => write_primitive!(buf, v),
            Self::Float64(v) => write_primitive!(buf, v),
            Self::Bool(v) => {
                buf.push(FLAG_SOME);
                buf.push(u8::from(*v));
            }
            Self::DateTime(dt) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(&dt.and_utc().timestamp().to_le_bytes());
            }
            Self::Date(d) => {
                buf.push(FLAG_SOME);
                let days = (*d - UNIX_EPOCH).num_days() as i32;
                buf.extend_from_slice(&days.to_le_bytes());
            }
            Self::Uuid(u) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(u.as_bytes());
            }
            Self::String(s) => {
                buf.push(FLAG_SOME);
                let s_bytes = s.as_bytes();
                buf.extend_from_slice(&(s_bytes.len() as u32).to_le_bytes());
                buf.extend_from_slice(s_bytes);
            }
            Self::Bytes(b) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
                buf.extend_from_slice(b);
            }
            Self::Object(buffer) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(&(buffer.len() as u32).to_le_bytes());
                buf.extend_from_slice(buffer);
            }
            Self::List(items) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
                for item in items {
                    item.write_to(buf);
                }
            }
            Self::Map(entries) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(&(entries.len() as u32).to_le_bytes());
                for (key, value) in entries {
                    key.write_to(buf);
                    value.write_to(buf);
                }
            }
        }
    }

    /// Deserializes a value from raw bytes according to the expected field type.
    ///
    /// Returns `None` if the bytes are corrupted or don't match the expected type.
    ///
    /// This is a convenience wrapper around `from_bytes_with_rest()` that discards
    /// the remaining bytes.
    pub fn from_bytes(bytes: &[u8], field_type: &FieldType) -> Option<Self> {
        Self::from_bytes_with_rest(bytes, field_type, 0).map(|(value, _rest)| value)
    }

    /// Deserializes a value and returns the remaining bytes.
    ///
    /// This is useful for parsing multiple values from a single byte slice
    /// without copying or reallocating.
    ///
    /// # Arguments
    ///
    /// * `bytes` — raw byte slice to parse
    /// * `field_type` — expected type of the value
    /// * `depth` — current recursion depth (prevents stack overflow)
    ///
    /// # Returns
    ///
    /// * `Some((value, remaining_bytes))` if successful
    /// * `None` if parsing fails or max depth exceeded
    pub fn from_bytes_with_rest<'a>(
        bytes: &'a [u8],
        field_type: &FieldType,
        depth: usize,
    ) -> Option<(Self, &'a [u8])> {
        if depth > MAX_DEPTH {
            return None;
        }

        let (&flag, data) = bytes.split_first()?;

        if flag == FLAG_NONE {
            return Some((Self::None, data));
        }

        if flag != FLAG_SOME {
            return None;
        }

        match field_type {
            FieldType::Int8 => {
                let (&b, rest) = data.split_first()?;
                Some((Self::Int8(b as i8), rest))
            }
            FieldType::UInt8 => {
                let (&b, rest) = data.split_first()?;
                Some((Self::UInt8(b), rest))
            }
            FieldType::Bool => {
                let (&b, rest) = data.split_first()?;
                Some((Self::Bool(b != 0), rest))
            }
            FieldType::Int16 => {
                let (raw, rest) = data.split_at_checked(2)?;
                Some((Self::Int16(i16::from_le_bytes(raw.try_into().ok()?)), rest))
            }
            FieldType::UInt16 => {
                let (raw, rest) = data.split_at_checked(2)?;
                Some((Self::UInt16(u16::from_le_bytes(raw.try_into().ok()?)), rest))
            }
            FieldType::Int32 => {
                let (raw, rest) = data.split_at_checked(4)?;
                Some((Self::Int32(i32::from_le_bytes(raw.try_into().ok()?)), rest))
            }
            FieldType::UInt32 => {
                let (raw, rest) = data.split_at_checked(4)?;
                Some((Self::UInt32(u32::from_le_bytes(raw.try_into().ok()?)), rest))
            }
            FieldType::Float32 => {
                let (raw, rest) = data.split_at_checked(4)?;
                Some((
                    Self::Float32(f32::from_le_bytes(raw.try_into().ok()?)),
                    rest,
                ))
            }
            FieldType::Int64 => {
                let (raw, rest) = data.split_at_checked(8)?;
                Some((Self::Int64(i64::from_le_bytes(raw.try_into().ok()?)), rest))
            }
            FieldType::UInt64 => {
                let (raw, rest) = data.split_at_checked(8)?;
                Some((Self::UInt64(u64::from_le_bytes(raw.try_into().ok()?)), rest))
            }
            FieldType::Float64 => {
                let (raw, rest) = data.split_at_checked(8)?;
                Some((
                    Self::Float64(f64::from_le_bytes(raw.try_into().ok()?)),
                    rest,
                ))
            }
            FieldType::DateTime => {
                let (raw, rest) = data.split_at_checked(8)?;
                let timestamp = i64::from_le_bytes(raw.try_into().ok()?);
                let dt = DateTime::from_timestamp(timestamp, 0)?.naive_utc();
                Some((Self::DateTime(dt), rest))
            }
            FieldType::Date => {
                let (raw, rest) = data.split_at_checked(4)?;
                let days = i32::from_le_bytes(raw.try_into().ok()?);
                let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
                let date = epoch.checked_add_signed(chrono::Duration::days(days as i64))?;
                Some((Self::Date(date), rest))
            }
            FieldType::Uuid => {
                let (raw, rest) = data.split_at_checked(16)?;
                let uuid = Uuid::from_slice(raw).ok()?;
                Some((Self::Uuid(uuid), rest))
            }
            FieldType::String => {
                let (len_bytes, rest) = data.split_at_checked(4)?;
                let len = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
                let (str_bytes, final_rest) = rest.split_at_checked(len)?;

                let s = std::str::from_utf8(str_bytes).ok()?.to_string();
                Some((Self::String(s), final_rest))
            }
            FieldType::Bytes => {
                let (len_bytes, rest) = data.split_at_checked(4)?;
                let len = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
                let (b_bytes, final_rest) = rest.split_at_checked(len)?;
                Some((Self::Bytes(b_bytes.to_vec()), final_rest))
            }
            FieldType::Object(_) => {
                let (len_bytes, rest) = data.split_at_checked(4)?;
                let len = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
                let (obj_bytes, final_rest) = rest.split_at_checked(len)?;
                Some((Self::Object(obj_bytes.to_vec()), final_rest))
            }
            FieldType::List(item_type) => {
                let (len_bytes, mut current_data) = data.split_at_checked(4)?;
                let count = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;

                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    let (item, rest) =
                        Self::from_bytes_with_rest(current_data, item_type, depth + 1)?;
                    items.push(item);
                    current_data = rest;
                }
                Some((Self::List(items), current_data))
            }
            FieldType::Map(key_type, value_type) => {
                let (len_bytes, mut current_data) = data.split_at_checked(4)?;
                let count = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;

                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let (key, rest_after_key) =
                        Self::from_bytes_with_rest(current_data, key_type, depth + 1)?;
                    let (value, rest_after_value) =
                        Self::from_bytes_with_rest(rest_after_key, value_type, depth + 1)?;
                    entries.push((key, value));
                    current_data = rest_after_value;
                }
                Some((Self::Map(entries), current_data))
            }
        }
    }

    /// Returns `true` if this value is `None`.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}
