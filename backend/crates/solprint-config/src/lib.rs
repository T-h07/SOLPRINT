//! SOLPRINT configuration.
//!
//! # Precedence
//!
//! 1. Process environment variables (highest).
//! 2. The env file (`SOLPRINT_ENV_FILE`, else `./.env`, else `../.env`).
//!    Values already present in the process environment are never overridden.
//! 3. Built-in safe defaults for non-sensitive settings (lowest).
//!
//! # Guarantees
//!
//! * Every problem is reported at once, by key name, without the value.
//! * Secrets live in [`Secret`] and cannot be printed.
//! * Phase 00 is non-executing: `TRADING_MODE=live` or
//!   `LIVE_TRADING_ENABLED=true` is a hard configuration error.
//! * Provider credentials are optional; none are needed to start.

mod secret;

use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use solprint_core::TradingMode;
use url::Url;

pub use secret::Secret;

/// Deployment environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Test,
    Production,
}

impl AppEnv {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Test => "test",
            Self::Production => "production",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Pretty,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub app: AppConfig,
    pub http: HttpConfig,
    pub database: DatabaseConfig,
    pub log: LogConfig,
    pub trading: TradingConfig,
    pub providers: ProviderConfig,
    pub gateway: GatewayConfig,
    pub data_engine: DataEngineConfig,
    pub radar: RadarConfig,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub env: AppEnv,
    pub host: IpAddr,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub request_timeout: Duration,
    pub body_limit_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Full connection URL including credentials. Never log it; use
    /// [`DatabaseConfig::redacted_target`].
    pub url: Secret<Url>,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
    /// How long startup waits for PostgreSQL before failing.
    pub startup_timeout: Duration,
    /// Whether this process applies pending migrations at startup.
    pub run_migrations: bool,
}

impl DatabaseConfig {
    /// `host:port/database` without user, password or query parameters.
    #[must_use]
    pub fn redacted_target(&self) -> String {
        let url = self.url.expose();
        format!(
            "{}:{}{}",
            url.host_str().unwrap_or("?"),
            url.port().unwrap_or(5432),
            url.path()
        )
    }
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub level: LogLevel,
    pub format: LogFormat,
}

#[derive(Debug, Clone, Copy)]
pub struct TradingConfig {
    pub mode: TradingMode,
    pub live_trading_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub helius_api_key: Option<Secret<String>>,
    pub jupiter_api_key: Option<Secret<String>>,
    pub dexscreener_base_url: Url,
    /// May embed a provider key in its path or query, so it is treated as a
    /// secret. Use [`redacted_origin`] for diagnostics.
    pub solana_fallback_rpc_url: Secret<Url>,
}

/// GATEWAY (Phase 01): limits, budgets and provider switches. Defaults sit
/// below the documented free-tier ceilings in
/// `docs/architecture/PROVIDER_CAPABILITIES.md`.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub enabled: bool,
    pub default_deadline: Duration,
    pub connect_timeout: Duration,
    pub global_queue_limit: usize,
    pub provider_queue_limit: usize,
    pub max_response_bytes: usize,
    pub cache_max_entries: u64,
    pub flush_interval: Duration,
    pub usage_retention_days: u32,
    pub metrics_retention_days: u32,
    pub streams_enabled: bool,
    pub helius: HeliusConfig,
    pub jupiter: JupiterConfig,
    pub dexscreener: DexScreenerConfig,
    pub fallback: FallbackConfig,
}

/// Commitment the Data Engine retrieves transactions at. `processed` is not
/// offered: `getTransaction` does not accept it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalCommitment {
    Confirmed,
    Finalized,
}

impl RetrievalCommitment {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Finalized => "finalized",
        }
    }
}

/// DATA ENGINE settings (Phase 02). Every limit is a local ceiling, not a
/// statement about what the provider budget can afford.
#[derive(Debug, Clone)]
pub struct DataEngineConfig {
    pub enabled: bool,
    /// Subscribe enabled targets to live streams at startup (needs
    /// `GATEWAY_STREAMS_ENABLED=true`). Off by default.
    pub auto_ingest_enabled: bool,
    pub max_active_targets: usize,
    pub fetch_concurrency: usize,
    pub decode_concurrency: usize,
    /// Non-terminal ingestion jobs above which backfills pause and live
    /// intake reports saturation.
    pub queue_capacity: u64,
    pub job_lease: Duration,
    pub job_max_attempts: u32,
    pub default_commitment: RetrievalCommitment,
    pub finality_interval: Duration,
    pub finality_batch_size: usize,
    pub backfill_enabled: bool,
    pub max_concurrent_backfills: usize,
    pub backfill_page_size: u16,
    pub backfill_max_pages: u32,
    pub backfill_max_signatures: u32,
    pub price_enrichment_enabled: bool,
    pub price_max_age: Duration,
    pub outbox_batch_size: usize,
    pub outbox_poll_interval: Duration,
    /// How long the stored-aggregate snapshot served to monitoring routes is
    /// reused before it is recomputed.
    pub metrics_refresh: Duration,
    pub evidence_max_bytes: usize,
    /// Raw evidence of finalized transactions older than this is pruned
    /// (hash and normalized records kept). `0` keeps it indefinitely.
    pub evidence_retention_days: u32,
    pub job_retention_days: u32,
    pub outbox_retention_days: u32,
}

/// RADAR settings (Phase 03). Every count is a ceiling, never a target, and
/// every threshold is an experimental engineering default, not a validated
/// trading parameter. Effective limits are the stricter of these, the DATA
/// ENGINE's and GATEWAY's.
#[derive(Debug, Clone)]
pub struct RadarConfig {
    /// Local RADAR work (event consumption, screening, scoring, API).
    pub enabled: bool,
    /// Autonomous market discovery and provider-backed sampling and history.
    /// **Off by default**: turning it on is the owner's explicit opt-in to
    /// spend provider credits on discovery.
    pub auto_discovery_enabled: bool,
    pub market_discovery_interval: Duration,
    pub candidate_reevaluation_interval: Duration,
    pub registry_maintenance_interval: Duration,
    pub max_market_targets: u32,
    pub max_active_market_targets: u32,
    pub max_market_samples_per_cycle: u32,
    pub max_active_candidates: u32,
    pub max_investigating_candidates: u32,
    pub max_live_monitored_wallets: u32,
    pub max_concurrent_sample_jobs: u32,
    pub max_concurrent_history_jobs: u32,
    pub max_pending_jobs: u32,
    pub initial_sample_signatures: u32,
    pub expanded_sample_signatures: u32,
    pub history_stage_a_days: u32,
    pub history_stage_a_signatures: u32,
    pub history_stage_b_days: u32,
    pub history_stage_b_signatures: u32,
    pub history_stage_c_days: u32,
    pub history_stage_c_signatures: u32,
    /// Ceiling on RADAR's share of the currently available normal provider
    /// allowance, in basis points (not extra credits).
    pub normal_budget_share_bps: u32,
    /// Scheduling shares (basis points, sum 10 000): active established,
    /// emerging, existing Data Engine activity, historical, exploration.
    pub share_established_bps: u32,
    pub share_emerging_bps: u32,
    pub share_local_activity_bps: u32,
    pub share_historical_bps: u32,
    pub share_exploration_bps: u32,
    pub min_pool_liquidity_usd: u64,
    pub min_volume_24h_usd: u64,
    /// Minimum reported transactions in the **1-hour** window.
    pub min_txns_1h: u32,
    pub max_market_observation_age: Duration,
    pub min_qualifying_swaps: u32,
    pub min_distinct_trading_days: u32,
    pub min_matched_outcomes: u32,
    pub min_matched_coverage_bps: u32,
    pub qualification_max_inactive_days: u32,
    pub activity_half_life_days: u32,
    pub reduce_monitoring_after_days: u32,
    pub dormant_after_days: u32,
    pub archive_after_days: u32,
    pub monitoring_min_duration: Duration,
    pub monitoring_cooldown: Duration,
    pub market_observation_retention_days: u32,
    pub snapshot_retention_days: u32,
    pub job_retention_days: u32,
}

