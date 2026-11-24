pub mod page;
pub mod record;
use fury_core::schema::registry::SchemaRegistry;
use wasm_bindgen::prelude::*;

pub use fury_core::schema::registry::get_global_registry;

/// An isolated schema snapshot for one client or API version.
#[wasm_bindgen]
pub struct WasmRegistry {
    schemas: SchemaRegistry,
}

#[wasm_bindgen]
impl WasmRegistry {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: &js_sys::Uint8Array) -> Result<WasmRegistry, JsValue> {
        if bytes.length() as usize > SchemaRegistry::MAX_ENCODED_BYTES {
            return Err(JsValue::from_str("registry exceeds byte limit"));
        }
        let schemas = SchemaRegistry::default();
        schemas
            .import_cache(&bytes.to_vec())
            .map_err(|err| JsValue::from_str(&err.to_string()))?;
        Ok(Self { schemas })
    }

    /// Open or create one record with this registry.
    #[wasm_bindgen(js_name = openRecord)]
    pub fn open_record(
        &self,
        schema_id: &str,
        bytes: Option<js_sys::Uint8Array>,
    ) -> Result<record::WasmRecord, JsValue> {
        let schema = self
            .schemas
            .get(schema_id)
            .ok_or_else(|| JsValue::from_str("unknown record schema"))?;
        record::WasmRecord::with_schema(schema, bytes.as_ref())
    }

    /// Decode one page. Signed and unsigned 64-bit fields are always bigint.
    #[wasm_bindgen(js_name = decodePage)]
    pub fn decode_page(
        &self,
        schema_id: &str,
        bytes: &js_sys::Uint8Array,
        max_rows: f64,
    ) -> Result<js_sys::Array, JsValue> {
        let schema = self
            .schemas
            .get(schema_id)
            .ok_or_else(|| JsValue::from_str("unknown page schema"))?;
        page::decode_with_schema(&schema, bytes, max_rows, true)
    }
}

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
pub fn load_registry(bytes: &[u8]) -> Result<(), JsValue> {
    get_global_registry()
        .import_cache(bytes)
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

#[wasm_bindgen(js_name = loadRegistry)]
pub fn load_registry_js(bytes: &js_sys::Uint8Array) -> Result<(), JsValue> {
    if bytes.length() as usize > SchemaRegistry::MAX_ENCODED_BYTES {
        return Err(JsValue::from_str("registry exceeds byte limit"));
    }
    load_registry(&bytes.to_vec())
}
