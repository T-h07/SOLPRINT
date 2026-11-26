//! Shared application state handed to request handlers.

use std::sync::Arc;
use std::time::Duration;

use solprint_config::Config;
use solprint_core::UtcTimestamp;
use solprint_storage::Database;
use tokio::time::Instant;

use crate::contracts::{
    CredentialState, ExecutionCapability, ProviderId, ProviderState, ProviderStatus,
    TradingModeDto, TradingState,
};

/// Build metadata baked in at compile time.
#[derive(Debug, Clone, Copy)]
pub struct BuildInfo {
    pub version: &'static str,
    pub git_commit: Option<&'static str>,
    pub profile: &'static str,
}

impl BuildInfo {
    /// Metadata of the current build. `SOLPRINT_GIT_COMMIT` is accepted only
    /// if it looks like a git hash, so arbitrary build-time strings never
    /// reach the API.
    #[must_use]
    pub fn current(version: &'static str) -> Self {
        let git_commit = option_env!("SOLPRINT_GIT_COMMIT")
            .filter(|c| (7..=40).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()));
        Self {
            version,
            git_commit,
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HttpLimits {
    pub request_timeout: Duration,
    pub body_limit_bytes: usize,
}

#[derive(Debug)]
struct Inner {
    environment: &'static str,
    build: BuildInfo,
    trading: TradingState,
    providers: Vec<ProviderStatus>,
    database: Database,
    started_at: UtcTimestamp,
    started_instant: Instant,
    http: HttpLimits,
}

/// Cheaply clonable handle (`Arc`) to immutable application state.
#[derive(Debug, Clone)]
pub struct AppState(Arc<Inner>);

impl AppState {
    #[must_use]
    pub fn new(config: &Config, database: Database, build: BuildInfo) -> Self {
        let credential = |configured: bool| {
            if configured {
                CredentialState::Configured
            } else {
                CredentialState::Absent
            }
        };
        let provider = |id, name: &str, role: &str, credential| ProviderStatus {
            id,
            name: name.to_owned(),
            role: role.to_owned(),
            state: ProviderState::NotInitialized,
            credential,
            last_checked_at: None,
        };
        let providers = vec![
            provider(
                ProviderId::Helius,
                "Helius",
                "Primary RPC and WebSocket",
                credential(config.providers.helius_api_key.is_some()),
            ),
            provider(
                ProviderId::Jupiter,
                "Jupiter",
                "Price and routing",
                credential(config.providers.jupiter_api_key.is_some()),
            ),
            provider(
                ProviderId::DexScreener,
                "DEX Screener",
                "Market data",
                CredentialState::NotRequired,
            ),
            provider(
                ProviderId::SolanaRpc,
                "Solana RPC",
                "Fallback RPC",
                CredentialState::NotRequired,
            ),
        ];
        let trading = TradingState {
            mode: if config.trading.mode.is_live() {
                TradingModeDto::Live
            } else {
                TradingModeDto::Paper
            },
            live_trading_enabled: config.trading.live_trading_enabled,
            execution_capability: ExecutionCapability::None,
        };
        Self(Arc::new(Inner {
            environment: config.app.env.as_str(),
            build,
            trading,
            providers,
            database,
            started_at: UtcTimestamp::now(),
            started_instant: Instant::now(),
            http: HttpLimits {
                request_timeout: config.http.request_timeout,
                body_limit_bytes: config.http.body_limit_bytes,
            },
        }))
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.0.database
    }

    #[must_use]
    pub fn environment(&self) -> &'static str {
        self.0.environment
    }

    #[must_use]
    pub fn build(&self) -> BuildInfo {
        self.0.build
    }

    #[must_use]
    pub fn trading_state(&self) -> TradingState {
        self.0.trading.clone()
    }

    #[must_use]
    pub fn providers(&self) -> &[ProviderStatus] {
        &self.0.providers
    }

    #[must_use]
    pub fn started_at(&self) -> UtcTimestamp {
        self.0.started_at
    }

    /// Uptime from the monotonic clock.
    #[must_use]
    pub fn uptime(&self) -> Duration {
        self.0.started_instant.elapsed()
    }

    #[must_use]
    pub fn http_limits(&self) -> HttpLimits {
        self.0.http
    }
}
