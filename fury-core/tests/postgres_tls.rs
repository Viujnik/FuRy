#![cfg(feature = "db")]

use fury_core::db::pool::DatabasePool;
use sqlx_core::query_scalar::query_scalar;

#[tokio::test]
#[ignore = "requires the disposable PostgreSQL TLS cluster"]
async fn checks_encryption_and_peer_identity() {
    let port = std::env::var("FURY_TEST_TLS_PORT").expect("test port required");
    let certs = std::env::var("FURY_TEST_TLS_DIRECTORY").expect("test CA directory required");
    let url = |host, ca| {
        format!("postgresql://fury@{host}:{port}/postgres?sslmode=verify-full&sslrootcert={certs}/{ca}.crt")
    };

    let pool = DatabasePool::connect(&url("localhost", "root"))
        .await
        .unwrap();
    let encrypted: bool = query_scalar("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()")
        .fetch_one(pool.inner())
        .await
        .unwrap();
    assert!(encrypted);
    pool.inner().close().await;

    for (host, ca) in [("127.0.0.1", "root"), ("localhost", "other")] {
        match DatabasePool::connect(&url(host, ca)).await {
            Ok(_) => panic!("verify-full accepted the invalid host or CA"),
            Err(error) => assert!(
                error.to_string().to_lowercase().contains("certificate"),
                "{error}"
            ),
        }
    }
}
