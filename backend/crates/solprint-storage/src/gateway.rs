//! GATEWAY persistence: billing-cycle ledger, usage and metric buckets, and
//! stream checkpoints (`migrations/20261126000002_gateway.sql`).
//!
//! All counters are additive upserts so concurrent flushes and restarts can
//! never lose or double-apply a delta that was committed. Values are `u64` in
//! Rust and `BIGINT` in SQL; conversions saturate instead of wrapping.

use solprint_core::UtcTimestamp;
use sqlx::Row;
use time::OffsetDateTime;

use crate::{Database, StorageError};

fn to_db(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn from_db(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn ts(t: &UtcTimestamp) -> OffsetDateTime {
    *t.as_offset_datetime()
}

fn utc(t: OffsetDateTime) -> UtcTimestamp {
    UtcTimestamp::from_offset_datetime(t)
}

fn map_err(e: &sqlx::Error) -> StorageError {
    StorageError::from_sqlx(e)
}

/// One provider billing cycle as recorded locally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingCycleRow {
    pub provider: String,
    pub cycle_start: UtcTimestamp,
    pub cycle_end: UtcTimestamp,
    pub credit_limit: u64,
    pub reserve_credits: u64,
    pub credits_used: u64,
    pub updated_at: UtcTimestamp,
}

/// Additive usage delta for one provider / minute / category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageDelta {
    pub provider: String,
    pub bucket_start: UtcTimestamp,
    pub category: String,
    pub requests: u64,
    pub credits: u64,
    pub bytes: u64,
}

/// Additive metric delta for one provider / operation / minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetricDelta {
    pub provider: String,
    pub operation: String,
    pub bucket_start: UtcTimestamp,
    pub success_count: u64,
    pub error_count: u64,
    pub cache_hits: u64,
    /// Counts per fixed latency bucket (layout owned by `solprint-gateway`).
    pub latency_hist: Vec<u64>,
}

/// Aggregated usage per provider and hour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageHistoryRow {
    pub provider: String,
    pub hour_start: UtcTimestamp,
    pub requests: u64,
    pub credits: u64,
    pub bytes: u64,
}

/// Last processed position of one stream subscription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamCheckpointRow {
    pub stream_key: String,
    pub kind: String,
    /// `processed` | `confirmed` | `finalized`.
    pub commitment: String,
    pub last_slot: Option<u64>,
    pub last_signature: Option<String>,
    /// `CONTINUOUS` | `RECOVERED` | `GAP` | `UNKNOWN`.
    pub continuity: String,
    pub updated_at: UtcTimestamp,
}

/// Rows removed by one retention pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PruneOutcome {
    pub usage_rows: u64,
    pub metric_rows: u64,
}

impl Database {
    /// The ledger row for one cycle, if it exists.
    pub async fn gateway_billing_cycle(
        &self,
        provider: &str,
        cycle_start: &UtcTimestamp,
    ) -> Result<Option<BillingCycleRow>, StorageError> {
        let row = sqlx::query(
            "SELECT provider, cycle_start, cycle_end, credit_limit, reserve_credits,
                    credits_used, updated_at
             FROM solprint.gateway_billing_cycles
             WHERE provider = $1 AND cycle_start = $2",
        )
        .bind(provider)
        .bind(ts(cycle_start))
        .fetch_optional(self.pool())
        .await
        .map_err(|e| map_err(&e))?;
        Ok(row.map(|r| BillingCycleRow {
            provider: r.get("provider"),
            cycle_start: utc(r.get("cycle_start")),
            cycle_end: utc(r.get("cycle_end")),
            credit_limit: from_db(r.get("credit_limit")),
            reserve_credits: from_db(r.get("reserve_credits")),
            credits_used: from_db(r.get("credits_used")),
            updated_at: utc(r.get("updated_at")),
        }))
    }

    /// Create the cycle if needed and add `credits_delta` to it. The limit and
    /// reserve are refreshed from configuration on every call. Returns the
    /// new cumulative total.
    pub async fn gateway_add_cycle_credits(
        &self,
        provider: &str,
        cycle_start: &UtcTimestamp,
        cycle_end: &UtcTimestamp,
        credit_limit: u64,
        reserve_credits: u64,
        credits_delta: u64,
    ) -> Result<u64, StorageError> {
        let total: i64 = sqlx::query_scalar(
            "INSERT INTO solprint.gateway_billing_cycles
                 (provider, cycle_start, cycle_end, credit_limit, reserve_credits, credits_used)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (provider, cycle_start) DO UPDATE
                 SET credits_used    = solprint.gateway_billing_cycles.credits_used + EXCLUDED.credits_used,
                     credit_limit    = EXCLUDED.credit_limit,
                     reserve_credits = EXCLUDED.reserve_credits,
                     cycle_end       = EXCLUDED.cycle_end,
                     updated_at      = now()
             RETURNING credits_used",
        )
        .bind(provider)
        .bind(ts(cycle_start))
        .bind(ts(cycle_end))
        .bind(to_db(credit_limit))
        .bind(to_db(reserve_credits.min(credit_limit)))
        .bind(to_db(credits_delta))
        .fetch_one(self.pool())
        .await
        .map_err(|e| map_err(&e))?;
        Ok(from_db(total))
    }

