//! Minimal FuRy round trip with an application-supplied schema.
use fury_core::encoder::buffer::BinaryRecord;
use fury_core::encoder::value::ValueType;
use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema};
use std::sync::Arc;

fn main() -> fury_core::error::Result<()> {
    let schema = Arc::new(ModelSchema::new(
        "User",
        vec![FieldSchema::new("id", FieldType::Int64, false)],
    ));
    let mut outgoing = BinaryRecord::new(Arc::clone(&schema));
    outgoing.set_field("id", ValueType::Int64(42))?;
    let incoming = BinaryRecord::from_raw_bytes(outgoing.finish()?.to_vec(), &schema)?;
    assert_eq!(incoming.get_field_value("id"), Some(ValueType::Int64(42)));
    Ok(())
}
