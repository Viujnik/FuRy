use sqlx::AnyPool;
use std::sync::{Arc, Once};

use crate::error::Result;

static INSTALL_DRIVERS: Once = Once::new();

/// Database connection pool wrapper.
///
/// Provides a thread-safe pool of database connections with health checking
/// and connection management. Wraps SQLx's `AnyPool` for database-agnostic access.
///
/// # Examples
///
/// ```rust
/// let pool = DatabasePool::connect("postgresql://localhost/mydb").await?;
/// if pool.is_healthy().await {
///     println!("Database is ready!");
/// }
/// ```
#[derive(Clone)]
pub struct DatabasePool {
    inner: Arc<AnyPool>,
}

impl DatabasePool {
    /// Creates a new database connection pool.
    ///
    /// Installs SQLx drivers on first call, then connects to the database
    /// using the provided connection string.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the database is unreachable.
    pub async fn connect(connection_string: &str) -> Result<Self> {
        INSTALL_DRIVERS.call_once(|| {
            sqlx::any::install_default_drivers();
        });

        let pool = AnyPool::connect(connection_string).await?;
        pool.acquire().await?;

        Ok(Self {
            inner: Arc::new(pool),
        })
    }

    /// Returns a reference to the underlying SQLx pool.
    #[must_use]
    pub fn inner(&self) -> &AnyPool {
        &self.inner
    }

    /// Checks if the database connection pool is healthy.
    ///
    /// Performs a lightweight network ping (`SELECT 1`) to verify that the database
    /// is actually responding. This is suitable for Kubernetes health checks and
    /// FastAPI lifespan hooks.
    ///
    /// # Performance
    ///
    /// This method executes a network query, so it takes ~1-5ms. For high-frequency
    /// internal monitoring, consider using `is_healthy_lightweight()` instead.
    pub async fn is_healthy(&self) -> bool {
        sqlx::query("SELECT 1").execute(&*self.inner).await.is_ok()
    }

    /// Lightweight health check without network I/O.
    ///
    /// Checks pool state (idle connections, max size) without executing queries.
    /// This is O(1) and does not touch the network, but does NOT guarantee that
    /// the database is actually responding.
    ///
    /// # When to use
    ///
    /// Use this for high-frequency internal monitoring (e.g., every second).
    /// For Kubernetes health checks, use `is_healthy()` instead.
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