    /// Add usage deltas atomically (all or nothing).
    pub async fn gateway_record_usage(&self, deltas: &[UsageDelta]) -> Result<(), StorageError> {
        if deltas.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await.map_err(|e| map_err(&e))?;
        for d in deltas {
            sqlx::query(
                "INSERT INTO solprint.gateway_usage_buckets
                     (provider, bucket_start, category, requests, credits, bytes)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (provider, bucket_start, category) DO UPDATE
                     SET requests = solprint.gateway_usage_buckets.requests + EXCLUDED.requests,
                         credits  = solprint.gateway_usage_buckets.credits  + EXCLUDED.credits,
                         bytes    = solprint.gateway_usage_buckets.bytes    + EXCLUDED.bytes",
            )
            .bind(&d.provider)
            .bind(ts(&d.bucket_start))
            .bind(&d.category)
            .bind(to_db(d.requests))
            .bind(to_db(d.credits))
            .bind(to_db(d.bytes))
            .execute(&mut *tx)
            .await
            .map_err(|e| map_err(&e))?;
        }
        tx.commit().await.map_err(|e| map_err(&e))
    }

    /// Add metric deltas atomically. Histograms are summed element-wise.
    pub async fn gateway_record_metrics(&self, deltas: &[MetricDelta]) -> Result<(), StorageError> {
        if deltas.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await.map_err(|e| map_err(&e))?;
        for d in deltas {
            let hist: Vec<i64> = d.latency_hist.iter().copied().map(to_db).collect();
            sqlx::query(
                "INSERT INTO solprint.gateway_metric_buckets
                     (provider, operation, bucket_start, success_count, error_count, cache_hits, latency_hist)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 ON CONFLICT (provider, operation, bucket_start) DO UPDATE
                     SET success_count = solprint.gateway_metric_buckets.success_count + EXCLUDED.success_count,
                         error_count   = solprint.gateway_metric_buckets.error_count   + EXCLUDED.error_count,
                         cache_hits    = solprint.gateway_metric_buckets.cache_hits    + EXCLUDED.cache_hits,
                         latency_hist  = (
                             SELECT array_agg(COALESCE(a, 0) + COALESCE(b, 0) ORDER BY i)
                             FROM unnest(solprint.gateway_metric_buckets.latency_hist, EXCLUDED.latency_hist)
                                  WITH ORDINALITY AS t(a, b, i)
                         )",
            )
            .bind(&d.provider)
            .bind(&d.operation)
            .bind(ts(&d.bucket_start))
            .bind(to_db(d.success_count))
            .bind(to_db(d.error_count))
            .bind(to_db(d.cache_hits))
            .bind(hist)
            .execute(&mut *tx)
            .await
            .map_err(|e| map_err(&e))?;
        }
        tx.commit().await.map_err(|e| map_err(&e))
    }