#[derive(Debug, Clone)]
pub struct HeliusConfig {
    /// Base RPC URL *without* the key; the key is appended at the point of use.
    pub rpc_url: Url,
    pub ws_url: Url,
    pub monthly_credits: u64,
    pub reserve_percent: u8,
    /// UTC day of month (1-28) on which the billing cycle starts.
    pub billing_cycle_start_day: u8,
    pub rpc_rps: u32,
    pub max_in_flight: usize,
    pub ws_max_subscriptions: usize,
    pub get_block_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct JupiterConfig {
    pub base_url: Url,
    /// Requests per minute for the shared general bucket (already capped to
    /// the keyless tier when no key is configured).
    pub rpm: u32,
    pub max_in_flight: usize,
    pub quote_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct DexScreenerConfig {
    pub rpm: u32,
    pub max_in_flight: usize,
    pub discovery_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct FallbackConfig {
    pub enabled: bool,
    pub rps: u32,
}

/// Scheme + host + port only; strips user info, path and query.
#[must_use]
pub fn redacted_origin(url: &Url) -> String {
    url.origin().ascii_serialization()
}

/// One configuration problem. Never contains the offending value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    pub key: &'static str,
    pub problem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("invalid configuration:{}", render_issues(.0))]
    Invalid(Vec<ConfigIssue>),
    #[error("could not read env file {path}: {reason}")]
    EnvFile { path: String, reason: String },
}

impl ConfigError {
    #[must_use]
    pub fn issues(&self) -> &[ConfigIssue] {
        match self {
            Self::Invalid(issues) => issues,
            Self::EnvFile { .. } => &[],
        }
    }

    #[must_use]
    pub fn has_issue(&self, key: &str) -> bool {
        self.issues().iter().any(|i| i.key == key)
    }
}

fn render_issues(issues: &[ConfigIssue]) -> String {
    issues.iter().fold(String::new(), |mut acc, i| {
        acc.push_str("\n  - ");
        acc.push_str(i.key);
        acc.push_str(": ");
        acc.push_str(&i.problem);
        acc
    })
}

impl Config {
    /// Load the env file (if any) into the process environment without
    /// overriding existing variables, then build the configuration from the
    /// process environment.
    pub fn load() -> Result<Self, ConfigError> {
        load_env_file()?;
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Build configuration from an arbitrary key lookup. Used directly by
    /// tests so they never depend on the real environment.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let mut r = Reader {
            lookup: &lookup,
            issues: Vec::new(),
        };

        let env = r.parse("APP_ENV", AppEnv::Development, |v| match v {
            "development" => Ok(AppEnv::Development),
            "test" => Ok(AppEnv::Test),
            "production" => Ok(AppEnv::Production),
            _ => Err("must be one of: development, test, production".into()),
        });
        let host: IpAddr = r.parse("APP_HOST", IpAddr::from([127, 0, 0, 1]), |v| {
            v.parse().map_err(|_| "must be an IP address".into())
        });
        let allow_non_loopback = r.bool("APP_ALLOW_NON_LOOPBACK", false);
        if !host.is_loopback() && !allow_non_loopback {
            r.issue(
                "APP_HOST",
                "non-loopback bind address requires APP_ALLOW_NON_LOOPBACK=true \
                 (only for container networking; Phase 00 is not approved for public exposure)",
            );
        }
        let port = r.range("APP_PORT", 8080_u64, 1, 65_535);

        let request_timeout = r.range("HTTP_REQUEST_TIMEOUT_SECS", 10, 1, 120);
        let body_limit_bytes = r.range("HTTP_BODY_LIMIT_BYTES", 65_536, 1_024, 1_048_576);

        let database_url = r
            .required("DATABASE_URL")
            .and_then(|raw| match Url::parse(&raw) {
                Ok(url) if matches!(url.scheme(), "postgres" | "postgresql") && url.has_host() => {
                    Some(url)
                }
                Ok(_) => {
                    r.issue("DATABASE_URL", "must be a postgres:// URL with a host");
                    None
                }
                Err(_) => {
                    r.issue("DATABASE_URL", "is not a valid URL");
                    None
                }
            });
        let max_connections = r.range("DATABASE_MAX_CONNECTIONS", 5, 1, 50);
        let acquire_timeout = r.range("DATABASE_ACQUIRE_TIMEOUT_SECS", 5, 1, 60);
        let startup_timeout = r.range("DATABASE_STARTUP_TIMEOUT_SECS", 30, 0, 300);
        let run_migrations = r.bool("DATABASE_RUN_MIGRATIONS", true);

        let level = r.parse("LOG_LEVEL", LogLevel::Info, |v| match v {
            "error" => Ok(LogLevel::Error),
            "warn" => Ok(LogLevel::Warn),
            "info" => Ok(LogLevel::Info),
            "debug" => Ok(LogLevel::Debug),
            "trace" => Ok(LogLevel::Trace),
            _ => Err("must be one of: error, warn, info, debug, trace".into()),
        });
        let default_format = if env == AppEnv::Development {
            LogFormat::Pretty
        } else {
            LogFormat::Json
        };
        let format = r.parse("LOG_FORMAT", default_format, |v| match v {
            "pretty" => Ok(LogFormat::Pretty),
            "json" => Ok(LogFormat::Json),
            _ => Err("must be one of: pretty, json".into()),
        });

        let mode = r.parse("TRADING_MODE", TradingMode::Paper, |v| {
            v.parse::<TradingMode>()
                .map_err(|_| "must be `paper` (or `live`, which Phase 00 rejects)".into())
        });
        if mode.is_live() {
            r.issue(
                "TRADING_MODE",
                "LIVE_TRADING_NOT_PERMITTED: live trading is not available in Phase 00; use `paper`",
            );
        }
        let live_trading_enabled = r.bool("LIVE_TRADING_ENABLED", false);
        if live_trading_enabled {
            r.issue(
                "LIVE_TRADING_ENABLED",
                "LIVE_TRADING_NOT_PERMITTED: must be `false` in Phase 00",
            );
        }

