use crate::db::pool::DatabasePool;
use crate::encoder::buffer::FlatBufferBuilder;
use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::{FieldType, ModelSchema, SchemaRegistry};
use sqlx::any::{AnyArguments, AnyRow};
use sqlx::{Arguments, Row, query_with};
use std::sync::Arc;

pub struct QueryExecutor {
    pool: DatabasePool,
    schema_registry: SchemaRegistry,
}

impl QueryExecutor {
    #[must_use]
    pub fn new(pool: DatabasePool, schema_registry: SchemaRegistry) -> Self {
        Self {
            pool,
            schema_registry,
        }
    }

    pub async fn execute_raw(&self, query: &str, args: Vec<SqlValue>) -> Result<Vec<AnyRow>> {
        let mut query_args = AnyArguments::default();

        for arg in args {
            arg.bind_to(&mut query_args).map_err(|e| {
                FuryError::Encoding(format!("Failed to bind query argument: {}", e).into())
            })?;
        }

        let sql_safe_query = sqlx::AssertSqlSafe(query);

        let rows = query_with::<sqlx::Any, AnyArguments>(sql_safe_query, query_args)
            .fetch_all(self.pool.inner())
            .await?;

        Ok(rows)
    }

    pub fn row_to_buffer(&self, row: &AnyRow, schema: &ModelSchema) -> Result<FlatBufferBuilder> {
        let mut builder = FlatBufferBuilder::new(schema.clone());

        for field in schema.fields() {
            let value = self.extract_value(row, &field.name(), &field.field_type())?;
            builder.set_field(&field.name(), value)?;
        }

        Ok(builder)
    }

    fn extract_value(
        &self,
        _row: &AnyRow,
        field_name: &str,
        field_type: &FieldType,
    ) -> Result<ValueType> {
        macro_rules! extract {
            ($sqlx_type:ty, $variant:ident, $expected_str:expr) => {{
                let val: Option<$sqlx_type> =
                    _row.try_get(field_name)
                        .map_err(|e| FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: $expected_str.into(),
                            actual: e.to_string().into(),
                        })?;
                Ok(val.map(ValueType::$variant).unwrap_or(ValueType::None))
            }};
        }

