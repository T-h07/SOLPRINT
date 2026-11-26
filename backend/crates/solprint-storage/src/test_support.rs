//! Disposable databases for integration tests (feature `test-support`).
//!
//! Tests never touch the development database. They require
//! `SOLPRINT_TEST_DATABASE_URL`, pointing at a *disposable* PostgreSQL server
//! (the `scripts/test.ps1` throwaway container, also used by `scripts/verify.ps1`). Each test
//! creates its own uniquely named database on that server and drops it
//! afterwards.

#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use solprint_config::{DatabaseConfig, Secret};
use sqlx::postgres::PgConnection;
use sqlx::{AssertSqlSafe, Connection};
use url::Url;

pub const TEST_DATABASE_ENV: &str = "SOLPRINT_TEST_DATABASE_URL";

/// A uniquely named database that exists for the lifetime of one test.
#[derive(Debug)]
pub struct TestDatabase {
    admin_url: Url,
    name: String,
    config: DatabaseConfig,
}

impl TestDatabase {
    /// Create a fresh, empty database. Panics with a clear message when the
    /// disposable test server is not configured.
    pub async fn create() -> Self {
        let raw = std::env::var(TEST_DATABASE_ENV).unwrap_or_else(|_| {
            panic!("{TEST_DATABASE_ENV} is not set; run scripts/test.ps1 or see docs/development/TESTING.md")
        });
        let admin_url = Url::parse(&raw).expect("SOLPRINT_TEST_DATABASE_URL is not a URL");
        let name = format!("solprint_test_{}", uuid::Uuid::new_v4().simple());
        let mut admin = PgConnection::connect(admin_url.as_str())
            .await
            .expect("cannot connect to the disposable test PostgreSQL server");
        // `name` is "solprint_test_" + 32 hex chars generated above: no
        // user input reaches this statement. Identifiers cannot be bound.
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE \"{name}\"")))
            .execute(&mut admin)
            .await
            .expect("CREATE DATABASE failed");
        let _ = admin.close().await;

        let mut url = admin_url.clone();
        url.set_path(&format!("/{name}"));
        let config = DatabaseConfig {
            url: Secret::new(url),
            max_connections: 4,
            acquire_timeout: Duration::from_secs(5),
            startup_timeout: Duration::from_secs(10),
            run_migrations: true,
        };
        Self {
            admin_url,
            name,
            config,
        }
    }

    #[must_use]
    pub fn config(&self) -> DatabaseConfig {
        self.config.clone()
    }

    /// Simulate an outage: terminate every connection and drop the database.
    pub async fn drop_database(&self) {
        let mut admin = PgConnection::connect(self.admin_url.as_str())
            .await
            .expect("cannot connect to the disposable test PostgreSQL server");
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.name
        )))
        .execute(&mut admin)
        .await
        .expect("DROP DATABASE failed");
        let _ = admin.close().await;
    }

    /// Drop the database. Call at the end of each test.
    pub async fn cleanup(self) {
        self.drop_database().await;
    }
}
