//! Shared framing for individual records and sequences of records.
use super::{limits::DecodeBudget, value::ValueType, DecodeLimits};
use crate::error::{FuryError, Result};
use crate::schema::registry::ModelSchema;

pub(crate) const HEADER: usize = 6;
pub(crate) const VERSION: u8 = 3;

fn invalid(message: &str) -> FuryError {
    FuryError::InvalidSchema(message.into())
}

// The common case costs one byte. The escape keeps the full u32 range.
pub(crate) fn length_size(len: usize) -> usize {
    if len < 255 {
        1
    } else {
        5
    }
}

pub(crate) fn write_entry_header(output: &mut Vec<u8>, name: &[u8], len: usize) {
    output.push(name.len() as u8);
    output.extend_from_slice(name);
    if len < 255 {
        output.push(len as u8);
    } else {
        output.push(255);
        output.extend_from_slice(&(len as u32).to_le_bytes());
    }
}

pub(crate) fn finish_frame(frame: &mut [u8]) {
    let len = frame.len() as u32;
    frame[..4].copy_from_slice(&len.to_le_bytes());
    frame[4..HEADER].copy_from_slice(&[b'F', VERSION]);
}

/// A checked field. Unknown payloads can be skipped or retained without interpretation.
pub struct FrameField<'a> {
    pub name: &'a str,
    pub index: Option<usize>,
    /// Already validated UTF-8 for a present string field.
    pub text: Option<&'a str>,
    pub payload: &'a [u8],
    pub(crate) entry: usize,
    pub(crate) start: usize,
    pub(crate) values: usize,
}

/// Borrows a frame and walks it without constructing an index or copying payloads.
pub struct FrameReader<'a, 'limits> {
    rest: &'a [u8],
    size: usize,
    fields: usize,
    budget: DecodeBudget<'limits>,
}

impl<'a, 'limits> FrameReader<'a, 'limits> {
    pub fn new(frame: &'a [u8], limits: &'limits DecodeLimits) -> Result<Self> {
        let (header, rest) = frame
            .split_first_chunk::<HEADER>()
            .ok_or_else(|| invalid("truncated frame header"))?;
        if frame.len() > limits.max_frame_bytes
            || frame.len() != u32::from_le_bytes(header[..4].try_into().unwrap()) as usize
        {
            return Err(invalid("invalid frame length"));
        }
        if header[4] != b'F' || !matches!(header[5], 2 | VERSION) {
            return Err(invalid("unsupported FuRy frame"));
        }
        let mut budget = DecodeBudget::new(limits);
        budget.short_values = header[5] == VERSION;
        Ok(Self {
            rest,
            size: frame.len(),
            fields: 0,
            budget,
        })
    }

    #[inline]
    pub fn next_field<'field>(
        &mut self,
        schema: &'field ModelSchema,
    ) -> Result<Option<FrameField<'field>>>
    where
        'a: 'field,
    {
        let Some((&name_len, tail)) = self.rest.split_first() else {
            return Ok(None);
        };
        if self.fields >= self.budget.limits.max_fields {
            return Err(invalid("frame exceeds field limit"));
        }
        let (name, tail) = tail
            .split_at_checked(name_len.into())
            .ok_or_else(|| invalid("truncated field name"))?;
        let (name, index) = match schema.fields().get(self.fields) {
            Some(field) if field.name().as_bytes() == name => (field.name(), Some(self.fields)),
            _ => {
                let name =
                    std::str::from_utf8(name).map_err(|_| invalid("invalid UTF-8 field name"))?;
                (name, schema.field_index(name))
            }
        };
        let (len, tail) = if self.budget.short_values {
            let (&short, tail) = tail
                .split_first()
                .ok_or_else(|| invalid("missing field length"))?;
            if short == 255 {
                let (len, tail) = read_length(tail)?;
                if len < 255 {
                    return Err(invalid("nonminimal field length"));
                }
                (len, tail)
            } else {
                (usize::from(short), tail)
            }
        } else {
            read_length(tail)?
        };
        let (payload, rest) = tail
            .split_at_checked(len)
            .ok_or_else(|| invalid("truncated field value"))?;
        let before = self.budget.remaining;
        let mut text = None;
        if let Some(i) = index {
            let field = &schema.fields()[i];
            let ty = field.field_type();
            let valid =
                if *ty == crate::schema::field::FieldType::String && payload.first() != Some(&0) {
                    text = payload.split_first().and_then(|(&tag, data)| {
                        let (bytes, rest) =
                            super::value::read_blob(tag, data, self.budget.short_values)?;
                        if !rest.is_empty() {
                            return None;
                        }
                        std::str::from_utf8(bytes).ok()
                    });
                    text.is_some() && self.budget.consume(0).is_some()
                } else {
                    (field.is_nullable() || payload.first() != Some(&0))
                        && ValueType::validate_bytes(payload, ty, &mut self.budget)
                };
            if !valid {
                return Err(invalid(&format!("invalid value for field '{name}'")));
            }
        }
        let entry = self.size - self.rest.len();
        self.rest = rest;
        self.fields += 1;
        Ok(Some(FrameField {
            name,
            index,
            payload,
            text,
            entry,
            start: self.size - rest.len() - payload.len(),
            values: before - self.budget.remaining,
        }))
    }
}

fn read_length(bytes: &[u8]) -> Result<(usize, &[u8])> {
    let (raw, tail) = bytes
        .split_first_chunk::<4>()
        .ok_or_else(|| invalid("truncated field length"))?;
    Ok((u32::from_le_bytes(*raw) as usize, tail))
}

/// Append a complete projected record directly to a page. Roll back on any failure.
pub fn append_row(
    page: &mut Vec<u8>,
    schema: &ModelSchema,
    limits: &DecodeLimits,
    mut value_at: impl FnMut(&crate::schema::registry::FieldSchema) -> Result<ValueType>,
) -> Result<()> {
    let start = page.len();
    let max = limits.max_frame_bytes.min(u32::MAX as usize);
    if HEADER > max.saturating_sub(start) || schema.fields_count() > limits.max_fields {
        return Err(invalid("page exceeds byte or field limit"));
    }
    page.resize(start + HEADER, 0);
    let mut remaining = limits.max_total_values;
    let result = schema.fields().iter().try_for_each(|field| {
        let value = value_at(field)?;
        if field.name().len() > 255
            || (value.is_none() && !field.is_nullable())
            || !value.matches_type(field.field_type(), 0)
        {
            return Err(invalid(&format!("invalid field '{}'", field.name())));
        }
        let (len, values) = value
            .encoded_size(limits)
            .ok_or_else(|| invalid("field exceeds value limit"))?;
        remaining = remaining
            .checked_sub(values)
            .ok_or_else(|| invalid("record exceeds value limit"))?;
        let header_len = 1 + field.name().len() + length_size(len);
        if header_len > max - page.len() || len > max - page.len() - header_len {
            return Err(invalid("page exceeds byte limit"));
        }
        write_entry_header(page, field.name().as_bytes(), len);
        value.write_to(page);
        Ok(())
    });
    if let Err(error) = result {
        page.truncate(start);
        return Err(error);
    }
    finish_frame(&mut page[start..]);
    Ok(())
}
