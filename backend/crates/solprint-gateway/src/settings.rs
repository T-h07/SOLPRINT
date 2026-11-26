//! GATEWAY settings, derived from validated configuration.
//!
//! Kept separate from `solprint_config::Config` so tests can point GATEWAY at
//! loopback mock servers (`allow_insecure_loopback`), which configuration
//! itself never permits.

use std::time::Duration;

use solprint_config::{Config, Secret};
use url::Url;

use crate::budget::BudgetPolicy;

#[derive(Debug, Clone)]
pub struct HeliusSettings {
    pub api_key: Option<Secret<String>>,
    pub rpc_url: Url,
    pub ws_url: Url,
    pub budget: BudgetPolicy,
    pub rps: u32,
    pub max_in_flight: usize,
    pub ws_max_subscriptions: usize,
    pub get_block_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct JupiterSettings {
    pub api_key: Option<Secret<String>>,
    pub base_url: Url,
    pub rpm: u32,
    pub max_in_flight: usize,
    pub quote_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct DexScreenerSettings {
    pub base_url: Url,
    pub rpm: u32,
    pub max_in_flight: usize,
    pub discovery_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct FallbackSettings {
    pub url: Secret<Url>,
    pub enabled: bool,
    pub rps: u32,
}

#[derive(Debug, Clone)]
pub struct GatewaySettings {
    pub enabled: bool,
    pub default_deadline: Duration,
    pub connect_timeout: Duration,
    pub max_response_bytes: usize,
    pub global_queue_limit: usize,
    pub provider_queue_limit: usize,
    pub cache_max_entries: u64,
    pub flush_interval: Duration,
    pub usage_retention: Duration,
    pub metrics_retention: Duration,
    pub streams_enabled: bool,
    /// WebSocket heartbeat: ping after this much silence, and treat the
    /// connection as dead if no pong follows within `stream_pong_timeout`.
    /// Not configurable from the environment; tests shorten it.
    pub stream_ping_interval: Duration,
    pub stream_pong_timeout: Duration,
    pub helius: HeliusSettings,
    pub jupiter: JupiterSettings,
    pub dexscreener: DexScreenerSettings,
    pub fallback: FallbackSettings,
    /// Tests only: allow plain-text loopback mock servers.
    pub allow_insecure_loopback: bool,
}

const DAY: Duration = Duration::from_secs(86_400);

impl GatewaySettings {
    #[must_use]
    pub fn from_config(c: &Config) -> Self {
        let g = &c.gateway;
        Self {
            enabled: g.enabled,
            default_deadline: g.default_deadline,
            connect_timeout: g.connect_timeout,
            max_response_bytes: g.max_response_bytes,
            global_queue_limit: g.global_queue_limit,
            provider_queue_limit: g.provider_queue_limit,
            cache_max_entries: g.cache_max_entries,
            flush_interval: g.flush_interval,
            usage_retention: DAY * g.usage_retention_days,
            metrics_retention: DAY * g.metrics_retention_days,
            streams_enabled: g.streams_enabled,
            stream_ping_interval: crate::stream::PING_INTERVAL,
            stream_pong_timeout: crate::stream::PONG_TIMEOUT,
            helius: HeliusSettings {
                api_key: c.providers.helius_api_key.clone(),
                rpc_url: g.helius.rpc_url.clone(),
                ws_url: g.helius.ws_url.clone(),
                budget: BudgetPolicy {
                    monthly_credits: g.helius.monthly_credits,
                    reserve_percent: g.helius.reserve_percent,
                    cycle_start_day: g.helius.billing_cycle_start_day,
                },
                rps: g.helius.rpc_rps,
                max_in_flight: g.helius.max_in_flight,
                ws_max_subscriptions: g.helius.ws_max_subscriptions,
                get_block_enabled: g.helius.get_block_enabled,
            },
            jupiter: JupiterSettings {
                api_key: c.providers.jupiter_api_key.clone(),
                base_url: g.jupiter.base_url.clone(),
                rpm: g.jupiter.rpm,
                max_in_flight: g.jupiter.max_in_flight,
                quote_enabled: g.jupiter.quote_enabled,
            },
            dexscreener: DexScreenerSettings {
                base_url: c.providers.dexscreener_base_url.clone(),
                rpm: g.dexscreener.rpm,
                max_in_flight: g.dexscreener.max_in_flight,
                discovery_enabled: g.dexscreener.discovery_enabled,
            },
            fallback: FallbackSettings {
                url: c.providers.solana_fallback_rpc_url.clone(),
                enabled: g.fallback.enabled,
                rps: g.fallback.rps,
            },
            allow_insecure_loopback: false,
        }
    }

    /// Origins the transport may contact.
    #[must_use]
    pub fn allowed_origins(&self) -> Vec<Url> {
        vec![
            self.helius.rpc_url.clone(),
            self.jupiter.base_url.clone(),
            self.dexscreener.base_url.clone(),
            self.fallback.url.expose().clone(),
        ]
    }
}
