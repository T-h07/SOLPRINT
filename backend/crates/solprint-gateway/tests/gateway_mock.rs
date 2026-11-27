//! GATEWAY against loopback mock providers: JSON-RPC semantics, budget,
//! breaker, fallback, rate limiting, caching, Jupiter and DEX Screener.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::json;
use solprint_core::{TokenMint, WalletAddress};
use solprint_gateway::models::QuoteRequest;
use solprint_gateway::{
    CachePolicy, Gateway, GatewayError, MemoryStore, Priority, Provenance, ProviderId,
    RequestContext,
};
use support::{MockServer, Reply, rpc_error, rpc_method, rpc_result, settings};

const SOL: &str = "So11111111111111111111111111111111111111112";
const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const SYSTEM: &str = "11111111111111111111111111111111";

fn ctx(p: Priority) -> RequestContext {
    RequestContext::new("test", p).with_timeout(Duration::from_secs(5))
}

async fn gateway_with(s: solprint_gateway::GatewaySettings) -> (Gateway, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::default());
    let g = Gateway::new(s, store.clone()).unwrap();
    g.load_ledger().await;
    (g, store)
}

async fn helius(
    script: impl Fn(&support::Recorded) -> Reply + Send + Sync + 'static,
) -> (Gateway, Arc<MemoryStore>, MockServer) {
    let mock = MockServer::start(script).await;
    let mut s = settings();
    s.helius.rpc_url = mock.url.clone();
    let (g, store) = gateway_with(s).await;
    (g, store, mock)
}

#[tokio::test]
async fn rpc_success_is_metered_cached_and_carries_provenance() {
    let (g, _store, mock) = helius(|r| rpc_result(r, &json!(123_456))).await;
    let first = g.get_slot(&ctx(Priority::P1)).await.unwrap();
    assert_eq!(first.value, 123_456);
    assert_eq!(first.provider, ProviderId::Helius);
    assert_eq!(first.provenance, Provenance::Direct);
    assert_eq!(first.credits_charged, 1);

    let second = g.get_slot(&ctx(Priority::P1)).await.unwrap();
    assert_eq!(second.provenance, Provenance::FreshCache);
    assert_eq!(second.credits_charged, 0);
    assert_eq!(mock.hits(), 1, "second read must come from cache");

    let req = &mock.requests()[0];
    assert_eq!(req.method, "POST");
    assert_eq!(rpc_method(req), "getSlot");
    assert!(
        req.query.contains("api-key="),
        "key travels in the query only"
    );
    assert_eq!(g.snapshot().budget.unwrap().credits_used, 1);
}

