//! GATEWAY (Phase 01): the sole, typed, cost-aware access layer to external
//! market and chain data providers. Read-only by construction: nothing here
//! builds, signs or submits transactions.
//!
//! See `docs/architecture/GATEWAY.md`.

pub mod breaker;
pub mod budget;
pub mod cache;
pub mod coalesce;
pub mod context;
pub mod dexscreener;
pub mod engine;
pub mod error;
pub mod gateway;
pub mod health;
pub mod http_json;
pub mod jsonrpc;
pub mod jupiter;
pub mod metrics;
pub mod models;
pub mod numbers;
pub mod operations;
pub mod ratelimit;
pub mod rpc;
pub mod scheduler;
pub mod settings;
pub mod snapshot;
pub mod store;
pub mod stream;
pub mod traits;
pub mod transport;

pub use context::{
    CachePolicy, Freshness, GatewayResponse, Priority, Provenance, ProviderId, RequestContext,
};
pub use error::GatewayError;
pub use gateway::Gateway;
pub use settings::GatewaySettings;
pub use snapshot::GatewaySnapshot;
pub use store::{GatewayStore, MemoryStore};
pub use traits::{
    BlockchainProvider, BlockchainStreamProvider, MarketDataProvider, SwapQuoteProvider,
    TokenMetadataProvider, TokenPriceProvider,
};
