use fury_core::encoder::{buffer::BinaryRecord, value::ValueType, DecodeLimits};
use fury_core::schema::registry::{FieldSchema, FieldType, ModelSchema};
use std::sync::Arc;

const ID42: &[u8] = include_bytes!("fixtures/v2-id42.fury");
const NESTED: &[u8] = include_bytes!("fixtures/v2-nested.fury");

fn schema(fields: &[(&str, FieldType, bool)]) -> Arc<ModelSchema> {
    Arc::new(ModelSchema::new(
        "WireTest",
        fields
            .iter()
            .map(|(name, ty, nullable)| FieldSchema::new(*name, ty.clone(), *nullable))
            .collect(),
    ))
}

fn id_schema() -> Arc<ModelSchema> {
    schema(&[("id", FieldType::Int64, false)])
}

// Independent encoding of the frame envelope. Fixtures contain manually specified payloads.
fn raw_frame(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 0, b'F', 2];
    for (name, value) in entries {
        bytes.push(name.len().try_into().unwrap());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_le_bytes());
        bytes.extend_from_slice(value);
    }
    let size = bytes.len() as u32;
    bytes[..4].copy_from_slice(&size.to_le_bytes());
    bytes
}

#[test]
fn golden_i64_agrees_in_both_directions() {
    let s = id_schema();
    let loaded = BinaryRecord::from_raw_bytes(ID42.to_vec(), &s).unwrap();
    assert_eq!(loaded.get_field_value("id"), Some(ValueType::Int64(42)));
    assert_eq!(loaded.get_field_offset("id"), Some((13, 9)));
    let mut builder = BinaryRecord::new(s);
    builder.set_field("id", ValueType::Int64(42)).unwrap();
    assert_eq!(builder.finish().unwrap(), b"\x0b\0\0\0F\x03\x02id\x01\x56");
    assert_eq!(loaded.as_bytes().unwrap(), ID42);
}

#[test]
fn golden_nested_types_keep_boundaries_and_explicit_null() {
    let collection = FieldType::List(Box::new(FieldType::Map(
        Box::new(FieldType::String),
        Box::new(FieldType::Int16),
    )));
    let s = schema(&[
        ("lo", FieldType::Int16, false),
        ("hi", FieldType::UInt64, false),
        ("note", FieldType::String, true),
        ("items", collection, false),
    ]);
    let values = [
        ("lo", ValueType::Int16(i16::MIN)),
        ("hi", ValueType::UInt64(1 << 63 | 42)),
        ("note", ValueType::None),
        (
            "items",
            ValueType::List(vec![ValueType::Map(vec![(
                ValueType::String("ёж".into()),
                ValueType::Int16(-17),
            )])]),
        ),
    ];
    let parsed = BinaryRecord::from_raw_bytes(NESTED.to_vec(), &s).unwrap();
    let mut builder = BinaryRecord::new(s.clone());
    for (name, value) in values.clone() {
        assert_eq!(parsed.get_field_value(name), Some(value.clone()));
        builder.set_field(name, value).unwrap();
    }
    let written = builder.finish().unwrap();
    assert_eq!(written[5], 3);
    let reread = BinaryRecord::from_raw_bytes(written.to_vec(), &s).unwrap();
    for (name, value) in values {
        assert_eq!(reread.get_field_value(name), Some(value));
    }
}

#[test]
fn rejects_truncated_frames_and_unsupported_version() {
    let s = id_schema();
    for i in 0..ID42.len() {
        assert!(
            BinaryRecord::from_raw_bytes(ID42[..i].to_vec(), &s).is_err(),
            "prefix {i}"
        );
    }
    let mut different_version = ID42.to_vec();
    different_version[5] = 1;
    assert!(BinaryRecord::from_raw_bytes(different_version, &s).is_err());
    let mut trailing = ID42.to_vec();
    trailing.push(0);
    assert!(BinaryRecord::from_raw_bytes(trailing, &s).is_err());
}

#[test]
fn field_payload_is_exact_while_standalone_decoder_allows_a_prefix() {
    let s = schema(&[("ok", FieldType::Bool, false)]);
    let suffix = [1, 1, 0];
    assert_eq!(
        ValueType::from_bytes(&suffix, &FieldType::Bool),
        Some(ValueType::Bool(true))
    );
    assert_eq!(
        ValueType::from_bytes_with_rest(&suffix, &FieldType::Bool, 0),
        Some((ValueType::Bool(true), &suffix[2..]))
    );
    for invalid in [&[1, 2][..], &[0], &suffix, &[1], &[]] {
        assert!(BinaryRecord::from_raw_bytes(raw_frame(&[("ok", invalid)]), &s).is_err());
    }
}

