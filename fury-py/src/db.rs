use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3_async_runtimes::tokio::future_into_py;
use std::sync::Arc;

use crate::conversion::python_to_sql_value;
use crate::errors::fury_error_to_pyerr;
use crate::get_global_registry;
use crate::models::create_py_model_instance;
use fury_core::db::executor::QueryExecutor;
use fury_core::db::executor::SqlValue;
use fury_core::db::pool::DatabasePool;
use fury_core::error::FuryError;

/// Database connection and query executor for Python.
///
/// Provides async database operations with FuRy's binary serialization.
/// Wraps SQLx connection pool and query executor for Python integration.
///
/// # Examples
///
/// ```python
/// from fury import FuryDB
///
/// db = FuryDB.connect("postgresql://user:pass@localhost/db")
/// user = await db.fetch_one(UserModel, "SELECT * FROM users WHERE id = $1", [1])
/// ```
#[pyclass]
pub struct FuryDB {
    executor: Arc<QueryExecutor>,
}

#[pymethods]
impl FuryDB {
    /// Creates a new database connection pool.
    ///
    /// # Arguments
    ///
    /// * `connection_string` — PostgreSQL connection URL (e.g., "postgresql://user:pass@localhost/db")
    ///
    /// # Errors
    ///
    /// Returns `RuntimeError` if the connection fails or the database is unreachable.
    #[staticmethod]
    fn connect(connection_string: &str) -> PyResult<Self> {
        let pool = pyo3_async_runtimes::tokio::get_runtime()
            .block_on(DatabasePool::connect(connection_string))
            .map_err(fury_error_to_pyerr)?;

        let registry = get_global_registry().clone();
        let executor = Arc::new(QueryExecutor::new(pool, registry));

        Ok(Self { executor })
    }

    /// Fetches a single row from the database and converts it to a Python model instance.
    ///
    /// # Arguments
    ///
    /// * `model_class` — Python class (must be registered in the global schema registry)
    /// * `query` — SQL query with `$1`, `$2`, etc. placeholders
    /// * `args` — Optional list of query arguments
    ///
    /// # Errors
    ///
    /// - `ValueError` if query returns 0 or more than 1 row
    /// - `ValueError` if schema is not found
    /// - `RuntimeError` if database query fails
    #[pyo3(name = "fetch_one")]
    fn py_fetch_one<'py>(
        &self,
        py: Python<'py>,
        model_class: Bound<'py, PyAny>,
        query: String,
        args: Option<Bound<'py, PyList>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let model_name: String = model_class.getattr("__name__")?.extract()?;
        let sql_args = parse_sql_args(args)?;
        let executor = Arc::clone(&self.executor);

        let model_class_py: Py<PyAny> = model_class.unbind();

        future_into_py(py, async move {
            let rows = executor
                .execute_raw(&query, sql_args)
                .await
                .map_err(fury_error_to_pyerr)?;

            if rows.len() != 1 {
                return Err(fury_error_to_pyerr(FuryError::QueryResultRowMismatch {
                    expected: 1,
                    actual: rows.len(),
                }));
            }

            let schema = get_global_registry().get(&model_name).ok_or_else(|| {
                fury_error_to_pyerr(FuryError::SchemaNotFound(Arc::from(model_name)))
            })?;

            let mut buffer = executor
                .row_to_buffer(&rows[0], &schema)
                .map_err(fury_error_to_pyerr)?;

            buffer.finish().map_err(fury_error_to_pyerr)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let model_class_bound = model_class_py.bind(py);
                let instance: Py<PyAny> =
                    create_py_model_instance(model_class_bound.clone(), &schema, &buffer)?;
                Ok(instance)
            })
        })
    }

    /// Fetches all rows from the database and converts them to a list of Python model instances.
    ///
    /// # Arguments
    ///
    /// * `model_class` — Python class (must be registered in the global schema registry)
    /// * `query` — SQL query with `$1`, `$2`, etc. placeholders
    /// * `args` — Optional list of query arguments
    ///
    /// # Errors
    ///
    /// - `ValueError` if schema is not found
    /// - `RuntimeError` if database query fails
    #[pyo3(name = "fetch_all")]
    fn py_fetch_all<'py>(
        &self,
        py: Python<'py>,
        model_class: Bound<'py, PyAny>,
        query: String,
        args: Option<Bound<'py, PyList>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let model_name: String = model_class.getattr("__name__")?.extract()?;
        let sql_args = parse_sql_args(args)?;
        let executor = Arc::clone(&self.executor);

        let model_class_py: Py<PyAny> = model_class.unbind();

        future_into_py(py, async move {
            let rows = executor
                .execute_raw(&query, sql_args)
                .await
                .map_err(fury_error_to_pyerr)?;

            let schema = executor.schema_registry().get(&model_name).ok_or_else(|| {
                fury_error_to_pyerr(FuryError::SchemaNotFound(Arc::from(model_name)))
            })?;

            let mut buffers = Vec::with_capacity(rows.len());
            for row in rows {
                let mut buffer = executor
                    .row_to_buffer(&row, &schema)
                    .map_err(fury_error_to_pyerr)?;
                buffer.finish().map_err(fury_error_to_pyerr)?;
                buffers.push(buffer);
            }

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let model_class_bound = model_class_py.bind(py);
                let list = PyList::empty(py);

                for buffer in buffers {
                    let instance: Py<PyAny> =
                        create_py_model_instance(model_class_bound.clone(), &schema, &buffer)?;
                    list.append(instance.bind(py))?;
                }

                Ok(list.into_any().unbind())
            })
        })
    }

    /// Executes a SQL query and returns the number of affected rows.
    ///
    /// # Arguments
    ///
    /// * `query` — SQL query with `$1`, `$2`, etc. placeholders
    /// * `args` — Optional list of query arguments
    ///
    /// # Errors
    ///
    /// - `RuntimeError` if database query fails
    #[pyo3(name = "execute")]
    fn py_execute<'py>(
        &self,
        py: Python<'py>,
        query: String,
        args: Option<Bound<'py, PyList>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let sql_args = parse_sql_args(args)?;
        let executor = Arc::clone(&self.executor);

        future_into_py(py, async move {
            let rows = executor
                .execute_raw(&query, sql_args)
                .await
                .map_err(fury_error_to_pyerr)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let len = rows.len();
                let py_int = len.into_pyobject(py)?;
                Ok(py_int.into_any().unbind())
            })
        })
    }
}

/// Converts a Python list of arguments to a vector of `SqlValue`.
fn parse_sql_args(args: Option<Bound<'_, PyList>>) -> PyResult<Vec<SqlValue>> {
    args.map(|list| {
        list.iter()
            .map(|arg| python_to_sql_value(&arg))
            .collect::<PyResult<Vec<_>>>()
    })
    .unwrap_or_else(|| Ok(Vec::new()))
}
