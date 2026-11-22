use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema, SchemaRegistry};
use std::cell::Cell;

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_directory_is_not_a_registry_cache() {
    let executable = std::env::current_exe().unwrap();
    let directory = executable.parent().unwrap();
    let error = SchemaRegistry::load_from_file(directory).err().unwrap();
    assert!(error.to_string().contains("must be a regular file"));
}

#[test]
fn encoding_checks_the_model_count_while_collecting() {
    let schema = ModelSchema::new("Reading", vec![]);
    let observed = Cell::new(0);
    let models = std::iter::repeat_n(&schema, 5_000).inspect(|_| {
        observed.set(observed.get() + 1);
    });

    let result = SchemaRegistry::encode_schemas(models);

    assert!(result.unwrap_err().to_string().contains("too many schemas"));
    assert_eq!(observed.get(), 4_097);
}

#[test]
fn a_failed_cache_import_preserves_the_existing_schema() {
    let original = ModelSchema::new(
        "Reading",
        vec![FieldSchema::new("value", FieldType::Int64, false)],
    );
    let revised = ModelSchema::new(
        "Reading",
        vec![FieldSchema::new("label", FieldType::String, true)],
    );
    let registry = SchemaRegistry::default();
    registry
        .import_cache(&SchemaRegistry::encode_schemas([&original]).unwrap())
        .unwrap();
    let encoding = SchemaRegistry::encode_schemas([&revised]).unwrap();
    let incomplete = &encoding[..encoding.len() - 1];

    assert!(registry.import_cache(incomplete).is_err());
    assert_eq!(registry.get("Reading").as_deref(), Some(&original));

    registry.import_cache(&encoding).unwrap();
    assert_eq!(registry.get("Reading").as_deref(), Some(&revised));
    assert!(registry
        .get("Reading")
        .unwrap()
        .find_field("label")
        .is_some());
}