        let helius_api_key = r.optional_secret("HELIUS_API_KEY");
        let jupiter_api_key = r.optional_secret("JUPITER_API_KEY");
        let dexscreener_base_url =
            r.https_url("DEXSCREENER_BASE_URL", "https://api.dexscreener.com");
        let solana_fallback_rpc_url =
            r.https_url("SOLANA_FALLBACK_RPC_URL", "https://api.mainnet.solana.com");

        let gw_enabled = r.bool("GATEWAY_ENABLED", true);
        let gw_deadline = r.range("GATEWAY_DEFAULT_DEADLINE_MS", 10_000, 100, 60_000);
        let gw_connect = r.range("GATEWAY_CONNECT_TIMEOUT_MS", 5_000, 100, 30_000);
        let gw_global_q = r.range("GATEWAY_GLOBAL_QUEUE_LIMIT", 512, 1, 4_096);
        let gw_provider_q = r.range("GATEWAY_PROVIDER_QUEUE_LIMIT", 128, 1, 1_024);
        if gw_provider_q > gw_global_q {
            r.issue(
                "GATEWAY_PROVIDER_QUEUE_LIMIT",
                "must not exceed GATEWAY_GLOBAL_QUEUE_LIMIT",
            );
        }
        let gw_max_body = r.range("GATEWAY_MAX_RESPONSE_BYTES", 8_388_608, 65_536, 67_108_864);
        let gw_cache = r.range("GATEWAY_CACHE_MAX_ENTRIES", 10_000, 0, 1_000_000);
        let gw_flush = r.range("GATEWAY_FLUSH_INTERVAL_SECS", 30, 5, 600);
        let gw_usage_ret = r.range("GATEWAY_USAGE_RETENTION_DAYS", 90, 1, 3_650);
        let gw_metric_ret = r.range("GATEWAY_METRICS_RETENTION_DAYS", 14, 1, 365);
        let gw_streams = r.bool("GATEWAY_STREAMS_ENABLED", false);

        let helius_rpc_url = r.https_url("HELIUS_RPC_URL", "https://mainnet.helius-rpc.com");
        let helius_ws_url = r.wss_url("HELIUS_WS_URL", "wss://mainnet.helius-rpc.com");
        for (key, url) in [
            ("HELIUS_RPC_URL", &helius_rpc_url),
            ("HELIUS_WS_URL", &helius_ws_url),
        ] {
            if url
                .as_ref()
                .is_some_and(|u| u.query().is_some() || !u.username().is_empty())
            {
                r.issue(
                    key,
                    "must not contain a query or user info; set HELIUS_API_KEY instead",
                );
            }
        }
        let helius_credits = r.range("HELIUS_MONTHLY_CREDITS", 1_000_000, 0, 1_000_000_000);
        let helius_reserve = r.range("HELIUS_RESERVE_PERCENT", 20, 0, 90);
        let helius_cycle_day = r.range("HELIUS_BILLING_CYCLE_START_DAY", 1, 1, 28);
        let helius_rps = r.range("HELIUS_RPC_RPS", 5, 1, 10);
        let helius_in_flight = r.range("HELIUS_MAX_IN_FLIGHT", 4, 1, 16);
        let helius_ws_subs = r.range("HELIUS_WS_MAX_SUBSCRIPTIONS", 100, 1, 1_000);
        let helius_get_block = r.bool("HELIUS_GET_BLOCK_ENABLED", false);

        let jupiter_base = r.https_url("JUPITER_BASE_URL", "https://api.jup.ag");
        // Documented tiers: 60 RPM with a key, 30 RPM keyless.
        let jupiter_ceiling = if jupiter_api_key.is_some() { 60 } else { 30 };
        let jupiter_rpm = r.range("JUPITER_RPM", 50, 1, 60).min(jupiter_ceiling);
        let jupiter_in_flight = r.range("JUPITER_MAX_IN_FLIGHT", 2, 1, 4);
        let jupiter_quote = r.bool("JUPITER_QUOTE_ENABLED", true);

        let dex_rpm = r.range("DEXSCREENER_RPM", 240, 1, 300);
        let dex_in_flight = r.range("DEXSCREENER_MAX_IN_FLIGHT", 4, 1, 8);
        let dex_discovery = r.bool("DEXSCREENER_DISCOVERY_ENABLED", false);

        let fallback_enabled = r.bool("SOLANA_FALLBACK_ENABLED", true);
        let fallback_rps = r.range("SOLANA_FALLBACK_RPS", 2, 1, 4);

        let de_enabled = r.bool("DATA_ENGINE_ENABLED", true);
        let de_auto = r.bool("DATA_ENGINE_AUTO_INGEST_ENABLED", false);
        let de_targets = r.range("DATA_ENGINE_MAX_ACTIVE_TARGETS", 25, 1, 100);
        if de_targets > helius_ws_subs {
            r.issue(
                "DATA_ENGINE_MAX_ACTIVE_TARGETS",
                "must not exceed HELIUS_WS_MAX_SUBSCRIPTIONS",
            );
        }
        let de_fetch = r.range("DATA_ENGINE_FETCH_CONCURRENCY", 4, 1, 16);
        if de_fetch > helius_in_flight {
            r.issue(
                "DATA_ENGINE_FETCH_CONCURRENCY",
                "must not exceed HELIUS_MAX_IN_FLIGHT",
            );
        }
        let de_decode = r.range("DATA_ENGINE_DECODE_CONCURRENCY", 4, 1, 16);
        let de_queue = r.range("DATA_ENGINE_QUEUE_CAPACITY", 2_048, 16, 100_000);
        let de_lease = r.range("DATA_ENGINE_JOB_LEASE_SECONDS", 60, 10, 600);
        let de_attempts = r.range("DATA_ENGINE_JOB_MAX_ATTEMPTS", 5, 1, 20);
        let de_commitment = r.parse(
            "DATA_ENGINE_DEFAULT_COMMITMENT",
            RetrievalCommitment::Confirmed,
            |v| match v {
                "confirmed" => Ok(RetrievalCommitment::Confirmed),
                "finalized" => Ok(RetrievalCommitment::Finalized),
                _ => Err(
                    "must be `confirmed` or `finalized` (getTransaction rejects processed)".into(),
                ),
            },
        );
        let de_final_every = r.range("DATA_ENGINE_FINALITY_RECONCILE_INTERVAL_SECS", 15, 5, 3_600);
        let de_final_batch = r.range("DATA_ENGINE_FINALITY_BATCH_SIZE", 64, 1, 256);
        let de_backfill = r.bool("DATA_ENGINE_BACKFILL_ENABLED", true);
        let de_backfills = r.range("DATA_ENGINE_MAX_CONCURRENT_BACKFILLS", 1, 1, 2);
        let de_page = r.range("DATA_ENGINE_BACKFILL_PAGE_SIZE", 100, 1, 1_000);
        let de_pages = r.range("DATA_ENGINE_BACKFILL_MAX_PAGES", 10, 1, 100);
        let de_sigs = r.range("DATA_ENGINE_BACKFILL_MAX_SIGNATURES", 1_000, 1, 10_000);
        let de_price = r.bool("DATA_ENGINE_PRICE_ENRICHMENT_ENABLED", false);
        let de_price_age = r.range("DATA_ENGINE_PRICE_MAX_AGE_SECS", 15, 1, 300);
        let de_outbox_batch = r.range("DATA_ENGINE_OUTBOX_BATCH_SIZE", 100, 1, 1_000);
        let de_outbox_poll = r.range("DATA_ENGINE_OUTBOX_POLL_INTERVAL_MS", 250, 50, 10_000);
        let de_metrics = r.range("DATA_ENGINE_METRICS_REFRESH_SECS", 15, 1, 300);
        let de_evidence = r.range(
            "DATA_ENGINE_EVIDENCE_MAX_BYTES",
            1_048_576,
            65_536,
            8_388_608,
        );
        if de_evidence > gw_max_body {
            r.issue(
                "DATA_ENGINE_EVIDENCE_MAX_BYTES",
                "must not exceed GATEWAY_MAX_RESPONSE_BYTES",
            );
        }
        let de_evidence_days = r.range("DATA_ENGINE_EVIDENCE_RETENTION_DAYS", 90, 0, 3_650);
        let de_job_days = r.range("DATA_ENGINE_JOB_RETENTION_DAYS", 14, 1, 365);
        let de_outbox_days = r.range("DATA_ENGINE_OUTBOX_RETENTION_DAYS", 7, 1, 365);

