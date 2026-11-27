//! PostgreSQL storage foundation.
//!
//! * Bounded pool with acquisition timeouts.
//! * Startup connection with a deadline (fail closed if PostgreSQL never
//!   becomes reachable).
//! * Embedded, append-only SQLx migrations. The backend process is the
//!   migration owner (`DATABASE_RUN_MIGRATIONS=true`, the default); with it
//!   disabled, startup verifies that every embedded migration is applied
//!   and refuses to start otherwise.
//! * Runtime queries only (`sqlx::query`), so builds never need a database
//!   and there is no offline query metadata to maintain.

use std::time::Duration;

use solprint_config::DatabaseConfig;
use solprint_core::UtcTimestamp;
use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions};
use sqlx::types::Uuid;
use sqlx::{ConnectOptions, Row};
use tokio::time::{Instant, sleep, timeout};

pub mod data_engine;
pub mod gateway;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use gateway::{
    BillingCycleRow, MetricDelta, PruneOutcome, StreamCheckpointRow, UsageDelta, UsageHistoryRow,
};

/// All migrations, embedded at compile time from `backend/migrations`.
pub static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// Upper bound for a readiness probe so a hung database cannot hang `/health/ready`.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// Sanitized storage errors. Messages never include connection strings,
/// credentials or raw driver output.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database unavailable")]
    Unavailable,
    #[error("database did not become available within {0:?}")]
    StartupTimeout(Duration),
    #[error("migration failed: {0}")]
    Migration(String),
    #[error("database has {0} unapplied migration(s) and DATABASE_RUN_MIGRATIONS=false")]
    MigrationsPending(usize),
    #[error("database query failed")]
    Query,
}

impl StorageError {
    pub(crate) fn from_sqlx(err: &sqlx::Error) -> Self {
        match err {
            sqlx::Error::Io(_)
            | sqlx::Error::Tls(_)
            | sqlx::Error::PoolTimedOut
            | sqlx::Error::PoolClosed
            | sqlx::Error::WorkerCrashed => Self::Unavailable,
            _ => Self::Query,
        }
    }
}

/// Outcome of a database health probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseHealth {
    Available { latency: Duration },
    Unavailable,
}

/// The installation record maintained in `solprint.instance`.
#[derive(Debug, Clone)]
pub struct InstanceRecord {
    pub instance_id: Uuid,
    pub created_at: UtcTimestamp,
    pub start_count: i64,
}

