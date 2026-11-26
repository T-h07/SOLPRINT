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
        let solana_fallback_rpc_url = r.https_url(
            "SOLANA_FALLBACK_RPC_URL",
            "https://api.mainnet-beta.solana.com",
        );

        if !r.issues.is_empty() {
            return Err(ConfigError::Invalid(r.issues));
        }

        // Every `None` above recorded an issue, so these are present.
        let (Some(database_url), Some(dexscreener_base_url), Some(solana_fallback_rpc_url)) =
            (database_url, dexscreener_base_url, solana_fallback_rpc_url)
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
             dexscreener={} fallback_rpc={}",
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
        )
    }
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
        let raw = self.get(key).unwrap_or_else(|| default.to_owned());
        match Url::parse(&raw) {
            Ok(url) if url.scheme() == "https" && url.has_host() => Some(url),
            Ok(_) => {
                self.issue(key, "must be an https:// URL with a host");
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
