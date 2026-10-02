use sqlx::PgPool;
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
        let pool = PgPool::connect(connection_string).await?;
        pool.acquire().await?;
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
        sqlx::query("SELECT 1").execute(&*self.inner).await.is_ok()
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