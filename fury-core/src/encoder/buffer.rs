use super::limits::{DecodeBudget, DecodeLimits};
use super::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::ModelSchema;
use std::collections::HashMap;
use std::sync::Arc;

const WIRE_MAGIC: u8 = b'F';
const WIRE_VERSION: u8 = 2;
const HEADER_SIZE: usize = 6;
const ENTRY_HEADER_SIZE: usize = 5;
const DENSE_INDEX_LIMIT: usize = 4096;

// A frame fits in a u32. A value consumes at least one byte, so its nested
// value count also fits. Entry offset zero belongs to the header.
#[derive(Clone, Copy, Default)]
struct Location {
    entry: u32,
    start: u32,
    len: u32,
    values: u32,
}

impl Location {
    fn entry_len(self) -> usize {
        (self.start + self.len - self.entry) as usize
    }

    fn coordinates(self) -> (usize, usize) {
        (self.start as usize, self.len as usize)
    }
}

struct Locations {
    // Common known fields share the schema's name index without copying names.
    dense: Vec<Location>,
    sparse: HashMap<Box<str>, Location>,
}

impl Locations {
    fn new(schema: &ModelSchema, limits: &DecodeLimits) -> Self {
        Self {
            dense: vec![
                Location::default();
                schema
                    .fields_count()
                    .min(limits.max_fields)
                    .min(DENSE_INDEX_LIMIT)
            ],
            sparse: HashMap::new(),
        }
    }

    fn get(&self, index: usize, name: &str) -> Option<Location> {
        let location = self.dense.get(index).or_else(|| self.sparse.get(name))?;
        (location.entry != 0).then_some(*location)
    }

    fn insert(&mut self, index: Option<usize>, name: &str, value: Location) -> Option<Location> {
        if let Some(slot) = index.and_then(|i| self.dense.get_mut(i)) {
            let previous = std::mem::replace(slot, value);
            (previous.entry != 0).then_some(previous)
        } else {
            self.sparse.insert(name.into(), value)
        }
    }

    fn remove(&mut self, index: usize, name: &str) {
        if let Some(slot) = self.dense.get_mut(index) {
            *slot = Location::default();
        } else {
            self.sparse.remove(name);
        }
    }
}

/// A typed record holding a contiguous FuRy wire-v2 frame.
///
/// Raw field bytes borrow the frame. Typed reads materialize a value.
/// ```text
/// [u32 LE frame size]['F'][2] ([u8 name length][name][u32 LE value length][value])*
/// ```
pub struct BinaryRecord {
    schema: Arc<ModelSchema>,
    buffer: Vec<u8>,
    offsets: Locations,
    finished: bool,
    has_dead_entries: bool,
    limits: DecodeLimits,
    // Current fields, excluding entries a later update superseded.
    live_bytes: usize,
    live_values: usize,
    live_fields: usize,
}

impl BinaryRecord {
    /// Starts an empty record with the default wire budgets.
    #[must_use]
    pub fn new(schema: Arc<ModelSchema>) -> Self {
        Self::new_with_limits(schema, DecodeLimits::default())
    }

    /// Starts an empty record with explicit wire budgets.
    #[must_use]
    pub fn new_with_limits(schema: Arc<ModelSchema>, limits: DecodeLimits) -> Self {
        let estimated = schema
            .fields()
            .iter()
            .take(limits.max_fields.min(DENSE_INDEX_LIMIT))
            .map(|field| {
                ENTRY_HEADER_SIZE
                    + field.name().len().min(u8::MAX as usize)
                    + 1
                    + field.field_type().fixed_size().unwrap_or(8)
            })
            .sum::<usize>();
        let mut buffer = Vec::with_capacity(
            (estimated + HEADER_SIZE)
                .min(limits.max_frame_bytes)
                .min(64 * 1024),
        );
        buffer.extend_from_slice(&[0; HEADER_SIZE]);
        let offsets = Locations::new(&schema, &limits);
        Self {
            schema,
            buffer,
            offsets,
            finished: false,
            has_dead_entries: false,
            limits,
            live_bytes: HEADER_SIZE,
            live_values: 0,
            live_fields: 0,
        }
    }

    /// Writes a field while the record is open for editing.
    pub fn set_field(&mut self, name: &str, value: ValueType) -> Result<()> {
        if self.finished {
            return Err(FuryError::BufferAlreadyFinished);
        }
        self.update_field(name, value)
    }

