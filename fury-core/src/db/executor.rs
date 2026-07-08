use crate::db::pool::DatabasePool;
use crate::encoder::buffer::BinaryRecord;
use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::{FieldType, ModelSchema, SchemaRegistry};
use sqlx::any::{AnyArguments, AnyRow};
use sqlx::{query_with, Arguments, Row};
use std::sync::Arc;

/// Executes SQL queries and converts results to binary format.
///
/// Bridges SQLx database queries with FuRy's binary serialization engine.
/// Handles type conversion from SQL types to `ValueType` and builds `BinaryRecord`
/// instances for zero-copy field access.
pub struct QueryExecutor {
    pool: DatabasePool,
    schema_registry: SchemaRegistry,
}

impl QueryExecutor {
    /// Creates a new query executor with the given pool and schema registry.
    #[must_use]
    pub fn new(pool: DatabasePool, schema_registry: SchemaRegistry) -> Self {
        Self {
            pool,
            schema_registry,
        }
    }

    /// Executes a raw SQL query with the provided arguments.
    ///
    /// Uses `AssertSqlSafe` to bypass SQLx's compile-time query checking,
    /// allowing dynamic query construction at runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails to execute or arguments cannot be bound.
    pub async fn execute_raw(&self, query: &str, args: Vec<SqlValue>) -> Result<Vec<AnyRow>> {
        let mut query_args = AnyArguments::default();

        for arg in args {
            arg.bind_to(&mut query_args).map_err(|e| {
                FuryError::Encoding(format!("Failed to bind query argument: {}", e).into())
            })?;
        }

        let rows = query_with::<sqlx::Any, AnyArguments>(sqlx::AssertSqlSafe(query), query_args)
            .fetch_all(self.pool.inner())
            .await?;

        Ok(rows)
    }

    /// Converts a database row to a binary record according to the schema.
    ///
    /// Extracts each field from the row, converts it to `ValueType`, and
    /// builds a `BinaryRecord` with zero-copy field access.
    ///
    /// # Errors
    ///
    /// Returns an error if field extraction or type conversion fails.
    pub fn row_to_buffer(&self, row: &AnyRow, schema: &Arc<ModelSchema>) -> Result<BinaryRecord> {
        let mut builder = BinaryRecord::new(Arc::clone(schema));

        for field in schema.fields() {
            let value = self.extract_value(row, field.name(), field.field_type())?;
            builder.set_field(field.name(), value)?;
        }

        Ok(builder)
    }

    /// Extracts a field value from a database row and converts it to `ValueType`.
    ///
    /// # Errors
    ///
    /// Returns an error if the field cannot be extracted or type conversion fails.
    fn extract_value(
        &self,
        row: &AnyRow,
        field_name: &str,
        field_type: &FieldType,
    ) -> Result<ValueType> {
        macro_rules! extract {
            ($sqlx_type:ty, $variant:ident, $expected:expr) => {{
                let val: Option<$sqlx_type> = row
                    .try_get(field_name)
                    .map_err(|e| type_mismatch(field_name, $expected, e))?;
                Ok(val.map(ValueType::$variant).unwrap_or(ValueType::None))
            }};
        }

        macro_rules! extract_cast {
            ($target:ty, $source:ty, $variant:ident, $expected:expr) => {{
                let val: Option<$source> = row
                    .try_get(field_name)
                    .map_err(|e| type_mismatch(field_name, $expected, e))?;
                Ok(val
                    .map(|v| ValueType::$variant(v as $target))
                    .unwrap_or(ValueType::None))
            }};
        }

        match field_type {
            FieldType::Int8 => extract_cast!(i8, i16, Int8, "Int8"),
            FieldType::Int16 => extract!(i16, Int16, "Int16"),
            FieldType::Int32 => extract!(i32, Int32, "Int32"),
            FieldType::Int64 => extract!(i64, Int64, "Int64"),

            FieldType::UInt8 => extract_cast!(u8, i16, UInt8, "UInt8"),
            FieldType::UInt16 => extract_cast!(u16, i32, UInt16, "UInt16"),
            FieldType::UInt32 => extract_cast!(u32, i64, UInt32, "UInt32"),
            FieldType::UInt64 => {
                let val: Option<i64> = row
                    .try_get(field_name)
                    .map_err(|e| type_mismatch(field_name, "UInt64", e))?;

                match val {
                    Some(v) => {
                        let u_val = u64::try_from(v).map_err(|_| FuryError::TypeMismatch {
                            field: field_name.into(),
                            expected: "UInt64 (non-negative)".into(),
                            actual: format!("negative i64 value: {}", v).into(),
                        })?;
                        Ok(ValueType::UInt64(u_val))
                    }
                    None => Ok(ValueType::None),
                }
            }

            FieldType::Float32 => extract!(f32, Float32, "Float32"),
            FieldType::Float64 => extract!(f64, Float64, "Float64"),
            FieldType::Bool => extract!(bool, Bool, "Bool"),
            FieldType::String => extract!(String, String, "String"),
            FieldType::Bytes => extract!(Vec<u8>, Bytes, "Bytes"),

            FieldType::Date => {
                let val: Option<String> = row
                    .try_get(field_name)
                    .map_err(|e| type_mismatch(field_name, "Date", e))?;

                match val {
                    Some(s) => {
                        let d = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                            .map_err(|e| type_mismatch(field_name, "Date (YYYY-MM-DD)", e))?;
                        Ok(ValueType::Date(d))
                    }
                    None => Ok(ValueType::None),
                }
            }

            FieldType::DateTime => {
                let val: Option<String> = row
                    .try_get(field_name)
                    .map_err(|e| type_mismatch(field_name, "DateTime", e))?;

                match val {
                    Some(s) => {
                        let dt = chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                            .map_err(|e| {
                                type_mismatch(field_name, "DateTime (YYYY-MM-DD HH:MM:SS)", e)
                            })?;
                        Ok(ValueType::DateTime(dt))
                    }
                    None => Ok(ValueType::None),
                }
            }

            FieldType::Uuid => {
                let val: Option<String> = row
                    .try_get(field_name)
                    .map_err(|e| type_mismatch(field_name, "Uuid", e))?;

                match val {
                    Some(s) => {
                        let u = uuid::Uuid::parse_str(&s)
                            .map_err(|e| type_mismatch(field_name, "UUID string", e))?;
                        Ok(ValueType::Uuid(u))
                    }
                    None => Ok(ValueType::None),
                }
            }

            FieldType::List(_) | FieldType::Map(_, _) | FieldType::Object(_) => {
                Err(FuryError::Encoding(
                    format!(
                        "Collection/Object types cannot be extracted directly from row \
                         for field '{}'",
                        field_name
                    )
                    .into(),
                ))
            }
        }
    }

    /// Returns a reference to the schema registry.
    #[must_use]
    pub fn schema_registry(&self) -> &SchemaRegistry {
        &self.schema_registry
    }

    /// Returns a reference to the database pool.
    #[must_use]
    pub fn pool(&self) -> &DatabasePool {
        &self.pool
    }
}

/// SQL query argument value for binding to prepared statements.
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
    /// Binds this value to SQLx query arguments.
    ///
    /// Unsigned and sub-16-bit integers are cast to the smallest signed type
    /// supported by SQLx `Any` driver.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be bound to the arguments.
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

/// Helper function to create type mismatch errors.
fn type_mismatch(field: &str, expected: &str, err: impl std::fmt::Display) -> FuryError {
    FuryError::TypeMismatch {
        field: field.into(),
        expected: expected.into(),
        actual: err.to_string().into(),
    }
}