        let radar = read_radar(&mut r, de_backfills, de_targets);

        if !r.issues.is_empty() {
            return Err(ConfigError::Invalid(r.issues));
        }

        // Every `None` above recorded an issue, so these are present.
        let (
            Some(database_url),
            Some(dexscreener_base_url),
            Some(solana_fallback_rpc_url),
            Some(helius_rpc_url),
            Some(helius_ws_url),
            Some(jupiter_base),
        ) = (
            database_url,
            dexscreener_base_url,
            solana_fallback_rpc_url,
            helius_rpc_url,
            helius_ws_url,
            jupiter_base,
        )
        else {
            return Err(ConfigError::Invalid(vec![ConfigIssue {
                key: "CONFIG",
                problem: "internal validation inconsistency".into(),
            }]));
        };

        Ok(Self {
            app: AppConfig {
                env,
                host,
                port: u16::try_from(port).unwrap_or(8080),
            },
            http: HttpConfig {
                request_timeout: Duration::from_secs(request_timeout),
                body_limit_bytes: usize::try_from(body_limit_bytes).unwrap_or(65_536),
            },
            database: DatabaseConfig {
                url: Secret::new(database_url),
                max_connections: u32::try_from(max_connections).unwrap_or(5),
                acquire_timeout: Duration::from_secs(acquire_timeout),
                startup_timeout: Duration::from_secs(startup_timeout),
                run_migrations,
            },
            log: LogConfig { level, format },
            trading: TradingConfig {
                mode,
                live_trading_enabled,
            },
            providers: ProviderConfig {
                helius_api_key,
                jupiter_api_key,
                dexscreener_base_url,
                solana_fallback_rpc_url: Secret::new(solana_fallback_rpc_url),
            },
            gateway: GatewayConfig {
                enabled: gw_enabled,
                default_deadline: Duration::from_millis(gw_deadline),
                connect_timeout: Duration::from_millis(gw_connect),
                global_queue_limit: to_usize(gw_global_q),
                provider_queue_limit: to_usize(gw_provider_q),
                max_response_bytes: to_usize(gw_max_body),
                cache_max_entries: gw_cache,
                flush_interval: Duration::from_secs(gw_flush),
                usage_retention_days: to_u32(gw_usage_ret),
                metrics_retention_days: to_u32(gw_metric_ret),
                streams_enabled: gw_streams,
                helius: HeliusConfig {
                    rpc_url: helius_rpc_url,
                    ws_url: helius_ws_url,
                    monthly_credits: helius_credits,
                    reserve_percent: u8::try_from(helius_reserve).unwrap_or(20),
                    billing_cycle_start_day: u8::try_from(helius_cycle_day).unwrap_or(1),
                    rpc_rps: to_u32(helius_rps),
                    max_in_flight: to_usize(helius_in_flight),
                    ws_max_subscriptions: to_usize(helius_ws_subs),
                    get_block_enabled: helius_get_block,
                },
                jupiter: JupiterConfig {
                    base_url: jupiter_base,
                    rpm: to_u32(jupiter_rpm),
                    max_in_flight: to_usize(jupiter_in_flight),
                    quote_enabled: jupiter_quote,
                },
                dexscreener: DexScreenerConfig {
                    rpm: to_u32(dex_rpm),
                    max_in_flight: to_usize(dex_in_flight),
                    discovery_enabled: dex_discovery,
                },
                fallback: FallbackConfig {
                    enabled: fallback_enabled,
                    rps: to_u32(fallback_rps),
                },
            },
            data_engine: DataEngineConfig {
                enabled: de_enabled,
                auto_ingest_enabled: de_auto,
                max_active_targets: to_usize(de_targets),
                fetch_concurrency: to_usize(de_fetch),
                decode_concurrency: to_usize(de_decode),
                queue_capacity: de_queue,
                job_lease: Duration::from_secs(de_lease),
                job_max_attempts: to_u32(de_attempts),
                default_commitment: de_commitment,
                finality_interval: Duration::from_secs(de_final_every),
                finality_batch_size: to_usize(de_final_batch),
                backfill_enabled: de_backfill,
                max_concurrent_backfills: to_usize(de_backfills),
                backfill_page_size: u16::try_from(de_page).unwrap_or(100),
                backfill_max_pages: to_u32(de_pages),
                backfill_max_signatures: to_u32(de_sigs),
                price_enrichment_enabled: de_price,
                price_max_age: Duration::from_secs(de_price_age),
                outbox_batch_size: to_usize(de_outbox_batch),
                outbox_poll_interval: Duration::from_millis(de_outbox_poll),
                metrics_refresh: Duration::from_secs(de_metrics),
                evidence_max_bytes: to_usize(de_evidence),
                evidence_retention_days: to_u32(de_evidence_days),
                job_retention_days: to_u32(de_job_days),
                outbox_retention_days: to_u32(de_outbox_days),
            },
            radar,
        })
    }

    /// A summary that is safe to log: no credentials, no full URLs.
    #[must_use]
    pub fn safe_summary(&self) -> SafeSummary<'_> {
        SafeSummary(self)
    }
}

/// `Display` adapter producing a single-line, secret-free description.
#[derive(Debug)]
pub struct SafeSummary<'a>(&'a Config);

