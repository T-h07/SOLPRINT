//! PostgreSQL integration tests.
//!
//! Ignored by default because they need a disposable PostgreSQL server.
//! Run with `scripts/test.ps1` (starts one) or, against another disposable
//! server, with
//! `SOLPRINT_TEST_DATABASE_URL` set and `-- --include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use solprint_config::{DatabaseConfig, Secret};
use solprint_storage::test_support::TestDatabase;
use solprint_storage::{Database, DatabaseHealth, MIGRATOR, StorageError};

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn fresh_database_migrates_and_is_healthy() {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();

    assert_eq!(
        db.applied_migration_versions().await.unwrap(),
        Vec::<i64>::new()
    );
    assert!(matches!(
        db.verify_migrations().await,
        Err(StorageError::MigrationsPending(n)) if n == MIGRATOR.iter().count()
    ));

    db.migrate().await.unwrap();
    db.verify_migrations().await.unwrap();
    assert_eq!(
        db.applied_migration_versions().await.unwrap().len(),
        MIGRATOR.iter().count()
    );
    assert!(matches!(
        db.health().await,
        DatabaseHealth::Available { .. }
    ));

    db.close().await;
    tdb.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn repeated_migration_and_startup_preserve_state() {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    let first = db.register_start("0.1.0").await.unwrap();
    assert_eq!(first.start_count, 1);

    // Re-running migrations is a no-op and must not reset data.
    db.migrate().await.unwrap();
    let second = db.register_start("0.1.1").await.unwrap();
    assert_eq!(second.instance_id, first.instance_id);
    assert_eq!(second.created_at, first.created_at);
    assert_eq!(second.start_count, 2);
    db.close().await;

    // A brand-new pool (simulated process restart) sees the same row.
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    let third = db.register_start("0.1.1").await.unwrap();
    assert_eq!(third.instance_id, first.instance_id);
    assert_eq!(third.start_count, 3);

    // The singleton constraint forbids a second instance row.
    let dup = sqlx::query(
        "INSERT INTO solprint.instance (singleton, last_app_version) VALUES (FALSE, 'x')",
    )
    .execute(db.pool())
    .await;
    assert!(dup.is_err());

    db.close().await;
    tdb.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn modified_applied_migration_is_detected() {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    sqlx::query("UPDATE _sqlx_migrations SET checksum = '\\x00'::bytea")
        .execute(db.pool())
        .await
        .unwrap();
    let err = db.migrate().await.unwrap_err();
    assert!(err.to_string().contains("modified"), "{err}");
    db.close().await;
    tdb.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn health_reports_outage_after_database_disappears() {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    assert!(matches!(
        db.health().await,
        DatabaseHealth::Available { .. }
    ));
    tdb.drop_database().await;
    assert_eq!(db.health().await, DatabaseHealth::Unavailable);
    db.close().await;
}

/// Runs without a database: an unreachable server must fail fast and
/// sanitized, without echoing the connection string.
#[tokio::test]
async fn unreachable_database_fails_startup_with_sanitized_error() {
    let config = DatabaseConfig {
        url: Secret::new(
            "postgres://user:synthetic-secret-pw@127.0.0.1:1/solprint"
                .parse()
                .unwrap(),
        ),
        max_connections: 1,
        acquire_timeout: Duration::from_millis(200),
        startup_timeout: Duration::from_millis(300),
        run_migrations: true,
    };
    let err = Database::connect(&config).await.unwrap_err();
    assert!(matches!(err, StorageError::StartupTimeout(_)));
    assert!(!format!("{err} {err:?}").contains("synthetic-secret-pw"));

    let lazy = Database::connect_lazy(&config).unwrap();
    assert_eq!(lazy.health().await, DatabaseHealth::Unavailable);
}
