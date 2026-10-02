use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::ModelSchema;
use std::collections::HashMap;
use std::sync::Arc;

const WIRE_MAGIC: u8 = b'F';
const WIRE_VERSION: u8 = 2;

const SIZE_LEN: usize = 4;
const MAGIC_OFFSET: usize = SIZE_LEN;
const VERSION_OFFSET: usize = SIZE_LEN + 1;
const HEADER_SIZE: usize = SIZE_LEN + 2;

const NAME_LEN_SIZE: usize = 1;
const VALUE_LEN_SIZE: usize = 4;

/// A high-performance binary record with a dynamic schema.
///
/// Stores fields sequentially in a flat byte array, providing ultra-fast,
/// zero-copy field access by calculating boundaries during serialization.
///
/// # Format of buffer
///
/// ```text
/// [4 bytes: header (total size)][field1_data][field2_data]...
/// ```

pub struct BinaryRecord {
    schema: Arc<ModelSchema>,
    buffer: Vec<u8>,
    // Mapping from field name to its exact coordinates: (start_offset, length)
    offsets: HashMap<String, (usize, usize)>,
    is_finished: bool,
    reopened: bool,
}

impl BinaryRecord {
    /// Creates a new empty record with a pre-allocated memory buffer based on the schema.
    #[must_use]
    pub fn new(schema: Arc<ModelSchema>) -> Self {
        let fields_count = schema.fields_count();
        let estimated_size = HEADER_SIZE
            + schema
                .fields()
                .iter()
                .map(|field| {
                    NAME_LEN_SIZE
                        + field.name().len()
                        + VALUE_LEN_SIZE
                        + match field.field_type().fixed_size() {
                            Some(size) => 1 + size,
                            None => 1 + 8,
                        }
                })
                .sum::<usize>();
        let mut buffer = Vec::with_capacity(estimated_size);
        buffer.extend_from_slice(&[0u8; HEADER_SIZE]);

        Self {
            schema,
            buffer,
            offsets: HashMap::with_capacity(fields_count),
            is_finished: false,
            reopened: false,
        }
    }

    fn write_entry(buf: &mut Vec<u8>, name: &str, value: &ValueType) -> Result<(usize, usize)> {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u8::MAX as usize {
            return Err(FuryError::Encoding(
                format!("field name '{name}' longer than 255 bytes").into(),
            ));
        }

        buf.push(name_bytes.len() as u8);
        buf.extend_from_slice(name_bytes);

        let len_pos = buf.len();
        buf.extend_from_slice(&[0u8; VALUE_LEN_SIZE]);
        let value_start = buf.len();
        value.write_to(buf);
        let value_len = buf.len() - value_start;

        buf[len_pos..len_pos + VALUE_LEN_SIZE].copy_from_slice(&(value_len as u32).to_le_bytes());

        Ok((value_start, value_len))
    }

    fn fill_header(buf: &mut [u8]) {
        let size = buf.len() as u32;
        buf[0..SIZE_LEN].copy_from_slice(&size.to_le_bytes());
        buf[MAGIC_OFFSET] = WIRE_MAGIC;
        buf[VERSION_OFFSET] = WIRE_VERSION;
    }

    /// Serializes and appends a field value to the record buffer.
    ///
    /// # Errors
    ///
    /// * `FuryError::BufferAlreadyFinished` - If `finish()` has already been called on this record.
    /// * `FuryError::FieldNotFound` - If the requested field name does not exist in the model schema.
    pub fn set_field(&mut self, name: &str, value: ValueType) -> Result<()> {
        if self.is_finished {
            return Err(FuryError::BufferAlreadyFinished);
        }

        let field = self
            .schema
            .find_field(name)
            .ok_or_else(|| FuryError::FieldNotFound {
                field: name.into(),
                schema: self.schema.name().into(),
            })?;

        let (start_offset, field_len) = Self::write_entry(&mut self.buffer, field.name(), &value)?;

        self.offsets
            .insert(field.name().to_string(), (start_offset, field_len));

        Ok(())
    }

    /// Returns a direct slice of raw bytes corresponding to the given field name.
    ///
    /// This operation is strictly $O(1)$ and executes without any memory allocations or data copying.
    ///
    /// Returns `Some(&[u8])` containing the raw field bytes if the field exists and has been
    /// serialized, or `None` if the field name is not found in the record's offsets.
    #[must_use]
    pub fn get_field_bytes(&self, name: &str) -> Option<&[u8]> {
        let &(offset, len) = self.offsets.get(name)?;

        self.buffer.get(offset..(offset + len))
    }

    /// Retrieves and parses a field's raw bytes back into its typed `ValueType` representation.
    ///
    /// Returns `Some(ValueType)` if the field is found and its binary payload is successfully
    /// deserialized into a valid system type.
    ///
    /// Returns `None` if the field name does not exist in the record, or if the raw byte
    /// sequence is corrupted and fails the type verification layout.
    #[must_use]
    pub fn get_field_value(&self, name: &str) -> Option<ValueType> {
        let bytes = self.get_field_bytes(name)?;
        let field = self.schema.find_field(name)?;
        ValueType::from_bytes(bytes, &field.field_type())
    }

    /// Returns the byte coordinates of a field within the record buffer.
    ///
    /// Returns `Some((offset, length))` where:
    /// - `offset` — starting byte position of the field in the buffer
    /// - `length` — number of bytes occupied by the field
    ///
    /// Returns `None` if the field does not exist or has not been set.
    pub fn get_field_offset(&self, name: &str) -> Option<(usize, usize)> {
        self.offsets.get(name).copied()
    }

