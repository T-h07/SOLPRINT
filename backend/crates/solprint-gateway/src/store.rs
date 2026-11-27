//! Persistence port for GATEWAY state.
//!
//! The production implementation delegates to `solprint-storage` (the only
//! crate that speaks SQL). [`MemoryStore`] backs unit and mock tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use solprint_core::UtcTimestamp;
use solprint_storage::{
    BillingCycleRow, Database, MetricDelta, PruneOutcome, StorageError, StreamCheckpointRow,
    UsageDelta, UsageHistoryRow,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait GatewayStore: Send + Sync + std::fmt::Debug {
    fn billing_cycle<'a>(
        &'a self,
        provider: &'a str,
        cycle_start: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Option<BillingCycleRow>, StorageError>>;

    #[allow(clippy::too_many_arguments)]
    fn add_cycle_credits<'a>(
        &'a self,
        provider: &'a str,
        cycle_start: &'a UtcTimestamp,
        cycle_end: &'a UtcTimestamp,
        credit_limit: u64,
        reserve_credits: u64,
        delta: u64,
    ) -> BoxFuture<'a, Result<u64, StorageError>>;

    fn record_usage<'a>(
        &'a self,
        deltas: &'a [UsageDelta],
    ) -> BoxFuture<'a, Result<(), StorageError>>;

    fn record_metrics<'a>(
        &'a self,
        deltas: &'a [MetricDelta],
    ) -> BoxFuture<'a, Result<(), StorageError>>;

    fn usage_history<'a>(
        &'a self,
        since: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Vec<UsageHistoryRow>, StorageError>>;

    fn metric_window<'a>(
        &'a self,
        since: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Vec<MetricDelta>, StorageError>>;

    fn upsert_checkpoint<'a>(
        &'a self,
        cp: &'a StreamCheckpointRow,
    ) -> BoxFuture<'a, Result<(), StorageError>>;

    fn checkpoints(&self) -> BoxFuture<'_, Result<Vec<StreamCheckpointRow>, StorageError>>;

    fn prune<'a>(
        &'a self,
        usage_before: &'a UtcTimestamp,
        metrics_before: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<PruneOutcome, StorageError>>;
}

impl GatewayStore for Database {
    fn billing_cycle<'a>(
        &'a self,
        provider: &'a str,
        cycle_start: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Option<BillingCycleRow>, StorageError>> {
        Box::pin(self.gateway_billing_cycle(provider, cycle_start))
    }

    fn add_cycle_credits<'a>(
        &'a self,
        provider: &'a str,
        cycle_start: &'a UtcTimestamp,
        cycle_end: &'a UtcTimestamp,
        credit_limit: u64,
        reserve_credits: u64,
        delta: u64,
    ) -> BoxFuture<'a, Result<u64, StorageError>> {
        Box::pin(self.gateway_add_cycle_credits(
            provider,
            cycle_start,
            cycle_end,
            credit_limit,
            reserve_credits,
            delta,
        ))
    }

    fn record_usage<'a>(
        &'a self,
        deltas: &'a [UsageDelta],
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(self.gateway_record_usage(deltas))
    }

    fn record_metrics<'a>(
        &'a self,
        deltas: &'a [MetricDelta],
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(self.gateway_record_metrics(deltas))
    }

    fn usage_history<'a>(
        &'a self,
        since: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Vec<UsageHistoryRow>, StorageError>> {
        Box::pin(self.gateway_usage_history(since))
    }

    fn metric_window<'a>(
        &'a self,
        since: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Vec<MetricDelta>, StorageError>> {
        Box::pin(self.gateway_metric_window(since))
    }

    fn upsert_checkpoint<'a>(
        &'a self,
        cp: &'a StreamCheckpointRow,
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(self.gateway_upsert_checkpoint(cp))
    }

    fn checkpoints(&self) -> BoxFuture<'_, Result<Vec<StreamCheckpointRow>, StorageError>> {
        Box::pin(self.gateway_checkpoints())
    }

    fn prune<'a>(
        &'a self,
        usage_before: &'a UtcTimestamp,
        metrics_before: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<PruneOutcome, StorageError>> {
        Box::pin(self.gateway_prune(usage_before, metrics_before))
    }
}

/// In-memory store for tests. `fail` simulates an unavailable database.
#[derive(Debug, Default)]
pub struct MemoryStore {
    inner: Mutex<MemoryInner>,
}

#[derive(Debug, Default)]
struct MemoryInner {
    fail: bool,
    cycles: Vec<BillingCycleRow>,
    usage: Vec<UsageDelta>,
    metrics: Vec<MetricDelta>,
    checkpoints: Vec<StreamCheckpointRow>,
}