    /// Writes a field, opening a finished record on success.
    /// Validation failure leaves the existing data and finished state intact.
    pub fn update_field(&mut self, name: &str, value: ValueType) -> Result<()> {
        let index = self
            .schema
            .field_index(name)
            .ok_or_else(|| FuryError::FieldNotFound {
                field: name.into(),
                schema: self.schema.name().into(),
            })?;
        let field = &self.schema.fields()[index];
        if (value.is_none() && !field.is_nullable()) || !value.matches_type(field.field_type(), 0) {
            return Err(FuryError::TypeMismatch {
                field: name.into(),
                expected: field.field_type().to_string().into(),
                actual: "wrong value variant, nullability, depth or temporal precision".into(),
            });
        }
        if name.len() > u8::MAX as usize {
            return Err(FuryError::Encoding(
                "field names can contain at most 255 bytes".into(),
            ));
        }
        let (len, values) = value.encoded_size(&self.limits).ok_or_else(|| {
            FuryError::Encoding("field exceeds the configured value budget".into())
        })?;
        let previous = self.offsets.get(index, name).unwrap_or_default();
        let is_new = previous.entry == 0;
        let entry_len = ENTRY_HEADER_SIZE + name.len();
        let max_bytes = self.limits.max_frame_bytes.min(u32::MAX as usize);
        let size_error = || FuryError::Encoding("record exceeds the configured byte budget".into());
        let entry_len = entry_len.checked_add(len).ok_or_else(size_error)?;
        let live_fields = self.live_fields + usize::from(is_new);
        let live_bytes = (self.live_bytes - previous.entry_len())
            .checked_add(entry_len)
            .filter(|size| *size <= max_bytes)
            .ok_or_else(size_error)?;
        let live_values = self.live_values - previous.values as usize;
        if live_fields > self.limits.max_fields
            || values > self.limits.max_total_values - live_values
        {
            return Err(FuryError::Encoding(
                "record exceeds the configured field or value budget".into(),
            ));
        }

        // Byte and value preflights are complete. Compact obsolete entries
        // before appending when keeping them would exceed the frame budget.
        if entry_len > max_bytes - self.buffer.len() {
            self.offsets.remove(index, name);
            self.compact(live_bytes - entry_len);
        } else {
            self.has_dead_entries |= !is_new;
        }
        let entry = self.buffer.len() as u32;
        self.buffer.push(name.len() as u8);
        self.buffer.extend_from_slice(name.as_bytes());
        self.buffer.extend_from_slice(&(len as u32).to_le_bytes());
        let start = self.buffer.len() as u32;
        value.write_to(&mut self.buffer);
        self.offsets.insert(
            Some(index),
            name,
            Location {
                entry,
                start,
                len: len as u32,
                values: values as u32,
            },
        );
        self.live_bytes = live_bytes;
        self.live_values = live_values + values;
        self.live_fields = live_fields;
        self.finished = false;
        Ok(())
    }

    /// Borrows an encoded field payload for a field in the reader's schema.
    #[must_use]
    pub fn get_field_bytes(&self, name: &str) -> Option<&[u8]> {
        let (start, len) = self.get_field_offset(name)?;
        Some(&self.buffer[start..start + len])
    }

    /// Decodes a field according to the reader's schema. Missing fields return
    /// None. Explicit nil in a nullable field returns Some(ValueType::None).
    #[must_use]
    pub fn get_field_value(&self, name: &str) -> Option<ValueType> {
        let index = self.schema.field_index(name)?;
        let (start, len) = self.offsets.get(index, name)?.coordinates();
        ValueType::from_bytes_with_limits(
            &self.buffer[start..start + len],
            self.schema.fields()[index].field_type(),
            &self.limits,
        )
    }

    /// Returns the current offset and length for a field in the reader's schema.
    pub fn get_field_offset(&self, name: &str) -> Option<(usize, usize)> {
        let index = self.schema.field_index(name)?;
        Some(self.offsets.get(index, name)?.coordinates())
    }

    /// Finalizes a record. An edit can compact previous entries without re-encoding any retained field.
    pub fn finish(&mut self) -> Result<&[u8]> {
        if self.finished {
            return Err(FuryError::BufferAlreadyFinished);
        }
        if self.live_bytes > self.limits.max_frame_bytes {
            return Err(FuryError::Encoding(
                "record exceeds the configured byte budget".into(),
            ));
        }
        if self.has_dead_entries {
            self.compact(self.live_bytes);
        }
        let len = self.buffer.len() as u32;
        self.buffer[..4].copy_from_slice(&len.to_le_bytes());
        self.buffer[4..HEADER_SIZE].copy_from_slice(&[WIRE_MAGIC, WIRE_VERSION]);
        self.finished = true;
        Ok(&self.buffer)
    }

