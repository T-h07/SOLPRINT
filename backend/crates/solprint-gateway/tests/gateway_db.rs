//! GATEWAY persistence against PostgreSQL.
//!
//! Ignored by default: they need a disposable PostgreSQL server. Run with
//! `scripts/test.ps1`, which starts one and passes `--include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use solprint_core::UtcTimestamp;
use solprint_gateway::{CachePolicy, Gateway, Priority, RequestContext};
use solprint_storage::test_support::TestDatabase;
use solprint_storage::{Database, MetricDelta, StreamCheckpointRow, UsageDelta};
use support::{MockServer, rpc_result, settings};
use time::OffsetDateTime;

async fn db() -> (TestDatabase, Arc<Database>) {
    let tdb = TestDatabase::create().await;
    let db = Database::connect(&tdb.config()).await.unwrap();
    db.migrate().await.unwrap();
    (tdb, Arc::new(db))
}

fn at(unix: i64) -> UtcTimestamp {
    UtcTimestamp::from_offset_datetime(OffsetDateTime::from_unix_timestamp(unix).unwrap())
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn credit_ledger_survives_restarts() {
    let (tdb, db) = db().await;
    let mock = MockServer::start(|r| rpc_result(r, &json!(1))).await;
    let mut s = settings();
    s.helius.rpc_url = mock.url.clone();

    let g = Gateway::new(s.clone(), db.clone()).unwrap();
    g.load_ledger().await;
    for _ in 0..3 {
        let c = RequestContext::new("test", Priority::P1)
            .with_cache(CachePolicy::Refresh)
            .with_timeout(Duration::from_secs(5));
        g.get_slot(&c).await.unwrap();
    }
    g.flush().await;
    assert_eq!(g.snapshot().persistence.last_flush_ok, Some(true));

    let restarted = Gateway::new(s, db.clone()).unwrap();
    restarted.load_ledger().await;
    let b = restarted.snapshot().budget.unwrap();
    assert!(b.ledger_loaded);
    assert_eq!(b.credits_used, 3);

    let row = db
        .gateway_billing_cycle("helius", &b.cycle_start)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.credits_used, 3);
    assert_eq!(row.credit_limit, 1_000_000);
    assert_eq!(row.reserve_credits, 200_000);
    tdb.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn cycle_credit_upserts_are_additive() {
    let (tdb, db) = db().await;
    let (start, end) = (at(1_767_225_600), at(1_769_904_000));
    assert_eq!(
        db.gateway_add_cycle_credits("helius", &start, &end, 100, 20, 5)
            .await
            .unwrap(),
        5
    );
    assert_eq!(
        db.gateway_add_cycle_credits("helius", &start, &end, 200, 40, 7)
            .await
            .unwrap(),
        12
    );
    let row = db
        .gateway_billing_cycle("helius", &start)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.credit_limit, 200, "limits follow configuration");
    tdb.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn usage_and_metric_buckets_accumulate_and_prune() {
    let (tdb, db) = db().await;
    let old = at(1_700_000_000);
    let now = UtcTimestamp::now();
    let bucket = at(OffsetDateTime::now_utc().unix_timestamp().div_euclid(60) * 60);
    let usage = |b: &UtcTimestamp, credits| UsageDelta {
        provider: "helius".into(),
        bucket_start: *b,
        category: "getSlot".into(),
        requests: 1,
        credits,
        bytes: 10,
    };
    db.gateway_record_usage(&[usage(&bucket, 1), usage(&old, 1)])
        .await
        .unwrap();
    db.gateway_record_usage(&[usage(&bucket, 2)]).await.unwrap();
    let metric = |b: &UtcTimestamp| MetricDelta {
        provider: "helius".into(),
        operation: "getSlot".into(),
        bucket_start: *b,
        success_count: 1,
        error_count: 0,
        cache_hits: 0,
        latency_hist: vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    };
    db.gateway_record_metrics(&[metric(&bucket), metric(&old)])
        .await
        .unwrap();
    db.gateway_record_metrics(&[metric(&bucket)]).await.unwrap();

    let hist = db.gateway_usage_history(&at(1_699_000_000)).await.unwrap();
    let recent: u64 = hist
        .iter()
        .filter(|h| h.hour_start.as_offset_datetime() > old.as_offset_datetime())
        .map(|h| h.credits)
        .sum();
    assert_eq!(recent, 3, "same bucket adds up");

    // The window sums every bucket since `since`, per provider and operation.
    let since = at(bucket.as_offset_datetime().unix_timestamp() - 3_600);
    let window = db.gateway_metric_window(&since).await.unwrap();
    let current = window
        .iter()
        .find(|m| m.provider == "helius" && m.operation == "getSlot")
        .unwrap();
    assert_eq!(current.bucket_start, since);
    assert_eq!(
        current.success_count, 2,
        "old bucket excluded, same bucket adds up"
    );
    assert_eq!(current.latency_hist[1], 2, "histograms add element-wise");
    let all = db.gateway_metric_window(&at(1_699_000_000)).await.unwrap();
    assert_eq!(all[0].success_count, 3);

    let pruned = db.gateway_prune(&now, &now).await.unwrap();
    assert!(pruned.usage_rows >= 1);
    assert!(pruned.metric_rows >= 1);
    let after = db.gateway_usage_history(&at(1_699_000_000)).await.unwrap();
    assert!(
        after
            .iter()
            .all(|h| h.hour_start.as_offset_datetime() >= old.as_offset_datetime())
    );
    tdb.cleanup().await;
}

#[tokio::test]
#[ignore = "requires SOLPRINT_TEST_DATABASE_URL (disposable PostgreSQL)"]
async fn checkpoints_keep_last_known_position_and_are_validated() {
    let (tdb, db) = db().await;
    let sig = support::sig(1);
    let cp = StreamCheckpointRow {
        stream_key: "test.logs".into(),
        kind: "logs".into(),
        commitment: "confirmed".into(),
        last_slot: Some(10),
        last_signature: Some(sig.clone()),
        continuity: "CONTINUOUS".into(),
        updated_at: UtcTimestamp::now(),
    };
    db.gateway_upsert_checkpoint(&cp).await.unwrap();
    // A later update without a position keeps the previous one.
    db.gateway_upsert_checkpoint(&StreamCheckpointRow {
        last_slot: None,
        last_signature: None,
        continuity: "GAP".into(),
        ..cp.clone()
    })
    .await
    .unwrap();
    let rows = db.gateway_checkpoints().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].last_slot, Some(10));
    assert_eq!(rows[0].last_signature.as_deref(), Some(sig.as_str()));
    assert_eq!(rows[0].continuity, "GAP");

    // Constraints reject values outside the documented vocabulary.
    let bad = StreamCheckpointRow {
        continuity: "MAYBE".into(),
        ..cp.clone()
    };
    assert!(db.gateway_upsert_checkpoint(&bad).await.is_err());
    let bad = StreamCheckpointRow {
        commitment: "max".into(),
        ..cp
    };
    assert!(db.gateway_upsert_checkpoint(&bad).await.is_err());
    tdb.cleanup().await;
}
