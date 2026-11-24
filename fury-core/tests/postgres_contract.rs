#![cfg(feature = "db")]

use fury_core::{
    db::{
        executor::{QueryExecutor, SqlValue},
        pool::DatabasePool,
    },
    encoder::value::ValueType,
    error::FuryError,
    schema::registry::{FieldSchema, FieldType, ModelSchema, SchemaRegistry},
};
use std::sync::Arc;

fn field(ty: FieldType, nullable: bool) -> Arc<ModelSchema> {
    Arc::new(ModelSchema::new(
        "Result",
        vec![FieldSchema::new("n", ty, nullable)],
    ))
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server"]
async fn bounded_pages_preserve_rows_and_fail_atomically() {
    let db = database().await;
    let schema = field(FieldType::Int64, false);
    let query = "SELECT n FROM generate_series(-1::bigint, $1) AS n ORDER BY n";
    let args = vec![SqlValue::Int64(2)];
    let rows = db.execute_raw(query, args.clone()).await.unwrap();
    let mut expected = Vec::new();
    for row in rows {
        let mut record = db.row_to_buffer(&row, &schema).unwrap();
        record.finish().unwrap();
        expected.extend_from_slice(record.as_bytes().unwrap());
    }
    let page = db
        .fetch_page(query, args.clone(), &schema, 4)
        .await
        .unwrap();
    assert_eq!(page, expected);
    {
        assert!(db
            .fetch_page(query, args.clone(), &schema, 3)
            .await
            .is_err());
        for invalid in [
            "SELECT 1::bigint AS n, 2::bigint AS n",
            "SELECT NULL::bigint AS n",
            "SELECT '3'::text AS n",
        ] {
            assert!(db.fetch_page(invalid, vec![], &schema, 1).await.is_err());
        }
        assert!(db.fetch_page(query, args.clone(), &schema, 4).await.is_ok());
    }
}

async fn database() -> QueryExecutor {
    let dsn = std::env::var("FURY_TEST_DATABASE_URL").expect("disposable database required");
    QueryExecutor::new(
        DatabasePool::connect(&dsn).await.unwrap(),
        SchemaRegistry::default(),
    )
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server"]
async fn checked_integer_ranges() {
    let db = database().await;
    for (ty, min, max, sql) in [
        (FieldType::Int8, -128, 127, "SELECT $1::int4::int2 AS n"),
        (FieldType::UInt8, 0, 255, "SELECT $1::int4::int2 AS n"),
        (FieldType::UInt16, 0, 65535, "SELECT $1::int4 AS n"),
    ] {
        let schema = field(ty, false);
        for n in [min - 1, min, max, max + 1] {
            let rows = db.execute_raw(sql, vec![SqlValue::Int32(n)]).await.unwrap();
            let record = db.row_to_buffer(&rows[0], &schema);
            if (min..=max).contains(&n) {
                let value = match record.unwrap().get_field_value("n").unwrap() {
                    ValueType::Int8(v) => i32::from(v),
                    ValueType::UInt8(v) => i32::from(v),
                    ValueType::UInt16(v) => i32::from(v),
                    other => panic!("wrong value type: {other:?}"),
                };
                assert_eq!(value, n);
            } else {
                assert!(matches!(record, Err(FuryError::TypeMismatch { .. })));
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server"]
async fn mapped_column_must_be_unambiguous() {
    let db = database().await;
    let schema = field(FieldType::Int32, false);
    let duplicate = db
        .execute_raw("SELECT 1 AS n, 2 AS n", vec![])
        .await
        .unwrap();
    assert!(db.row_to_buffer(&duplicate[0], &schema).is_err());
    let extra = db
        .execute_raw("SELECT 42 AS n, 1 AS x, 2 AS x", vec![])
        .await
        .unwrap();
    assert_eq!(
        db.row_to_buffer(&extra[0], &schema)
            .unwrap()
            .get_field_value("n"),
        Some(ValueType::Int32(42))
    );
}

#[tokio::test]
#[ignore = "requires a disposable PostgreSQL server"]
async fn null_type_and_bound_bigint() {
    let db = database().await;
    let nullable = field(FieldType::Int64, true);
    let required = field(FieldType::Int64, false);
    let wrong = db
        .execute_raw("SELECT NULL::text AS n", vec![])
        .await
        .unwrap();
    assert!(db.row_to_buffer(&wrong[0], &nullable).is_err());
    let rows = db
        .execute_raw("SELECT NULL::int8 AS n", vec![])
        .await
        .unwrap();
    assert_eq!(
        db.row_to_buffer(&rows[0], &nullable)
            .unwrap()
            .get_field_value("n"),
        Some(ValueType::None)
    );
    assert!(db.row_to_buffer(&rows[0], &required).is_err());
    let rows = db
        .execute_raw("SELECT $1::int8 AS n", vec![SqlValue::Int64(i64::MAX)])
        .await
        .unwrap();
    assert_eq!(
        db.row_to_buffer(&rows[0], &required)
            .unwrap()
            .get_field_value("n"),
        Some(ValueType::Int64(i64::MAX))
    );
}