    // Each location currently points into the old frame. Disjoint live entry
    // ranges remain valid while copying, and their positions are rewritten in place.
    fn compact(&mut self, capacity: usize) {
        let mut entries: Vec<_> = self
            .offsets
            .dense
            .iter_mut()
            .filter(|slot| slot.entry != 0)
            .chain(self.offsets.sparse.values_mut())
            .collect();
        entries.sort_unstable_by_key(|location| location.entry);
        let mut buffer = Vec::with_capacity(capacity);
        buffer.extend_from_slice(&[0; HEADER_SIZE]);
        for location in entries {
            let prefix = location.start - location.entry;
            let old = location.entry as usize;
            let len = location.entry_len();
            location.entry = buffer.len() as u32;
            location.start = location.entry + prefix;
            buffer.extend_from_slice(&self.buffer[old..old + len]);
        }
        self.buffer = buffer;
        self.has_dead_entries = false;
    }

    /// Returns a finished frame with the original received bytes or current edits.
    pub fn as_bytes(&self) -> Result<&[u8]> {
        if !self.finished {
            return Err(FuryError::BufferNotFinished);
        }
        Ok(&self.buffer)
    }

    /// Parses one complete wire-v2 frame using the default limits.
    pub fn from_raw_bytes(data: Vec<u8>, schema: &Arc<ModelSchema>) -> Result<Self> {
        Self::from_raw_bytes_with_limits(data, schema, DecodeLimits::default())
    }

    /// Parses one frame under explicit limits and validates known values
    /// without materializing them. Unknown values are opaque.
    pub fn from_raw_bytes_with_limits(
        data: Vec<u8>,
        schema: &Arc<ModelSchema>,
        limits: DecodeLimits,
    ) -> Result<Self> {
        if data.len() > limits.max_frame_bytes {
            return Err(FuryError::InvalidSchema("frame exceeds byte limit".into()));
        }
        let Some(([a, b, c, d, magic, version], mut rest)) =
            data.split_first_chunk::<HEADER_SIZE>()
        else {
            return Err(FuryError::InvalidSchema("truncated frame header".into()));
        };
        if data.len() != u32::from_le_bytes([*a, *b, *c, *d]) as usize {
            return Err(FuryError::InvalidSchema(
                "frame length differs from the header".into(),
            ));
        }
        if *magic != WIRE_MAGIC || *version != WIRE_VERSION {
            return Err(FuryError::InvalidSchema(
                "expected a FuRy wire-v2 frame".into(),
            ));
        }

        let mut offsets = Locations::new(schema, &limits);
        let mut budget = DecodeBudget::new(&limits);
        let mut physical_fields = 0;
        let mut live_fields = 0;
        let mut live_values = 0;
        let mut live_bytes = data.len();
        let mut has_dead_entries = false;
        while let Some((&name_len, tail)) = rest.split_first() {
            if physical_fields >= limits.max_fields {
                return Err(FuryError::InvalidSchema("frame exceeds field limit".into()));
            }
            physical_fields += 1;
            let entry = data.len() - rest.len();
            let (name, tail) = tail
                .split_at_checked(usize::from(name_len))
                .ok_or_else(|| FuryError::InvalidSchema("truncated field name".into()))?;
            let name = std::str::from_utf8(name)
                .map_err(|_| FuryError::InvalidSchema("invalid UTF-8 field name".into()))?;
            let (raw_len, tail) = tail
                .split_first_chunk::<4>()
                .ok_or_else(|| FuryError::InvalidSchema("truncated field length".into()))?;
            let len = u32::from_le_bytes(*raw_len);
            let (raw, tail) = tail
                .split_at_checked(len as usize)
                .ok_or_else(|| FuryError::InvalidSchema("truncated field value".into()))?;

            let index = schema.field_index(name);
            let before = budget.remaining;
            if let Some(i) = index {
                let field = &schema.fields()[i];
                if (!field.is_nullable() && raw.first() == Some(&0))
                    || !ValueType::validate_bytes(raw, field.field_type(), &mut budget)
                {
                    return Err(FuryError::InvalidSchema(
                        format!("invalid value for field '{name}'").into(),
                    ));
                }
            }
            let values = before - budget.remaining;
            let location = Location {
                entry: entry as u32,
                start: (entry + ENTRY_HEADER_SIZE + name.len()) as u32,
                len,
                values: values as u32,
            };
            if let Some(previous) = offsets.insert(index, name, location) {
                has_dead_entries = true;
                live_values -= previous.values as usize;
                live_bytes -= previous.entry_len();
            } else {
                live_fields += 1;
            }
            live_values += values;
            rest = tail;
        }

        Ok(Self {
            buffer: data,
            offsets,
            schema: Arc::clone(schema),
            finished: true,
            has_dead_entries,
            limits,
            live_bytes,
            live_values,
            live_fields,
        })
    }

    /// Opens a finished record for editing.
    pub fn reopen(&mut self) {
        self.finished = false;
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    #[must_use]
    pub fn schema(&self) -> &Arc<ModelSchema> {
        &self.schema
    }

    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.finished
    }
}