impl fmt::Display for SafeSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = self.0;
        write!(
            f,
            "env={} bind={}:{} db={} db_max_connections={} migrations={} log={}/{:?} \
             trading_mode={} live_trading_enabled={} helius_key={} jupiter_key={} \
             dexscreener={} fallback_rpc={} gateway={} streams={} \
             helius_credits={}/month reserve={}% data_engine={} auto_ingest={} \
             commitment={} radar={} radar_auto_discovery={}",
            c.app.env.as_str(),
            c.app.host,
            c.app.port,
            c.database.redacted_target(),
            c.database.max_connections,
            c.database.run_migrations,
            c.log.level.as_str(),
            c.log.format,
            c.trading.mode,
            c.trading.live_trading_enabled,
            presence(c.providers.helius_api_key.is_some()),
            presence(c.providers.jupiter_api_key.is_some()),
            redacted_origin(&c.providers.dexscreener_base_url),
            redacted_origin(c.providers.solana_fallback_rpc_url.expose()),
            c.gateway.enabled,
            c.gateway.streams_enabled,
            c.gateway.helius.monthly_credits,
            c.gateway.helius.reserve_percent,
            c.data_engine.enabled,
            c.data_engine.auto_ingest_enabled,
            c.data_engine.default_commitment.as_str(),
            c.radar.enabled,
            c.radar.auto_discovery_enabled,
        )
    }
}

/// Read and cross-validate the RADAR keys. `de_backfills` and `de_targets`
/// are the DATA ENGINE limits RADAR's own ceilings must fit inside.
fn read_radar<F: Fn(&str) -> Option<String>>(
    r: &mut Reader<'_, F>,
    de_backfills: u64,
    de_targets: u64,
) -> RadarConfig {
    let secs = Duration::from_secs;
    let enabled = r.bool("RADAR_ENABLED", true);
    let auto = r.bool("RADAR_AUTO_DISCOVERY_ENABLED", false);
    let discovery = r.range("RADAR_MARKET_DISCOVERY_INTERVAL_SECS", 300, 60, 86_400);
    let reeval = r.range("RADAR_CANDIDATE_REEVALUATION_INTERVAL_SECS", 900, 60, 86_400);
    let maint = r.range("RADAR_REGISTRY_MAINTENANCE_INTERVAL_SECS", 3_600, 60, 86_400);
    let markets = r.range("RADAR_MAX_MARKET_TARGETS", 100, 1, 1_000);
    let active_markets = r.range("RADAR_MAX_ACTIVE_MARKET_TARGETS", 20, 1, 200);
    if active_markets > markets {
        r.issue(
            "RADAR_MAX_ACTIVE_MARKET_TARGETS",
            "must not exceed RADAR_MAX_MARKET_TARGETS",
        );
    }
    let samples_per_cycle = r.range("RADAR_MAX_MARKET_SAMPLES_PER_CYCLE", 5, 1, 50);
    let candidates = r.range("RADAR_MAX_ACTIVE_CANDIDATES", 500, 1, 10_000);
    let investigating = r.range("RADAR_MAX_INVESTIGATING_CANDIDATES", 25, 1, 500);
    if investigating > candidates {
        r.issue(
            "RADAR_MAX_INVESTIGATING_CANDIDATES",
            "must not exceed RADAR_MAX_ACTIVE_CANDIDATES",
        );
    }
    let live = r.range("RADAR_MAX_LIVE_MONITORED_WALLETS", 10, 0, 100);
    if live > de_targets {
        r.issue(
            "RADAR_MAX_LIVE_MONITORED_WALLETS",
            "must not exceed DATA_ENGINE_MAX_ACTIVE_TARGETS",
        );
    }
    let sample_jobs = r.range("RADAR_MAX_CONCURRENT_SAMPLE_JOBS", 2, 1, 8);
    let history_jobs = r.range("RADAR_MAX_CONCURRENT_HISTORY_JOBS", 2, 1, 8);
    if history_jobs > de_backfills.max(1) * 2 {
        r.issue(
            "RADAR_MAX_CONCURRENT_HISTORY_JOBS",
            "must not exceed twice DATA_ENGINE_MAX_CONCURRENT_BACKFILLS",
        );
    }
    let pending = r.range("RADAR_MAX_PENDING_JOBS", 1_000, 10, 100_000);
    let s_init = r.range("RADAR_INITIAL_SAMPLE_SIGNATURES", 25, 1, 1_000);
    let s_exp = r.range("RADAR_EXPANDED_SAMPLE_SIGNATURES", 100, 1, 1_000);
    if s_init > s_exp {
        r.issue(
            "RADAR_INITIAL_SAMPLE_SIGNATURES",
            "must not exceed RADAR_EXPANDED_SAMPLE_SIGNATURES",
        );
    }
    let a_days = r.range("RADAR_INITIAL_HISTORY_DAYS", 7, 1, 365);
    let a_sigs = r.range("RADAR_INITIAL_HISTORY_MAX_SIGNATURES", 50, 1, 10_000);
    let b_days = r.range("RADAR_EXPANDED_HISTORY_DAYS", 30, 1, 365);
    let b_sigs = r.range("RADAR_EXPANDED_HISTORY_MAX_SIGNATURES", 200, 1, 10_000);
    let c_days = r.range("RADAR_EXTENDED_HISTORY_DAYS", 90, 1, 365);
    let c_sigs = r.range("RADAR_EXTENDED_HISTORY_MAX_SIGNATURES", 500, 1, 10_000);
    if !(a_days <= b_days && b_days <= c_days) {
        r.issue(
            "RADAR_EXPANDED_HISTORY_DAYS",
            "history stages must not shrink (A <= B <= C days)",
        );
    }
    if !(a_sigs <= b_sigs && b_sigs <= c_sigs) {
        r.issue(
            "RADAR_EXPANDED_HISTORY_MAX_SIGNATURES",
            "history stages must not shrink (A <= B <= C signatures)",
        );
    }
    let share = r.range("RADAR_NORMAL_BUDGET_SHARE_BPS", 4_000, 0, 10_000);
    let est = r.range("RADAR_SHARE_ESTABLISHED_BPS", 3_500, 0, 10_000);
    let emg = r.range("RADAR_SHARE_EMERGING_BPS", 2_500, 0, 10_000);
    let loc = r.range("RADAR_SHARE_LOCAL_ACTIVITY_BPS", 2_000, 0, 10_000);
    let his = r.range("RADAR_SHARE_HISTORICAL_BPS", 1_000, 0, 10_000);
    let exp = r.range("RADAR_SHARE_EXPLORATION_BPS", 1_000, 0, 10_000);
    if est + emg + loc + his + exp != 10_000 {
        r.issue(
            "RADAR_SHARE_EXPLORATION_BPS",
            "the five RADAR_SHARE_*_BPS values must sum to 10000",
        );
    }
    if exp < 500 {
        r.issue(
            "RADAR_SHARE_EXPLORATION_BPS",
            "exploration must keep at least 500 bps (starvation floor)",
        );
    }
    let liq = r.range("RADAR_MIN_POOL_LIQUIDITY_USD", 5_000, 0, 1_000_000_000);
    let vol = r.range("RADAR_MIN_VOLUME_24H_USD", 10_000, 0, 1_000_000_000);
    let txns = r.range("RADAR_MIN_TXNS_1H", 20, 0, 1_000_000);
    let age = r.range("RADAR_MAX_MARKET_OBSERVATION_AGE_SECS", 300, 30, 86_400);
    let q_swaps = r.range("RADAR_MIN_QUALIFYING_SWAPS", 10, 1, 10_000);
    let q_days = r.range("RADAR_MIN_DISTINCT_TRADING_DAYS", 3, 1, 365);
    let q_out = r.range("RADAR_MIN_MATCHED_OUTCOMES", 3, 1, 10_000);
    let q_cov = r.range("RADAR_MIN_MATCHED_COVERAGE_BPS", 6_000, 0, 10_000);
    let q_inactive = r.range("RADAR_QUALIFICATION_MAX_INACTIVE_DAYS", 7, 1, 365);
    let half_life = r.range("RADAR_ACTIVITY_HALF_LIFE_DAYS", 7, 1, 365);
    let reduce = r.range("RADAR_REDUCE_MONITORING_AFTER_DAYS", 7, 1, 365);
    let dormant = r.range("RADAR_DORMANT_AFTER_DAYS", 14, 1, 365);
    let archive = r.range("RADAR_ARCHIVE_AFTER_DAYS", 60, 1, 3_650);
    if !(reduce <= dormant && dormant < archive) {
        r.issue(
            "RADAR_DORMANT_AFTER_DAYS",
            "requires REDUCE_MONITORING <= DORMANT < ARCHIVE days",
        );
    }
    let mon_min = r.range("RADAR_MONITORING_MIN_DURATION_SECS", 3_600, 60, 604_800);
    let mon_cool = r.range("RADAR_MONITORING_COOLDOWN_SECS", 3_600, 60, 604_800);
    let obs_days = r.range("RADAR_MARKET_OBSERVATION_RETENTION_DAYS", 30, 1, 3_650);
    let snap_days = r.range("RADAR_SNAPSHOT_RETENTION_DAYS", 90, 1, 3_650);
    let job_days = r.range("RADAR_JOB_RETENTION_DAYS", 30, 1, 3_650);
    RadarConfig {
        enabled,
        auto_discovery_enabled: auto,
        market_discovery_interval: secs(discovery),
        candidate_reevaluation_interval: secs(reeval),
        registry_maintenance_interval: secs(maint),
        max_market_targets: to_u32(markets),
        max_active_market_targets: to_u32(active_markets),
        max_market_samples_per_cycle: to_u32(samples_per_cycle),
        max_active_candidates: to_u32(candidates),
        max_investigating_candidates: to_u32(investigating),
        max_live_monitored_wallets: to_u32(live),
        max_concurrent_sample_jobs: to_u32(sample_jobs),
        max_concurrent_history_jobs: to_u32(history_jobs),
        max_pending_jobs: to_u32(pending),
        initial_sample_signatures: to_u32(s_init),
        expanded_sample_signatures: to_u32(s_exp),
        history_stage_a_days: to_u32(a_days),
        history_stage_a_signatures: to_u32(a_sigs),
        history_stage_b_days: to_u32(b_days),
        history_stage_b_signatures: to_u32(b_sigs),
        history_stage_c_days: to_u32(c_days),
        history_stage_c_signatures: to_u32(c_sigs),
        normal_budget_share_bps: to_u32(share),
        share_established_bps: to_u32(est),
        share_emerging_bps: to_u32(emg),
        share_local_activity_bps: to_u32(loc),
        share_historical_bps: to_u32(his),
        share_exploration_bps: to_u32(exp),
        min_pool_liquidity_usd: liq,
        min_volume_24h_usd: vol,
        min_txns_1h: to_u32(txns),
        max_market_observation_age: secs(age),
        min_qualifying_swaps: to_u32(q_swaps),
        min_distinct_trading_days: to_u32(q_days),
        min_matched_outcomes: to_u32(q_out),
        min_matched_coverage_bps: to_u32(q_cov),
        qualification_max_inactive_days: to_u32(q_inactive),
        activity_half_life_days: to_u32(half_life),
        reduce_monitoring_after_days: to_u32(reduce),
        dormant_after_days: to_u32(dormant),
        archive_after_days: to_u32(archive),
        monitoring_min_duration: secs(mon_min),
        monitoring_cooldown: secs(mon_cool),
        market_observation_retention_days: to_u32(obs_days),
        snapshot_retention_days: to_u32(snap_days),
        job_retention_days: to_u32(job_days),
    }
}

