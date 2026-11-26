//! Normalized event envelope (contract only).
//!
//! Phase 00 defines the shape every future module will use when it emits a
//! normalized event. There is intentionally no event bus, no persistence and
//! no ingestion pipeline here; those arrive with the DATA ENGINE.

use serde::{Deserialize, Serialize};

use crate::ids::{CorrelationId, EventId};
use crate::reason::ReasonCode;
use crate::time::UtcTimestamp;

/// Where a piece of data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DataSource {
    Helius,
    Jupiter,
    DexScreener,
    SolanaRpc,
    /// Produced by SOLPRINT itself (derived/computed data).
    Internal,
    /// Synthetic data from the fixture layer. Must always be labeled in the UI.
    Fixture,
}

/// Schema version of an event payload. Bumped on any breaking payload change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SchemaVersion(pub u16);

/// Quality / provenance metadata attached to an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataQuality {
    /// `true` when some expected fields or sources were missing.
    pub partial: bool,
    /// Reason codes explaining any degradation (empty when complete).
    pub reasons: Vec<ReasonCode>,
}

/// The normalized envelope wrapping every event payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope<T> {
    pub id: EventId,
    /// Stable dotted name, e.g. `chain.slot`, `wallet.trade_observed`.
    pub event_type: String,
    pub schema_version: SchemaVersion,
    pub source: DataSource,
    /// Event time at the source, when the source provides it.
    pub source_at: Option<UtcTimestamp>,
    /// When SOLPRINT observed the event.
    pub observed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
    pub quality: DataQuality,
    pub payload: T,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serializes_with_explicit_provenance() {
        let env = EventEnvelope {
            id: EventId::new(),
            event_type: "chain.slot".to_owned(),
            schema_version: SchemaVersion(1),
            source: DataSource::Fixture,
            source_at: None,
            observed_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
            quality: DataQuality {
                partial: false,
                reasons: vec![],
            },
            payload: serde_json::json!({ "slot": 1 }),
        };
        let v = serde_json::to_value(&env).unwrap();
        assert_eq!(v["source"], "FIXTURE");
        assert_eq!(v["schema_version"], 1);
        assert!(v["source_at"].is_null());
    }
}