    /// Hourly usage totals since `since`, oldest first.
    pub async fn gateway_usage_history(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<UsageHistoryRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT provider, date_trunc('hour', bucket_start) AS hour_start,
                    SUM(requests)::BIGINT AS requests, SUM(credits)::BIGINT AS credits,
                    SUM(bytes)::BIGINT AS bytes
             FROM solprint.gateway_usage_buckets
             WHERE bucket_start >= $1
             GROUP BY provider, hour_start
             ORDER BY hour_start, provider",
        )
        .bind(ts(since))
        .fetch_all(self.pool())
        .await
        .map_err(|e| map_err(&e))?;
        Ok(rows
            .into_iter()
            .map(|r| UsageHistoryRow {
                provider: r.get("provider"),
                hour_start: utc(r.get("hour_start")),
                requests: from_db(r.get("requests")),
                credits: from_db(r.get("credits")),
                bytes: from_db(r.get("bytes")),
            })
            .collect())
    }

    /// Metrics summed over `[since, now]` per provider and operation. The
    /// returned `bucket_start` is `since`.
    pub async fn gateway_metric_window(
        &self,
        since: &UtcTimestamp,
    ) -> Result<Vec<MetricDelta>, StorageError> {
        let rows = sqlx::query(
            "SELECT provider, operation, latency_hist, success_count, error_count, cache_hits
             FROM solprint.gateway_metric_buckets
             WHERE bucket_start >= $1
             ORDER BY provider, operation",
        )
        .bind(ts(since))
        .fetch_all(self.pool())
        .await
        .map_err(|e| map_err(&e))?;
        let mut out: Vec<MetricDelta> = Vec::new();
        for r in rows {
            let provider: String = r.get("provider");
            let operation: String = r.get("operation");
            let hist: Vec<i64> = r.get("latency_hist");
            let idx = match out
                .iter()
                .position(|m| m.provider == provider && m.operation == operation)
            {
                Some(i) => i,
                None => {
                    out.push(MetricDelta {
                        provider,
                        operation,
                        bucket_start: *since,
                        success_count: 0,
                        error_count: 0,
                        cache_hits: 0,
                        latency_hist: Vec::new(),
                    });
                    out.len() - 1
                }
            };
            let m = &mut out[idx];
            m.success_count = m
                .success_count
                .saturating_add(from_db(r.get("success_count")));
            m.error_count = m.error_count.saturating_add(from_db(r.get("error_count")));
            m.cache_hits = m.cache_hits.saturating_add(from_db(r.get("cache_hits")));
            if m.latency_hist.len() < hist.len() {
                m.latency_hist.resize(hist.len(), 0);
            }
            for (slot, v) in m.latency_hist.iter_mut().zip(hist) {
                *slot = slot.saturating_add(from_db(v));
            }
        }
        Ok(out)
    }

    pub async fn gateway_upsert_checkpoint(
        &self,
        cp: &StreamCheckpointRow,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO solprint.gateway_stream_checkpoints
                 (stream_key, kind, commitment, last_slot, last_signature, continuity, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (stream_key) DO UPDATE
                 SET kind = EXCLUDED.kind,
                     commitment = EXCLUDED.commitment,
                     last_slot = COALESCE(EXCLUDED.last_slot, solprint.gateway_stream_checkpoints.last_slot),
                     last_signature = COALESCE(EXCLUDED.last_signature, solprint.gateway_stream_checkpoints.last_signature),
                     continuity = EXCLUDED.continuity,
                     updated_at = EXCLUDED.updated_at",
        )
        .bind(&cp.stream_key)
        .bind(&cp.kind)
        .bind(&cp.commitment)
        .bind(cp.last_slot.map(to_db))
        .bind(cp.last_signature.as_deref())
        .bind(&cp.continuity)
        .bind(ts(&cp.updated_at))
        .execute(self.pool())
        .await
        .map_err(|e| map_err(&e))?;
        Ok(())
    }

    pub async fn gateway_checkpoints(&self) -> Result<Vec<StreamCheckpointRow>, StorageError> {
        let rows = sqlx::query(
            "SELECT stream_key, kind, commitment, last_slot, last_signature, continuity, updated_at
             FROM solprint.gateway_stream_checkpoints ORDER BY stream_key",
        )
        .fetch_all(self.pool())
        .await
        .map_err(|e| map_err(&e))?;
        Ok(rows
            .into_iter()
            .map(|r| StreamCheckpointRow {
                stream_key: r.get("stream_key"),
                kind: r.get("kind"),
                commitment: r.get("commitment"),
                last_slot: r.get::<Option<i64>, _>("last_slot").map(from_db),
                last_signature: r.get("last_signature"),
                continuity: r.get("continuity"),
                updated_at: utc(r.get("updated_at")),
            })
            .collect())
    }

    /// Delete usage buckets older than `usage_before` and metric buckets
    /// older than `metrics_before`. Billing cycles and checkpoints are kept.
    pub async fn gateway_prune(
        &self,
        usage_before: &UtcTimestamp,
        metrics_before: &UtcTimestamp,
    ) -> Result<PruneOutcome, StorageError> {
        let usage =
            sqlx::query("DELETE FROM solprint.gateway_usage_buckets WHERE bucket_start < $1")
                .bind(ts(usage_before))
                .execute(self.pool())
                .await
                .map_err(|e| map_err(&e))?
                .rows_affected();
        let metrics =
            sqlx::query("DELETE FROM solprint.gateway_metric_buckets WHERE bucket_start < $1")
                .bind(ts(metrics_before))
                .execute(self.pool())
                .await
                .map_err(|e| map_err(&e))?
                .rows_affected();
        Ok(PruneOutcome {
            usage_rows: usage,
            metric_rows: metrics,
        })
    }
}
