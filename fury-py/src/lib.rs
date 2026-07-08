use pyo3::prelude::*;
use std::sync::OnceLock;

mod conversion;
mod db;
mod errors;
mod models;

use db::FuryDB;
use fury_core::schema::registry::SchemaRegistry; // ✅ Один импорт
use models::BaseModel;

static GLOBAL_REGISTRY: OnceLock<SchemaRegistry> = OnceLock::new();

pub fn get_global_registry() -> &'static SchemaRegistry {
    GLOBAL_REGISTRY.get_or_init(SchemaRegistry::new)
}

#[pymodule]
fn fury(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<FuryDB>()?;
    m.add_class::<BaseModel>()?;
    Ok(())
}
