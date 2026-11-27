//! DATA ENGINE (Phase 02): evidence-backed ingestion, reconstruction and
//! decoding of Solana transactions into normalized activities and
//! user-level swaps.
//!
//! Read-only by construction: nothing here builds, signs or submits
//! transactions, and every provider request goes through GATEWAY.
//! See `docs/architecture/DATA_ENGINE.md`.

pub mod attribution;
pub mod balances;
pub mod borsh;
pub mod decoders;
pub mod engine;
pub mod evidence;
pub mod fixtures;
pub mod mapping;
pub mod model;
pub mod movements;
pub mod normalize;
pub mod pipeline;
pub mod programs;
pub mod quality;
pub mod reconstruct;
pub mod source;

pub use decoders::Registry;
pub use model::{DECODER_VERSION, DecodedTransaction};
pub use pipeline::{DecodeError, decode, decode_tx};