#[tokio::test]
async fn rpc_error_inside_http_200_is_typed_and_not_retried() {
    let (g, _s, mock) =
        helius(|r| rpc_error(r, -32602, "Invalid params: see https://x?api-key=abc")).await;
    let err = g.get_slot(&ctx(Priority::P1)).await.unwrap_err();
    match err {
        GatewayError::Rpc { code, message, .. } => {
            assert_eq!(code, -32602);
            assert!(
                !message.contains("api-key"),
                "messages are sanitized: {message}"
            );
            assert!(!message.contains("https://"));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(mock.hits(), 1);
}

#[tokio::test]
async fn mismatched_response_id_is_rejected() {
    let (g, _s, _mock) = helius(|_| Reply::ok(r#"{"jsonrpc":"2.0","id":999999,"result":1}"#)).await;
    let err = g.get_slot(&ctx(Priority::P1)).await.unwrap_err();
    assert!(matches!(err, GatewayError::Decode { .. }), "{err:?}");
}

#[tokio::test]
async fn null_account_means_not_found() {
    let (g, _s, mock) =
        helius(|r| rpc_result(r, &json!({"context": {"slot": 77}, "value": null}))).await;
    let address = WalletAddress::parse(SYSTEM).unwrap();
    let r = g
        .get_account_info(&ctx(Priority::P1), &address)
        .await
        .unwrap();
    assert!(r.value.is_none());
    assert_eq!(r.context_slot, Some(77));
    // Negative results are cached briefly too.
    let again = g
        .get_account_info(&ctx(Priority::P1), &address)
        .await
        .unwrap();
    assert!(again.value.is_none());
    assert_eq!(mock.hits(), 1);
}

#[tokio::test]
async fn account_data_is_decoded_with_integer_lamports() {
    let (g, _s, _mock) = helius(|r| {
        rpc_result(
            r,
            &json!({"context": {"slot": 5}, "value": {
                "lamports": 18_446_744_073_709_551_615_u64, "owner": SYSTEM, "data": ["AQID", "base64"],
                "executable": false, "rentEpoch": 18_446_744_073_709_551_615_u64, "space": 3
            }}),
        )
    })
    .await;
    let r = g
        .get_account_info(&ctx(Priority::P1), &WalletAddress::parse(SYSTEM).unwrap())
        .await
        .unwrap();
    let acc = r.value.unwrap();
    assert_eq!(acc.lamports.get(), u64::MAX);
    assert_eq!(acc.data_base64, "AQID");
}

#[tokio::test]
async fn oversized_payloads_are_refused() {
    let mock = MockServer::start(|r| {
        let id = r.json()["id"].clone();
        Reply::ok(format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":"{}"}}"#,
            "a".repeat(200_000)
        ))
    })
    .await;
    let mut s = settings();
    s.helius.rpc_url = mock.url.clone();
    s.max_response_bytes = 65_536;
    let (g, _store) = gateway_with(s).await;
    let err = g.get_version(&ctx(Priority::P1)).await.unwrap_err();
    assert!(
        matches!(err, GatewayError::PayloadTooLarge { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn rate_limited_primary_falls_back_to_public_rpc_when_allowed() {
    let primary = MockServer::start(|_| Reply::status(429, "{}").header("retry-after", "1")).await;
    let fallback = MockServer::start(|r| rpc_result(r, &json!(42))).await;
    let mut s = settings();
    s.helius.rpc_url = primary.url.clone();
    s.fallback.url = solprint_config::Secret::new(fallback.url.clone());
    s.fallback.enabled = true;
    let (g, _store) = gateway_with(s).await;

    // P5 does not retry, so exactly one primary attempt is made.
    let r = g
        .get_slot(&ctx(Priority::P5).allow_fallback(true))
        .await
        .unwrap();
    assert_eq!(r.value, 42);
    assert_eq!(r.provider, ProviderId::SolanaRpc);
    assert_eq!(r.provenance, Provenance::Fallback);
    assert_eq!(r.credits_charged, 0, "public RPC costs no Helius credits");
    assert_eq!(primary.hits(), 1);
    assert_eq!(fallback.hits(), 1);
    assert!(
        !fallback.requests()[0].query.contains("api-key"),
        "the key never leaves for the fallback"
    );
}

#[tokio::test]
async fn fallback_is_not_used_without_permission() {
    let primary = MockServer::start(|_| Reply::status(503, "{}")).await;
    let fallback = MockServer::start(|r| rpc_result(r, &json!(42))).await;
    let mut s = settings();
    s.helius.rpc_url = primary.url.clone();
    s.fallback.url = solprint_config::Secret::new(fallback.url.clone());
    s.fallback.enabled = true;
    let (g, _store) = gateway_with(s).await;
    let err = g
        .get_slot(&ctx(Priority::P5).allow_fallback(false))
        .await
        .unwrap_err();
    assert!(
        matches!(err, GatewayError::Http { status: 503, .. }),
        "{err:?}"
    );
    assert_eq!(fallback.hits(), 0);
}

#[tokio::test]
async fn circuit_opens_after_consecutive_failures() {
    let (g, _s, mock) = helius(|_| Reply::status(500, "{}")).await;
    for _ in 0..5 {
        let c = ctx(Priority::P5).with_cache(CachePolicy::Refresh);
        assert!(g.get_slot(&c).await.is_err());
    }
    assert_eq!(mock.hits(), 5);
    let err = g
        .get_slot(&ctx(Priority::P5).with_cache(CachePolicy::Refresh))
        .await
        .unwrap_err();
    assert!(matches!(err, GatewayError::CircuitOpen { .. }), "{err:?}");
    assert_eq!(mock.hits(), 5, "an open circuit sends nothing");
    let snap = g.snapshot();
    let h = snap
        .providers
        .iter()
        .find(|p| p.provider == ProviderId::Helius)
        .unwrap();
    assert_eq!(h.breaker.state.as_str(), "OPEN");
}

#[tokio::test]
async fn transient_errors_are_retried_for_normal_priorities() {
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = Arc::clone(&n);
    let (g, _s, mock) = helius(move |r| {
        if n2.fetch_add(1, Ordering::SeqCst) == 0 {
            Reply::status(502, "{}")
        } else {
            rpc_result(r, &json!(9))
        }
    })
    .await;
    let r = g.get_slot(&ctx(Priority::P1)).await.unwrap();
    assert_eq!(r.value, 9);
    assert_eq!(mock.hits(), 2);
    // Both attempts were dispatched, so both are charged.
    assert_eq!(g.snapshot().budget.unwrap().credits_used, 2);
}

#[tokio::test]
async fn budget_sheds_low_priorities_first_and_persists_across_restarts() {
    let mock = MockServer::start(|r| rpc_result(r, &json!(1))).await;
    let mut s = settings();
    s.helius.rpc_url = mock.url.clone();
    // 10 credits, 2 reserved -> 8 normal; conservation at 70 % (5.6).
    s.helius.budget.monthly_credits = 10;
    s.helius.rps = 10;
    let store = Arc::new(MemoryStore::default());
    let g = Gateway::new(s.clone(), store.clone()).unwrap();
    g.load_ledger().await;

    let refresh = |p| ctx(p).with_cache(CachePolicy::Refresh);
    let mut admitted = 0;
    loop {
        match g.get_slot(&refresh(Priority::P5)).await {
            Ok(_) => admitted += 1,
            Err(GatewayError::BudgetDenied { .. }) => break,
            Err(e) => panic!("unexpected {e:?}"),
        }
        assert!(admitted < 10, "P5 must be shed before the budget is spent");
    }
    let hits_before = mock.hits();
    assert!((1..=6).contains(&admitted), "admitted {admitted}");
    // A denied request is never sent.
    assert!(g.get_slot(&refresh(Priority::P5)).await.is_err());
    assert_eq!(mock.hits(), hits_before);
    // P0 is still admitted.
    g.get_slot(&refresh(Priority::P0)).await.unwrap();

    g.flush().await;
    let used = g.snapshot().budget.unwrap().credits_used;
    assert_eq!(store.credits_used("helius"), used);

    // A restarted process sees the same usage.
    let g2 = Gateway::new(s, store.clone()).unwrap();
    g2.load_ledger().await;
    assert_eq!(g2.snapshot().budget.unwrap().credits_used, used);
}

#[tokio::test]
async fn unloaded_ledger_admits_only_high_priorities() {
    let (g, _s, mock) = {
        let mock = MockServer::start(|r| rpc_result(r, &json!(1))).await;
        let mut s = settings();
        s.helius.rpc_url = mock.url.clone();
        let store = Arc::new(MemoryStore::default());
        store.set_failing(true);
        let g = Gateway::new(s, store.clone()).unwrap();
        g.load_ledger().await; // fails: usage unknown
        (g, store, mock)
    };
    let snap = g.snapshot().budget.unwrap();
    assert_eq!(snap.state.as_str(), "ACCOUNT_USAGE_UNKNOWN");
    assert!(matches!(
        g.get_slot(&ctx(Priority::P3)).await,
        Err(GatewayError::BudgetDenied { .. })
    ));
    g.get_slot(&ctx(Priority::P2)).await.unwrap();
    assert_eq!(mock.hits(), 1);
}

#[tokio::test]
async fn concurrent_identical_reads_are_coalesced() {
    let (g, _s, mock) = helius(|r| {
        rpc_result(r, &json!({"solana-core": "2.2.0", "feature-set": 1}))
            .delayed(Duration::from_millis(200))
    })
    .await;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let g = g.clone();
        tasks.push(tokio::spawn(async move {
            g.get_version(&ctx(Priority::P1)).await
        }));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap().unwrap().value.solana_core, "2.2.0");
    }
    assert_eq!(mock.hits(), 1);
}

#[tokio::test]
async fn deadlines_bound_slow_providers() {
    let (g, _s, _mock) = helius(|r| rpc_result(r, &json!(1)).delayed(Duration::from_secs(5))).await;
    let started = std::time::Instant::now();
    let err = g
        .get_slot(
            &RequestContext::new("test", Priority::P5).with_timeout(Duration::from_millis(300)),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            GatewayError::Timeout { .. } | GatewayError::DeadlineExceeded
        ),
        "{err:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn cancellation_stops_a_request() {
    let (g, _s, _mock) = helius(|r| rpc_result(r, &json!(1)).delayed(Duration::from_secs(5))).await;
    let cancel = tokio_util::sync::CancellationToken::new();
    let c = ctx(Priority::P5).with_cancel(cancel.clone());
    let task = tokio::spawn(async move { g.get_slot(&c).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    let err = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(err, GatewayError::Cancelled);
}

#[tokio::test]
async fn get_block_is_disabled_by_default_and_sends_nothing() {
    let (g, _s, mock) = helius(|r| rpc_result(r, &json!(null))).await;
    assert!(g.get_block(&ctx(Priority::P1), 1).await.is_err());
    assert_eq!(mock.hits(), 0);
}

#[tokio::test]
async fn missing_helius_key_disables_helius() {
    let mut s = settings();
    s.helius.api_key = None;
    let (g, _store) = gateway_with(s).await;
    assert!(matches!(
        g.get_slot(&ctx(Priority::P1)).await,
        Err(GatewayError::Disabled {
            provider: ProviderId::Helius,
            ..
        })
    ));
}

#[tokio::test]
async fn signature_pagination_is_bounded() {
    let (g, _s, mock) = helius(|r| {
        let before = r.json()["params"][1]["before"].as_str().map(str::to_owned);
        let base: u8 = if before.is_none() { 1 } else { 3 };
        rpc_result(
            r,
            &json!([
                {"signature": support::sig(base), "slot": 100, "err": null, "blockTime": 1_700_000_000, "confirmationStatus": "finalized"},
                {"signature": support::sig(base + 1), "slot": 99, "err": {"InstructionError": [0, "Custom"]}, "blockTime": null}
            ]),
        )
    })
    .await;
    let q = solprint_gateway::models::SignatureQuery {
        address: WalletAddress::parse(SYSTEM).unwrap(),
        before: None,
        until: None,
        page_limit: 2,
        max_pages: 2,
    };
    let page = g
        .get_signatures_for_address(&ctx(Priority::P1), &q)
        .await
        .unwrap()
        .value;
    assert_eq!(page.items.len(), 4);
    assert_eq!(page.pages_fetched, 2);
    assert!(!page.complete, "a full last page means more may exist");
    assert!(page.next_before.is_some());
    assert!(page.items[1].failed);
    assert_eq!(mock.hits(), 2);
}

// ---------------------------------------------------------------- Jupiter

async fn jupiter(
    script: impl Fn(&support::Recorded) -> Reply + Send + Sync + 'static,
    key: bool,
) -> (Gateway, MockServer) {
    let mock = MockServer::start(script).await;
    let mut s = settings();
    s.jupiter.base_url = mock.url.clone();
    if key {
        s.jupiter.api_key = Some(solprint_config::Secret::new("jup-test-key".into()));
    }
    let (g, _store) = gateway_with(s).await;
    (g, mock)
}

fn quote_req() -> QuoteRequest {
    QuoteRequest {
        input_mint: TokenMint::parse(SOL).unwrap(),
        output_mint: TokenMint::parse(USDC).unwrap(),
        amount: 1_000_000_000,
        slippage_bps: Some(50),
    }
}

#[tokio::test]
async fn jupiter_quote_is_quote_only() {
    let (g, mock) = jupiter(
        |_| {
            Reply::ok(format!(
                r#"{{"inputMint":"{SOL}","outputMint":"{USDC}","inAmount":"1000000000","outAmount":"147120000","otherAmountThreshold":"146384400","swapMode":"ExactIn","slippageBps":50,"transaction":null,"routePlan":[]}}"#
            ))
        },
        true,
    )
    .await;
    let q = g
        .jupiter_quote(&ctx(Priority::P1), &quote_req())
        .await
        .unwrap();
    assert_eq!(q.value.out_amount, 147_120_000);
    let req = &mock.requests()[0];
    assert_eq!(req.path, "/swap/v2/order");
    assert!(
        !req.query.contains("taker"),
        "no taker: the response can never carry a transaction"
    );
    assert_eq!(req.headers.get("x-api-key").unwrap(), "jup-test-key");
}

#[tokio::test]
async fn jupiter_response_with_a_transaction_is_refused() {
    let (g, _mock) = jupiter(
        |_| {
            Reply::ok(format!(
                r#"{{"inputMint":"{SOL}","outputMint":"{USDC}","inAmount":"1","outAmount":"1","otherAmountThreshold":"1","transaction":"AQAB"}}"#
            ))
        },
        false,
    )
    .await;
    assert!(matches!(
        g.jupiter_quote(&ctx(Priority::P1), &quote_req()).await,
        Err(GatewayError::Prohibited(_))
    ));
}

#[tokio::test]
async fn jupiter_no_route_is_typed() {
    let (g, _mock) = jupiter(
        |_| Reply::status(400, r#"{"error":"No routes found"}"#),
        false,
    )
    .await;
    assert_eq!(
        g.jupiter_quote(&ctx(Priority::P1), &quote_req())
            .await
            .unwrap_err(),
        GatewayError::NoRoute
    );
}

#[tokio::test]
async fn jupiter_prices_keep_exact_decimals() {
    let (g, mock) = jupiter(
        |_| {
            Reply::ok(format!(
                r#"{{"{SOL}":{{"usdPrice":147.123456789012345678,"blockId":1,"decimals":9}}}}"#
            ))
        },
        false,
    )
    .await;
    let mints = [
        TokenMint::parse(SOL).unwrap(),
        TokenMint::parse(SOL).unwrap(),
    ];
    let r = g.jupiter_prices(&ctx(Priority::P1), &mints).await.unwrap();
    assert_eq!(r.value.len(), 1);
    assert_eq!(
        r.value[0].usd_price.get().to_string(),
        "147.123456789012345678"
    );
    assert_eq!(r.freshness, solprint_gateway::Freshness::UnknownSourceAge);
    let req = &mock.requests()[0];
    assert_eq!(req.path, "/price/v3");
    assert_eq!(req.query.matches(SOL).count(), 1, "ids are de-duplicated");
    assert!(req.headers.get("x-api-key").is_none());
}

#[tokio::test]
async fn jupiter_429_is_rate_limited_with_retry_after() {
    let (g, _mock) = jupiter(
        |_| Reply::status(429, "{}").header("retry-after", "2"),
        false,
    )
    .await;
    let err = g
        .jupiter_prices(&ctx(Priority::P5), &[TokenMint::parse(SOL).unwrap()])
        .await
        .unwrap_err();
    match err {
        GatewayError::RateLimited { retry_after, .. } => {
            assert_eq!(retry_after, Some(Duration::from_secs(2)))
        }
        other => panic!("unexpected {other:?}"),
    }
    let snap = g.snapshot();
    let j = snap
        .providers
        .iter()
        .find(|p| p.provider == ProviderId::Jupiter)
        .unwrap();
    assert!(j.limiters.iter().any(|l| l.upstream_limited >= 1));
}

// ----------------------------------------------------------- DEX Screener

fn pair_json(pair: &str) -> String {
    format!(
        r#"{{"chainId":"solana","dexId":"raydium","url":"https://dexscreener.com/solana/x","pairAddress":"{pair}",
            "baseToken":{{"address":"{SOL}","name":"Wrapped SOL","symbol":"SOL"}},
            "quoteToken":{{"address":"{USDC}","name":"USD Coin","symbol":"USDC"}},
            "priceNative":"147.1","priceUsd":"147.10","liquidity":{{"usd":1000.5,"base":1,"quote":2}},
            "fdv":1,"marketCap":2,"pairCreatedAt":1700000000000,
            "txns":{{"m5":{{"buys":1,"sells":2}}}},"volume":{{"h24":10.25}},"priceChange":{{"h24":-1.5}}}}"#
    )
}

#[tokio::test]
async fn dex_pairs_by_tokens_parse_and_cache() {
    let pair = support::addr(9);
    let body = format!("[{}]", pair_json(&pair));
    let mock = MockServer::start(move |_| Reply::ok(body.clone())).await;
    let mut s = settings();
    s.dexscreener.base_url = mock.url.clone();
    let (g, _store) = gateway_with(s).await;
    let mints = [TokenMint::parse(SOL).unwrap()];
    let r = g
        .dex_pairs_by_tokens(&ctx(Priority::P1), &mints)
        .await
        .unwrap();
    assert_eq!(r.value.len(), 1);
    assert_eq!(r.value[0].price_usd.unwrap().get().to_string(), "147.10");
    assert_eq!(r.freshness, solprint_gateway::Freshness::UnknownSourceAge);
    assert!(mock.requests()[0].path.starts_with("/tokens/v1/solana/"));
    g.dex_pairs_by_tokens(&ctx(Priority::P1), &mints)
        .await
        .unwrap();
    assert_eq!(mock.hits(), 1);
}

#[tokio::test]
async fn dex_discovery_is_off_by_default() {
    let mock = MockServer::start(|_| Reply::ok("[]")).await;
    let mut s = settings();
    s.dexscreener.base_url = mock.url.clone();
    let (g, _store) = gateway_with(s).await;
    assert!(g.dex_latest_profiles(&ctx(Priority::P3)).await.is_err());
    assert!(g.dex_boosts(&ctx(Priority::P3), true).await.is_err());
    assert_eq!(mock.hits(), 0);
}

// -------------------------------------------------------------- Monitoring

#[tokio::test]
async fn snapshots_and_stored_reads_never_contact_providers() {
    let (g, _s, mock) = helius(|r| rpc_result(r, &json!(1))).await;
    g.get_slot(&ctx(Priority::P1)).await.unwrap();
    let before = mock.hits();
    for _ in 0..20 {
        let snap = g.snapshot();
        assert!(snap.streams.is_none());
        assert_eq!(
            snap.streams_disabled_reason,
            Some("GATEWAY_STREAMS_ENABLED=false")
        );
        let _ = g.usage_history(&solprint_core::UtcTimestamp::now()).await;
        let _ = g.stored_checkpoints().await;
    }
    assert_eq!(mock.hits(), before);
    let helius = g.snapshot();
    let h = helius
        .providers
        .iter()
        .find(|p| p.provider == ProviderId::Helius)
        .unwrap();
    assert_eq!(h.health.state.as_str(), "HEALTHY");
    assert!(h.latency.p50_us.is_some());
}

#[tokio::test]
async fn usage_and_metrics_are_flushed_to_the_store() {
    let (g, store, _mock) = helius(|r| rpc_result(r, &json!(1))).await;
    g.get_slot(&ctx(Priority::P1)).await.unwrap();
    g.flush().await;
    assert!(!store.usage_rows().is_empty());
    assert!(!store.metric_rows().is_empty());
    assert_eq!(g.snapshot().persistence.last_flush_ok, Some(true));

    store.set_failing(true);
    g.get_slot(&ctx(Priority::P1).with_cache(CachePolicy::Refresh))
        .await
        .unwrap();
    g.flush().await;
    let p = g.snapshot().persistence;
    assert_eq!(p.last_flush_ok, Some(false));
    assert!(p.flush_failures >= 1);
    // Nothing is lost: the next successful flush writes the pending credits.
    store.set_failing(false);
    g.flush().await;
    assert_eq!(store.credits_used("helius"), 2);
}

// ------------------------------------------------- batching and versions

#[tokio::test]
async fn multiple_accounts_are_chunked_at_the_documented_maximum() {
    let (g, _s, mock) = helius(|r| {
        let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        let n = body["params"][0].as_array().unwrap().len();
        rpc_result(
            r,
            &json!({"context": {"slot": 7}, "value": vec![serde_json::Value::Null; n]}),
        )
    })
    .await;
    let addrs: Vec<WalletAddress> = (1..=150_u8)
        .map(|n| WalletAddress::parse(&support::addr(n)).unwrap())
        .collect();
    let r = g
        .get_multiple_accounts(&ctx(Priority::P1), &addrs)
        .await
        .unwrap();
    assert_eq!(r.value.len(), 150, "every requested account is answered");
    let sizes: Vec<usize> = mock
        .requests()
        .iter()
        .map(|q| {
            let b: serde_json::Value = serde_json::from_slice(&q.body).unwrap();
            b["params"][0].as_array().unwrap().len()
        })
        .collect();
    assert_eq!(sizes, vec![100, 50], "chunks of at most 100 keys");
}

#[tokio::test]
async fn dex_token_batches_never_exceed_thirty_addresses() {
    let mock = MockServer::start(|_| Reply::ok("[]")).await;
    let mut s = settings();
    s.dexscreener.base_url = mock.url.clone();
    let (g, _store) = gateway_with(s).await;
    let mints: Vec<TokenMint> = (1..=31_u8)
        .map(|n| TokenMint::parse(&support::addr(n)).unwrap())
        .collect();
    g.dex_pairs_by_tokens(&ctx(Priority::P1), &mints)
        .await
        .unwrap();
    let counts: Vec<usize> = mock
        .requests()
        .iter()
        .map(|q| q.path.rsplit('/').next().unwrap().split(',').count())
        .collect();
    assert_eq!(counts, vec![30, 1]);
}

#[tokio::test]
async fn versioned_and_legacy_transactions_are_supported() {
    let s0 = support::sig(4);
    let s1 = support::sig(5);
    let a = s0.clone();
    let (g, _s, mock) = helius(move |r| {
        let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        let sig = body["params"][0].as_str().unwrap().to_owned();
        let version = if sig == a { json!(0) } else { json!("legacy") };
        rpc_result(
            r,
            &json!({"slot": 42, "blockTime": 1_700_000_000, "version": version,
                    "transaction": {"signatures": [sig], "message": {}},
                    "meta": {"err": null}}),
        )
    })
    .await;
    let c = ctx(Priority::P1).with_commitment(solprint_core::CommitmentLevel::Confirmed);
    let v0 = g
        .get_transaction(
            &c,
            &solprint_core::TransactionSignature::parse(&s0).unwrap(),
        )
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(v0.version.as_deref(), Some("0"));
    let legacy = g
        .get_transaction(
            &c,
            &solprint_core::TransactionSignature::parse(&s1).unwrap(),
        )
        .await
        .unwrap()
        .value
        .unwrap();
    assert_eq!(legacy.version.as_deref(), Some("legacy"));
    let body: serde_json::Value = serde_json::from_slice(&mock.requests()[0].body).unwrap();
    assert_eq!(body["params"][1]["maxSupportedTransactionVersion"], 0);
    assert_eq!(body["params"][1]["commitment"], "confirmed");
}

#[tokio::test]
async fn jupiter_endpoints_share_one_rate_group() {
    let (g, mock) = jupiter(
        |_| Reply::status(429, "{}").header("retry-after", "5"),
        false,
    )
    .await;
    g.jupiter_prices(&ctx(Priority::P5), &[TokenMint::parse(SOL).unwrap()])
        .await
        .unwrap_err();
    let hits = mock.hits();
    // A quote is a different endpoint but the same documented bucket: it
    // must wait out the provider's cooldown, not send.
    let short = RequestContext::new("test", Priority::P3).with_timeout(Duration::from_secs(1));
    assert!(g.jupiter_quote(&short, &quote_req()).await.is_err());
    assert_eq!(mock.hits(), hits, "no request during the shared cooldown");
}

#[tokio::test]
async fn signature_statuses_keep_order_and_null_means_unknown() {
    use solprint_core::{CommitmentLevel, TransactionSignature};
    let (g, _s, mock) = helius(|r| {
        rpc_result(
            r,
            &json!({"context": {"slot": 500}, "value": [
                {"slot": 410, "confirmations": null, "err": null, "confirmationStatus": "finalized", "status": {"Ok": null}},
                null,
                {"slot": 499, "confirmations": 1, "err": {"InstructionError": [0, {"Custom": 1}]}, "confirmationStatus": "confirmed", "status": {"Err": {}}}
            ]}),
        )
    })
    .await;
    let sig = |c: char| TransactionSignature::parse(&c.to_string().repeat(88)).unwrap();
    let sigs = [sig('2'), sig('3'), sig('4')];
    let r = g
        .get_signature_statuses(&ctx(Priority::P4), &sigs, false)
        .await
        .unwrap();
    assert_eq!(r.value.len(), 3);
    let first = r.value[0].as_ref().unwrap();
    assert_eq!(first.slot, 410);
    assert_eq!(first.confirmation_status, Some(CommitmentLevel::Finalized));
    assert!(!first.failed);
    assert!(
        r.value[1].is_none(),
        "null stays unknown, never dropped/failed"
    );
    let third = r.value[2].as_ref().unwrap();
    assert!(third.failed);
    assert_eq!(third.confirmations, Some(1));
    assert_eq!(r.credits_charged, 1);
    let req = &mock.requests()[0];
    assert_eq!(rpc_method(req), "getSignatureStatuses");
    assert!(
        String::from_utf8_lossy(&req.body).contains("searchTransactionHistory"),
        "config object always sent"
    );

    // Not cached: a second call is a second request.
    g.get_signature_statuses(&ctx(Priority::P4), &sigs, false)
        .await
        .unwrap();
    assert_eq!(mock.hits(), 2);
    assert!(
        g.get_signature_statuses(&ctx(Priority::P4), &[], false)
            .await
            .is_err()
    );
}
