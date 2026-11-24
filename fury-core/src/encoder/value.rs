use chrono::{DateTime, NaiveDate, NaiveDateTime, Timelike};
use uuid::Uuid;

use super::limits::{DecodeBudget, DecodeLimits, HARD_MAX_DEPTH};
use crate::schema::field::FieldType;

/// Null marker for optional fields.
const FLAG_NONE: u8 = 0x00;

/// Value-present marker for optional fields.
const FLAG_SOME: u8 = 0x01;

/// Maximum recursion depth for nested types (List, Map, Object).
/// Prevents stack overflow from malicious or deeply nested data.
const MAX_DEPTH: usize = HARD_MAX_DEPTH;

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

// Codes 0 and 1 retain their v2 meanings. For an integer, codes 2..=253
// carry a small value directly. The two remaining codes shorten larger words.
fn zigzag(value: i64) -> u64 {
    ((value as u64) << 1) ^ ((value >> 63) as u64)
}

fn integer_tag(magnitude: u64, width: usize) -> (u8, usize) {
    if magnitude <= 251 {
        (magnitude as u8 + 2, 0)
    } else if width > 2 && magnitude <= u16::MAX.into() {
        (254, 2)
    } else if width > 4 && magnitude <= u32::MAX.into() {
        (255, 4)
    } else {
        (FLAG_SOME, width)
    }
}

fn short_integer<'a>(tag: u8, data: &'a [u8], ty: &FieldType) -> Option<(ValueType, &'a [u8])> {
    let width = ty.fixed_size()?;
    let (magnitude, rest) = match tag {
        2..=253 => (u64::from(tag - 2), data),
        254 if width > 2 => {
            let (bytes, rest) = data.split_first_chunk::<2>()?;
            (u64::from(u16::from_le_bytes(*bytes)), rest)
        }
        255 if width > 4 => {
            let (bytes, rest) = data.split_first_chunk::<4>()?;
            (u64::from(u32::from_le_bytes(*bytes)), rest)
        }
        _ => return None,
    };
    if integer_tag(magnitude, width).0 != tag {
        return None;
    }
    let signed = (magnitude >> 1) as i64 ^ -((magnitude & 1) as i64);
    let value = match ty {
        FieldType::Int8 => ValueType::Int8(signed as i8),
        FieldType::Int16 => ValueType::Int16(signed as i16),
        FieldType::Int32 => ValueType::Int32(signed as i32),
        FieldType::Int64 => ValueType::Int64(signed),
        FieldType::UInt8 => ValueType::UInt8(magnitude as u8),
        FieldType::UInt16 => ValueType::UInt16(magnitude as u16),
        FieldType::UInt32 => ValueType::UInt32(magnitude as u32),
        FieldType::UInt64 => ValueType::UInt64(magnitude),
        _ => return None,
    };
    Some((value, rest))
}

fn write_integer(buf: &mut Vec<u8>, magnitude: u64, raw: &[u8]) {
    let (tag, len) = integer_tag(magnitude, raw.len());
    buf.push(tag);
    if tag == FLAG_SOME {
        buf.extend_from_slice(raw);
    } else {
        buf.extend_from_slice(&magnitude.to_le_bytes()[..len]);
    }
}

fn write_blob(buf: &mut Vec<u8>, bytes: &[u8]) {
    if bytes.len() <= 253 {
        buf.push(bytes.len() as u8 + 2);
    } else {
        buf.push(FLAG_SOME);
        buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    }
    buf.extend_from_slice(bytes);
}

fn blob_type(ty: &FieldType) -> bool {
    matches!(
        ty,
        FieldType::String | FieldType::Bytes | FieldType::Object(_)
    )
}

