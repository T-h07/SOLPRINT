//! Runtime of the Data Engine: the durable job queue workers, finality
//! reconciliation, bounded backfills, live-stream intake, the outbox
//! dispatcher and maintenance. Every loop is cancellation-aware; on shutdown
//! leased work is returned to the queue without consuming attempts.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::Duration;

use solprint_config::DataEngineConfig;
use solprint_core::{CommitmentLevel, TransactionSignature, UtcTimestamp};
use solprint_gateway::Priority;
use solprint_gateway::models::SignatureQuery;
use solprint_gateway::stream::{
    StreamEvent, StreamEventKind, StreamHandle, SubscriptionId, SubscriptionSpec,
};
use solprint_storage::data_engine::{
    BackfillPage, DeBackfill, DeCounts, DeJob, IssueWrite, NewObservation, OutboxWrite,
    PersistOutcome,
};
use solprint_storage::{Database, StorageError};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::mapping::{self, EVENT_SCHEMA, EvidenceMeta, events};
use crate::model::DECODER_VERSION;
use crate::quality::{Issue, IssueCode};
use crate::source::{ChainSource, FetchedTx, SourceError, wallet};
use crate::{Registry, decode};

/// Built-in idempotent consumer: records delivery counts per event type.
pub const DIAGNOSTIC_CONSUMER: &str = "diagnostic_log";
const PERSIST_CONFLICT_RETRIES: usize = 3;
const BUDGET_PAUSE: Duration = Duration::from_secs(60);
const DISABLED_PAUSE: Duration = Duration::from_secs(300);
const IDLE_POLL: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(300);
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30);
const RETENTION_INTERVAL: Duration = Duration::from_secs(3_600);
/// How often live subscriptions are reconciled with the enabled targets.
const TARGET_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);
/// After this many inconclusive checks, finality tracking stops
/// (`FINALITY_UNKNOWN` stays visible; nothing is assumed).
const MAX_FINALITY_CHECKS: i32 = 20;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("data source: {0}")]
    Source(#[from] SourceError),
    #[error("invalid input: {0}")]
    Invalid(&'static str),
}

/// Process-local counters. Read by monitoring without touching providers.
#[derive(Debug, Default)]
pub struct Metrics {
    pub jobs_claimed: AtomicU64,
    pub fetched: AtomicU64,
    pub fetched_cached: AtomicU64,
    pub persisted: AtomicU64,
    pub unchanged: AtomicU64,
    pub conflicts: AtomicU64,
    pub lease_lost: AtomicU64,
    pub unavailable: AtomicU64,
    pub unsupported: AtomicU64,
    pub quarantined: AtomicU64,
    pub retries: AtomicU64,
    pub budget_pauses: AtomicU64,
    pub decode_failures: AtomicU64,
    pub credits_charged: AtomicU64,
    pub finality_checks: AtomicU64,
    pub finality_transitions: AtomicU64,
    pub stream_observations: AtomicU64,
    pub stream_gaps: AtomicU64,
    pub backfill_pages: AtomicU64,
    pub outbox_delivered: AtomicU64,
    pub decode_micros_total: AtomicU64,
    last_error: Mutex<Option<(&'static str, UtcTimestamp)>>,
    delivered_by_type: Mutex<BTreeMap<String, u64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetricsSnapshot {
    pub jobs_claimed: u64,
    pub fetched: u64,
    pub fetched_cached: u64,
    pub persisted: u64,
    pub unchanged: u64,
    pub conflicts: u64,
    pub lease_lost: u64,
    pub unavailable: u64,
    pub unsupported: u64,
    pub quarantined: u64,
    pub retries: u64,
    pub budget_pauses: u64,
    pub decode_failures: u64,
    pub credits_charged: u64,
    pub finality_checks: u64,
    pub finality_transitions: u64,
    pub stream_observations: u64,
    pub stream_gaps: u64,
    pub backfill_pages: u64,
    pub outbox_delivered: u64,
    pub decode_micros_total: u64,
    pub last_error_code: Option<String>,
    pub last_error_at: Option<UtcTimestamp>,
    pub delivered_by_type: Vec<(String, u64)>,
}

fn bump(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

impl Metrics {
    fn error(&self, code: &'static str) {
        *self
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some((code, UtcTimestamp::now()));
    }

    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
        let last = *self
            .last_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        MetricsSnapshot {
            jobs_claimed: g(&self.jobs_claimed),
            fetched: g(&self.fetched),
            fetched_cached: g(&self.fetched_cached),
            persisted: g(&self.persisted),
            unchanged: g(&self.unchanged),
            conflicts: g(&self.conflicts),
            lease_lost: g(&self.lease_lost),
            unavailable: g(&self.unavailable),
            unsupported: g(&self.unsupported),
            quarantined: g(&self.quarantined),
            retries: g(&self.retries),
            budget_pauses: g(&self.budget_pauses),
            decode_failures: g(&self.decode_failures),
            credits_charged: g(&self.credits_charged),
            finality_checks: g(&self.finality_checks),
            finality_transitions: g(&self.finality_transitions),
            stream_observations: g(&self.stream_observations),
            stream_gaps: g(&self.stream_gaps),
            backfill_pages: g(&self.backfill_pages),
            outbox_delivered: g(&self.outbox_delivered),
            decode_micros_total: g(&self.decode_micros_total),
            last_error_code: last.map(|(c, _)| c.to_owned()),
            last_error_at: last.map(|(_, t)| t),
            delivered_by_type: self
                .delivered_by_type
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect(),
        }
    }
}

/// What happened to one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobOutcome {
    Persisted { revision: i32 },
    Unchanged { revision: i32 },
    Retried,
    Released,
    Unavailable,
    Unsupported,
    Quarantined,
    LeaseLost,
}

/// Cached local aggregates for monitoring endpoints.
#[derive(Debug, Clone)]
pub struct CountsCache {
    pub at: UtcTimestamp,
    pub counts: DeCounts,
}

#[must_use]
pub const fn priority_of(rank: i16) -> Priority {
    match rank {
        i16::MIN..=0 => Priority::P0,
        1 => Priority::P1,
        2 => Priority::P2,
        3 => Priority::P3,
        4 => Priority::P4,
        _ => Priority::P5,
    }
}

#[must_use]
pub fn priority_rank(label: &str) -> i16 {
    match label {
        "P2" => 2,
        "P3" => 3,
        "P4" => 4,
        _ => 5,
    }
}

/// Exponential backoff: 2 s · 2^attempts, capped at five minutes.
#[must_use]
pub fn backoff(attempts: i32) -> Duration {
    let exp = u32::try_from(attempts.clamp(0, 8)).unwrap_or(8);
    Duration::from_secs(2u64.saturating_mul(1u64 << exp)).min(MAX_BACKOFF)
}

fn commitment_of(s: &str) -> CommitmentLevel {
    if s == "finalized" {
        CommitmentLevel::Finalized
    } else {
        CommitmentLevel::Confirmed
    }
}

fn commitment_label(c: Option<CommitmentLevel>) -> Option<String> {
    c.map(|c| {
        match c {
            CommitmentLevel::Processed => "processed",
            CommitmentLevel::Confirmed => "confirmed",
            CommitmentLevel::Finalized => "finalized",
        }
        .to_owned()
    })
}

fn later(d: Duration) -> UtcTimestamp {
    let now = UtcTimestamp::now();
    let t = now
        .as_offset_datetime()
        .checked_add(time::Duration::seconds(
            i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        ))
        .unwrap_or(*now.as_offset_datetime());
    UtcTimestamp::from_offset_datetime(t)
}

fn issue(subject: &str, code: IssueCode, detail: impl Into<String>) -> IssueWrite {
    let i = Issue::new(code, None, detail);
    IssueWrite {
        signature: if subject.len() >= 64 && !subject.contains(':') {
            Some(subject.to_owned())
        } else {
            None
        },
        target_id: None,
        backfill_id: None,
        code: i.code.as_str().to_owned(),
        severity: i.severity.as_str().to_owned(),
        detail: Some(i.detail.clone()),
        decoder_version: Some(DECODER_VERSION.to_owned()),
        dedup_key: i.dedup_key(subject),
    }
}

/// The engine, generic over its chain source (GATEWAY in production).
#[derive(Debug)]
pub struct Engine<S: ChainSource> {
    db: Database,
    source: Arc<S>,
    cfg: DataEngineConfig,
    registry: Registry,
    owner: Uuid,
    metrics: Arc<Metrics>,
    counts: RwLock<Option<CountsCache>>,
    extra_consumers: RwLock<Vec<String>>,
}

impl<S: ChainSource> Engine<S> {
    #[must_use]
    pub fn new(db: Database, source: Arc<S>, cfg: DataEngineConfig) -> Self {
        Self {
            db,
            source,
            cfg,
            registry: Registry::standard(),
            owner: Uuid::new_v4(),
            metrics: Arc::new(Metrics::default()),
            counts: RwLock::new(None),
            extra_consumers: RwLock::new(Vec::new()),
        }
    }

    /// Outbox consumers whose delivery gates outbox retention: the built-in
    /// diagnostic consumer plus every registered module consumer (RADAR).
    #[must_use]
    pub fn consumers(&self) -> Vec<String> {
        let mut c = vec![DIAGNOSTIC_CONSUMER.to_owned()];
        c.extend(
            self.extra_consumers
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .cloned(),
        );
        c
    }

    /// Register a module's outbox consumer so retention never deletes an
    /// event that consumer has not yet received.
    pub fn register_consumer(&self, name: &str) {
        let mut c = self
            .extra_consumers
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if !c.iter().any(|x| x == name) {
            c.push(name.to_owned());
        }
    }

    #[must_use]
    pub const fn owner(&self) -> Uuid {
        self.owner
    }

    #[must_use]
    pub const fn config(&self) -> &DataEngineConfig {
        &self.cfg
    }

    #[must_use]
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.db
    }

    #[must_use]
    pub fn counts(&self) -> Option<CountsCache> {
        self.counts
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn max_attempts(&self) -> i32 {
        i32::try_from(self.cfg.job_max_attempts.clamp(1, 20)).unwrap_or(5)
    }

    // ---- intake ---------------------------------------------------------------------------

    /// Queue one signature observed manually (CLI) or by a stream.
    #[allow(clippy::too_many_arguments)]
    pub async fn observe(
        &self,
        signature: &str,
        source: &str,
        target_id: Option<Uuid>,
        commitment: Option<CommitmentLevel>,
        slot: Option<u64>,
        failed_hint: Option<bool>,
        priority: i16,
    ) -> Result<solprint_storage::data_engine::EnqueueOutcome, EngineError> {
        TransactionSignature::parse(signature).map_err(|_| EngineError::Invalid("signature"))?;
        let depth = self.db.de_queue_depth().await?;
        if depth >= self.cfg.queue_capacity {
            self.db
                .de_record_issue(&issue(
                    "queue",
                    IssueCode::QueueSaturated,
                    format!("depth {depth}"),
                ))
                .await?;
            return Err(EngineError::Invalid("queue saturated"));
        }
        let target = target_id.map(|t| t.to_string()).unwrap_or_default();
        let obs = NewObservation {
            signature: signature.to_owned(),
            source: source.to_owned(),
            target_id,
            backfill_id: None,
            commitment: commitment_label(commitment),
            slot,
            failed_hint,
            stream_generation: None,
            observed_at: UtcTimestamp::now(),
            dedup_key: format!("{source}:{target}:{signature}"),
        };
        Ok(self
            .db
            .de_observe(
                &obs,
                priority,
                self.cfg.default_commitment.as_str(),
                self.max_attempts(),
            )
            .await?)
    }

    // ---- job processing --------------------------------------------------------------------

    async fn fetch_for(
        &self,
        job: &DeJob,
        cancel: &CancellationToken,
    ) -> Result<Option<FetchedTx>, SourceError> {
        if job.source == "REPROCESS"
            && let Ok(Some((_, commitment, provider, raw))) =
                self.db.de_canonical_evidence(&job.signature).await
            && commitment == job.required_commitment
        {
            let slot = crate::evidence::parse(&raw).map(|e| e.slot).unwrap_or(0);
            return Ok(Some(FetchedTx {
                raw_json: raw,
                slot,
                provider: match provider.as_str() {
                    "HELIUS" => "HELIUS",
                    "FIXTURE" => "FIXTURE",
                    _ => "SOLANA_RPC",
                },
                fetched_at: UtcTimestamp::now(),
                credits: 0,
                cached: true,
            }));
        }
        let sig =
            TransactionSignature::parse(&job.signature).map_err(|_| SourceError::Permanent {
                code: "INVALID_SIGNATURE",
            })?;
        let r = self
            .source
            .transaction(
                &sig,
                commitment_of(&job.required_commitment),
                priority_of(job.priority),
                job.source == "FINALITY",
                cancel,
            )
            .await;
        if let Ok(Some(f)) = &r {
            bump(&self.metrics.fetched);
            if f.cached {
                bump(&self.metrics.fetched_cached);
            }
            self.metrics
                .credits_charged
                .fetch_add(f.credits, Ordering::Relaxed);
        }
        r
    }

    async fn finish(
        &self,
        job: &DeJob,
        state: &'static str,
        code: IssueCode,
        detail: &str,
    ) -> Result<JobOutcome, EngineError> {
        let i = issue(&job.signature, code, detail);
        let ok = self
            .db
            .de_finish_job(job.job_id, self.owner, state, Some(code.as_str()), Some(&i))
            .await?;
        if !ok {
            bump(&self.metrics.lease_lost);
            return Ok(JobOutcome::LeaseLost);
        }
        Ok(match state {
            "UNAVAILABLE" => {
                bump(&self.metrics.unavailable);
                JobOutcome::Unavailable
            }
            "UNSUPPORTED" => {
                bump(&self.metrics.unsupported);
                JobOutcome::Unsupported
            }
            _ => {
                bump(&self.metrics.quarantined);
                JobOutcome::Quarantined
            }
        })
    }

    async fn retry(
        &self,
        job: &DeJob,
        code: &'static str,
        delay: Duration,
    ) -> Result<JobOutcome, EngineError> {
        bump(&self.metrics.retries);
        self.metrics.error(code);
        match self
            .db
            .de_retry_job(job.job_id, self.owner, code, delay)
            .await?
            .as_deref()
        {
            Some("QUARANTINED") => {
                bump(&self.metrics.quarantined);
                self.db
                    .de_record_issue(&issue(
                        &job.signature,
                        IssueCode::TransactionUnavailable,
                        format!("attempts exhausted: {code}"),
                    ))
                    .await?;
                Ok(JobOutcome::Quarantined)
            }
            Some(_) => Ok(JobOutcome::Retried),
            None => {
                bump(&self.metrics.lease_lost);
                Ok(JobOutcome::LeaseLost)
            }
        }
    }

    /// Process one leased job end to end.
    pub async fn process_job(
        &self,
        job: &DeJob,
        cancel: &CancellationToken,
    ) -> Result<JobOutcome, EngineError> {
        let fetched = match self.fetch_for(job, cancel).await {
            Ok(Some(f)) => f,
            Ok(None) => {
                // Not (yet) visible at the required commitment. Never inferred
                // as orphaned or failed.
                if job.attempts >= job.max_attempts {
                    return self
                        .finish(
                            job,
                            "UNAVAILABLE",
                            IssueCode::TransactionUnavailable,
                            "not returned at required commitment",
                        )
                        .await;
                }
                return self.retry(job, "TX_NOT_FOUND", backoff(job.attempts)).await;
            }
            Err(SourceError::Budget) => {
                bump(&self.metrics.budget_pauses);
                self.metrics.error("BUDGET_DENIED");
                self.db
                    .de_release_job(job.job_id, self.owner, BUDGET_PAUSE, Some("BUDGET_DENIED"))
                    .await?;
                return Ok(JobOutcome::Released);
            }
            Err(SourceError::Cancelled) => {
                self.db
                    .de_release_job(job.job_id, self.owner, Duration::ZERO, None)
                    .await?;
                return Ok(JobOutcome::Released);
            }
            Err(SourceError::Disabled { code }) => {
                self.metrics.error(code);
                self.db
                    .de_release_job(job.job_id, self.owner, DISABLED_PAUSE, Some(code))
                    .await?;
                return Ok(JobOutcome::Released);
            }
            Err(SourceError::UnsupportedVersion) => {
                return self
                    .finish(
                        job,
                        "UNSUPPORTED",
                        IssueCode::EvidenceParseFailed,
                        "unsupported transaction version",
                    )
                    .await;
            }
            Err(SourceError::Transient { code, retry_after }) => {
                return self
                    .retry(
                        job,
                        code,
                        retry_after
                            .unwrap_or_else(|| backoff(job.attempts))
                            .min(MAX_BACKOFF),
                    )
                    .await;
            }
            Err(SourceError::Permanent { code }) => {
                return self.retry(job, code, backoff(job.attempts)).await;
            }
        };
        if fetched.raw_json.len() > self.cfg.evidence_max_bytes {
            return self
                .finish(
                    job,
                    "QUARANTINED",
                    IssueCode::EvidenceTooLarge,
                    &format!("{} bytes", fetched.raw_json.len()),
                )
                .await;
        }
        let started = std::time::Instant::now();
        let decoded = match decode(&fetched.raw_json, &self.registry) {
            Ok(d) => d,
            Err(e) => {
                bump(&self.metrics.decode_failures);
                return self
                    .finish(job, "QUARANTINED", e.issue_code(), &e.to_string())
                    .await;
            }
        };
        self.metrics.decode_micros_total.fetch_add(
            u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        if decoded.signature != job.signature {
            return self
                .finish(
                    job,
                    "QUARANTINED",
                    IssueCode::EvidenceParseFailed,
                    "evidence signature mismatch",
                )
                .await;
        }
        let meta = EvidenceMeta {
            commitment: if job.required_commitment == "finalized" {
                "finalized"
            } else {
                "confirmed"
            },
            provider: fetched.provider,
            fetched_at: fetched.fetched_at,
            raw_json: fetched.raw_json,
            first_observed_at: job.created_at,
            next_finality_check_at: Some(later(self.cfg.finality_interval)),
            correlation_id: Uuid::new_v4(),
        };
        for _ in 0..PERSIST_CONFLICT_RETRIES {
            let state = self.db.de_tx_state(&job.signature).await?;
            let w = mapping::transaction_write(&decoded, &meta, &state);
            // Swaps that vanish in this revision are retracted with the
            // reason the schema records: new evidence, or a new decoder.
            let reason = mapping::retraction_reason(&state, &w.evidence.content_sha256);
            match self
                .db
                .de_persist_transaction(
                    &w,
                    state.current_revision,
                    Some((job.job_id, self.owner)),
                    reason,
                )
                .await?
            {
                PersistOutcome::Applied { revision } => {
                    bump(&self.metrics.persisted);
                    self.db
                        .de_observe_tokens(&mapping::token_observations(&decoded))
                        .await?;
                    return Ok(JobOutcome::Persisted { revision });
                }
                PersistOutcome::Unchanged { revision } => {
                    bump(&self.metrics.unchanged);
                    return Ok(JobOutcome::Unchanged { revision });
                }
                PersistOutcome::LeaseLost => {
                    bump(&self.metrics.lease_lost);
                    return Ok(JobOutcome::LeaseLost);
                }
                PersistOutcome::Conflict => bump(&self.metrics.conflicts),
            }
        }
        self.retry(job, "PERSIST_CONFLICT", backoff(0)).await
    }

    /// Claim and process one batch. Returns the number of jobs claimed.
    pub async fn run_batch(
        self: &Arc<Self>,
        cancel: &CancellationToken,
    ) -> Result<usize, EngineError> {
        if !self.source.admits(Priority::P2) {
            return Ok(0);
        }
        let limit = i64::try_from(self.cfg.fetch_concurrency.max(1)).unwrap_or(1);
        let jobs = self
            .db
            .de_claim_jobs(self.owner, limit, self.cfg.job_lease)
            .await?;
        let n = jobs.len();
        self.metrics
            .jobs_claimed
            .fetch_add(u64::try_from(n).unwrap_or(0), Ordering::Relaxed);
        let mut set = JoinSet::new();
        for job in jobs {
            let me = Arc::clone(self);
            let c = cancel.clone();
            set.spawn(async move {
                if c.is_cancelled() {
                    let _ = me.db.de_release_job(job.job_id, me.owner, Duration::ZERO, None).await;
                    return;
                }
                if let Err(e) = me.process_job(&job, &c).await {
                    tracing::warn!(target: "solprint::data_engine", job = job.job_id, error = %e, "job processing failed");
                    me.metrics.error("JOB_ERROR");
                    // Hand the job back with backoff instead of leaving it
                    // leased until the lease expires. Attempts still count, so
                    // a persistent failure ends in quarantine.
                    let _ = me
                        .db
                        .de_retry_job(job.job_id, me.owner, "JOB_ERROR", backoff(job.attempts))
                        .await;
                }
            });
        }
        while set.join_next().await.is_some() {}
        Ok(n)
    }

    // ---- finality ----------------------------------------------------------------------------

    /// One reconciliation pass over transactions whose check is due.
    pub async fn finality_tick(&self, cancel: &CancellationToken) -> Result<usize, EngineError> {
        if !self.source.admits(Priority::P4) {
            return Ok(0);
        }
        let limit = i64::try_from(self.cfg.finality_batch_size.clamp(1, 256)).unwrap_or(100);
        let due = self.db.de_finality_due(limit).await?;
        if due.is_empty() {
            return Ok(0);
        }
        let sigs: Vec<TransactionSignature> = due
            .iter()
            .filter_map(|r| TransactionSignature::parse(&r.signature).ok())
            .collect();
        if sigs.len() != due.len() {
            return Err(EngineError::Invalid("stored signature"));
        }
        let res = match self.source.signature_statuses(&sigs, cancel).await {
            Ok(r) => r,
            Err(SourceError::Budget) => {
                bump(&self.metrics.budget_pauses);
                return Ok(0);
            }
            Err(e) => {
                self.metrics.error(e.code());
                return Ok(0);
            }
        };
        self.metrics
            .credits_charged
            .fetch_add(res.credits, Ordering::Relaxed);
        for (row, status) in due.iter().zip(res.statuses.iter()) {
            bump(&self.metrics.finality_checks);
            let backoff_mult = u32::try_from(row.finality_checks.clamp(0, 6)).unwrap_or(6);
            let next = if row.finality_checks + 1 >= MAX_FINALITY_CHECKS {
                None
            } else {
                Some(later(
                    self.cfg
                        .finality_interval
                        .saturating_mul(1u32 << backoff_mult),
                ))
            };
            let (new, issue_w, refetch): (&str, Option<IssueWrite>, bool) = match status {
                Some(s) if s.slot != row.slot => (
                    "POSSIBLY_ORPHANED",
                    Some(issue(
                        &row.signature,
                        IssueCode::SlotMismatch,
                        format!("stored slot {} status slot {}", row.slot, s.slot),
                    )),
                    true,
                ),
                Some(s) if s.confirmation_status == Some(CommitmentLevel::Finalized) => {
                    ("FINALIZED", None, false)
                }
                Some(_) => ("OBSERVED_CONFIRMED", None, false),
                // Unknown to the status cache: never inferred as orphaned. A
                // finalized refetch decides.
                None => (
                    "FINALITY_UNKNOWN",
                    Some(issue(
                        &row.signature,
                        IssueCode::FinalityUnknown,
                        "status unavailable",
                    )),
                    true,
                ),
            };
            let outbox = self.finality_events(&row.signature, &row.finality, new);
            let next = if new == "FINALIZED" { None } else { next };
            if self
                .db
                .de_apply_finality(
                    &row.signature,
                    &row.finality,
                    new,
                    next.as_ref(),
                    &outbox,
                    issue_w.as_ref(),
                )
                .await?
                && new != row.finality
            {
                bump(&self.metrics.finality_transitions);
            }
            if refetch {
                self.db.de_requeue_finalized(&row.signature).await?;
            }
        }
        Ok(due.len())
    }

    fn finality_events(&self, signature: &str, from: &str, to: &str) -> Vec<OutboxWrite> {
        let ty = match to {
            "FINALIZED" => events::TRANSACTION_FINALIZED,
            "FINALITY_UNKNOWN" | "POSSIBLY_ORPHANED" => events::FINALITY_UNKNOWN,
            "ORPHANED" => events::TRANSACTION_ORPHANED,
            _ => return Vec::new(),
        };
        if from == to {
            return Vec::new();
        }
        vec![OutboxWrite {
            event_id: Uuid::new_v4(),
            event_type: ty.to_owned(),
            schema_version: EVENT_SCHEMA,
            aggregate_type: "TRANSACTION".to_owned(),
            aggregate_id: signature.to_owned(),
            // Stamped with the transaction's current revision by storage.
            aggregate_revision: 0,
            // Derived locally by the reconciler, not observed from a provider.
            source: "INTERNAL".to_owned(),
            source_event_time: None,
            observed_at: UtcTimestamp::now(),
            correlation_id: Uuid::new_v4(),
            causation_id: None,
            commitment: None,
            // Finality is not a decode-quality state; the transition is in
            // the payload.
            quality_state: "NOT_APPLICABLE".to_owned(),
            payload_json: serde_json::json!({"signature": signature, "from": from, "to": to})
                .to_string(),
        }]
    }

    // ---- backfill ------------------------------------------------------------------------------

    /// Advance at most one backfill by up to its remaining page budget.
    #[allow(clippy::too_many_lines)]
    pub async fn backfill_tick(
        &self,
        cancel: &CancellationToken,
    ) -> Result<Option<DeBackfill>, EngineError> {
        if !self.cfg.backfill_enabled {
            return Ok(None);
        }
        let max_running = i64::try_from(self.cfg.max_concurrent_backfills.max(1)).unwrap_or(1);
        let Some(mut bf) = self
            .db
            .de_claim_backfill(self.owner, self.cfg.job_lease, max_running)
            .await?
        else {
            return Ok(None);
        };
        let id = bf.backfill_id;
        let address = wallet(&bf.address)?;
        loop {
            if cancel.is_cancelled() {
                self.db
                    .de_set_backfill_status(
                        id,
                        Some(self.owner),
                        "PAUSED",
                        Some("SHUTDOWN"),
                        None,
                        None,
                    )
                    .await?;
                break;
            }
            if let Some(cur) = self.db.de_backfill(id).await? {
                bf = cur;
            }
            if bf.cancel_requested {
                self.db
                    .de_set_backfill_status(
                        id,
                        Some(self.owner),
                        "CANCELLED",
                        Some("CANCEL_REQUESTED"),
                        None,
                        None,
                    )
                    .await?;
                break;
            }
            if bf.pages_fetched >= bf.max_pages || bf.discovered_count >= bf.max_signatures {
                self.db
                    .de_set_backfill_status(
                        id,
                        Some(self.owner),
                        "COMPLETED",
                        Some("LIMIT_REACHED"),
                        Some("PARTIAL_HISTORY"),
                        None,
                    )
                    .await?;
                self.db
                    .de_record_issue(&IssueWrite {
                        backfill_id: Some(id),
                        ..issue(
                            &format!("backfill:{id}"),
                            IssueCode::BackfillIncomplete,
                            "page or signature limit reached",
                        )
                    })
                    .await?;
                break;
            }
            if self.db.de_queue_depth().await? >= self.cfg.queue_capacity {
                self.db
                    .de_set_backfill_status(
                        id,
                        Some(self.owner),
                        "PAUSED_BY_QUEUE",
                        Some("QUEUE_FULL"),
                        None,
                        None,
                    )
                    .await?;
                break;
            }
            if !self.source.admits(Priority::P5) {
                bump(&self.metrics.budget_pauses);
                self.db
                    .de_set_backfill_status(
                        id,
                        Some(self.owner),
                        "PAUSED_BY_BUDGET",
                        Some("BUDGET_DENIED"),
                        None,
                        None,
                    )
                    .await?;
                break;
            }
            let before = bf
                .cursor_before
                .as_deref()
                .or(bf.start_before_signature.as_deref());
            let remaining = bf.max_signatures.saturating_sub(bf.discovered_count).max(1);
            let page_limit =
                u16::try_from(bf.page_size.min(remaining).clamp(1, 1_000)).unwrap_or(100);
            let query = SignatureQuery {
                address: address.clone(),
                before: before.and_then(|b| TransactionSignature::parse(b).ok()),
                until: bf
                    .until_signature
                    .as_deref()
                    .and_then(|u| TransactionSignature::parse(u).ok()),
                page_limit,
                max_pages: 1,
            };
            let page = match self
                .source
                .signatures_for_address(&query, commitment_of(&bf.commitment), cancel)
                .await
            {
                Ok(p) => p,
                Err(SourceError::Budget) => {
                    bump(&self.metrics.budget_pauses);
                    self.db
                        .de_set_backfill_status(
                            id,
                            Some(self.owner),
                            "PAUSED_BY_BUDGET",
                            Some("BUDGET_DENIED"),
                            None,
                            None,
                        )
                        .await?;
                    break;
                }
                Err(SourceError::Cancelled) => {
                    self.db
                        .de_set_backfill_status(
                            id,
                            Some(self.owner),
                            "PAUSED",
                            Some("SHUTDOWN"),
                            None,
                            None,
                        )
                        .await?;
                    break;
                }
                Err(e @ (SourceError::Transient { .. } | SourceError::Disabled { .. })) => {
                    self.metrics.error(e.code());
                    self.db
                        .de_set_backfill_status(
                            id,
                            Some(self.owner),
                            "PAUSED_BY_PROVIDER",
                            Some(e.code()),
                            None,
                            Some(e.code()),
                        )
                        .await?;
                    break;
                }
                Err(e) => {
                    self.metrics.error(e.code());
                    self.db
                        .de_set_backfill_status(
                            id,
                            Some(self.owner),
                            "FAILED",
                            Some(e.code()),
                            None,
                            Some(e.code()),
                        )
                        .await?;
                    break;
                }
            };
            self.metrics
                .credits_charged
                .fetch_add(page.credits, Ordering::Relaxed);
            bump(&self.metrics.backfill_pages);
            let mut boundary = false;
            let mut obs = Vec::new();
            for item in &page.page.items {
                if bf.min_slot.is_some_and(|m| item.slot < m) {
                    boundary = true;
                    break;
                }
                obs.push(NewObservation {
                    signature: item.signature.as_str().to_owned(),
                    source: "BACKFILL".to_owned(),
                    target_id: bf.target_id,
                    backfill_id: Some(id),
                    commitment: commitment_label(item.confirmation_status),
                    slot: Some(item.slot),
                    failed_hint: Some(item.failed),
                    stream_generation: None,
                    observed_at: UtcTimestamp::now(),
                    dedup_key: format!("bf:{id}:{}", item.signature.as_str()),
                });
            }
            let exhausted = page.page.complete && !boundary && page.page.next_before.is_none();
            let p = BackfillPage {
                oldest_slot: obs.iter().filter_map(|o| o.slot).min(),
                newest_slot: obs.iter().filter_map(|o| o.slot).max(),
                observations: obs,
                next_cursor: page
                    .page
                    .next_before
                    .as_ref()
                    .map(|s| s.as_str().to_owned()),
                credits_spent: i32::try_from(page.credits).unwrap_or(i32::MAX),
                history_exhausted: exhausted
                    || (page.page.items.is_empty() && bf.until_signature.is_none()),
                priority: 5,
                max_attempts: self.max_attempts(),
                required_commitment: bf.commitment.clone(),
            };
            if self
                .db
                .de_backfill_page(id, self.owner, &p)
                .await?
                .is_none()
            {
                bump(&self.metrics.lease_lost);
                break;
            }
            let done = boundary
                || page.page.complete
                || page.page.items.is_empty()
                || p.next_cursor.is_none();
            if done {
                let coverage = if boundary {
                    "BOUNDARY_REACHED"
                } else if p.history_exhausted {
                    "ADDRESS_HISTORY_EXHAUSTED"
                } else {
                    "PARTIAL_HISTORY"
                };
                let reason = if boundary {
                    "MIN_SLOT_REACHED"
                } else if bf.until_signature.is_some() {
                    "UNTIL_REACHED"
                } else {
                    "HISTORY_EXHAUSTED"
                };
                self.db
                    .de_set_backfill_status(
                        id,
                        Some(self.owner),
                        "COMPLETED",
                        Some(reason),
                        Some(coverage),
                        None,
                    )
                    .await?;
                if let (Some(t), Some(cur)) = (bf.target_id, self.db.de_backfill(id).await?) {
                    let target_cov = if coverage == "ADDRESS_HISTORY_EXHAUSTED" {
                        coverage
                    } else {
                        "PARTIAL_HISTORY"
                    };
                    self.db
                        .de_update_target_coverage(
                            t,
                            cur.oldest_slot_seen,
                            cur.newest_slot_seen,
                            coverage == "ADDRESS_HISTORY_EXHAUSTED",
                            target_cov,
                        )
                        .await?;
                }
                break;
            }
        }
        Ok(self.db.de_backfill(id).await?)
    }

    // ---- outbox ------------------------------------------------------------------------------

    /// Deliver pending events to the built-in diagnostic consumer.
    /// Delivery is idempotent per (consumer, event).
    pub async fn outbox_tick(&self) -> Result<usize, EngineError> {
        let limit = i64::try_from(self.cfg.outbox_batch_size.clamp(1, 1_000)).unwrap_or(100);
        let pending = self
            .db
            .de_outbox_pending(DIAGNOSTIC_CONSUMER, limit)
            .await?;
        for ev in &pending {
            {
                let mut m = self
                    .metrics
                    .delivered_by_type
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let c = m.entry(ev.event_type.clone()).or_insert(0);
                *c = c.saturating_add(1);
            }
            tracing::debug!(target: "solprint::data_engine::events", event = %ev.event_type, aggregate = %ev.aggregate_id, seq = ev.seq, "domain event");
            self.db
                .de_outbox_mark(DIAGNOSTIC_CONSUMER, ev.event_id, true, None, Duration::ZERO)
                .await?;
            bump(&self.metrics.outbox_delivered);
        }
        Ok(pending.len())
    }

    // ---- maintenance -------------------------------------------------------------------------

    pub async fn refresh_counts(&self) -> Result<(), EngineError> {
        let counts = self.db.de_counts().await?;
        *self.counts.write().unwrap_or_else(PoisonError::into_inner) = Some(CountsCache {
            at: UtcTimestamp::now(),
            counts,
        });
        Ok(())
    }

    pub async fn retention(
        &self,
    ) -> Result<solprint_storage::data_engine::RetentionOutcome, EngineError> {
        Ok(self
            .db
            .de_retention(
                self.cfg.evidence_retention_days,
                self.cfg.job_retention_days,
                self.cfg.outbox_retention_days,
                &self.consumers(),
            )
            .await?)
    }

    // ---- live streams --------------------------------------------------------------------------

    /// Record one GATEWAY stream event: a logs notification or a recovered
    /// signature becomes an observation, and a gap or unknown continuity becomes
    /// an open quality issue on the target. Other kinds are ignored.
    pub async fn record_stream_event(
        &self,
        ev: &StreamEvent,
        tid: Option<Uuid>,
        prio: i16,
    ) -> Result<(), EngineError> {
        match &ev.kind {
            StreamEventKind::Logs {
                signature, failed, ..
            } => {
                bump(&self.metrics.stream_observations);
                self.observe(
                    signature.as_str(),
                    "LIVE_STREAM",
                    tid,
                    ev.commitment,
                    ev.context_slot,
                    Some(*failed),
                    prio,
                )
                .await
                .map(|_| ())
            }
            StreamEventKind::Recovered {
                signature,
                slot,
                failed,
                ..
            } => {
                bump(&self.metrics.stream_observations);
                self.observe(
                    signature.as_str(),
                    "STREAM_RECOVERY",
                    tid,
                    ev.commitment,
                    Some(*slot),
                    Some(*failed),
                    prio,
                )
                .await
                .map(|_| ())
            }
            StreamEventKind::Gap {
                reason,
                from_slot,
                to_slot,
            } => {
                bump(&self.metrics.stream_gaps);
                let subject = format!(
                    "stream:{}:{}",
                    tid.map(|t| t.to_string()).unwrap_or_default(),
                    ev.generation
                );
                let mut w = issue(
                    &subject,
                    IssueCode::StreamGap,
                    format!("{reason} {from_slot:?}..{to_slot:?}"),
                );
                w.target_id = tid;
                self.db.de_record_issue(&w).await.map_err(EngineError::from)
            }
            StreamEventKind::ContinuityUnknown { reason } => {
                bump(&self.metrics.stream_gaps);
                let subject = format!(
                    "stream:{}:{}",
                    tid.map(|t| t.to_string()).unwrap_or_default(),
                    ev.generation
                );
                let mut w = issue(&subject, IssueCode::ContinuityUnknown, *reason);
                w.target_id = tid;
                self.db.de_record_issue(&w).await.map_err(EngineError::from)
            }
            _ => Ok(()),
        }
    }

    async fn consume_streams(self: Arc<Self>, handle: StreamHandle, cancel: CancellationToken) {
        let Some(mut rx) = handle.take_events() else {
            tracing::warn!(target: "solprint::data_engine", "stream events already taken; live intake disabled");
            return;
        };
        let mut subs: HashMap<SubscriptionId, (Uuid, i16)> = HashMap::new();
        self.reconcile_subscriptions(&handle, &mut subs).await;
        let mut reconcile = tokio::time::interval(TARGET_RECONCILE_INTERVAL);
        reconcile.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        reconcile.tick().await;
        loop {
            let ev = tokio::select! {
                () = cancel.cancelled() => break,
                _ = reconcile.tick() => {
                    self.reconcile_subscriptions(&handle, &mut subs).await;
                    continue;
                }
                ev = rx.recv() => match ev { Some(ev) => ev, None => break },
            };
            let target = ev.subscription.and_then(|s| subs.get(&s).copied());
            let (tid, prio) = (target.map(|t| t.0), target.map_or(2, |t| t.1));
            let r = self.record_stream_event(&ev, tid, prio).await;
            if let Err(e) = r {
                tracing::warn!(target: "solprint::data_engine", error = %e, "stream event not recorded");
            }
        }
        for id in subs.keys() {
            let _ = handle.unsubscribe(*id).await;
        }
    }
}

impl<S: ChainSource> Engine<S> {
    /// Bring live subscriptions in line with the enabled wallet targets:
    /// subscribe targets that were enabled (for example by RADAR monitoring)
    /// and unsubscribe targets that were disabled. At most
    /// `max_active_targets` are subscribed; existing subscriptions are kept,
    /// so an unchanged target set causes no subscription churn.
    async fn reconcile_subscriptions(
        &self,
        handle: &StreamHandle,
        subs: &mut HashMap<SubscriptionId, (Uuid, i16)>,
    ) {
        let targets = match self.db.de_targets().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(target: "solprint::data_engine", error = %e, "could not load targets");
                return;
            }
        };
        let wanted: Vec<_> = targets
            .iter()
            .filter(|t| t.enabled && t.target_type == "WALLET_ADDRESS")
            .take(self.cfg.max_active_targets)
            .collect();
        let stale: Vec<SubscriptionId> = subs
            .iter()
            .filter(|(_, (tid, _))| !wanted.iter().any(|t| t.target_id == *tid))
            .map(|(id, _)| *id)
            .collect();
        for id in stale {
            let _ = handle.unsubscribe(id).await;
            subs.remove(&id);
        }
        for t in wanted {
            if subs.values().any(|(tid, _)| *tid == t.target_id) {
                continue;
            }
            let Ok(addr) = wallet(&t.address) else {
                continue;
            };
            let spec = SubscriptionSpec::Logs {
                mentions: addr,
                commitment: commitment_of(&t.commitment),
            };
            match handle
                .subscribe(spec, Some(format!("de:{}", t.target_id)))
                .await
            {
                Ok(id) => {
                    subs.insert(id, (t.target_id, priority_rank(&t.priority)));
                }
                Err(e) => {
                    tracing::warn!(target: "solprint::data_engine", code = e.code(), "target subscription failed")
                }
            }
        }
    }
}

async fn sleep_or_cancel(d: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! {
        () = cancel.cancelled() => false,
        () = tokio::time::sleep(d) => true,
    }
}

/// Running engine tasks.
#[derive(Debug)]
pub struct EngineHandle {
    tasks: JoinSet<()>,
    cancel: CancellationToken,
}

impl EngineHandle {
    /// Stop every loop and wait for them; leased work goes back to the queue.
    pub async fn shutdown<S: ChainSource>(mut self, engine: &Engine<S>) {
        self.cancel.cancel();
        while self.tasks.join_next().await.is_some() {}
        match engine.db.de_release_owner(engine.owner).await {
            Ok(n) if n > 0 => {
                tracing::info!(target: "solprint::data_engine", released = n, "released leased jobs")
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(target: "solprint::data_engine", error = %e, "could not release leases")
            }
        }
    }
}

/// Start the engine loops. `streams` is used only when auto-ingest is on.
pub fn start<S: ChainSource>(
    engine: &Arc<Engine<S>>,
    streams: Option<StreamHandle>,
    parent: &CancellationToken,
) -> EngineHandle {
    let cancel = parent.child_token();
    let mut tasks = JoinSet::new();
    for _ in 0..engine.cfg.decode_concurrency.clamp(1, 16) {
        let (e, c) = (Arc::clone(engine), cancel.clone());
        tasks.spawn(async move {
            loop {
                let idle = match e.run_batch(&c).await {
                    Ok(0) => IDLE_POLL,
                    Ok(_) => Duration::ZERO,
                    Err(err) => {
                        tracing::warn!(target: "solprint::data_engine", error = %err, "fetch batch failed");
                        IDLE_POLL.saturating_mul(5)
                    }
                };
                if c.is_cancelled() || (!idle.is_zero() && !sleep_or_cancel(idle, &c).await) {
                    break;
                }
            }
        });
    }
    {
        let (e, c) = (Arc::clone(engine), cancel.clone());
        tasks.spawn(async move {
            while sleep_or_cancel(e.cfg.finality_interval.max(Duration::from_secs(1)), &c).await {
                if let Err(err) = e.finality_tick(&c).await {
                    tracing::warn!(target: "solprint::data_engine", error = %err, "finality pass failed");
                }
            }
        });
    }
    if engine.cfg.backfill_enabled {
        let (e, c) = (Arc::clone(engine), cancel.clone());
        tasks.spawn(async move {
            while sleep_or_cancel(Duration::from_secs(5), &c).await {
                if let Err(err) = e.backfill_tick(&c).await {
                    tracing::warn!(target: "solprint::data_engine", error = %err, "backfill pass failed");
                }
            }
        });
    }
    {
        let (e, c) = (Arc::clone(engine), cancel.clone());
        tasks.spawn(async move {
            while sleep_or_cancel(e.cfg.outbox_poll_interval.max(Duration::from_millis(100)), &c).await {
                if let Err(err) = e.outbox_tick().await {
                    tracing::warn!(target: "solprint::data_engine", error = %err, "outbox pass failed");
                }
            }
        });
    }
    {
        let (e, c) = (Arc::clone(engine), cancel.clone());
        tasks.spawn(async move {
            let mut since_retention = RETENTION_INTERVAL;
            let tick = e.cfg.metrics_refresh.clamp(Duration::from_secs(1), MAINTENANCE_INTERVAL);
            loop {
                if let Err(err) = e.db.de_recover_expired_leases().await {
                    tracing::warn!(target: "solprint::data_engine", error = %err, "lease recovery failed");
                }
                if let Err(err) = e.refresh_counts().await {
                    tracing::warn!(target: "solprint::data_engine", error = %err, "counts refresh failed");
                }
                if since_retention >= RETENTION_INTERVAL {
                    since_retention = Duration::ZERO;
                    match e.retention().await {
                        Ok(r) => tracing::info!(target: "solprint::data_engine", evidence = r.evidence_pruned, jobs = r.jobs_deleted, outbox = r.outbox_deleted, "retention pass"),
                        Err(err) => tracing::warn!(target: "solprint::data_engine", error = %err, "retention failed"),
                    }
                }
                if !sleep_or_cancel(tick, &c).await {
                    break;
                }
                since_retention = since_retention.saturating_add(tick);
            }
        });
    }
    if engine.cfg.auto_ingest_enabled
        && let Some(h) = streams
    {
        let (e, c) = (Arc::clone(engine), cancel.clone());
        tasks.spawn(e.consume_streams(h, c));
    }
    EngineHandle { tasks, cancel }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded_and_monotonic() {
        let mut prev = Duration::ZERO;
        for a in 0..20 {
            let b = backoff(a);
            assert!(b >= prev && b <= MAX_BACKOFF);
            prev = b;
        }
        assert_eq!(backoff(0), Duration::from_secs(2));
        assert_eq!(backoff(-3), Duration::from_secs(2));
    }

    #[test]
    fn priorities_round_trip() {
        for p in Priority::ALL {
            assert_eq!(priority_of(i16::from(p.rank())), p);
        }
        assert_eq!(priority_rank("P2"), 2);
        assert_eq!(priority_rank("bogus"), 5);
    }

    #[test]
    fn issue_subjects_do_not_claim_signatures() {
        assert!(
            issue("queue", IssueCode::QueueSaturated, "x")
                .signature
                .is_none()
        );
        assert!(
            issue("stream:abc:1", IssueCode::StreamGap, "x")
                .signature
                .is_none()
        );
    }
}
