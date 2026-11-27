//! RADAR domain events (written to `radar_outbox` in the same transaction as
//! the registry change they describe). Payloads carry identifiers, states and
//! reasons, never provider responses, secrets or personal data.

use serde_json::{Value, json};
use solprint_core::UtcTimestamp;
use solprint_storage::radar::RadarEventWrite;
use uuid::Uuid;

pub const SCHEMA: i16 = 1;

pub const WALLET_CANDIDATE_DISCOVERED: &str = "WalletCandidateDiscovered";
pub const WALLET_LIFECYCLE_CHANGED: &str = "WalletLifecycleChanged";
pub const WALLET_QUALIFICATION_CHANGED: &str = "WalletQualificationChanged";
pub const WALLET_PRIORITY_CHANGED: &str = "WalletPriorityChanged";
pub const WALLET_MONITORING_CHANGED: &str = "WalletMonitoringChanged";
pub const WALLET_REACTIVATED: &str = "WalletReactivated";

pub const ALL: [&str; 6] = [
    WALLET_CANDIDATE_DISCOVERED,
    WALLET_LIFECYCLE_CHANGED,
    WALLET_QUALIFICATION_CHANGED,
    WALLET_PRIORITY_CHANGED,
    WALLET_MONITORING_CHANGED,
    WALLET_REACTIVATED,
];

/// Build a wallet event. `revision` is the wallet revision the change
/// produces; `(wallet, revision, type)` is unique, so a retried evaluation
/// never emits the same event twice.
#[must_use]
pub fn wallet_event(
    event_type: &str,
    address: &str,
    revision: i32,
    source: &str,
    causation: Option<Uuid>,
    payload: Value,
) -> RadarEventWrite {
    let mut p = payload;
    if let Value::Object(m) = &mut p {
        m.insert("wallet".to_owned(), json!(address));
        m.insert("schemaVersion".to_owned(), json!(SCHEMA));
    }
    RadarEventWrite {
        event_id: Uuid::new_v4(),
        event_type: event_type.to_owned(),
        schema_version: SCHEMA,
        aggregate_type: "WALLET".to_owned(),
        aggregate_id: address.to_owned(),
        aggregate_revision: revision.max(1),
        correlation_id: Uuid::new_v4(),
        causation_id: causation,
        source: source.to_owned(),
        observed_at: UtcTimestamp::now(),
        payload_json: p.to_string(),
    }
}
