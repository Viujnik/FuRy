use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema, SchemaRegistry};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(case: &str) -> Self {
        let path = std::env::temp_dir().join(format!("fury-cli-{case}-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn generate(root: &TestDirectory) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fury"))
        .args(["generate", "-t", "ts", "-s", "all", "-d", "output"])
        .current_dir(&root.0)
        .output()
        .unwrap()
}

#[test]
fn output_conflict_leaves_existing_files_intact() {
    let root = TestDirectory::new("output");
    let message = ModelSchema::new(
        "Message",
        vec![FieldSchema::new("id", FieldType::Int64, false)],
    );
    let summary = ModelSchema::new("Summary", vec![]);
    let cache = SchemaRegistry::encode_schemas([&message, &summary]).unwrap();
    fs::write(root.0.join(".fury_registry.bin"), &cache).unwrap();

    let output = root.0.join("output");
    let cache_path = output.join(".fury_registry.bin");
    let message_path = output.join("message.ts");
    let summary_path = output.join("summary.ts");
    fs::create_dir_all(&cache_path).unwrap();
    fs::write(&message_path, "// previous output\n").unwrap();

    let result = generate(&root);
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(!result.status.success());
    assert!(stderr.contains("generated output path is not a regular file"));
    assert_eq!(
        fs::read_to_string(&message_path).unwrap(),
        "// previous output\n"
    );
    assert!(!summary_path.exists());
    assert!(cache_path.is_dir());

    fs::remove_dir(&cache_path).unwrap();
    let result = generate(&root);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&cache_path).unwrap(), cache);
    assert!(
        fs::read_to_string(&message_path)
            .unwrap()
            .contains("class Message")
    );
    assert!(
        fs::read_to_string(&summary_path)
            .unwrap()
            .contains("class Summary")
    );
    assert_eq!(fs::read_dir(output).unwrap().count(), 3);
}

#[test]
fn a_directory_is_not_a_python_schema_source() {
    let root = TestDirectory::new("source");
    fs::create_dir(root.0.join("models.py")).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_fury"))
        .args([
            "generate",
            "-f",
            "python",
            "-t",
            "ts",
            "-s",
            "models.py",
            "-d",
            "output",
        ])
        .env("FURY_PYTHON", root.0.join("test-python-does-not-exist"))
        .current_dir(&root.0)
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("Python schema source must be a regular file")
    );
    assert!(!root.0.join("output").exists());
}
