use sqlx::AnyPool;
use std::sync::Arc;

use crate::error::Result;

#[derive(Clone)]
pub struct DatabasePool {
    inner: Arc<AnyPool>,
}

impl std::fmt::Debug for DatabasePool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabasePool")
            .field("pool_size", &self.inner.size())
            .field("idle_connections", &self.inner.num_idle())
            .finish()
    }
}

impl DatabasePool {
    pub async fn connect(connection_string: &str) -> Result<Self> {
        sqlx::any::install_default_drivers();

        let pool = AnyPool::connect(connection_string).await?;
        pool.acquire().await?;

        Ok(Self {
            inner: Arc::new(pool),
        })
    }

    #[must_use]
    pub fn inner(&self) -> &AnyPool {
        &self.inner
    }

    pub async fn is_healthy(&self) -> bool {
        self.inner.acquire().await.is_ok()
    }
}