/// Values were range-checked by [`Reader::range`], so these never saturate.
fn to_usize(v: u64) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

fn to_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

const fn presence(configured: bool) -> &'static str {
    if configured { "configured" } else { "absent" }
}

struct Reader<'a, F> {
    lookup: &'a F,
    issues: Vec<ConfigIssue>,
}

impl<F: Fn(&str) -> Option<String>> Reader<'_, F> {
    fn issue(&mut self, key: &'static str, problem: impl Into<String>) {
        self.issues.push(ConfigIssue {
            key,
            problem: problem.into(),
        });
    }

    /// Trimmed value; empty is treated as unset.
    fn get(&self, key: &str) -> Option<String> {
        (self.lookup)(key)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    }

    fn required(&mut self, key: &'static str) -> Option<String> {
        let value = self.get(key);
        if value.is_none() {
            self.issue(key, "is required");
        }
        value
    }

    fn parse<T>(
        &mut self,
        key: &'static str,
        default: T,
        f: impl FnOnce(&str) -> Result<T, String>,
    ) -> T {
        match self.get(key) {
            None => default,
            Some(raw) => match f(&raw.to_ascii_lowercase()) {
                Ok(v) => v,
                Err(problem) => {
                    self.issue(key, problem);
                    default
                }
            },
        }
    }

    fn bool(&mut self, key: &'static str, default: bool) -> bool {
        self.parse(key, default, |v| match v {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err("must be `true` or `false`".into()),
        })
    }

    fn range(&mut self, key: &'static str, default: u64, min: u64, max: u64) -> u64 {
        self.parse(key, default, |v| match v.parse::<u64>() {
            Ok(n) if (min..=max).contains(&n) => Ok(n),
            _ => Err(format!("must be an integer in {min}..={max}")),
        })
    }

    fn optional_secret(&mut self, key: &'static str) -> Option<Secret<String>> {
        let value = self.get(key)?;
        if value.chars().any(char::is_whitespace) {
            self.issue(key, "must not contain whitespace");
            return None;
        }
        Some(Secret::new(value))
    }

    fn https_url(&mut self, key: &'static str, default: &str) -> Option<Url> {
        self.secure_url(key, default, "https")
    }

    fn wss_url(&mut self, key: &'static str, default: &str) -> Option<Url> {
        self.secure_url(key, default, "wss")
    }

    fn secure_url(&mut self, key: &'static str, default: &str, scheme: &str) -> Option<Url> {
        let raw = self.get(key).unwrap_or_else(|| default.to_owned());
        match Url::parse(&raw) {
            Ok(url) if url.scheme() == scheme && url.has_host() => Some(url),
            Ok(_) => {
                self.issue(key, format!("must be a {scheme}:// URL with a host"));
                None
            }
            Err(_) => {
                self.issue(key, "is not a valid URL");
                None
            }
        }
    }
}