        macro_rules! extract_cast {
            ($sqlx_type:ty, $fallback_type:ty, $variant:ident, $expected_str:expr) => {{
                let val: Option<$fallback_type> =
                    _row.try_get(field_name)
                        .map_err(|e| FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: $expected_str.into(),
                            actual: e.to_string().into(),
                        })?;
                Ok(val
                    .map(|v| ValueType::$variant(v as $sqlx_type))
                    .unwrap_or(ValueType::None))
            }};
        }

        match field_type {
            FieldType::UInt64 => {
                let val: Option<i64> = _row.try_get::<Option<i64>, _>(field_name).map_err(|e| {
                    FuryError::TypeMismatch {
                        field: Arc::from(field_name),
                        expected: "UInt64".into(),
                        actual: e.to_string().into(),
                    }
                })?;

                if let Some(v) = val {
                    let u_val = u64::try_from(v).map_err(|_| FuryError::TypeMismatch {
                        field: Arc::from(field_name),
                        expected: "UInt64 (non-negative)".into(),
                        actual: format!("negative i64 database value: {}", v).into(),
                    })?;
                    Ok(ValueType::UInt64(u_val))
                } else {
                    Ok(ValueType::None)
                }
            }

            FieldType::Int8 => extract_cast!(i8, i16, Int8, "Int8"),
            FieldType::UInt8 => extract_cast!(u8, i16, UInt8, "UInt8"),
            FieldType::UInt16 => extract_cast!(u16, i32, UInt16, "UInt16"),
            FieldType::UInt32 => extract_cast!(u32, i64, UInt32, "UInt32"),

            FieldType::Int16 => extract!(i16, Int16, "Int16"),
            FieldType::Int32 => extract!(i32, Int32, "Int32"),
            FieldType::Int64 => extract!(i64, Int64, "Int64"),
            FieldType::Float32 => extract!(f32, Float32, "Float32"),
            FieldType::Float64 => extract!(f64, Float64, "Float64"),
            FieldType::Bool => extract!(bool, Bool, "Bool"),
            FieldType::String => extract!(String, String, "String"),
            FieldType::Bytes => extract!(Vec<u8>, Bytes, "Bytes"),

            FieldType::Date => {
                let val: Option<String> =
                    _row.try_get(field_name)
                        .map_err(|e| FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: "Date".into(),
                            actual: e.to_string().into(),
                        })?;

                if let Some(s) = val {
                    let d = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(|e| {
                        FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: "Valid Date String".into(),
                            actual: e.to_string().into(),
                        }
                    })?;
                    Ok(ValueType::Date(d))
                } else {
                    Ok(ValueType::None)
                }
            }

            FieldType::DateTime => {
                let val: Option<String> =
                    _row.try_get(field_name)
                        .map_err(|e| FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: "DateTime".into(),
                            actual: e.to_string().into(),
                        })?;

                if let Some(s) = val {
                    let dt = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                        .map_err(|e| FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: "Valid DateTime String".into(),
                            actual: e.to_string().into(),
                        })?;
                    Ok(ValueType::DateTime(dt))
                } else {
                    Ok(ValueType::None)
                }
            }

            FieldType::Uuid => {
                let val: Option<String> =
                    _row.try_get(field_name)
                        .map_err(|e| FuryError::TypeMismatch {
                            field: Arc::from(field_name),
                            expected: "Uuid".into(),
                            actual: e.to_string().into(),
                        })?;

                if let Some(s) = val {
                    let u = uuid::Uuid::parse_str(&s).map_err(|e| FuryError::TypeMismatch {
                        field: Arc::from(field_name),
                        expected: "Valid UUID String".into(),
                        actual: e.to_string().into(),
                    })?;
                    Ok(ValueType::Uuid(u))
                } else {
                    Ok(ValueType::None)
                }
            }

            FieldType::List(_) | FieldType::Map(_, _) | FieldType::Object(_) => {
                Err(FuryError::Encoding(
                    format!(
                        "Collection/Object types cannot be extracted directly from row. \
                        Use separate queries or JSON formats for field '{}'",
                        field_name
                    )
                    .into(),
                ))
            }
        }
    }

    #[must_use]
    pub fn schema_registry(&self) -> &SchemaRegistry {
        &self.schema_registry
    }

    #[must_use]
    pub fn pool(&self) -> &DatabasePool {
        &self.pool
    }
}

#[derive(Clone, Debug)]
pub enum SqlValue {
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    Float32(f32),
    Float64(f64),
    String(String),
    Bytes(Vec<u8>),
    Bool(bool),
    Null,
}

impl SqlValue {
    pub fn bind_to(
        self,
        arguments: &mut AnyArguments,
    ) -> std::result::Result<(), sqlx::error::BoxDynError> {
        match self {
            SqlValue::Int8(v) => arguments.add(v as i16)?,
            SqlValue::UInt8(v) => arguments.add(v as i16)?,
            SqlValue::UInt16(v) => arguments.add(v as i32)?,
            SqlValue::UInt32(v) => arguments.add(v as i64)?,
            SqlValue::UInt64(v) => arguments.add(v as i64)?,

            // Нативные типы для SQLx Any
            SqlValue::Int16(v) => arguments.add(v)?,
            SqlValue::Int32(v) => arguments.add(v)?,
            SqlValue::Int64(v) => arguments.add(v)?,
            SqlValue::Float32(v) => arguments.add(v)?,
            SqlValue::Float64(v) => arguments.add(v)?,
            SqlValue::String(v) => arguments.add(v)?,
            SqlValue::Bytes(v) => arguments.add(v)?,
            SqlValue::Bool(v) => arguments.add(v)?,
            SqlValue::Null => arguments.add(Option::<i32>::None)?,
        };

        Ok(())
    }
}
