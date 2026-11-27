//! RADAR (Phase 03): autonomous, evidence-backed wallet discovery.
//!
//! RADAR finds Solana memecoin markets on its own, samples their recent
//! transactions **through the DATA ENGINE**, extracts the wallets that the
//! DATA ENGINE verified as swap owners, and keeps a durable, explainable
//! registry of research candidates that it investigates progressively under
//! a bounded provider budget.
//!
//! Read-only by construction: nothing here builds, signs or submits a
//! transaction, holds keys, or identifies the people behind wallets. A
//! candidate, its priority score and its qualification are research
//! bookkeeping, never trading signals. See `docs/architecture/RADAR.md`.

pub mod budget;
pub mod classify;
pub mod engine;
pub mod events;
pub mod feed;
pub mod fixtures;
pub mod lifecycle;
pub mod market;
pub mod model;
pub mod monitoring;
pub mod scoring;
pub mod screening;

pub use engine::{DataEngineLimits, Radar, RadarError, RadarHandle, RadarInfo, start};
pub use feed::{FixtureFeed, GatewayFeed, MarketFeed};
