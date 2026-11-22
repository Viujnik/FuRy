pub mod record;
use wasm_bindgen::prelude::*;

pub use fury_core::schema::registry::get_global_registry;

/// Installs a permanent system crash diagnostic hook.
/// Forwarding unexpected Rust panics and trace paths straight to the browser console.
#[wasm_bindgen]
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}

/// Hydrates the global schema registry from a bincode blob
/// (the exact contents of `.fury_registry.bin`).
///
/// On wasm32 there is no filesystem, so `SchemaRegistry::new` (registry.rs)
/// always starts with an empty map. Call this ONCE at app startup, before
/// constructing any `WasmRecord`.
///
/// # Errors
/// Returns a `JsValue` error if the blob is not a valid bincode `Vec<ModelSchema>`.
#[wasm_bindgen(js_name = loadRegistry)]
pub fn load_registry(bytes: &[u8]) -> Result<(), JsValue> {
    get_global_registry()
        .import_cache(bytes)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}
