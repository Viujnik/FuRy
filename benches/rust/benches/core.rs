//! Core codec benchmarks.
//!
//! Three questions, three bench groups:
//!
//!   1. `encode` — how long does it take to turn 12 fields into bytes?
//!      Compared against serde_json and rmp-serde.
//!
//!   2. `decode_construct` — how long does it take to consume a payload and
//!      build an offset map (fury) or a value graph (serde_json, rmp-serde)?
//!      This is the closest analog to `JSON.parse`.
//!
//!   3. `decode_access3` — same as above plus reading three fields. This is
//!      the closest analog to "render a row in a table".
//!
//! We deliberately do NOT bench the `set_field` path with the same buffer
//! reused; BinaryRecord is a builder, not a slab. Each iteration constructs
//! a fresh one, matching the actual usage in fury-py.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use fury_core::encoder::buffer::BinaryRecord;
use fury_core::encoder::value::ValueType;
use fury_core::schema::field::FieldType;
use fury_core::schema::registry::{FieldSchema, ModelSchema};
use std::sync::Arc;

/// The same 12-field shape the DB benchmark uses.
/// Kept in sync manually — the schema is small and this is the only place
/// in Rust that needs to match. If it drifts, the bench is still valid in
/// isolation; only the cross-language comparison would be affected.
fn user_schema() -> Arc<ModelSchema> {
    Arc::new(ModelSchema::new(
        "User",
        vec![
            FieldSchema::new("id", FieldType::Int64, false),
            FieldSchema::new("username", FieldType::String, false),
            FieldSchema::new("email", FieldType::String, false),
            FieldSchema::new("display_name", FieldType::String, true),
            FieldSchema::new("is_active", FieldType::Bool, false),
            FieldSchema::new("reputation", FieldType::Int64, false),
            FieldSchema::new("flags", FieldType::Int64, false),
            FieldSchema::new("external_id", FieldType::Uuid, false),
            FieldSchema::new("created_at", FieldType::DateTime, false),
            FieldSchema::new("updated_at", FieldType::DateTime, false),
            FieldSchema::new("bio", FieldType::String, true),
            FieldSchema::new("avatar", FieldType::Bytes, true),
        ],
    ))
}

/// A representative row: some small fields, one medium (bio ~1.5 KB),
/// one large (avatar 8 KB). Sizes match seed.sql.
fn sample_values() -> Vec<(&'static str, ValueType)> {
    use chrono::NaiveDate;
    use uuid::Uuid;

    let bio = "lorem ipsum dolor sit amet ".repeat(64); // ~1.6 KB
    let avatar = vec![0xABu8; 8192]; // 8 KB

    vec![
        ("id", ValueType::Int64(42)),
        ("username", ValueType::String("user_42".into())),
        ("email", ValueType::String("user_42@example.com".into())),
        ("display_name", ValueType::String("User 42".into())),
        ("is_active", ValueType::Bool(true)),
        ("reputation", ValueType::Int64(7919)),
        ("flags", ValueType::Int64(104729)),
        ("external_id", ValueType::Uuid(Uuid::nil())),
        (
            "created_at",
            ValueType::DateTime(
                NaiveDate::from_ymd_opt(2024, 6, 1)
                    .unwrap()
                    .and_hms_opt(12, 0, 0)
                    .unwrap(),
            ),
        ),
        (
            "updated_at",
            ValueType::DateTime(
                NaiveDate::from_ymd_opt(2024, 6, 2)
                    .unwrap()
                    .and_hms_opt(12, 0, 0)
                    .unwrap(),
            ),
        ),
        ("bio", ValueType::String(bio)),
        ("avatar", ValueType::Bytes(avatar)),
    ]
}

/// serde_json needs a shaped struct; the alternative is a HashMap which
/// adds allocation noise to the measurement.
#[derive(serde::Serialize, serde::Deserialize)]
struct UserRow {
    id: i64,
    username: String,
    email: String,
    display_name: Option<String>,
    is_active: bool,
    reputation: i64,
    flags: i64,
    external_id: uuid::Uuid,
    created_at: chrono::NaiveDateTime,
    updated_at: chrono::NaiveDateTime,
    bio: Option<String>,
    avatar: Option<Vec<u8>>,
}

