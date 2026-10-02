use crate::db::pool::DatabasePool;
use crate::encoder::buffer::BinaryRecord;
use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::{FieldType, ModelSchema, SchemaRegistry};
use sqlx::postgres::{PgArguments, PgRow};
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

    /// Builds a ready-to-run sqlx query with all arguments bound.
    ///
    /// Single point of argument binding and query construction: both
    /// `execute_raw` (SELECT) and `execute_affected` (INSERT/UPDATE/DELETE)
    /// go through here, so binding errors and driver changes live in one place.
    ///
    /// # Errors
    ///
    /// Returns an error if any argument cannot be bound to the statement.
    fn prepare_query<'q>(
        &self,
        query: &'q str,
        args: Vec<SqlValue>,
    ) -> Result<sqlx::query::Query<'q, sqlx::Postgres, PgArguments>> {
        let mut query_args = PgArguments::default();
        for arg in args {
            arg.bind_to(&mut query_args).map_err(|e| {
                FuryError::Encoding(format!("Failed to bind query argument: {}", e).into())
            })?;
        }
        Ok(query_with::<sqlx::Postgres, PgArguments>(
            sqlx::AssertSqlSafe(query),
            query_args,
        ))
    }

    /// Executes a raw SQL query with the provided arguments.
    ///
    /// Uses `AssertSqlSafe` to bypass SQLx's compile-time query checking,
    /// allowing dynamic query construction at runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails to execute or arguments cannot be bound.
    pub async fn execute_raw(&self, query: &str, args: Vec<SqlValue>) -> Result<Vec<PgRow>> {
        let rows = self
            .prepare_query(query, args)?
            .fetch_all(self.pool.inner())
            .await?;
        Ok(rows)
    }

    /// Executes a statement and returns the driver-reported affected row count.
    ///
    /// Unlike `execute_raw` (`fetch_all`), this uses sqlx `execute()`, which for
    /// INSERT/UPDATE/DELETE without RETURNING returns `rows_affected()` instead
    /// of an empty rowset.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails to execute or arguments cannot be bound.
    pub async fn execute_affected(&self, query: &str, args: Vec<SqlValue>) -> Result<u64> {
        let result = self
            .prepare_query(query, args)?
            .execute(self.pool.inner())
            .await?;
        Ok(result.rows_affected())
    }

    /// Converts a database row to a binary record according to the schema.
    ///
    /// Extracts each field from the row, converts it to `ValueType`, and
    /// builds a `BinaryRecord` with zero-copy field access.
    ///
    /// # Errors
    ///
    /// Returns an error if field extraction or type conversion fails.
    pub fn row_to_buffer(&self, row: &PgRow, schema: &Arc<ModelSchema>) -> Result<BinaryRecord> {
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
        row: &PgRow,
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
                    .try_get::<Option<$source>, _>(field_name)
                    .map_err(|e| type_mismatch(field_name, $expected, e))?;
                Ok(val
                    .map(|v| ValueType::$variant(v as $target))
                    .unwrap_or(ValueType::None))
            }};
        }

        match field_type {
            FieldType::Int8 => extract_cast!(i8, i16, Int8, "Int8"),
            FieldType::Int16 => extract!(i16, Int16, "Int16"),
            FieldType::Int32 => match row.try_get::<Option<i32>, _>(field_name) {
                Ok(val) => Ok(val.map(ValueType::Int32).unwrap_or(ValueType::None)),
                Err(_) => extract_cast!(i32, i16, Int32, "Int32"),
            },
            FieldType::Int64 => match row.try_get::<Option<i64>, _>(field_name) {
                Ok(val) => Ok(val.map(ValueType::Int64).unwrap_or(ValueType::None)),
                Err(_) => extract_cast!(i64, i32, Int64, "Int64"),
            },
            FieldType::UInt8 => extract_cast!(u8, i16, UInt8, "UInt8"),
            FieldType::UInt16 => extract_cast!(u16, i32, UInt16, "UInt16"),
            FieldType::UInt32 => match row.try_get::<Option<i64>, _>(field_name) {
                Ok(val) => match val {
                    Some(v) => {
                        let u = u32::try_from(v).map_err(|_| FuryError::TypeMismatch {
                            field: field_name.into(),
                            expected: "UInt32 (non-negative)".into(),
                            actual: format!("out of range: {v}").into(),
                        })?;
                        Ok(ValueType::UInt32(u))
                    }
                    None => Ok(ValueType::None),
                },
                Err(_) => {
                    let val: Option<i32> = row.try_get(field_name).map_err(|e| {
                        type_mismatch(field_name, "UInt32 (fallback from Int32)", e)
                    })?;
                    match val {
                        Some(v) => {
                            let u = u32::try_from(v).map_err(|_| FuryError::TypeMismatch {
                                field: field_name.into(),
                                expected: "UInt32 (non-negative)".into(),
                                actual: format!("negative i32 value: {v}").into(),
                            })?;
                            Ok(ValueType::UInt32(u))
                        }
                        None => Ok(ValueType::None),
                    }
                }
            },
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

            FieldType::Float64 => match row.try_get::<Option<f64>, _>(field_name) {
                Ok(val) => Ok(val.map(ValueType::Float64).unwrap_or(ValueType::None)),
                Err(_) => extract_cast!(f64, f32, Float64, "Float64"),
            },
            FieldType::Bool => extract!(bool, Bool, "Bool"),
            FieldType::String => extract!(String, String, "String"),
            FieldType::Bytes => extract!(Vec<u8>, Bytes, "Bytes"),
            FieldType::Date => extract!(chrono::NaiveDate, Date, "Date"),
            FieldType::DateTime => extract!(chrono::NaiveDateTime, DateTime, "DateTime"),
            FieldType::Uuid => extract!(uuid::Uuid, Uuid, "Uuid"),
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
    DateTime(chrono::NaiveDateTime),
    Date(chrono::NaiveDate),
    Uuid(uuid::Uuid),
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
        arguments: &mut PgArguments,
    ) -> std::result::Result<(), sqlx::error::BoxDynError> {
        match self {
            SqlValue::Int8(v) => arguments.add(v as i16)?,
            SqlValue::UInt8(v) => arguments.add(v as i16)?,
            SqlValue::UInt16(v) => arguments.add(v as i32)?,
            SqlValue::UInt32(v) => arguments.add(v as i64)?,
            SqlValue::UInt64(v) => arguments.add(i64::try_from(v).map_err(|_| {
                sqlx::error::BoxDynError::from(format!("UInt64 {v} exceeds i64::MAX"))
            })?)?,
            SqlValue::Int16(v) => arguments.add(v)?,
            SqlValue::Int32(v) => arguments.add(v)?,
            SqlValue::Int64(v) => arguments.add(v)?,
            SqlValue::Float32(v) => arguments.add(v)?,
            SqlValue::Float64(v) => arguments.add(v)?,
            SqlValue::String(v) => arguments.add(v)?,
            SqlValue::Bytes(v) => arguments.add(v)?,
            SqlValue::Bool(v) => arguments.add(v)?,
            SqlValue::DateTime(v) => arguments.add(v)?,
            SqlValue::Date(v) => arguments.add(v)?,
            SqlValue::Uuid(v) => arguments.add(v)?,
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
