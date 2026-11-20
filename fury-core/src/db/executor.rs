use crate::db::pool::DatabasePool;
use crate::encoder::buffer::BinaryRecord;
use crate::encoder::value::ValueType;
use crate::error::{FuryError, Result};
use crate::schema::registry::{FieldType, ModelSchema, SchemaRegistry};
use sqlx_core::arguments::Arguments;
use sqlx_core::column::Column;
use sqlx_core::decode::Decode;
use sqlx_core::error::BoxDynError;
use sqlx_core::query::{query_with, Query};
use sqlx_core::row::Row;
use sqlx_core::sql_str::AssertSqlSafe;
use sqlx_core::types::Type;
use sqlx_core::value::ValueRef;
use sqlx_postgres::{PgArguments, PgRow, Postgres};
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
    ) -> Result<Query<'q, Postgres, PgArguments>> {
        let mut query_args = PgArguments::default();
        for arg in args {
            arg.bind_to(&mut query_args).map_err(|e| {
                FuryError::Encoding(format!("Failed to bind query argument: {}", e).into())
            })?;
        }
        Ok(query_with::<Postgres, PgArguments>(
            AssertSqlSafe(query),
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
        for column in row.columns() {
            let name = column.name();
            if schema.find_field(name).is_some()
                && row.try_column(name)?.ordinal() != column.ordinal()
            {
                return Err(FuryError::Encoding(
                    format!("Ambiguous result column '{name}'; use a unique SQL alias").into(),
                ));
            }
        }
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
            ($source:ty, $variant:ident) => {{
                checked_get::<$source>(row, field_name)
                    .map(|v| v.map(ValueType::$variant).unwrap_or(ValueType::None))
                    .map_err(|err| type_mismatch(field_name, stringify!($variant), err))
            }};
        }

        macro_rules! convert {
            ($source:ty, $variant:ident, $target:ty) => {{
                match checked_get::<$source>(row, field_name) {
                    Ok(None) => Ok(ValueType::None),
                    Ok(Some(value)) => <$target>::try_from(value)
                        .map(ValueType::$variant)
                        .map_err(|_| type_mismatch(field_name, stringify!($variant), value)),
                    Err(err) => Err(type_mismatch(field_name, stringify!($variant), err)),
                }
            }};
        }

        match field_type {
            FieldType::Int8 => convert!(i16, Int8, i8),
            FieldType::Int16 => extract!(i16, Int16),
            FieldType::Int32 => extract!(i32, Int32).or_else(|_| convert!(i16, Int32, i32)),
            FieldType::Int64 => extract!(i64, Int64).or_else(|_| convert!(i32, Int64, i64)),
            FieldType::UInt8 => convert!(i16, UInt8, u8),
            FieldType::UInt16 => convert!(i32, UInt16, u16),
            FieldType::UInt32 => match checked_get::<i64>(row, field_name) {
                Ok(None) => Ok(ValueType::None),
                Ok(Some(value)) => u32::try_from(value)
                    .map(ValueType::UInt32)
                    .map_err(|_| type_mismatch(field_name, "UInt32", value)),
                Err(_) => convert!(i32, UInt32, u32),
            },
            FieldType::UInt64 => convert!(i64, UInt64, u64),
            FieldType::Float32 => extract!(f32, Float32),
            FieldType::Float64 => extract!(f64, Float64).or_else(|_| convert!(f32, Float64, f64)),
            FieldType::Bool => extract!(bool, Bool),
            FieldType::String => extract!(String, String),
            FieldType::Bytes => extract!(Vec<u8>, Bytes),
            FieldType::Date => extract!(chrono::NaiveDate, Date),
            FieldType::DateTime => extract!(chrono::NaiveDateTime, DateTime),
            FieldType::Uuid => extract!(uuid::Uuid, Uuid),
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

// Row::try_get skips SQLx type checks for null. The column contract still applies.
fn checked_get<'r, T>(
    row: &'r PgRow,
    name: &str,
) -> std::result::Result<Option<T>, sqlx_core::Error>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    let value = row.try_get_raw(name)?;
    let ty = value.type_info();
    if !T::compatible(&ty) {
        return Err(sqlx_core::Error::ColumnDecode {
            index: name.into(),
            source: format!("SQL type {ty} is incompatible with {}", T::type_info()).into(),
        });
    }
    Option::<T>::decode(value).map_err(|source| sqlx_core::Error::ColumnDecode {
        index: name.into(),
        source,
    })
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
    /// supported by the concrete PostgreSQL driver.
    ///
    /// # Errors
    ///
    /// Returns an error if the value cannot be bound to the arguments.
    pub fn bind_to(self, arguments: &mut PgArguments) -> std::result::Result<(), BoxDynError> {
        match self {
            SqlValue::Int8(v) => arguments.add(v as i16)?,
            SqlValue::UInt8(v) => arguments.add(v as i16)?,
            SqlValue::UInt16(v) => arguments.add(v as i32)?,
            SqlValue::UInt32(v) => arguments.add(v as i64)?,
            SqlValue::UInt64(v) => arguments.add(
                i64::try_from(v)
                    .map_err(|_| BoxDynError::from(format!("UInt64 {v} exceeds i64::MAX")))?,
            )?,
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
