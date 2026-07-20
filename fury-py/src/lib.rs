pub use fury_core::schema::registry::get_global_registry;
use pyo3::prelude::*;
mod conversion;
mod db;
mod errors;
mod models;

use db::FuryDB;
use models::BaseModel;

#[pymodule]
fn fury(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<FuryDB>()?;
    m.add_class::<BaseModel>()?;
    Ok(())
}
