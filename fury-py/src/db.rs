use pyo3::prelude::*;
use pyo3::types::PyList;
use pyo3_async_runtimes::tokio::future_into_py;
use std::sync::Arc;
use tokio::runtime::Runtime;

use fury_core::db::executor::QueryExecutor;
use fury_core::db::executor::SqlValue;
use fury_core::db::pool::DatabasePool;

use crate::conversion::python_to_sql_value;
use crate::errors::fury_error_to_pyerr;
use crate::get_global_registry;
use crate::models::create_py_model_instance;

#[pyclass]
pub struct FuryDB {
    executor: Arc<QueryExecutor>,
}

#[pymethods]
impl FuryDB {
    #[staticmethod]
    fn connect(connection_string: &str) -> PyResult<Self> {
        let runtime = Runtime::new().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!(
                "Failed to create Tokio runtime: {}",
                e
            ))
        })?;

        let pool = runtime
            .block_on(DatabasePool::connect(connection_string))
            .map_err(fury_error_to_pyerr)?;

        let registry = get_global_registry().clone();
        let executor = Arc::new(QueryExecutor::new(pool.clone(), registry));

        Ok(Self {
            executor,
        })
    }

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

        // 🔑 Отвязываем от GIL для передачи в async (Py<T> является Send)
        let model_class_py: Py<PyAny> = model_class.unbind();

        future_into_py(py, async move {
            let rows = executor
                .execute_raw(&query, sql_args)
                .await
                .map_err(fury_error_to_pyerr)?;

            if rows.is_empty() {
                return Err(pyo3::exceptions::PyValueError::new_err(
                    "Query returned 0 rows, expected 1",
                ));
            }
            if rows.len() > 1 {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "Query returned {} rows, expected 1",
                    rows.len()
                )));
            }

            let schema = get_global_registry().get(&model_name).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!("Schema not found: {}", model_name))
            })?;

            let mut buffer = executor
                .row_to_buffer(&rows[0], &schema)
                .map_err(fury_error_to_pyerr)?;

            buffer.finish().map_err(fury_error_to_pyerr)?;

            // 🔑 Захватываем GIL и возвращаем Py<PyAny> (не Bound!)
            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let model_class_bound = model_class_py.bind(py);
                let instance: Py<PyAny> =
                    create_py_model_instance(model_class_bound.clone(), &schema, &buffer)?;
                // Возвращаем Py<PyAny>, который является Send
                Ok(instance)
            })
        })
    }

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

            let schema = get_global_registry().get(&model_name).ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!("Schema not found: {}", model_name))
            })?;

            let mut buffers = Vec::with_capacity(rows.len());
            for row in rows {
                let mut buffer = executor
                    .row_to_buffer(&row, &schema)
                    .map_err(fury_error_to_pyerr)?;
                buffer.finish().map_err(fury_error_to_pyerr)?;
                buffers.push(buffer);
            }

            // 🔑 Возвращаем Py<PyList> (Send тип)
            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let model_class_bound = model_class_py.bind(py);
                let list = PyList::empty(py);

                for buffer in buffers {
                    let instance: Py<PyAny> =
                        create_py_model_instance(model_class_bound.clone(), &schema, &buffer)?;
                    // Привязываем Py обратно к GIL для append
                    list.append(instance.bind(py))?;
                }

                // Конвертируем Bound<PyList> -> Py<PyAny>
                Ok(list.into_any().unbind())
            })
        })
    }

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

            // 🔑 Возвращаем Py<PyInt> (Send тип)
            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let len = rows.len();
                let py_int = len.into_pyobject(py)?;
                Ok(py_int.into_any().unbind())
            })
        })
    }
}

fn parse_sql_args(args: Option<Bound<'_, PyList>>) -> PyResult<Vec<SqlValue>> {
    match args {
        Some(args_list) => {
            let mut sql_args = Vec::with_capacity(args_list.len());
            for arg in args_list.iter() {
                sql_args.push(python_to_sql_value(&arg)?);
            }
            Ok(sql_args)
        }
        None => Ok(Vec::new()),
    }
}