#[test]
fn missing_and_nullable_fields_are_distinct() {
    let s = schema(&[
        ("name", FieldType::String, true),
        ("id", FieldType::Int64, false),
    ]);
    let parsed = BinaryRecord::from_raw_bytes(raw_frame(&[("name", &[0])]), &s).unwrap();
    assert_eq!(parsed.get_field_value("name"), Some(ValueType::None));
    assert_eq!(parsed.get_field_value("id"), None);
    assert_eq!(parsed.get_field_offset("id"), None);
}

#[test]
fn legacy_duplicates_last_win_and_are_compacted_on_edit() {
    let s = id_schema();
    let first = [1, 41, 0, 0, 0, 0, 0, 0, 0];
    let last = [1, 42, 0, 0, 0, 0, 0, 0, 0];
    let bytes = raw_frame(&[("id", &first), ("id", &last)]);
    let mut parsed = BinaryRecord::from_raw_bytes(bytes.clone(), &s).unwrap();
    assert_eq!(parsed.as_bytes().unwrap(), bytes.as_slice());
    assert_eq!(parsed.get_field_value("id"), Some(ValueType::Int64(42)));
    parsed.update_field("id", ValueType::Int64(43)).unwrap();
    assert_eq!(parsed.finish().unwrap(), b"\x0b\0\0\0F\x03\x02id\x01\x58");
}

#[test]
fn opaque_field_survives_new_and_overwritten_known_entries() {
    let old = schema(&[
        ("id", FieldType::Int64, false),
        ("extra", FieldType::Bool, false),
    ]);
    let new = schema(&[
        ("id", FieldType::Int64, false),
        ("extra", FieldType::Bool, false),
        ("title", FieldType::String, false),
    ]);
    let utf8 = "Фурия 🚀".as_bytes();
    let mut title = vec![1];
    title.extend_from_slice(&(utf8.len() as u32).to_le_bytes());
    title.extend_from_slice(utf8);
    let mut record =
        BinaryRecord::from_raw_bytes(raw_frame(&[("id", &ID42[13..]), ("title", &title)]), &old)
            .unwrap();
    assert_eq!(record.get_field_bytes("title"), None);
    record.update_field("extra", ValueType::Bool(true)).unwrap();
    record.set_field("id", ValueType::Int64(99)).unwrap();
    let upgraded = BinaryRecord::from_raw_bytes(record.finish().unwrap().to_vec(), &new).unwrap();
    assert_eq!(upgraded.get_field_bytes("title"), Some(title.as_slice()));
    assert_eq!(
        upgraded.get_field_value("extra"),
        Some(ValueType::Bool(true))
    );
    assert_eq!(upgraded.get_field_value("id"), Some(ValueType::Int64(99)));
}

#[test]
fn failed_update_preserves_a_finished_record() {
    let s = id_schema();
    let mut parsed = BinaryRecord::from_raw_bytes(ID42.to_vec(), &s).unwrap();
    assert!(parsed.update_field("id", ValueType::Bool(true)).is_err());
    assert!(parsed.update_field("id", ValueType::None).is_err());
    assert!(parsed.update_field("unknown", ValueType::Int64(0)).is_err());
    assert!(parsed.is_finished());
    assert_eq!(parsed.as_bytes().unwrap(), ID42);
}

#[test]
fn frame_budgets_count_unknown_and_superseded_entries() {
    let s = id_schema();
    let repeated = raw_frame(&[("id", &ID42[13..]), ("id", &ID42[13..])]);
    let with_unknown = raw_frame(&[("id", &ID42[13..]), ("future", &[0])]);
    let mut limits = DecodeLimits {
        max_fields: 1,
        ..DecodeLimits::default()
    };
    for frame in [&repeated, &with_unknown] {
        assert!(BinaryRecord::from_raw_bytes_with_limits(frame.clone(), &s, limits).is_err());
    }
    limits.max_fields = 2;
    limits.max_total_values = 1;
    assert!(BinaryRecord::from_raw_bytes_with_limits(repeated, &s, limits).is_err());
    assert!(BinaryRecord::from_raw_bytes_with_limits(with_unknown, &s, limits).is_ok());
}

#[test]
fn bounded_compaction_preserves_a_sparse_known_field() {
    let s = schema(&[
        ("unused", FieldType::String, false),
        ("id", FieldType::Int64, false),
    ]);
    let limits = DecodeLimits {
        max_fields: 1,
        max_frame_bytes: ID42.len(),
        ..DecodeLimits::default()
    };
    let mut record = BinaryRecord::from_raw_bytes_with_limits(ID42.to_vec(), &s, limits).unwrap();
    for n in 42..64 {
        record.update_field("id", ValueType::Int64(n)).unwrap();
        assert!(record.len() <= ID42.len());
        assert!(record
            .set_field("unused", ValueType::String("x".into()))
            .is_err());
    }
    assert_eq!(record.get_field_value("id"), Some(ValueType::Int64(63)));
    assert_eq!(record.finish().unwrap(), b"\x0b\0\0\0F\x03\x02id\x01\x80");
}

