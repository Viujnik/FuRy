use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

pub use super::field::FieldType;

#[derive(Clone, Default)]
pub struct SchemaRegistry {
    inner: Arc<RwLock<HashMap<Arc<str>, ModelSchema>>>,
}

impl SchemaRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, schema: ModelSchema) {
        let mut map = self.inner.write();
        map.insert(schema.name.clone(), schema);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<ModelSchema> {
        let map = self.inner.read();
        map.get(name).cloned()
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        let map = self.inner.read();
        map.contains_key(name)
    }

    pub fn remove(&self, name: &str) -> Option<ModelSchema> {
        let mut map = self.inner.write();
        map.remove(name)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        let map = self.inner.read();
        map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        let map = self.inner.read();
        map.is_empty()
    }

    #[must_use]
    pub fn list_models(&self) -> Vec<Arc<str>> {
        let map = self.inner.read();
        let mut result = Vec::with_capacity(map.len());
        result.extend(map.keys().cloned());
        result
    }
}

#[derive(Clone, Debug)]
pub struct ModelSchema {
    name: Arc<str>,
    fields: Vec<FieldSchema>,
    field_index: Arc<RwLock<HashMap<Arc<str>, usize>>>,
}

impl PartialEq for ModelSchema {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.fields == other.fields
    }
}

impl ModelSchema {
    #[must_use]
    pub fn new(name: impl Into<Arc<str>>, fields: Vec<FieldSchema>) -> Self {
        Self {
            name: name.into(),
            fields,
            field_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn fields(&self) -> &[FieldSchema] {
        &self.fields
    }

    #[must_use]
    pub fn fields_count(&self) -> usize {
        self.fields.len()
    }

    #[must_use]
    pub fn find_field(&self, name: &str) -> Option<&FieldSchema> {
        {
            let index = self.field_index.read();
            if let Some(&idx) = index.get(name) {
                return self.fields.get(idx);
            }
        }

        let mut index = self.field_index.write();
        if index.is_empty() && !self.fields.is_empty() {
            *index = Self::build_field_index(&self.fields);
            if let Some(&idx) = index.get(name) {
                return self.fields.get(idx);
            }
        }

        None
    }

    #[must_use]
    pub fn field_index(&self, name: &str) -> Option<usize> {
        {
            let index = self.field_index.read();
            if let Some(&idx) = index.get(name) {
                return Some(idx);
            }
        }

        let mut index = self.field_index.write();
        if index.is_empty() && !self.fields.is_empty() {
            *index = Self::build_field_index(&self.fields);
            if let Some(&idx) = index.get(name) {
                return Some(idx);
            }
        }

        None
    }

    fn build_field_index(fields: &[FieldSchema]) -> HashMap<Arc<str>, usize> {
        let mut index = HashMap::with_capacity(fields.len());
        for (i, field) in fields.iter().enumerate() {
            index.insert(field.name.clone(), i);
        }
        index
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldSchema {
    name: Arc<str>,
    field_type: FieldType,
    nullable: bool,
}

impl FieldSchema {
    #[must_use]
    pub fn new(name: impl Into<Arc<str>>, field_type: FieldType, nullable: bool) -> Self {
        Self {
            name: name.into(),
            field_type,
            nullable,
        }
    }

    #[must_use]
    pub const fn is_nullable(&self) -> bool {
        self.nullable
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn field_type(&self) -> &FieldType {
        &self.field_type
    }
}
