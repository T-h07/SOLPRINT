//! Shared SOLPRINT domain primitives.
//!
//! This crate is deliberately independent of HTTP, PostgreSQL and any
//! provider SDK. Everything here must be safe to depend on from every other
//! SOLPRINT module.
//!
//! Financial rule: no floating-point value is ever the authoritative
//! representation of a balance, amount, fee or accounting figure. Amounts are
//! integer base units with checked arithmetic; prices and ratios use
//! fixed-precision decimals. See `docs/architecture/DATA_CONVENTIONS.md`.

pub mod amount;
pub mod event;
pub mod freshness;
pub mod ids;
pub mod reason;
pub mod solana;
pub mod time;
pub mod trading;

pub use amount::{AmountError, BasisPoints, Lamports, Price, Rounding, TokenAmount};
pub use event::{DataQuality, DataSource, EventEnvelope, SchemaVersion};
pub use freshness::DataFreshness;
pub use ids::{CorrelationId, EventId};
pub use reason::{ReasonCode, ReasonCodeError};
pub use solana::{
    AddressError, CommitmentLevel, Slot, TokenMint, TransactionSignature, WalletAddress,
};
pub use time::{EventTimestamps, UtcTimestamp};
pub use trading::{TradingMode, TradingModeParseError};
