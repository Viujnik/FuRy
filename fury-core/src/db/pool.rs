use sqlx_core::query::query;
use sqlx_postgres::{PgPool, PgPoolOptions};
use std::sync::Arc;

use crate::error::Result;

/// Database connection pool wrapper.
///
/// Wraps SQLx's concrete `PgPool`: the Postgres driver implements
/// `Decode`/`Type` for chrono/uuid types, which the `Any` driver does not.
#[derive(Clone)]
pub struct DatabasePool {
    inner: Arc<PgPool>,
}

impl DatabasePool {
    /// Creates a new database connection pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the database is unreachable.
    pub async fn connect(connection_string: &str) -> Result<Self> {
        Self::connect_with_limit(connection_string, None).await
    }

    /// Connects with an optional explicit cap for concurrent database connections.
    pub async fn connect_with_limit(
        connection_string: &str,
        max_connections: Option<u32>,
    ) -> Result<Self> {
        let mut options = PgPoolOptions::new();
        if let Some(max) = max_connections {
            if max == 0 {
                return Err(crate::error::FuryError::Database(
                    "max_connections must be positive".into(),
                ));
            }
            options = options.max_connections(max);
        }
        let pool = options.connect(connection_string).await?;
        Ok(Self {
            inner: Arc::new(pool),
        })
    }

    /// Returns a reference to the underlying SQLx pool.
    #[must_use]
    pub fn inner(&self) -> &PgPool {
        &self.inner
    }

    /// Checks if the database connection pool is healthy.
    pub async fn is_healthy(&self) -> bool {
        query("SELECT 1").execute(&*self.inner).await.is_ok()
    }

    /// Lightweight health check without network I/O.
    pub fn is_healthy_lightweight(&self) -> bool {
        !self.inner.is_closed()
            && (self.inner.num_idle() > 0
                || self.inner.size() < self.inner.options().get_max_connections())
    }
}

impl std::fmt::Debug for DatabasePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabasePool")
            .field("pool_size", &self.inner.size())
            .field("idle_connections", &self.inner.num_idle())
            .finish()
    }
}