fn as_serde_row() -> UserRow {
    use chrono::NaiveDate;
    UserRow {
        id: 42,
        username: "user_42".into(),
        email: "user_42@example.com".into(),
        display_name: Some("User 42".into()),
        is_active: true,
        reputation: 7919,
        flags: 104729,
        external_id: uuid::Uuid::nil(),
        created_at: NaiveDate::from_ymd_opt(2024, 6, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        updated_at: NaiveDate::from_ymd_opt(2024, 6, 2)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        bio: Some("lorem ipsum dolor sit amet ".repeat(64)),
        avatar: Some(vec![0xABu8; 8192]),
    }
}

fn build_fury(schema: &Arc<ModelSchema>) -> Vec<u8> {
    let mut rec = BinaryRecord::new(Arc::clone(schema));
    for (name, value) in sample_values() {
        rec.set_field(name, value).unwrap();
    }
    rec.finish().unwrap().to_vec()
}

// ────────────────────────────────────────────────────────────────────────
// Groups
// ────────────────────────────────────────────────────────────────────────

fn encode(c: &mut Criterion) {
    let schema = user_schema();
    let row = as_serde_row();

    let mut group = c.benchmark_group("encode");

    group.bench_function("fury_binary_record", |b| {
        b.iter(|| {
            let mut rec = BinaryRecord::new(Arc::clone(&schema));
            for (name, value) in sample_values() {
                rec.set_field(black_box(name), black_box(value)).unwrap();
            }
            black_box(rec.finish().unwrap().len())
        })
    });

    group.bench_function("serde_json", |b| {
        b.iter(|| black_box(serde_json::to_vec(&row).unwrap().len()))
    });

    group.bench_function("rmp_serde", |b| {
        b.iter(|| black_box(rmp_serde::to_vec(&row).unwrap().len()))
    });

    group.finish();
}

fn decode_construct(c: &mut Criterion) {
    let schema = user_schema();
    let fury_bytes = build_fury(&schema);
    let json_bytes = serde_json::to_vec(&as_serde_row()).unwrap();
    let rmp_bytes = rmp_serde::to_vec(&as_serde_row()).unwrap();

    let mut group = c.benchmark_group("decode_construct");

    group.bench_function("fury_from_raw_bytes", |b| {
        b.iter(|| {
            let rec = BinaryRecord::from_raw_bytes(black_box(fury_bytes.clone()), &schema).unwrap();
            black_box(rec.len())
        })
    });

    group.bench_function("serde_json_from_slice", |b| {
        b.iter(|| {
            let r: UserRow = serde_json::from_slice(black_box(&json_bytes)).unwrap();
            black_box(r.id)
        })
    });

    group.bench_function("rmp_serde_from_slice", |b| {
        b.iter(|| {
            let r: UserRow = rmp_serde::from_slice(black_box(&rmp_bytes)).unwrap();
            black_box(r.id)
        })
    });

    group.finish();
}

fn decode_access3(c: &mut Criterion) {
    let schema = user_schema();
    let fury_bytes = build_fury(&schema);
    let json_bytes = serde_json::to_vec(&as_serde_row()).unwrap();
    let rmp_bytes = rmp_serde::to_vec(&as_serde_row()).unwrap();

    let mut group = c.benchmark_group("decode_access3");

    group.bench_function("fury_decode_three_fields", |b| {
        b.iter(|| {
            let rec = BinaryRecord::from_raw_bytes(black_box(fury_bytes.clone()), &schema).unwrap();
            let id = rec.get_field_value("id").unwrap();
            let name = rec.get_field_value("username").unwrap();
            let active = rec.get_field_value("is_active").unwrap();
            black_box((id, name, active))
        })
    });

    group.bench_function("serde_json_decode_three_fields", |b| {
        b.iter(|| {
            let r: UserRow = serde_json::from_slice(black_box(&json_bytes)).unwrap();
            black_box((r.id, r.username, r.is_active))
        })
    });

    group.bench_function("rmp_serde_decode_three_fields", |b| {
        b.iter(|| {
            let r: UserRow = rmp_serde::from_slice(black_box(&rmp_bytes)).unwrap();
            black_box((r.id, r.username, r.is_active))
        })
    });

    group.finish();
}

/// Payload size is not a benchmark, but it's a number people ask for and
/// it belongs next to the timings, not in a separate doc.
fn payload_size(c: &mut Criterion) {
    let schema = user_schema();
    let fury_bytes = build_fury(&schema);
    let json_bytes = serde_json::to_vec(&as_serde_row()).unwrap();
    let rmp_bytes = rmp_serde::to_vec(&as_serde_row()).unwrap();

    let mut group = c.benchmark_group("payload_size_bytes");
    group.bench_with_input(
        BenchmarkId::new("fury", fury_bytes.len()),
        &fury_bytes,
        |b, v| b.iter(|| black_box(v.len())),
    );
    group.bench_with_input(
        BenchmarkId::new("serde_json", json_bytes.len()),
        &json_bytes,
        |b, v| b.iter(|| black_box(v.len())),
    );
    group.bench_with_input(
        BenchmarkId::new("rmp_serde", rmp_bytes.len()),
        &rmp_bytes,
        |b, v| b.iter(|| black_box(v.len())),
    );
    group.finish();
}

criterion_group!(
    benches,
    encode,
    decode_construct,
    decode_access3,
    payload_size
);
criterion_main!(benches);