#[derive(Debug, Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    fn pool_options(config: &DatabaseConfig) -> PgPoolOptions {
        PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(0)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(Some(Duration::from_secs(600)))
            .max_lifetime(Some(Duration::from_secs(1800)))
            .test_before_acquire(true)
    }

    fn connect_options(config: &DatabaseConfig) -> Result<PgConnectOptions, StorageError> {
        PgConnectOptions::from_url(config.url.expose())
            .map(|o| o.application_name("solprint"))
            .map_err(|_| StorageError::Unavailable)
    }

    /// Connect, retrying with backoff until `startup_timeout` elapses.
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, StorageError> {
        let options = Self::connect_options(config)?;
        let deadline = Instant::now() + config.startup_timeout;
        let mut backoff = Duration::from_millis(250);
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            match Self::pool_options(config)
                .connect_with(options.clone())
                .await
            {
                Ok(pool) => {
                    tracing::info!(
                        db = %config.redacted_target(),
                        max_connections = config.max_connections,
                        attempt,
                        "database connected"
                    );
                    return Ok(Self { pool });
                }
                Err(err) => {
                    let now = Instant::now();
                    if now >= deadline {
                        tracing::error!(
                            db = %config.redacted_target(),
                            attempt,
                            reason = %StorageError::from_sqlx(&err),
                            "database connection failed; giving up"
                        );
                        return Err(StorageError::StartupTimeout(config.startup_timeout));
                    }
                    tracing::warn!(
                        db = %config.redacted_target(),
                        attempt,
                        reason = %StorageError::from_sqlx(&err),
                        retry_in_ms = u64::try_from(backoff.as_millis()).unwrap_or(u64::MAX),
                        "database not reachable yet"
                    );
                    sleep(backoff.min(deadline - now)).await;
                    backoff = (backoff * 2).min(Duration::from_secs(5));
                }
            }
        }
    }

    /// Build a pool without connecting. Connections are opened on demand, so
    /// the process can start (and report not-ready) while PostgreSQL is down.
    /// Used by tests that exercise outage behaviour.
    pub fn connect_lazy(config: &DatabaseConfig) -> Result<Self, StorageError> {
        let options = Self::connect_options(config)?;
        Ok(Self {
            pool: Self::pool_options(config).connect_lazy_with(options),
        })
    }

    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Apply pending migrations. Already-applied migrations are verified by
    /// checksum; an edited historical migration aborts startup.
    pub async fn migrate(&self) -> Result<(), StorageError> {
        MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|e| StorageError::Migration(sanitize_migrate_error(&e)))?;
        tracing::info!(
            embedded = MIGRATOR.iter().count(),
            "database migrations up to date"
        );
        Ok(())
    }

    /// Fail unless every embedded migration has been applied successfully.
    pub async fn verify_migrations(&self) -> Result<(), StorageError> {
        let applied = self.applied_migration_versions().await?;
        let pending = MIGRATOR
            .iter()
            .filter(|m| !applied.contains(&m.version))
            .count();
        if pending == 0 {
            Ok(())
        } else {
            Err(StorageError::MigrationsPending(pending))
        }
    }

    /// Versions of successfully applied migrations, ascending.
    pub async fn applied_migration_versions(&self) -> Result<Vec<i64>, StorageError> {
        let exists: bool =
            sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
                .fetch_one(&self.pool)
                .await
                .map_err(|e| StorageError::from_sqlx(&e))?;
        if !exists {
            return Ok(Vec::new());
        }
        sqlx::query_scalar(
            "SELECT version FROM public._sqlx_migrations WHERE success ORDER BY version",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| StorageError::from_sqlx(&e))
    }

    /// Record this start in `solprint.instance` (created on first start).
    pub async fn register_start(&self, app_version: &str) -> Result<InstanceRecord, StorageError> {
        let row = sqlx::query(
            "INSERT INTO solprint.instance (singleton, last_app_version)
             VALUES (TRUE, $1)
             ON CONFLICT (singleton) DO UPDATE
                 SET last_started_at  = now(),
                     start_count      = solprint.instance.start_count + 1,
                     last_app_version = EXCLUDED.last_app_version
             RETURNING instance_id, created_at, start_count",
        )
        .bind(app_version)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| StorageError::from_sqlx(&e))?;
        Ok(InstanceRecord {
            instance_id: row.get("instance_id"),
            created_at: UtcTimestamp::from_offset_datetime(row.get("created_at")),
            start_count: row.get("start_count"),
        })
    }

    /// Cheap liveness probe of the database (`SELECT 1`), bounded in time.
    /// Latency is measured with the monotonic clock.
    pub async fn health(&self) -> DatabaseHealth {
        let started = Instant::now();
        let probe = sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&self.pool);
        match timeout(HEALTH_TIMEOUT, probe).await {
            Ok(Ok(1)) => DatabaseHealth::Available {
                latency: started.elapsed(),
            },
            _ => DatabaseHealth::Unavailable,
        }
    }

    /// Close the pool, waiting for checked-out connections to be returned.
    pub async fn close(&self) {
        self.pool.close().await;
        tracing::info!("database pool closed");
    }
}

fn sanitize_migrate_error(err: &sqlx::migrate::MigrateError) -> String {
    use sqlx::migrate::MigrateError;
    match err {
        MigrateError::VersionMismatch(v) => {
            format!("applied migration {v} was modified after being applied")
        }
        MigrateError::VersionMissing(v) => {
            format!("migration {v} is applied in the database but missing from this build")
        }
        MigrateError::Dirty(v) => format!("migration {v} previously failed part-way"),
        MigrateError::Execute(_) | MigrateError::ExecuteMigration(_, _) => {
            "a migration statement failed".to_owned()
        }
        _ => "migration error".to_owned(),
    }
}
