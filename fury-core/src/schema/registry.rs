pub use super::field::FieldType;
use crate::error::{FuryError, Result};
use bincode::Options;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::{Arc, OnceLock};

const MAX_REGISTRY_BYTES: usize = 4 * 1024 * 1024;
const MAX_MODELS: usize = 4096;
const MAX_FIELDS: usize = 4096;
const MAX_TYPE_DEPTH: usize = 16;
#[cfg(not(target_arch = "wasm32"))]
const REGISTRY_CACHE_FILE: &str = ".fury_registry.bin";
static GLOBAL_REGISTRY: OnceLock<SchemaRegistry> = OnceLock::new();

/// Returns the process-wide registry for this runtime.
pub fn get_global_registry() -> &'static SchemaRegistry {
    GLOBAL_REGISTRY.get_or_init(SchemaRegistry::new)
}

/// Thread-safe runtime registry whose schemas are shared by readers.
#[derive(Clone, Default)]
pub struct SchemaRegistry {
    inner: Arc<RwLock<HashMap<String, Arc<ModelSchema>>>>,
}

impl SchemaRegistry {
    /// Loads the working-directory cache when present and valid.
    #[must_use]
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(registry) = Self::load_from_file(REGISTRY_CACHE_FILE) {
            return registry;
        }
        Self::default()
    }

    /// Registers a valid model and attempts to persist the native cache.
    pub fn register(&self, schema: ModelSchema) {
        if let Err(error) = self.register_with_cache(schema, false) {
            eprintln!("[fury] registration failed: {error}");
        }
    }

    /// Registers a valid model. Ordinary runtime imports can work in a read-only directory.
    pub fn register_with_cache(&self, mut schema: ModelSchema, require_cache: bool) -> Result<()> {
        schema.validate()?;
        schema.field_index = ModelSchema::build_field_index(&schema.fields);
        let mut map = self.inner.write();
        if map.len() >= MAX_MODELS && !map.contains_key(schema.name()) {
            return Err(invalid("too many schemas"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let models = map
                .values()
                .map(Arc::as_ref)
                .filter(|old| old.name() != schema.name())
                .chain(std::iter::once(&schema));
            let encoded = Self::encode_schemas(models)?;
            if let Err(error) = write_cache(Path::new(REGISTRY_CACHE_FILE), &encoded) {
                if require_cache {
                    return Err(error);
                }
                eprintln!("[fury] registry persist failed: {error}");
            }
        }
        #[cfg(target_arch = "wasm32")]
        if require_cache {
            return Err(invalid("this platform has no registry cache file"));
        }
        map.insert(schema.name().to_owned(), Arc::new(schema));
        Ok(())
    }

    /// Read an entire validated cache and then add or replace its entries atomically.
    pub fn import_cache(&self, bytes: &[u8]) -> Result<()> {
        let parsed = Self::decode_cache(bytes)?;
        let mut map = self.inner.write();
        let added = parsed
            .iter()
            .filter(|s| !map.contains_key(s.name()))
            .count();
        if added > MAX_MODELS - map.len() {
            return Err(invalid("too many schemas"));
        }
        for schema in parsed {
            map.insert(schema.name().to_owned(), Arc::new(schema));
        }
        Ok(())
    }

    /// Encode the canonical fixed-width cache for the selected models.
    pub fn encode_schemas<'a>(
        schemas: impl IntoIterator<Item = &'a ModelSchema>,
    ) -> Result<Vec<u8>> {
        let mut schemas: Vec<_> = schemas.into_iter().collect();
        if schemas.len() > MAX_MODELS {
            return Err(invalid("too many schemas"));
        }
        schemas.sort_by(|a, b| a.name().cmp(b.name()));
        if schemas
            .windows(2)
            .any(|pair| pair[0].name() == pair[1].name())
        {
            return Err(invalid("duplicate model name"));
        }
        for schema in &schemas {
            schema.validate()?;
        }
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_REGISTRY_BYTES as u64)
            .serialize(&schemas)
            .map_err(|e| invalid(format!("registry encoding: {e}")))
    }

    fn decode_cache(bytes: &[u8]) -> Result<Vec<ModelSchema>> {
        preflight_registry(bytes)?;
        let mut schemas: Vec<ModelSchema> = bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .with_limit(MAX_REGISTRY_BYTES as u64)
            .reject_trailing_bytes()
            .deserialize(bytes)
            .map_err(|e| invalid(format!("registry decoding: {e}")))?;
        let mut names = HashSet::with_capacity(schemas.len());
        for schema in &mut schemas {
            schema.validate()?;
            schema.field_index = ModelSchema::build_field_index(&schema.fields);
            if !names.insert(schema.name().to_owned()) {
                return Err(invalid("duplicate model name"));
            }
        }
        Ok(schemas)
    }

    /// Load a validated native cache without changing the process-wide registry.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| invalid(format!("read registry: {e}")))?;
        let mut bytes = Vec::new();
        file.take((MAX_REGISTRY_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| invalid(format!("read registry: {e}")))?;
        let registry = Self::default();
        registry.import_cache(&bytes)?;
        Ok(registry)
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
        let mut names: Vec<_> = self.inner.read().keys().cloned().collect();
        names.sort();
        names
    }

    /// Returns a vector of all registered model schemas as Arc pointers.
    #[must_use]
    pub fn get_all_schemas(&self) -> Vec<Arc<ModelSchema>> {
        let mut schemas: Vec<_> = self.inner.read().values().cloned().collect();
        schemas.sort_by(|a, b| a.name().cmp(b.name()));
        schemas
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_cache(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_file_name(format!(".fury-registry-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| invalid(format!("create registry temporary file: {e}")))?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| std::fs::rename(&temp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result.map_err(|e| invalid(format!("save registry: {e}")))
}

/// Create a schema error with its diagnostic text.
fn invalid(message: impl Into<Box<str>>) -> FuryError {
    FuryError::InvalidSchema(message.into())
}

/// Check nested type depth and the names of referenced opaque objects.
fn validate_type(ty: &FieldType, depth: usize) -> Result<()> {
    if depth > MAX_TYPE_DEPTH {
        return Err(invalid("schema nesting limit exceeded"));
    }
    match ty {
        FieldType::List(inner) => validate_type(inner, depth + 1)?,
        FieldType::Map(k, v) => {
            validate_type(k, depth + 1)?;
            validate_type(v, depth + 1)?;
        }
        FieldType::Object(name) if name.is_empty() || name.len() > 255 => {
            return Err(invalid("invalid object schema name"))
        }
        _ => {}
    }
    Ok(())
}

// Wire representation of Vec<ModelSchema> (fixed-width bincode 1.x), excluding the skipped index.
// Keep this scanner in sync with FieldType/FieldSchema and guard it with roundtrip tests.
/// Walk the fixed-width bincode layout before deserialization can allocate it.
fn preflight_registry(bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(invalid("registry byte limit exceeded"));
    }
    struct Scan<'a> {
        rest: &'a [u8],
    }
    impl<'a> Scan<'a> {
        /// Advance the bounded scanner cursor and return the next exact byte range.
        fn take(&mut self, n: usize) -> Result<&'a [u8]> {
            if n > self.rest.len() {
                return Err(invalid("truncated registry"));
            }
            let (head, tail) = self.rest.split_at(n);
            self.rest = tail;
            Ok(head)
        }
        /// Read a fixed-width count and reject values over the caller's limit.
        fn count(&mut self, max: usize) -> Result<usize> {
            let n = u64::from_le_bytes(self.take(8)?.try_into().unwrap());
            if n > max as u64 {
                return Err(invalid("registry count or string limit exceeded"));
            }
            Ok(n as usize)
        }
        /// Read and validate a length-prefixed schema or field name.
        fn name(&mut self) -> Result<()> {
            let n = self.count(255)?;
            if n == 0 {
                return Err(invalid("empty schema or field name"));
            }
            std::str::from_utf8(self.take(n)?).map_err(|_| invalid("invalid registry UTF-8"))?;
            Ok(())
        }
        /// Scan a field type by its bincode tag, recursing only into supported container shapes.
        fn ty(&mut self, depth: usize) -> Result<()> {
            if depth > MAX_TYPE_DEPTH {
                return Err(invalid("schema nesting limit exceeded"));
            }
            match u32::from_le_bytes(self.take(4)?.try_into().unwrap()) {
                0..=15 => Ok(()),
                16 => self.ty(depth + 1),
                17 => {
                    self.ty(depth + 1)?;
                    self.ty(depth + 1)
                }
                18 => self.name(),
                _ => Err(invalid("unknown schema type")),
            }
        }
    }
    let mut s = Scan { rest: bytes };
    let models = s.count(MAX_MODELS)?;
    for _ in 0..models {
        s.name()?;
        let fields = s.count(MAX_FIELDS)?;
        for _ in 0..fields {
            s.name()?;
            s.ty(0)?;
            if s.take(1)?[0] > 1 {
                return Err(invalid("invalid nullable flag"));
            }
        }
    }
    if !s.rest.is_empty() {
        return Err(invalid("trailing registry bytes"));
    }
    Ok(())
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

    /// Check model and field names, uniqueness, field count and nested type validity.
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty() || self.name.len() > 255 {
            return Err(invalid("model name must contain 1..255 UTF-8 bytes"));
        }
        if self.fields.len() > MAX_FIELDS {
            return Err(invalid("too many schema fields"));
        }
        let mut names = HashSet::with_capacity(self.fields.len());
        for field in &self.fields {
            if field.name.is_empty() || field.name.len() > 255 {
                return Err(invalid("field name must contain 1..255 UTF-8 bytes"));
            }
            if !names.insert(field.name.as_str()) {
                return Err(invalid(format!("duplicate field '{}'", field.name)));
            }
            validate_type(&field.field_type, 0)?;
        }
        Ok(())
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
    // Called from the constructor and from SchemaRegistry::register.
    pub(crate) fn build_field_index(fields: &[FieldSchema]) -> HashMap<String, usize> {
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
