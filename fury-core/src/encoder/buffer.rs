use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::ModelSchema;

pub struct FlatBufferBuilder {
    schema: ModelSchema,
    buffer: Vec<u8>,
    offsets: Arc<RwLock<HashMap<Arc<str>, usize>>>,
    is_finished: bool,
}

impl FlatBufferBuilder {
    #[must_use]
    pub fn new(schema: ModelSchema) -> Self {
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
            offsets: Arc::new(RwLock::new(HashMap::with_capacity(fields_count))),
            is_finished: false,
        }
    }

    pub fn set_field(&mut self, name: &str, value: ValueType) -> Result<()> {
        let field = self
            .schema
            .find_field(name)
            .ok_or_else(|| FuryError::FieldNotFound {
                field: name.into(),
                schema: self.schema.name().into(),
            })?;

        let offset = self.buffer.len();
        value.write_to(&mut self.buffer);

        let mut offsets = self.offsets.write();
        offsets.insert(Arc::from(field.name()), offset);

        Ok(())
    }

    #[must_use]
    pub fn get_field_bytes(&self, name: &str) -> Option<&[u8]> {
        let offsets = self.offsets.read();
        let &offset = offsets.get(name)?;

        let next_offset = offsets.values().copied().filter(|&off| off > offset).min();
        let end = next_offset.unwrap_or_else(|| self.buffer.len());

        self.buffer.get(offset..end)
    }

    #[must_use]
    pub fn get_field_value(&self, name: &str) -> Option<ValueType> {
        let bytes = self.get_field_bytes(name)?;
        let field = self.schema.find_field(name)?;
        ValueType::from_bytes(bytes, &field.field_type())
    }

    pub fn finish(&mut self) -> Result<&[u8]> {
        if self.is_finished {
            return Err(FuryError::BufferAlreadyFinished);
        }

        let size = self.buffer.len() as u32;
        self.buffer[0..4].copy_from_slice(&size.to_le_bytes());

        self.is_finished = true;
        Ok(&self.buffer)
    }

    pub fn as_bytes(&self) -> Result<&[u8]> {
        if !self.is_finished {
            return Err(FuryError::BufferNotFinished);
        }
        Ok(&self.buffer)
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
    pub fn schema(&self) -> &ModelSchema {
        &self.schema
    }

    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.is_finished
    }
}