#[test]
fn validators_agree_at_the_nesting_boundary() {
    let mut ty = FieldType::Bool;
    let mut item = ValueType::Bool(true);
    for _ in 0..17 {
        ty = FieldType::List(Box::new(ty));
        item = ValueType::List(vec![item]);
    }
    let s = schema(&[("items", ty.clone(), false)]);
    let raw = item.to_bytes();
    assert!(BinaryRecord::new(s.clone())
        .set_field("items", item.clone())
        .is_err());
    assert!(BinaryRecord::from_raw_bytes(raw_frame(&[("items", &raw)]), &s).is_err());
    let (FieldType::List(inner), ValueType::List(mut items)) = (ty, item) else {
        unreachable!()
    };
    let s = schema(&[("items", *inner, false)]);
    let item = items.pop().unwrap();
    let mut builder = BinaryRecord::new(s.clone());
    builder.set_field("items", item.clone()).unwrap();
    let parsed = BinaryRecord::from_raw_bytes(builder.finish().unwrap().to_vec(), &s).unwrap();
    assert_eq!(parsed.get_field_value("items"), Some(item));
}

#[test]
fn datetime_write_rejects_loss_of_temporal_precision() {
    let s = schema(&[("date", FieldType::DateTime, false)]);
    let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 18).unwrap();
    let mut record = BinaryRecord::new(s.clone());
    let microseconds = day.and_hms_nano_opt(2, 3, 4, 123_000).unwrap();
    let nanoseconds = day.and_hms_nano_opt(2, 3, 4, 123_456).unwrap();
    assert!(record
        .set_field("date", ValueType::DateTime(nanoseconds))
        .is_err());
    record
        .set_field("date", ValueType::DateTime(microseconds))
        .unwrap();
    let parsed = BinaryRecord::from_raw_bytes(record.finish().unwrap().to_vec(), &s).unwrap();
    assert_eq!(
        parsed.get_field_value("date"),
        Some(ValueType::DateTime(microseconds))
    );
}

#[test]
fn deterministic_wire_smoke() {
    let count = std::env::var("FURY_FUZZ_CASES")
        .map(|value| {
            value
                .parse::<u32>()
                .expect("FURY_FUZZ_CASES must be an integer")
        })
        .unwrap_or(320);
    assert!((1..=1_000_000).contains(&count));
    let s = schema(&[
        ("id", FieldType::Int64, false),
        ("name", FieldType::String, true),
        ("flags", FieldType::List(Box::new(FieldType::Bool)), false),
    ]);
    let mut state = 0x82AA_14DD_B55B_1C23_u64;
    for i in 0..count {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let input = [
            ("id", ValueType::Int64(state as i64)),
            (
                "name",
                if state.is_multiple_of(3) {
                    ValueType::None
                } else {
                    ValueType::String(format!("строка {i}"))
                },
            ),
            (
                "flags",
                ValueType::List(
                    (0..(state % 8))
                        .map(|j| ValueType::Bool(state & (1 << (j + 24)) != 0))
                        .collect(),
                ),
            ),
        ];
        let mut writer = BinaryRecord::new(s.clone());
        for (name, expected) in &input {
            writer.set_field(name, expected.clone()).unwrap();
        }
        let encoded = writer.finish().unwrap().to_vec();
        let mut reader = BinaryRecord::from_raw_bytes(encoded.clone(), &s).unwrap();
        for (name, expected) in &input {
            assert_eq!(reader.get_field_value(name).as_ref(), Some(expected));
        }
        let truncation = state as usize % encoded.len();
        assert!(BinaryRecord::from_raw_bytes(encoded[..truncation].to_vec(), &s).is_err());
        let mut changed = encoded;
        let index = state.rotate_left(17) as usize % changed.len();
        changed[index] ^= 1 << (state % 8);
        if let Ok(mut candidate) = BinaryRecord::from_raw_bytes(changed, &s) {
            candidate
                .update_field("id", ValueType::Int64(i64::from(i)))
                .unwrap();
            let reparsed =
                BinaryRecord::from_raw_bytes(candidate.finish().unwrap().to_vec(), &s).unwrap();
            assert_eq!(
                reparsed.get_field_value("id"),
                Some(ValueType::Int64(i64::from(i)))
            );
        }
        reader
            .update_field("id", ValueType::Int64(i64::from(i)))
            .unwrap();
        let edited = BinaryRecord::from_raw_bytes(reader.finish().unwrap().to_vec(), &s).unwrap();
        for (name, expected) in &input[1..] {
            assert_eq!(edited.get_field_value(name).as_ref(), Some(expected));
        }
    }
}