fn env_file_candidates() -> Vec<PathBuf> {
    if let Ok(explicit) = std::env::var("SOLPRINT_ENV_FILE") {
        return vec![PathBuf::from(explicit)];
    }
    vec![PathBuf::from(".env"), PathBuf::from("../.env")]
}

fn load_env_file() -> Result<(), ConfigError> {
    let explicit = std::env::var_os("SOLPRINT_ENV_FILE").is_some();
    for path in env_file_candidates() {
        if path.is_file() {
            return load_env_path(&path);
        }
        if explicit {
            return Err(ConfigError::EnvFile {
                path: path.display().to_string(),
                reason: "file not found".into(),
            });
        }
    }
    Ok(())
}

fn load_env_path(path: &Path) -> Result<(), ConfigError> {
    dotenvy::from_path(path).map_err(|e| ConfigError::EnvFile {
        path: path.display().to_string(),
        // dotenvy's own message can echo the offending line (which may hold
        // a secret), so only the line number is reported.
        reason: match e {
            dotenvy::Error::LineParse(_, index) => format!("syntax error at position {index}"),
            dotenvy::Error::Io(io) => io.kind().to_string(),
            _ => "unreadable".into(),
        },
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const DB: &str = "postgres://solprint_app:synthetic-db-password-123@127.0.0.1:55432/solprint";

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("DATABASE_URL".into(), DB.into());
        for (k, v) in pairs {
            map.insert((*k).into(), (*v).into());
        }
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn minimal_config_uses_safe_defaults_and_needs_no_provider_keys() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.app.env, AppEnv::Development);
        assert!(c.app.host.is_loopback());
        assert_eq!(c.app.port, 8080);
        assert_eq!(c.trading.mode, TradingMode::Paper);
        assert!(!c.trading.live_trading_enabled);
        assert!(c.providers.helius_api_key.is_none());
        assert!(c.providers.jupiter_api_key.is_none());
        assert_eq!(c.database.max_connections, 5);
        assert_eq!(c.log.format, LogFormat::Pretty);
    }

    #[test]
    fn data_engine_defaults_are_conservative() {
        let c = cfg(&[]).unwrap();
        let d = &c.data_engine;
        assert!(!d.auto_ingest_enabled, "live intake must be opt-in");
        assert!(
            !d.price_enrichment_enabled,
            "price enrichment must be opt-in"
        );
        assert_eq!(d.default_commitment, RetrievalCommitment::Confirmed);
        assert_eq!(d.max_concurrent_backfills, 1);
        assert_eq!(d.backfill_max_pages, 10);
        assert_eq!(d.backfill_max_signatures, 1_000);
        assert!(d.fetch_concurrency <= c.gateway.helius.max_in_flight);
        assert!(d.max_active_targets <= c.gateway.helius.ws_max_subscriptions);
        assert!(d.evidence_max_bytes <= c.gateway.max_response_bytes);
    }

    #[test]
    fn radar_defaults_are_conservative_and_discovery_is_opt_in() {
        let c = cfg(&[]).unwrap();
        let r = &c.radar;
        assert!(r.enabled);
        assert!(
            !r.auto_discovery_enabled,
            "provider-backed discovery must be opt-in"
        );
        assert_eq!(r.normal_budget_share_bps, 4_000);
        assert_eq!(
            r.share_established_bps
                + r.share_emerging_bps
                + r.share_local_activity_bps
                + r.share_historical_bps
                + r.share_exploration_bps,
            10_000
        );
        assert!(r.max_live_monitored_wallets as usize <= c.data_engine.max_active_targets);
        assert!(r.history_stage_a_signatures <= r.history_stage_b_signatures);
        assert!(r.dormant_after_days < r.archive_after_days);
    }

    #[test]
    fn radar_rejects_inconsistent_limits() {
        for (pairs, key) in [
            (
                vec![("RADAR_SHARE_EXPLORATION_BPS", "2000")],
                "RADAR_SHARE_EXPLORATION_BPS",
            ),
            (
                vec![
                    ("RADAR_SHARE_EXPLORATION_BPS", "0"),
                    ("RADAR_SHARE_ESTABLISHED_BPS", "4500"),
                ],
                "RADAR_SHARE_EXPLORATION_BPS",
            ),
            (
                vec![("RADAR_MAX_ACTIVE_MARKET_TARGETS", "150")],
                "RADAR_MAX_ACTIVE_MARKET_TARGETS",
            ),
            (
                vec![("RADAR_MAX_LIVE_MONITORED_WALLETS", "30")],
                "RADAR_MAX_LIVE_MONITORED_WALLETS",
            ),
            (
                vec![("RADAR_INITIAL_HISTORY_MAX_SIGNATURES", "300")],
                "RADAR_EXPANDED_HISTORY_MAX_SIGNATURES",
            ),
            (
                vec![("RADAR_DORMANT_AFTER_DAYS", "90")],
                "RADAR_DORMANT_AFTER_DAYS",
            ),
            (
                vec![("RADAR_NORMAL_BUDGET_SHARE_BPS", "10001")],
                "RADAR_NORMAL_BUDGET_SHARE_BPS",
            ),
            (
                vec![("RADAR_MARKET_DISCOVERY_INTERVAL_SECS", "5")],
                "RADAR_MARKET_DISCOVERY_INTERVAL_SECS",
            ),
        ] {
            let err = cfg(&pairs).unwrap_err().to_string();
            assert!(err.contains(key), "{pairs:?}: {err}");
        }
    }

    #[test]
    fn data_engine_rejects_processed_commitment_and_out_of_range_values() {
        for (k, v) in [
            ("DATA_ENGINE_DEFAULT_COMMITMENT", "processed"),
            ("DATA_ENGINE_FETCH_CONCURRENCY", "0"),
            ("DATA_ENGINE_MAX_CONCURRENT_BACKFILLS", "3"),
            ("DATA_ENGINE_BACKFILL_PAGE_SIZE", "1001"),
            ("DATA_ENGINE_JOB_MAX_ATTEMPTS", "21"),
            ("DATA_ENGINE_EVIDENCE_MAX_BYTES", "1024"),
        ] {
            let err = cfg(&[(k, v)]).unwrap_err().to_string();
            assert!(err.contains(k), "{k}={v}: {err}");
        }
    }

    #[test]
    fn data_engine_limits_must_fit_gateway_limits() {
        let err = cfg(&[
            ("DATA_ENGINE_FETCH_CONCURRENCY", "16"),
            ("HELIUS_MAX_IN_FLIGHT", "4"),
        ])
        .unwrap_err()
        .to_string();
        assert!(err.contains("DATA_ENGINE_FETCH_CONCURRENCY"), "{err}");
        let c = cfg(&[("DATA_ENGINE_DEFAULT_COMMITMENT", "finalized")]).unwrap();
        assert_eq!(c.data_engine.default_commitment.as_str(), "finalized");
    }

    #[test]
    fn gateway_defaults_stay_below_documented_free_tier_ceilings() {
        let c = cfg(&[]).unwrap();
        let g = &c.gateway;
        assert!(g.enabled);
        assert!(!g.streams_enabled, "streams must be opt-in");
        assert_eq!(g.global_queue_limit, 512);
        assert_eq!(g.provider_queue_limit, 128);
        assert_eq!(g.helius.monthly_credits, 1_000_000);
        assert_eq!(g.helius.reserve_percent, 20);
        assert!(g.helius.rpc_rps <= 10);
        assert_eq!(g.helius.max_in_flight, 4);
        assert!(!g.helius.get_block_enabled);
        // No Jupiter key: capped to the keyless tier.
        assert_eq!(g.jupiter.rpm, 30);
        assert_eq!(g.jupiter.max_in_flight, 2);
        assert!(g.dexscreener.rpm <= 300);
        assert!(!g.dexscreener.discovery_enabled);
        assert_eq!(g.fallback.rps, 2);
        assert_eq!(
            c.providers.solana_fallback_rpc_url.expose().as_str(),
            "https://api.mainnet.solana.com/"
        );
        let with_key = cfg(&[("JUPITER_API_KEY", "synthetic-key"), ("JUPITER_RPM", "60")]).unwrap();
        assert_eq!(with_key.gateway.jupiter.rpm, 60);
    }

    #[test]
    fn gateway_limits_are_validated() {
        let err = cfg(&[
            ("HELIUS_RPC_RPS", "11"),
            ("HELIUS_RESERVE_PERCENT", "95"),
            ("HELIUS_BILLING_CYCLE_START_DAY", "31"),
            ("HELIUS_WS_URL", "ws://mainnet.helius-rpc.com"),
            (
                "HELIUS_RPC_URL",
                "https://mainnet.helius-rpc.com/?api-key=synthetic-inline-key",
            ),
            ("JUPITER_RPM", "61"),
            ("DEXSCREENER_RPM", "301"),
            ("GATEWAY_PROVIDER_QUEUE_LIMIT", "1024"),
            ("GATEWAY_GLOBAL_QUEUE_LIMIT", "64"),
        ])
        .unwrap_err();
        for key in [
            "HELIUS_RPC_RPS",
            "HELIUS_RESERVE_PERCENT",
            "HELIUS_BILLING_CYCLE_START_DAY",
            "HELIUS_WS_URL",
            "HELIUS_RPC_URL",
            "JUPITER_RPM",
            "DEXSCREENER_RPM",
            "GATEWAY_PROVIDER_QUEUE_LIMIT",
        ] {
            assert!(err.has_issue(key), "missing issue for {key}: {err}");
        }
        assert!(!err.to_string().contains("synthetic-inline-key"));
    }

    #[test]
    fn database_url_is_required() {
        let err = Config::from_lookup(|_| None).unwrap_err();
        assert!(err.has_issue("DATABASE_URL"));
    }

    #[test]
    fn live_trading_is_rejected_both_ways() {
        let err = cfg(&[("TRADING_MODE", "live")]).unwrap_err();
        assert!(err.has_issue("TRADING_MODE"));
        assert!(err.to_string().contains("LIVE_TRADING_NOT_PERMITTED"));

        let err = cfg(&[("LIVE_TRADING_ENABLED", "true")]).unwrap_err();
        assert!(err.has_issue("LIVE_TRADING_ENABLED"));

        let err = cfg(&[("TRADING_MODE", "LIVE"), ("LIVE_TRADING_ENABLED", "TRUE")]).unwrap_err();
        assert_eq!(err.issues().len(), 2);
    }

    #[test]
    fn invalid_values_are_all_reported_by_key() {
        let err = cfg(&[
            ("APP_PORT", "70000"),
            ("APP_ENV", "staging"),
            ("LOG_LEVEL", "loud"),
            ("LOG_FORMAT", "xml"),
            ("DATABASE_MAX_CONNECTIONS", "0"),
            ("LIVE_TRADING_ENABLED", "yes"),
            ("TRADING_MODE", "simulate"),
            ("DEXSCREENER_BASE_URL", "http://api.dexscreener.com"),
            ("SOLANA_FALLBACK_RPC_URL", "not a url"),
            ("DATABASE_URL", "mysql://x@y/z"),
        ])
        .unwrap_err();
        for key in [
            "APP_PORT",
            "APP_ENV",
            "LOG_LEVEL",
            "LOG_FORMAT",
            "DATABASE_MAX_CONNECTIONS",
            "LIVE_TRADING_ENABLED",
            "TRADING_MODE",
            "DEXSCREENER_BASE_URL",
            "SOLANA_FALLBACK_RPC_URL",
            "DATABASE_URL",
        ] {
            assert!(err.has_issue(key), "missing issue for {key}: {err}");
        }
    }

    #[test]
    fn non_loopback_bind_requires_explicit_opt_in() {
        assert!(
            cfg(&[("APP_HOST", "0.0.0.0")])
                .unwrap_err()
                .has_issue("APP_HOST")
        );
        let c = cfg(&[("APP_HOST", "0.0.0.0"), ("APP_ALLOW_NON_LOOPBACK", "true")]).unwrap();
        assert!(!c.app.host.is_loopback());
    }

    #[test]
    fn empty_values_count_as_unset() {
        let c = cfg(&[("HELIUS_API_KEY", "  "), ("APP_PORT", "")]).unwrap();
        assert!(c.providers.helius_api_key.is_none());
        assert_eq!(c.app.port, 8080);
    }

    #[test]
    fn secrets_never_appear_in_debug_summary_or_errors() {
        let helius = "synthetic-helius-key-abc123";
        let jupiter = "synthetic-jupiter-key-def456";
        let rpc = "https://rpc.example.com/?api-key=synthetic-rpc-key-ghi789";
        let c = cfg(&[
            ("HELIUS_API_KEY", helius),
            ("JUPITER_API_KEY", jupiter),
            ("SOLANA_FALLBACK_RPC_URL", rpc),
        ])
        .unwrap();
        let rendered = format!("{c:?}\n{}", c.safe_summary());
        for secret in [
            helius,
            jupiter,
            "synthetic-rpc-key-ghi789",
            "synthetic-db-password-123",
            "solprint_app",
        ] {
            assert!(!rendered.contains(secret), "leaked {secret}");
        }
        assert!(rendered.contains("helius_key=configured"));
        assert!(rendered.contains("db=127.0.0.1:55432/solprint"));

        // Errors name the key, never echo the rejected value.
        let err = cfg(&[("HELIUS_API_KEY", "synthetic secret with spaces")]).unwrap_err();
        assert!(err.has_issue("HELIUS_API_KEY"));
        assert!(!err.to_string().contains("synthetic secret"));
        let err = cfg(&[("DATABASE_URL", "postgres://u:synthetic-pw@")]).unwrap_err();
        assert!(!err.to_string().contains("synthetic-pw"));
    }
}
