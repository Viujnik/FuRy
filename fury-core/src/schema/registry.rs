pub use super::field::FieldType;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;

const REGISTRY_CACHE_FILE: &str = ".fury_registry.bin";

static GLOBAL_REGISTRY: OnceLock<SchemaRegistry> = OnceLock::new();

/// Returns a read-only static reference to the shared compilation schema registry.
///
/// This is the single source of truth for all environment layers.
pub fn get_global_registry() -> &'static SchemaRegistry {
    GLOBAL_REGISTRY.get_or_init(SchemaRegistry::new)
}

/// Global registry for model schemas.
///
/// Thread-safe storage for all registered models. Uses `Arc<RwLock<>>` for
/// concurrent read access with exclusive write access.
#[derive(Clone, Default)]
pub struct SchemaRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<ModelSchema>>>>,
}

impl SchemaRegistry {
    /// Creates a registry hydrated from the on-disk cache (`.fury_registry.bin`
    /// in the current working directory). On any read/parse failure — or on
    /// platforms without a filesystem (WASM32) — the registry starts empty.
    #[must_use]
    pub fn new() -> Self {
        let mut map = HashMap::new();

        if Path::new(REGISTRY_CACHE_FILE).exists() {
            if let Ok(mut file) = File::open(REGISTRY_CACHE_FILE) {
                let mut buf = Vec::new();
                if file.read_to_end(&mut buf).is_ok() {
                    if let Ok(schemas) = bincode::deserialize::<Vec<ModelSchema>>(&buf) {
                        for mut schema in schemas {
                            schema.field_index = ModelSchema::build_field_index(&schema.fields);
                            map.insert(schema.name().to_string(), Arc::new(schema));
                        }
                    }
                }
            }
        }

        Self {
            inner: Arc::new(RwLock::new(map)),
        }
    }

    /// Registers a new model schema.
    ///
    /// If a schema with the same name already exists, it will be replaced.
    pub fn register(&self, schema: ModelSchema) {
        let name = schema.name().to_string();
        let schema_arc = Arc::new(schema);

        let mut map = self.inner.write();
        map.insert(name, schema_arc);

        let schemas_to_save: Vec<ModelSchema> = map.values().map(|arc| (**arc).clone()).collect();

        if let Ok(encoded) = bincode::serialize(&schemas_to_save) {
            let tmp = format!("{REGISTRY_CACHE_FILE}.tmp");
            if fs::write(&tmp, &encoded).is_ok() {
                if let Err(e) = fs::rename(&tmp, REGISTRY_CACHE_FILE) {
                    eprintln!("[fury] registry persist failed: {e}");
                }
            } else {
                eprintln!("[fury] registry persist failed: cannot write tmp file");
            }
        }
    }

    /// Retrieves a model schema by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Arc<ModelSchema>> {
        let map = self.inner.read();
        map.get(name).cloned()
    }

    /// Checks if a schema with the given name exists.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        let map = self.inner.read();
        map.contains_key(name)
    }

    /// Removes a schema by name and returns it if found.
    pub fn remove(&self, name: &str) -> Option<Arc<ModelSchema>> {
        let mut map = self.inner.write();
        map.remove(name)
    }

    /// Returns the number of registered schemas.
    #[must_use]
    pub fn len(&self) -> usize {
        let map = self.inner.read();
        map.len()
    }

    /// Returns `true` if no schemas are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let map = self.inner.read();
        map.is_empty()
    }

    /// Returns a list of all registered model names.
    #[must_use]
    pub fn list_models(&self) -> Vec<String> {
        let map = self.inner.read();
        map.keys().cloned().collect()
    }

    /// Returns a vector of all registered model schemas as Arc pointers.
    #[must_use]
    pub fn get_all_schemas(&self) -> Vec<Arc<ModelSchema>> {
        self.inner.read().values().cloned().collect()
    }
}

/// Schema definition for a model.
///
/// Contains the model name, field definitions, and a pre-computed index
/// for fast field lookup by name.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModelSchema {
    name: String,
    fields: Vec<FieldSchema>,
    #[serde(skip)]
    field_index: HashMap<String, usize>,
}

impl ModelSchema {
    /// Creates a new model schema with the given name and fields.
    ///
    /// The field index is built immediately for fast lookup.
    #[must_use]
    pub fn new(name: impl Into<String>, fields: Vec<FieldSchema>) -> Self {
        let field_index = Self::build_field_index(&fields);
        Self {
            name: name.into(),
            fields,
            field_index,
        }
    }

    /// Returns the model name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns a slice of all fields.
    #[must_use]
    pub fn fields(&self) -> &[FieldSchema] {
        &self.fields
    }

    /// Returns the number of fields.
    #[must_use]
    pub fn fields_count(&self) -> usize {
        self.fields.len()
    }

    /// Finds a field by name.
    ///
    /// Returns `None` if the field does not exist.
    #[must_use]
    pub fn find_field(&self, name: &str) -> Option<&FieldSchema> {
        let idx = self.field_index.get(name)?;
        self.fields.get(*idx)
    }

    /// Returns the index of a field by name.
    ///
    /// Returns `None` if the field does not exist.
    #[must_use]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.field_index.get(name).copied()
    }

    // Helper function to eagerly compile a fast O(1) field lookup index.
    // Called once inside the constructor during ModelSchema initialization.
    fn build_field_index(fields: &[FieldSchema]) -> HashMap<String, usize> {
        let mut index = HashMap::with_capacity(fields.len());
        for (i, field) in fields.iter().enumerate() {
            index.insert(field.name().to_string(), i);
        }
        index
    }
}

/// Schema definition for a single field.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FieldSchema {
    name: String,
    field_type: FieldType,
    nullable: bool,
}

impl FieldSchema {
    /// Creates a new field schema.
    #[must_use]
    pub fn new(name: impl Into<String>, field_type: FieldType, nullable: bool) -> Self {
        Self {
            name: name.into(),
            field_type,
            nullable,
        }
    }

    /// Returns `true` if the field is nullable.
    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        self.nullable
    }

    /// Returns the field name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the field type.
    #[must_use]
    pub fn field_type(&self) -> &FieldType {
        &self.field_type
    }
}

impl PartialEq for ModelSchema {
    /// Determines equality between two model schemas.
    ///
    /// # Implementation Details
    ///
    /// Two schemas are considered equal if their names and ordered field sequences are identical.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fields == other.fields
    }
}