#[test]
fn adaptive_integers_and_blobs_have_exact_bounded_sizes() {
    for n in [
        i64::MIN,
        -2147483649,
        -32769,
        -32768,
        -127,
        -126,
        -1,
        0,
        42,
        125,
        126,
        32767,
        32768,
        2147483647,
        2147483648,
        i64::MAX,
    ] {
        let value = ValueType::Int64(n);
        let bytes = value.to_bytes();
        assert_eq!(value.size(), bytes.len());
        assert!(bytes.len() <= 9);
        assert_eq!(
            ValueType::from_bytes_with_limits(&bytes, &FieldType::Int64, &DecodeLimits::default()),
            Some(value)
        );
        for end in 0..bytes.len() {
            assert!(ValueType::from_bytes(&bytes[..end], &FieldType::Int64).is_none());
        }
    }
    for n in [0, 251, 252, 65535, 65536, u32::MAX as u64, u64::MAX] {
        let value = ValueType::UInt64(n);
        assert_eq!(
            ValueType::from_bytes(&value.to_bytes(), &FieldType::UInt64),
            Some(value)
        );
    }
    let s = schema(&[("v", FieldType::Bytes, false)]);
    for len in [0, 1, 252, 253, 254, 255, 16384] {
        let mut r = BinaryRecord::new(s.clone());
        let value = ValueType::Bytes(vec![42; len]);
        assert_eq!(value.size(), value.to_bytes().len());
        r.set_field("v", value.clone()).unwrap();
        let frame = r.finish().unwrap();
        let parsed = BinaryRecord::from_raw_bytes(frame.to_vec(), &s).unwrap();
        assert_eq!(parsed.get_field_value("v"), Some(value));
    }
}

#[test]
fn page_writer_and_record_writer_share_bytes_and_failed_rows_roll_back() {
    use fury_core::encoder::frame::append_row;
    let s = id_schema();
    let mut page = Vec::new();
    let limits = DecodeLimits {
        max_frame_bytes: 11,
        ..DecodeLimits::default()
    };
    append_row(&mut page, &s, &limits, |_| Ok(ValueType::Int64(42))).unwrap();
    assert_eq!(page, b"\x0b\0\0\0F\x03\x02id\x01\x56");
    assert!(append_row(&mut page, &s, &limits, |_| Ok(ValueType::Int64(42))).is_err());
    let before = page.clone();
    assert!(append_row(&mut page, &s, &DecodeLimits::default(), |_| Ok(
        ValueType::None
    ))
    .is_err());
    assert_eq!(page, before);
    assert!(BinaryRecord::from_raw_bytes(raw_frame(&[("id", &[0x56])]), &s).is_err());
}

#[test]
fn replacing_a_middle_field_retains_known_and_opaque_offsets() {
    let full = schema(&[
        ("id", FieldType::Int64, false),
        ("label", FieldType::String, false),
        ("extra", FieldType::String, false),
        ("flag", FieldType::Bool, false),
    ]);
    let subset = schema(&[
        ("id", FieldType::Int64, false),
        ("label", FieldType::String, false),
        ("flag", FieldType::Bool, false),
    ]);
    let mut initial = BinaryRecord::new(full.clone());
    initial.set_field("id", ValueType::Int64(42)).unwrap();
    initial
        .set_field("label", ValueType::String("a".repeat(300)))
        .unwrap();
    initial
        .set_field("extra", ValueType::String("opaque".into()))
        .unwrap();
    initial.set_field("flag", ValueType::Bool(true)).unwrap();
    let mut bytes = initial.finish().unwrap().to_vec();
    for label in ["x".to_owned(), "z".repeat(400)] {
        let mut edit = BinaryRecord::from_raw_bytes(bytes, &subset).unwrap();
        edit.update_field("label", ValueType::String(label.clone()))
            .unwrap();
        assert_eq!(edit.get_field_value("flag"), Some(ValueType::Bool(true)));
        bytes = edit.finish().unwrap().to_vec();
        let record = BinaryRecord::from_raw_bytes(bytes.clone(), &full).unwrap();
        assert_eq!(record.get_field_value("id"), Some(ValueType::Int64(42)));
        assert_eq!(
            record.get_field_value("extra"),
            Some(ValueType::String("opaque".into()))
        );
        assert_eq!(
            record.get_field_value("label"),
            Some(ValueType::String(label))
        );
    }
}
