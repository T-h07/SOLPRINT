//! Typed, read-only access to the RADAR registry for later phases.
//!
//! Phase 04 and later modules consume RADAR through this interface and the
//! RADAR outbox events (`events.rs`), never through RADAR's tables, its
//! scoring internals or provider responses. Everything returned is local,
//! point-in-time state with its policy version and coverage; nothing here
//! contacts a provider or changes the registry.
//!
//! A wallet's priority orders RADAR's own work. It is not a prediction or a
//! trading signal, and consumers must not treat it as one.

use solprint_core::UtcTimestamp;
use solprint_storage::radar::{EvidenceRow, WalletFilter, WalletRow, WalletSort};
use solprint_storage::{Database, StorageError};

use crate::model::{eligibility, lifecycle, qualification};

/// One registry entry as later phases see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletView {
    pub address: String,
    pub lifecycle: String,
    pub eligibility: String,
    pub qualification: Option<String>,
    /// 0–100 decimal string under `priority_policy`.
    pub priority_score: Option<String>,
    pub priority_policy: Option<String>,
    pub history_stage: String,
    pub coverage_state: String,
    pub coverage_start: Option<UtcTimestamp>,
    pub coverage_end: Option<UtcTimestamp>,
    pub monitoring_mode: String,
    pub last_verified_trade_at: Option<UtcTimestamp>,
    pub last_evaluated_at: Option<UtcTimestamp>,
    pub verified_swaps: u32,
    /// Registry revision; consumers deduplicate by `(address, revision)`.
    pub revision: u32,
}

impl WalletView {
    fn of(w: &WalletRow) -> Self {
        let n = |v: i32| u32::try_from(v.max(0)).unwrap_or(0);
        Self {
            address: w.address.clone(),
            lifecycle: w.lifecycle.clone(),
            eligibility: w.eligibility.clone(),
            qualification: w.qualification.clone(),
            priority_score: w.priority_score.clone(),
            priority_policy: w.priority_policy_version.clone(),
            history_stage: w.history_stage.clone(),
            coverage_state: w.history_coverage_state.clone(),
            coverage_start: w.history_coverage_start,
            coverage_end: w.history_coverage_end,
            monitoring_mode: w.monitoring_mode.clone(),
            last_verified_trade_at: w.last_verified_trade_at,
            last_evaluated_at: w.last_evaluated_at,
            verified_swaps: n(w.verified_swap_count),
            revision: n(w.revision),
        }
    }

    /// Ready for deeper analysis: eligible, qualified after full staged
    /// history, and not operator-excluded (excluded wallets are never
    /// eligible).
    #[must_use]
    pub fn ready_for_analysis(&self) -> bool {
        self.eligibility == eligibility::ELIGIBLE_CANDIDATE
            && self.qualification.as_deref() == Some(qualification::READY_FOR_DEEPER_ANALYSIS)
            && matches!(
                self.lifecycle.as_str(),
                lifecycle::PRIORITIZED | lifecycle::MONITORED
            )
    }
}

/// One verified swap that made RADAR record a wallet, with provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceView {
    /// DATA ENGINE swap id: fetch the normalized swap from the DATA ENGINE.
    pub swap_id: String,
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<UtcTimestamp>,
    pub protocol: String,
    pub finality: String,
    pub discovery_source: String,
    pub swap_revision: u32,
    pub valid: bool,
    pub invalidation_reason: Option<String>,
}

impl EvidenceView {
    fn of(e: &EvidenceRow) -> Self {
        Self {
            swap_id: e.swap_id.clone(),
            signature: e.signature.clone(),
            slot: e.slot,
            block_time: e.block_time,
            protocol: e.protocol.clone(),
            finality: e.finality.clone(),
            discovery_source: e.discovery_source.clone(),
            swap_revision: u32::try_from(e.swap_revision.max(0)).unwrap_or(0),
            valid: e.valid,
            invalidation_reason: e.invalidation_reason.clone(),
        }
    }
}

/// Read-only RADAR registry access.
#[derive(Debug, Clone)]
pub struct RadarQuery {
    db: Database,
}

impl RadarQuery {
    #[must_use]
    pub const fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn wallet(&self, address: &str) -> Result<Option<WalletView>, StorageError> {
        Ok(self
            .db
            .radar_wallet(address)
            .await?
            .as_ref()
            .map(WalletView::of))
    }

    /// Wallets ready for deeper analysis, highest priority first. `after` is
    /// the last `(priority_score, address)` of the previous page.
    pub async fn ready_for_analysis(
        &self,
        after: Option<(String, String)>,
        limit: i64,
    ) -> Result<Vec<WalletView>, StorageError> {
        let rows = self
            .db
            .radar_wallets(&WalletFilter {
                eligibility: Some(eligibility::ELIGIBLE_CANDIDATE.into()),
                qualification: Some(qualification::READY_FOR_DEEPER_ANALYSIS.into()),
                sort: WalletSort::Priority,
                after,
                limit,
                ..WalletFilter::default()
            })
            .await?;
        Ok(rows
            .iter()
            .map(WalletView::of)
            .filter(WalletView::ready_for_analysis)
            .collect())
    }

    /// Wallets with an active monitoring assignment.
    pub async fn monitored(&self, limit: i64) -> Result<Vec<WalletView>, StorageError> {
        let rows = self
            .db
            .radar_wallets(&WalletFilter {
                lifecycle: Some(lifecycle::MONITORED.into()),
                limit,
                ..WalletFilter::default()
            })
            .await?;
        Ok(rows.iter().map(WalletView::of).collect())
    }

    /// Discovery evidence, newest first. Invalidated evidence is included
    /// and labelled; consumers decide whether to use it.
    pub async fn evidence(
        &self,
        address: &str,
        limit: i64,
    ) -> Result<Vec<EvidenceView>, StorageError> {
        Ok(self
            .db
            .radar_wallet_evidence(address, limit)
            .await?
            .iter()
            .map(EvidenceView::of)
            .collect())
    }
}
