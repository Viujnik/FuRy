use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::ModelSchema;
use std::collections::HashMap;
use std::sync::Arc;

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
}

impl BinaryRecord {
    /// Creates a new empty record with a pre-allocated memory buffer based on the schema.
    #[must_use]
    pub fn new(schema: Arc<ModelSchema>) -> Self {
        let fields_count = schema.fields_count();
        let estimated_size = 4 + schema
            .fields()
            .iter()
            .map(|field| match field.field_type().fixed_size() {
                Some(size) => 1 + size,
                None => 1 + 8,
            })
            .sum::<usize>();
        let mut buffer = Vec::with_capacity(estimated_size);
        buffer.extend_from_slice(&[0u8; 4]);

        Self {
            schema,
            buffer,
            offsets: HashMap::with_capacity(fields_count),
            is_finished: false,
        }
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

        let start_offset = self.buffer.len();
        value.write_to(&mut self.buffer);

        let field_len = self.buffer.len() - start_offset;
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

    /// Retrieves and parses a field's raw bytes back into its typed `ValueType` representation.
    ///
    /// Returns `Some(ValueType)` if the field is found and its binary payload is successfully
    /// deserialized into a valid system type.
    ///
    /// Returns `None` if the field name does not exist in the record, or if the raw byte
    /// sequence is corrupted and fails the type verification layout.
    pub fn finish(&mut self) -> Result<&[u8]> {
        if self.is_finished {
            return Err(FuryError::BufferAlreadyFinished);
        }

        let size = self.buffer.len() as u32;
        self.buffer[0..4].copy_from_slice(&size.to_le_bytes());

        self.is_finished = true;
        Ok(&self.buffer)
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
        if data.len() < 4 {
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

        let mut offsets = HashMap::with_capacity(schema.fields_count());

        let mut cursor = 4;

        for field in schema.fields() {
            if cursor + 8 > data.len() {
                return Err(FuryError::InvalidSchema(Box::from(format!(
                    "Unexpected EOB parsing boundaries for field '{}'",
                    field.name()
                ))));
            }

            let offset = u32::from_le_bytes([
                data[cursor],
                data[cursor + 1],
                data[cursor + 2],
                data[cursor + 3],
            ]) as usize;
            let length = u32::from_le_bytes([
                data[cursor + 4],
                data[cursor + 5],
                data[cursor + 6],
                data[cursor + 7],
            ]) as usize;

            if offset + length > data.len() {
                return Err(FuryError::InvalidSchema(Box::from(format!(
                    "Out-of-bounds pointer layout detected for field '{}'",
                    field.name()
                ))));
            }

            offsets.insert(field.name().to_owned(), (offset, length));
            cursor += 8;
        }

        Ok(Self {
            buffer: data,
            offsets,
            schema: Arc::clone(schema),
            is_finished: true,
        })
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