/// Borrow a present blob and its unconsumed tail.
pub(super) fn read_blob(tag: u8, data: &[u8], short_values: bool) -> Option<(&[u8], &[u8])> {
    match tag {
        FLAG_SOME => {
            let (raw, tail) = data.split_first_chunk::<4>()?;
            tail.split_at_checked(u32::from_le_bytes(*raw) as usize)
        }
        2..=255 if short_values => data.split_at_checked(usize::from(tag - 2)),
        _ => None,
    }
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
/// - `0x01` (FLAG_SOME) — full-width present value
/// - `0x02..=0xff` — short integer or blob as determined by the schema
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
    /// Checks encoded lengths and the materialization budget before accepting a
    /// typed value in a record. This keeps setters and typed reads symmetric.
    pub(crate) fn encoded_size(&self, limits: &DecodeLimits) -> Option<(usize, usize)> {
        let mut budget = DecodeBudget::new(limits);
        let size = self.checked_encoded_size(0, &mut budget)?;
        Some((size, limits.max_total_values - budget.remaining))
    }

    /// Find the size of a typed value while enforcing its type, depth and item budget.
    fn checked_encoded_size(&self, depth: usize, budget: &mut DecodeBudget<'_>) -> Option<usize> {
        budget.consume(depth)?;
        match self {
            Self::String(value) => checked_variable_size(value.len()),
            Self::Bytes(value) | Self::Object(value) => checked_variable_size(value.len()),
            Self::List(items) => {
                if items.len() > budget.limits.max_collection_items
                    || !budget.allows_children(items.len())
                    || items.len() > u32::MAX as usize
                {
                    return None;
                }
                items.iter().try_fold(5usize, |size, item| {
                    size.checked_add(item.checked_encoded_size(depth + 1, budget)?)
                })
            }
            Self::Map(items) => {
                if items.len() > budget.limits.max_collection_items
                    || !budget.allows_children(items.len().checked_mul(2)?)
                    || items.len() > u32::MAX as usize
                {
                    return None;
                }
                items.iter().try_fold(5usize, |size, (key, value)| {
                    size.checked_add(key.checked_encoded_size(depth + 1, budget)?)?
                        .checked_add(value.checked_encoded_size(depth + 1, budget)?)
                })
            }
            _ => Some(self.size()),
        }
    }

    /// Calculates the exact byte size of this value when serialized.
    ///
    /// This is used for buffer pre-allocation to avoid reallocations.
    #[must_use]
    pub fn size(&self) -> usize {
        match self {
            // do not forget about 1 byte for the flag
            Self::None => 1,
            Self::Int8(v) => 1 + integer_tag(zigzag(*v as i64), 1).1,
            Self::UInt8(v) => 1 + integer_tag(*v as u64, 1).1,
            Self::Int16(v) => 1 + integer_tag(zigzag(*v as i64), 2).1,
            Self::UInt16(v) => 1 + integer_tag(*v as u64, 2).1,
            Self::Int32(v) => 1 + integer_tag(zigzag(*v as i64), 4).1,
            Self::UInt32(v) => 1 + integer_tag(*v as u64, 4).1,
            Self::Int64(v) => 1 + integer_tag(zigzag(*v), 8).1,
            Self::UInt64(v) => 1 + integer_tag(*v, 8).1,
            Self::Bool(_) => 2,
            Self::Float32(_) | Self::Date(_) => 5,
            Self::Float64(_) | Self::DateTime(_) => 9,
            Self::Uuid(_) => 17,
            Self::String(s) => blob_size(s.len()),
            Self::Bytes(b) => blob_size(b.len()),
            Self::List(items) => 5 + items.iter().map(|item| item.size()).sum::<usize>(),
            Self::Map(entries) => {
                5 + entries
                    .iter()
                    .map(|(k, v)| k.size() + v.size())
                    .sum::<usize>()
            }
            Self::Object(buffer) => blob_size(buffer.len()),
        }
    }
    /// Serializes this value into a new byte vector.
    ///
    /// This is a convenience method that allocates a new vector.
    /// For better performance, use `write_to()` with a pre-allocated buffer.
    /// This low-level helper does not enforce a schema or resource limits.
    /// DateTime uses microsecond precision; finer fractions and leap-second
    /// representations are normalized here. BinaryRecord rejects such inputs.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.size());
        self.write_to(&mut buf);
        buf
    }

    /// Serializes this value into the provided buffer.
    ///
    /// Appends bytes, growing the destination allocation when necessary.
    pub(crate) fn write_to(&self, buf: &mut Vec<u8>) {
        match self {
            Self::None => buf.push(FLAG_NONE),
            Self::Int8(v) => write_integer(buf, zigzag(i64::from(*v)), &v.to_le_bytes()),
            Self::UInt8(v) => write_integer(buf, *v as u64, &v.to_le_bytes()),
            Self::Int16(v) => write_integer(buf, zigzag(i64::from(*v)), &v.to_le_bytes()),
            Self::UInt16(v) => write_integer(buf, *v as u64, &v.to_le_bytes()),
            Self::Int32(v) => write_integer(buf, zigzag(i64::from(*v)), &v.to_le_bytes()),
            Self::UInt32(v) => write_integer(buf, *v as u64, &v.to_le_bytes()),
            Self::Float32(v) => write_primitive!(buf, v),
            Self::Int64(v) => write_integer(buf, zigzag(*v), &v.to_le_bytes()),
            Self::UInt64(v) => write_integer(buf, *v, &v.to_le_bytes()),
            Self::Float64(v) => write_primitive!(buf, v),
            Self::Bool(v) => {
                buf.push(FLAG_SOME);
                buf.push(u8::from(*v));
            }
            Self::DateTime(dt) => {
                buf.push(FLAG_SOME);
                buf.extend_from_slice(&dt.and_utc().timestamp_micros().to_le_bytes());
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
            Self::String(s) => write_blob(buf, s.as_bytes()),
            Self::Bytes(bytes) | Self::Object(bytes) => write_blob(buf, bytes),
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
    /// Reads the first value and ignores any remaining bytes.
    pub fn from_bytes(bytes: &[u8], field_type: &FieldType) -> Option<Self> {
        Self::from_bytes_with_rest(bytes, field_type, 0).map(|(value, _)| value)
    }

    /// Decodes exactly one value using explicit limits, before allocating
    /// collections. Each call has an independent value budget.
    pub fn from_bytes_with_limits(
        bytes: &[u8],
        field_type: &FieldType,
        limits: &DecodeLimits,
    ) -> Option<Self> {
        if bytes.len() > limits.max_frame_bytes {
            return None;
        }
        let mut budget = DecodeBudget::new(limits);
        let (value, rest) = Self::decode_rest(bytes, field_type, 0, &mut budget)?;
        rest.is_empty().then_some(value)
    }

    /// Validates one complete value without allocating decoded collections.
    /// Object contents remain opaque: v2 supplies only a schema name, not a resolver.
    pub(crate) fn validate_bytes(
        bytes: &[u8],
        ty: &FieldType,
        budget: &mut DecodeBudget<'_>,
    ) -> bool {
        Self::validated_rest(bytes, ty, 0, budget).is_some_and(|rest| rest.is_empty())
    }

    /// Validate the next encoded value and return the remaining slice without building a value.
    fn validated_rest<'a>(
        bytes: &'a [u8],
        ty: &FieldType,
        depth: usize,
        budget: &mut DecodeBudget<'_>,
    ) -> Option<&'a [u8]> {
        budget.consume(depth)?;
        let (&flag, data) = bytes.split_first()?;
        if flag == FLAG_NONE {
            return Some(data);
        }
        if blob_type(ty) {
            let (bytes, rest) = read_blob(flag, data, budget.short_values)?;
            if matches!(ty, FieldType::String) {
                std::str::from_utf8(bytes).ok()?;
            }
            return Some(rest);
        }
        if flag != FLAG_SOME {
            if !budget.short_values {
                return None;
            }
            return short_integer(flag, data, ty).map(|(_, rest)| rest);
        }
        if let Some(size) = ty.fixed_size() {
            let (raw, rest) = data.split_at_checked(size)?;
            match ty {
                FieldType::Bool if raw[0] > 1 => return None,
                FieldType::Date => {
                    let days = i32::from_le_bytes(raw.try_into().ok()?);
                    // Array width comes from fixed_size above.
                    UNIX_EPOCH.checked_add_signed(chrono::Duration::days(i64::from(days)))?;
                }
                FieldType::DateTime => {
                    DateTime::from_timestamp_micros(i64::from_le_bytes(raw.try_into().ok()?))?;
                }
                _ => {}
            }
            return Some(rest);
        }
        let (raw_len, mut rest) = data.split_at_checked(4)?;
        let count = u32::from_le_bytes(raw_len.try_into().ok()?) as usize;
        match ty {
            FieldType::List(inner) => {
                if count > rest.len()
                    || count > budget.limits.max_collection_items
                    || !budget.allows_children(count)
                {
                    return None;
                }
                for _ in 0..count {
                    rest = Self::validated_rest(rest, inner, depth + 1, budget)?;
                }
                Some(rest)
            }
            FieldType::Map(key, value) => {
                let children = count.checked_mul(2)?;
                if count > rest.len() / 2
                    || count > budget.limits.max_collection_items
                    || !budget.allows_children(children)
                {
                    return None;
                }
                for _ in 0..count {
                    rest = Self::validated_rest(rest, key, depth + 1, budget)?;
                    rest = Self::validated_rest(rest, value, depth + 1, budget)?;
                }
                Some(rest)
            }
            _ => None,
        }
    }

    /// Deserializes a value and returns the remaining bytes.
    ///
    /// This parses multiple values from one borrowed input slice. The tail is
    /// borrowed; decoded strings, bytes and collections still allocate/copy.
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
        let limits = DecodeLimits::default();
        let prefix = &bytes[..bytes.len().min(limits.max_frame_bytes)];
        let (value, remaining) =
            Self::decode_rest(prefix, field_type, depth, &mut DecodeBudget::new(&limits))?;
        Some((value, &bytes[prefix.len() - remaining.len()..]))
    }

    /// Decode one budgeted value into owned Rust data and keep the remaining slice.
    fn decode_rest<'a>(
        bytes: &'a [u8],
        field_type: &FieldType,
        depth: usize,
        budget: &mut DecodeBudget<'_>,
    ) -> Option<(Self, &'a [u8])> {
        budget.consume(depth)?;

        let (&flag, data) = bytes.split_first()?;

        if flag == FLAG_NONE {
            return Some((Self::None, data));
        }

        if flag != FLAG_SOME && !blob_type(field_type) {
            return short_integer(flag, data, field_type);
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
                if b > 1 {
                    return None;
                }
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
                let dt = DateTime::from_timestamp_micros(timestamp)?.naive_utc();
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
            FieldType::String | FieldType::Bytes | FieldType::Object(_) => {
                let (bytes, rest) = read_blob(flag, data, budget.short_values)?;
                let value = match field_type {
                    FieldType::String => Self::String(std::str::from_utf8(bytes).ok()?.to_owned()),
                    FieldType::Bytes => Self::Bytes(bytes.to_vec()),
                    _ => Self::Object(bytes.to_vec()),
                };
                Some((value, rest))
            }
            FieldType::List(item_type) => {
                let (len_bytes, mut current_data) = data.split_at_checked(4)?;
                let count = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
                // Every element needs at least a flag byte. Check before allocation.
                if count > current_data.len()
                    || count > budget.limits.max_collection_items
                    || !budget.allows_children(count)
                {
                    return None;
                }
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    let (item, rest) =
                        Self::decode_rest(current_data, item_type, depth + 1, budget)?;
                    items.push(item);
                    current_data = rest;
                }
                Some((Self::List(items), current_data))
            }
            FieldType::Map(key_type, value_type) => {
                let (len_bytes, mut current_data) = data.split_at_checked(4)?;
                let count = u32::from_le_bytes(len_bytes.try_into().ok()?) as usize;
                // Each key/value pair needs at least two flag bytes.
                let children = count.checked_mul(2)?;
                if count > current_data.len() / 2
                    || count > budget.limits.max_collection_items
                    || !budget.allows_children(children)
                {
                    return None;
                }
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let (key, rest_after_key) =
                        Self::decode_rest(current_data, key_type, depth + 1, budget)?;
                    let (value, rest_after_value) =
                        Self::decode_rest(rest_after_key, value_type, depth + 1, budget)?;
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

    /// Checks the variant, nesting limit and lossless temporal representation.
    /// Collection element nullability is not represented by FieldType in v2.
    pub fn matches_type(&self, ty: &FieldType, depth: usize) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        match (self, ty) {
            (Self::None, _) => true,
            (Self::DateTime(value), FieldType::DateTime) => {
                // Chrono can represent nanoseconds and leap seconds, while wire v2
                // stores ordinary epoch microseconds. Never silently change a
                // record field when the original value cannot be reconstructed.
                let nanos = value.nanosecond();
                nanos < 1_000_000_000 && nanos.is_multiple_of(1_000)
            }
            (Self::Int8(_), FieldType::Int8)
            | (Self::Int16(_), FieldType::Int16)
            | (Self::Int32(_), FieldType::Int32)
            | (Self::Int64(_), FieldType::Int64)
            | (Self::UInt8(_), FieldType::UInt8)
            | (Self::UInt16(_), FieldType::UInt16)
            | (Self::UInt32(_), FieldType::UInt32)
            | (Self::UInt64(_), FieldType::UInt64)
            | (Self::Float32(_), FieldType::Float32)
            | (Self::Float64(_), FieldType::Float64)
            | (Self::Bool(_), FieldType::Bool)
            | (Self::String(_), FieldType::String)
            | (Self::Bytes(_), FieldType::Bytes)
            | (Self::Date(_), FieldType::Date)
            | (Self::Uuid(_), FieldType::Uuid)
            | (Self::Object(_), FieldType::Object(_)) => true,
            (Self::List(items), FieldType::List(inner)) => {
                items.iter().all(|v| v.matches_type(inner, depth + 1))
            }
            (Self::Map(items), FieldType::Map(key, value)) => items
                .iter()
                .all(|(k, v)| k.matches_type(key, depth + 1) && v.matches_type(value, depth + 1)),
            _ => false,
        }
    }
}

/// Count the short or extended header for a representable blob.
fn checked_variable_size(length: usize) -> Option<usize> {
    if length > u32::MAX as usize {
        return None;
    }
    length.checked_add(if length <= 253 { 1 } else { 5 })
}

fn blob_size(length: usize) -> usize {
    length + if length <= 253 { 1 } else { 5 }
}
