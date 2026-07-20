pub mod record;
use wasm_bindgen::prelude::*;

// Re-export the static global registry reference directly out from the core layer.
pub use fury_core::schema::registry::get_global_registry;

#[wasm_bindgen]
/// Installs a permanent system crash diagnostic hook.
/// Forwarding unexpected Rust panics and trace paths straight to the browser console.
pub fn init_panic_hook() {
    console_error_panic_hook::set_once();
}