impl MemoryStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, MemoryInner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn set_failing(&self, fail: bool) {
        self.lock().fail = fail;
    }

    #[must_use]
    pub fn credits_used(&self, provider: &str) -> u64 {
        self.lock()
            .cycles
            .iter()
            .filter(|c| c.provider == provider)
            .map(|c| c.credits_used)
            .sum()
    }

    #[must_use]
    pub fn usage_rows(&self) -> Vec<UsageDelta> {
        self.lock().usage.clone()
    }

    #[must_use]
    pub fn metric_rows(&self) -> Vec<MetricDelta> {
        self.lock().metrics.clone()
    }

    #[must_use]
    pub fn checkpoint_rows(&self) -> Vec<StreamCheckpointRow> {
        self.lock().checkpoints.clone()
    }

    pub fn seed_checkpoint(&self, cp: StreamCheckpointRow) {
        self.lock().checkpoints.push(cp);
    }

    fn check(&self) -> Result<std::sync::MutexGuard<'_, MemoryInner>, StorageError> {
        let g = self.lock();
        if g.fail {
            Err(StorageError::Unavailable)
        } else {
            Ok(g)
        }
    }
}

impl GatewayStore for MemoryStore {
    fn billing_cycle<'a>(
        &'a self,
        provider: &'a str,
        cycle_start: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Option<BillingCycleRow>, StorageError>> {
        Box::pin(async move {
            let g = self.check()?;
            Ok(g.cycles
                .iter()
                .find(|c| c.provider == provider && c.cycle_start == *cycle_start)
                .cloned())
        })
    }

    fn add_cycle_credits<'a>(
        &'a self,
        provider: &'a str,
        cycle_start: &'a UtcTimestamp,
        cycle_end: &'a UtcTimestamp,
        credit_limit: u64,
        reserve_credits: u64,
        delta: u64,
    ) -> BoxFuture<'a, Result<u64, StorageError>> {
        Box::pin(async move {
            let mut g = self.check()?;
            if let Some(c) = g
                .cycles
                .iter_mut()
                .find(|c| c.provider == provider && c.cycle_start == *cycle_start)
            {
                c.credits_used = c.credits_used.saturating_add(delta);
                return Ok(c.credits_used);
            }
            g.cycles.push(BillingCycleRow {
                provider: provider.to_owned(),
                cycle_start: *cycle_start,
                cycle_end: *cycle_end,
                credit_limit,
                reserve_credits,
                credits_used: delta,
                updated_at: UtcTimestamp::now(),
            });
            Ok(delta)
        })
    }

    fn record_usage<'a>(
        &'a self,
        deltas: &'a [UsageDelta],
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            self.check()?.usage.extend_from_slice(deltas);
            Ok(())
        })
    }

    fn record_metrics<'a>(
        &'a self,
        deltas: &'a [MetricDelta],
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            self.check()?.metrics.extend_from_slice(deltas);
            Ok(())
        })
    }

    fn usage_history<'a>(
        &'a self,
        _since: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Vec<UsageHistoryRow>, StorageError>> {
        Box::pin(async move {
            let g = self.check()?;
            Ok(g.usage
                .iter()
                .map(|u| UsageHistoryRow {
                    provider: u.provider.clone(),
                    hour_start: u.bucket_start,
                    requests: u.requests,
                    credits: u.credits,
                    bytes: u.bytes,
                })
                .collect())
        })
    }

    fn metric_window<'a>(
        &'a self,
        _since: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<Vec<MetricDelta>, StorageError>> {
        Box::pin(async move { Ok(self.check()?.metrics.clone()) })
    }

    fn upsert_checkpoint<'a>(
        &'a self,
        cp: &'a StreamCheckpointRow,
    ) -> BoxFuture<'a, Result<(), StorageError>> {
        Box::pin(async move {
            let mut g = self.check()?;
            g.checkpoints.retain(|c| c.stream_key != cp.stream_key);
            g.checkpoints.push(cp.clone());
            Ok(())
        })
    }

    fn checkpoints(&self) -> BoxFuture<'_, Result<Vec<StreamCheckpointRow>, StorageError>> {
        Box::pin(async move { Ok(self.check()?.checkpoints.clone()) })
    }

    fn prune<'a>(
        &'a self,
        usage_before: &'a UtcTimestamp,
        metrics_before: &'a UtcTimestamp,
    ) -> BoxFuture<'a, Result<PruneOutcome, StorageError>> {
        Box::pin(async move {
            let mut g = self.check()?;
            let (u0, m0) = (g.usage.len(), g.metrics.len());
            g.usage.retain(|u| u.bucket_start >= *usage_before);
            g.metrics.retain(|m| m.bucket_start >= *metrics_before);
            Ok(PruneOutcome {
                usage_rows: u64::try_from(u0 - g.usage.len()).unwrap_or(0),
                metric_rows: u64::try_from(m0 - g.metrics.len()).unwrap_or(0),
            })
        })
    }
}