    /// Finalizes the record.
    ///
    /// * Fresh build (BUILDING): buffer is already tight — writes the 4-byte
    ///   header only. O(1).
    /// * Mutated-after-parse (REOPENED): rebuilds the buffer field-by-field,
    ///   dropping dead bytes left by overwritten fields. O(record size),
    ///   paid once per finish, only on this path.
    ///
    /// # Errors
    /// * `FuryError::BufferAlreadyFinished` — called twice.
    pub fn finish(&mut self) -> Result<&[u8]> {
        if self.is_finished {
            return Err(FuryError::BufferAlreadyFinished);
        }

        if self.reopened {
            self.rebuild_buffer()?;
            self.reopened = false;
        } else {
            Self::fill_header(&mut self.buffer);
        }

        self.is_finished = true;
        Ok(&self.buffer)
    }

    /// Rebuilds the buffer tightly from current field values.
    ///
    /// Parses every SET field in schema order (via the codec) and re-serializes
    /// it into a fresh buffer, then rewrites the header. Unset fields are skipped.
    /// Pre-allocates with the old buffer length as an upper bound — no reallocations.
    fn rebuild_buffer(&mut self) -> Result<()> {
        let mut entries: Vec<(String, ValueType)> = Vec::with_capacity(self.offsets.len());
        for field in self.schema.fields() {
            if self.offsets.contains_key(field.name()) {
                let value = self.get_field_value(field.name()).ok_or_else(|| {
                    FuryError::Encoding(
                        format!("corrupt field '{}' during repack", field.name()).into(),
                    )
                })?;
                entries.push((field.name().to_string(), value));
            }
        }

        let mut buf: Vec<u8> = Vec::with_capacity(self.buffer.len());
        buf.extend_from_slice(&[0u8; HEADER_SIZE]);
        let mut offsets = HashMap::with_capacity(entries.len());
        for (name, value) in &entries {
            let (start, len) = Self::write_entry(&mut buf, name, value)?;
            offsets.insert(name.clone(), (start, len));
        }

        Self::fill_header(&mut buf);

        self.buffer = buf;
        self.offsets = offsets;
        Ok(())
    }

    /// Returns the finalized raw byte payload of the entire record.
    ///
    /// # Errors
    ///
    /// * `FuryError::BufferNotFinished` - If called before freezing the record via `finish()`.
    pub fn as_bytes(&self) -> Result<&[u8]> {
        if !self.is_finished {
            return Err(FuryError::BufferNotFinished);
        }
        Ok(&self.buffer)
    }

    /// Compiles a `BinaryRecord` layout directly from a raw network byte stream.
    ///
    /// # Memory & Safety
    ///
    /// Performs strict frame boundary validation to prevent memory safety violations
    /// or integer overflow attacks during dynamic slice assignment.
    ///
    /// # Errors
    ///
    /// Returns `FuryError::InvalidSchema` if payload header constraints are broken.
    pub fn from_raw_bytes(data: Vec<u8>, schema: &Arc<ModelSchema>) -> Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(FuryError::InvalidSchema(Box::from(
                "Malformed packet stream: buffer size is below header requirements",
            )));
        }

        let total_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() != total_size {
            return Err(FuryError::InvalidSchema(Box::from(format!(
                "Integrity mismatch: payload frame size ({}) deviates from header declaration ({})",
                data.len(),
                total_size
            ))));
        }

        match (data[MAGIC_OFFSET], data[VERSION_OFFSET]) {
            (WIRE_MAGIC, WIRE_VERSION) => {}
            (WIRE_MAGIC, v) => {
                return Err(FuryError::InvalidSchema(Box::from(format!(
                    "Unsupported wire version {v} (this fury-core speaks v{WIRE_VERSION}). \
                     Rebuild all Fury components from the same workspace revision."
                ))))
            }
            _ => {
                return Err(FuryError::InvalidSchema(Box::from(
                    "Not a FuRy buffer: legacy header or foreign bytes",
                )))
            }
        }

        let mut offsets = HashMap::with_capacity(schema.fields_count());
        let mut cursor = HEADER_SIZE;

        while cursor < data.len() {
            let name_len = data[cursor] as usize;
            cursor += NAME_LEN_SIZE;
            let name_end = cursor + name_len;
            if name_end > data.len() {
                return Err(FuryError::InvalidSchema(Box::from(
                    "Unexpected EOB while reading field name",
                )));
            }
            let name = std::str::from_utf8(&data[cursor..name_end])
                .map_err(|_| FuryError::InvalidSchema(Box::from("Invalid UTF-8 field name")))?;
            cursor = name_end;

            if cursor + VALUE_LEN_SIZE > data.len() {
                return Err(FuryError::InvalidSchema(Box::from(
                    "Unexpected EOF while reading value length",
                )));
            }
            let value_len = u32::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]) as usize;
            cursor += VALUE_LEN_SIZE;
            let value_start = cursor;
            let value_end = value_start + value_len;
            if value_end > data.len() {
                return Err(FuryError::InvalidSchema(Box::from(format!(
                    "Unexpected EOB while parsing field '{name}'"
                ))));
            }
            if schema.find_field(name).is_some() {
                offsets.insert(name.to_string(), (value_start, value_len));
            }
            cursor = value_end;
        }

        Ok(Self {
            buffer: data,
            offsets,
            schema: Arc::clone(schema),
            is_finished: true,
            reopened: false,
        })
    }

    /// Reopens a finished record for mutation.
    ///
    /// The next `finish()` rebuilds the buffer from scratch (compaction),
    /// so overwritten fields leave NO dead bytes behind.
    pub fn reopen(&mut self) {
        self.is_finished = false;
        self.reopened = true;
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
        self.is_finished
    }
}
