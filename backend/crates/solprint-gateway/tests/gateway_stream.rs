//! Stream manager against a loopback WebSocket server: subscribe/ack,
//! notifications, reconnect with resubscription, bounded gap recovery,
//! de-duplication, backpressure and metering.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::result_large_err)]

mod support;

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use solprint_core::{CommitmentLevel, WalletAddress};
use solprint_gateway::stream::{
    ConnectionState, StreamEvent, StreamEventKind, StreamHandle, SubscriptionSpec,
};
use solprint_gateway::{Gateway, MemoryStore, Provenance};
use support::{MockServer, Reply, rpc_result, settings};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

const ADDR: &str = "11111111111111111111111111111111";

type Server = WebSocketStream<TcpStream>;

/// Accepts connections and hands each one to the test, in order.
async fn ws_server() -> (url::Url, mpsc::Receiver<(Server, String)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel(8);
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                return;
            };
            let path = Arc::new(std::sync::Mutex::new(String::new()));
            let p2 = Arc::clone(&path);
            let cb = move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                           resp| {
                *p2.lock().unwrap() = req.uri().to_string();
                Ok(resp)
            };
            if let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await {
                let uri = path.lock().unwrap().clone();
                if tx.send((ws, uri)).await.is_err() {
                    return;
                }
            }
        }
    });
    (url::Url::parse(&format!("ws://{addr}/")).unwrap(), rx)
}

/// Read the next text frame as JSON (skipping pings).
async fn next_json(ws: &mut Server) -> Value {
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("timed out waiting for a client frame")
            .unwrap()
            .unwrap();
        if let Message::Text(t) = msg {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

/// Answer one subscribe request with `server_id`; returns the request.
async fn ack(ws: &mut Server, server_id: u64) -> Value {
    let req = next_json(ws).await;
    let reply = json!({"jsonrpc": "2.0", "result": server_id, "id": req["id"]});
    ws.send(Message::text(reply.to_string())).await.unwrap();
    req
}

async fn notify(ws: &mut Server, method: &str, sub: u64, result: Value) {
    let m = json!({"jsonrpc": "2.0", "method": method, "params": {"result": result, "subscription": sub}});
    ws.send(Message::text(m.to_string())).await.unwrap();
}

fn logs_result(slot: u64, signature: &str) -> Value {
    json!({"context": {"slot": slot}, "value": {"signature": signature, "err": null, "logs": ["Program log: hi"]}})
}

struct Harness {
    gateway: Gateway,
    handle: StreamHandle,
    events: mpsc::Receiver<StreamEvent>,
    shutdown: CancellationToken,
    store: Arc<MemoryStore>,
}

async fn start(ws_url: url::Url, rpc: Option<&MockServer>, store: Arc<MemoryStore>) -> Harness {
    start_with(ws_url, rpc, store, |_| {}).await
}

async fn start_with(
    ws_url: url::Url,
    rpc: Option<&MockServer>,
    store: Arc<MemoryStore>,
    tune: impl FnOnce(&mut solprint_gateway::GatewaySettings),
) -> Harness {
    let mut s = settings();
    tune(&mut s);
    s.streams_enabled = true;
    s.helius.ws_url = ws_url;
    if let Some(rpc) = rpc {
        s.helius.rpc_url = rpc.url.clone();
    }
    let gateway = Gateway::new(s, store.clone()).unwrap();
    let shutdown = CancellationToken::new();
    let _task = gateway.start(shutdown.clone());
    let handle = gateway.streams().unwrap().clone();
    let events = handle.take_events().unwrap();
    Harness {
        gateway,
        handle,
        events,
        shutdown,
        store,
    }
}

/// Next non-connection-state event.
async fn next_event(rx: &mut mpsc::Receiver<StreamEvent>) -> StreamEvent {
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for an event")
            .unwrap();
        if !matches!(ev.kind, StreamEventKind::ConnectionState(_)) {
            return ev;
        }
    }
}

async fn wait_state(h: &StreamHandle, want: ConnectionState) {
    for _ in 0..100 {
        if h.snapshot().state == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("state never became {want:?}; is {:?}", h.snapshot().state);
}

#[tokio::test]
async fn slot_subscription_delivers_events_and_meters_bytes() {
    let (url, mut conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    let mut events = h.events;
    let sub = h
        .handle
        .subscribe(SubscriptionSpec::Slot, None)
        .await
        .unwrap();

    let (mut ws, uri) = conns.recv().await.unwrap();
    assert!(
        uri.contains("api-key="),
        "the key is sent in the handshake query"
    );
    let req = ack(&mut ws, 7).await;
    assert_eq!(req["method"], "slotSubscribe");
    wait_state(&h.handle, ConnectionState::Connected).await;

    notify(
        &mut ws,
        "slotNotification",
        7,
        json!({"slot": 100, "parent": 99, "root": 68}),
    )
    .await;
    let ev = next_event(&mut events).await;
    assert_eq!(ev.subscription, Some(sub));
    assert_eq!(ev.generation, 1);
    assert_eq!(ev.provenance, Provenance::Direct);
    assert!(matches!(
        ev.kind,
        StreamEventKind::Slot {
            slot: 100,
            parent: Some(99),
            root: Some(68)
        }
    ));

    let snap = h.handle.snapshot();
    assert!(snap.bytes_received > 0);
    assert!(snap.credits_charged >= 1, "a connection costs one credit");
    assert_eq!(snap.subscriptions.len(), 1);
    assert!(snap.subscriptions[0].active);

    // Unknown subscriptions are ignored.
    notify(&mut ws, "slotNotification", 999, json!({"slot": 1})).await;
    // Unsubscribe sends the server id.
    h.handle.unsubscribe(sub).await.unwrap();
    let un = next_json(&mut ws).await;
    assert_eq!(un["method"], "slotUnsubscribe");
    assert_eq!(un["params"][0], 7);
    h.shutdown.cancel();
}

#[tokio::test]
async fn failed_subscription_degrades_the_connection() {
    let (url, mut conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    let mut events = h.events;
    let spec = SubscriptionSpec::Account {
        address: WalletAddress::parse(ADDR).unwrap(),
        commitment: CommitmentLevel::Confirmed,
    };
    h.handle.subscribe(spec, None).await.unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    let req = next_json(&mut ws).await;
    let err = json!({"jsonrpc": "2.0", "error": {"code": -32602, "message": "Invalid param"}, "id": req["id"]});
    ws.send(Message::text(err.to_string())).await.unwrap();
    let ev = next_event(&mut events).await;
    assert!(matches!(
        ev.kind,
        StreamEventKind::SubscriptionFailed { .. }
    ));
    wait_state(&h.handle, ConnectionState::Degraded).await;
    h.shutdown.cancel();
}

#[tokio::test]
async fn reconnect_resubscribes_and_reports_unknown_continuity() {
    let (url, mut conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    let mut events = h.events;
    let sub = h
        .handle
        .subscribe(SubscriptionSpec::Slot, None)
        .await
        .unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    ack(&mut ws, 1).await;
    wait_state(&h.handle, ConnectionState::Connected).await;
    ws.close(None).await.unwrap();
    drop(ws);

    let (mut ws2, _) = conns.recv().await.unwrap();
    let req = ack(&mut ws2, 2).await;
    assert_eq!(req["method"], "slotSubscribe", "subscriptions are restored");
    let ev = next_event(&mut events).await;
    assert_eq!(ev.subscription, Some(sub));
    assert_eq!(ev.generation, 2);
    assert!(matches!(ev.kind, StreamEventKind::ContinuityUnknown { .. }));

    // Notifications on the new server id reach the same subscription.
    notify(
        &mut ws2,
        "slotNotification",
        2,
        json!({"slot": 5, "parent": 4, "root": 1}),
    )
    .await;
    let ev = next_event(&mut events).await;
    assert_eq!(ev.subscription, Some(sub));
    assert_eq!(ev.generation, 2);
    let snap = h.handle.snapshot();
    assert_eq!(snap.reconnects, 1);
    assert_eq!(snap.generation, 2);
    h.shutdown.cancel();
}

#[tokio::test]
async fn silent_connection_is_detected_by_heartbeat_and_replaced() {
    let (url, mut conns) = ws_server().await;
    let h = start_with(url, None, Arc::default(), |s| {
        s.stream_ping_interval = Duration::from_millis(200);
        s.stream_pong_timeout = Duration::from_millis(200);
    })
    .await;
    h.handle
        .subscribe(SubscriptionSpec::Slot, None)
        .await
        .unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    ack(&mut ws, 1).await;
    wait_state(&h.handle, ConnectionState::Connected).await;

    // The server keeps the TCP connection open but stops reading, so the
    // client's pings are never answered: a half-dead connection.
    let started = tokio::time::Instant::now();
    let (mut ws2, _) = tokio::time::timeout(Duration::from_secs(5), conns.recv())
        .await
        .expect("dead connection was never replaced")
        .unwrap();
    assert!(
        started.elapsed() >= Duration::from_millis(200),
        "replaced only after a missed pong"
    );
    let req = ack(&mut ws2, 2).await;
    assert_eq!(req["method"], "slotSubscribe", "subscription restored");
    wait_state(&h.handle, ConnectionState::Connected).await;
    let snap = h.handle.snapshot();
    assert_eq!(snap.reconnects, 1);
    assert_eq!(snap.generation, 2);
    drop(ws);
    h.shutdown.cancel();
}

#[tokio::test]
async fn logs_gap_is_recovered_oldest_first_with_dedup_and_checkpoints() {
    let s1 = support::sig(1);
    let s2 = support::sig(2);
    let s3 = support::sig(3);
    let (s2c, s3c) = (s2.clone(), s3.clone());
    let rpc = MockServer::start(move |r| {
        assert_eq!(support::rpc_method(r), "getSignaturesForAddress");
        // Newest first, as the RPC returns them.
        rpc_result(
            r,
            &json!([
                {"signature": s3c, "slot": 30, "err": null, "blockTime": 1_700_000_030},
                {"signature": s2c, "slot": 20, "err": null, "blockTime": 1_700_000_020}
            ]),
        )
    })
    .await;
    let (url, mut conns) = ws_server().await;
    let h = start(url, Some(&rpc), Arc::default()).await;
    let mut events = h.events;
    let spec = SubscriptionSpec::Logs {
        mentions: WalletAddress::parse(ADDR).unwrap(),
        commitment: CommitmentLevel::Confirmed,
    };
    let sub = h
        .handle
        .subscribe(spec, Some("test.logs".into()))
        .await
        .unwrap();

    let (mut ws, _) = conns.recv().await.unwrap();
    let req = ack(&mut ws, 11).await;
    assert_eq!(req["params"][0]["mentions"].as_array().unwrap().len(), 1);
    notify(&mut ws, "logsNotification", 11, logs_result(10, &s1)).await;
    let ev = next_event(&mut events).await;
    assert!(matches!(ev.kind, StreamEventKind::Logs { .. }));
    assert_eq!(ev.commitment, Some(CommitmentLevel::Confirmed));
    ws.close(None).await.unwrap();
    drop(ws);

    let (mut ws2, _) = conns.recv().await.unwrap();
    ack(&mut ws2, 12).await;
    let a = next_event(&mut events).await;
    let b = next_event(&mut events).await;
    match (&a.kind, &b.kind) {
        (
            StreamEventKind::Recovered { signature: x, .. },
            StreamEventKind::Recovered { signature: y, .. },
        ) => {
            assert_eq!(x.as_str(), s2, "oldest first");
            assert_eq!(y.as_str(), s3);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(a.provenance, Provenance::Recovery);
    assert_eq!(a.subscription, Some(sub));

    // The recovery query was bounded by the checkpoint.
    let q = rpc.requests()[0].json();
    assert_eq!(q["params"][1]["until"], s1.as_str());
    assert_eq!(q["params"][1]["limit"], 100);

    // The live stream re-delivering S3 is suppressed.
    notify(&mut ws2, "logsNotification", 12, logs_result(30, &s3)).await;
    let s4 = support::sig(4);
    notify(&mut ws2, "logsNotification", 12, logs_result(40, &s4)).await;
    let ev = next_event(&mut events).await;
    match ev.kind {
        StreamEventKind::Logs { signature, .. } => assert_eq!(signature.as_str(), s4),
        other => panic!("duplicate was not suppressed: {other:?}"),
    }
    let snap = h.handle.snapshot();
    assert_eq!(snap.recovered, 2);
    assert_eq!(snap.duplicates_suppressed, 1);
    assert_eq!(snap.subscriptions[0].continuity.as_str(), "RECOVERED");

    // Checkpoints reach the store on flush.
    h.gateway.flush().await;
    let rows = h.store.checkpoint_rows();
    let cp = rows.iter().find(|c| c.stream_key == "test.logs").unwrap();
    assert_eq!(cp.last_slot, Some(40));
    assert_eq!(cp.last_signature.as_deref(), Some(s4.as_str()));
    h.shutdown.cancel();
}

#[tokio::test]
async fn incomplete_recovery_reports_a_gap() {
    let rpc = MockServer::start(|_| Reply::status(500, "{}")).await;
    let store = Arc::new(MemoryStore::default());
    store.seed_checkpoint(solprint_storage::StreamCheckpointRow {
        stream_key: "restart.logs".into(),
        kind: "logs".into(),
        commitment: "confirmed".into(),
        last_slot: Some(10),
        last_signature: Some(support::sig(1)),
        continuity: "CONTINUOUS".into(),
        updated_at: solprint_core::UtcTimestamp::now(),
    });
    let (url, mut conns) = ws_server().await;
    let h = start(url, Some(&rpc), store).await;
    let mut events = h.events;
    let spec = SubscriptionSpec::Logs {
        mentions: WalletAddress::parse(ADDR).unwrap(),
        commitment: CommitmentLevel::Confirmed,
    };
    // Give the manager a moment to load stored checkpoints.
    tokio::time::sleep(Duration::from_millis(200)).await;
    h.handle
        .subscribe(spec, Some("restart.logs".into()))
        .await
        .unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    ack(&mut ws, 1).await;
    // A checkpoint from a previous run triggers recovery on first connect;
    // the RPC failing makes it a reported gap, never silent.
    let ev = next_event(&mut events).await;
    match ev.kind {
        StreamEventKind::Gap { from_slot, .. } => assert_eq!(from_slot, Some(10)),
        other => panic!("unexpected {other:?}"),
    }
    assert!(rpc.hits() >= 1);
    assert_eq!(h.handle.snapshot().gaps, 1);
    h.shutdown.cancel();
}

#[tokio::test]
async fn signature_subscription_ends_after_its_notification() {
    let (url, mut conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    let mut events = h.events;
    let spec = SubscriptionSpec::Signature {
        signature: solprint_core::TransactionSignature::parse(&support::sig(5)).unwrap(),
        commitment: CommitmentLevel::Finalized,
    };
    h.handle.subscribe(spec, None).await.unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    let req = ack(&mut ws, 3).await;
    assert_eq!(req["params"][1]["enableReceivedNotification"], false);
    notify(
        &mut ws,
        "signatureNotification",
        3,
        json!({"context": {"slot": 9}, "value": {"err": null}}),
    )
    .await;
    let ev = next_event(&mut events).await;
    assert!(matches!(
        ev.kind,
        StreamEventKind::Signature { failed: false, .. }
    ));
    for _ in 0..40 {
        if h.handle.snapshot().subscriptions.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(h.handle.snapshot().subscriptions.is_empty());
    h.shutdown.cancel();
}

#[tokio::test]
async fn a_slow_consumer_never_blocks_the_connection() {
    let (url, mut conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    let mut events = h.events;
    let sub = h
        .handle
        .subscribe(SubscriptionSpec::Slot, None)
        .await
        .unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    ack(&mut ws, 1).await;
    wait_state(&h.handle, ConnectionState::Connected).await;
    // Drain what is queued so far, then overflow the 1024-event channel.
    while events.try_recv().is_ok() {}
    for i in 0..1_200_u64 {
        notify(
            &mut ws,
            "slotNotification",
            1,
            json!({"slot": i, "parent": null, "root": null}),
        )
        .await;
    }
    for _ in 0..100 {
        if h.handle.snapshot().events_dropped > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let snap = h.handle.snapshot();
    assert!(snap.events_dropped >= 1, "overflow is counted");
    assert_eq!(
        snap.state,
        ConnectionState::Connected,
        "the connection stays up"
    );
    while events.try_recv().is_ok() {}
    notify(
        &mut ws,
        "slotNotification",
        1,
        json!({"slot": 5_000, "parent": null, "root": null}),
    )
    .await;
    let marker = next_event(&mut events).await;
    assert_eq!(marker.subscription, Some(sub));
    assert!(matches!(
        marker.kind,
        StreamEventKind::ContinuityUnknown { .. }
    ));
    let ev = next_event(&mut events).await;
    assert!(matches!(ev.kind, StreamEventKind::Slot { slot: 5_000, .. }));
    h.shutdown.cancel();
}

#[tokio::test]
async fn subscription_limits_and_validation() {
    let (url, _conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    let unfiltered = SubscriptionSpec::Program {
        program_id: WalletAddress::parse(ADDR).unwrap(),
        commitment: CommitmentLevel::Confirmed,
        filters: vec![],
    };
    assert!(h.handle.subscribe(unfiltered, None).await.is_err());
    assert!(
        h.handle
            .subscribe(SubscriptionSpec::Slot, Some("bad key!".into()))
            .await
            .is_err()
    );
    h.handle
        .subscribe(SubscriptionSpec::Root, Some("k".into()))
        .await
        .unwrap();
    assert!(
        h.handle
            .subscribe(SubscriptionSpec::Root, Some("k".into()))
            .await
            .is_err()
    );
    h.shutdown.cancel();
}

#[tokio::test]
async fn shutdown_stops_the_manager() {
    let (url, mut conns) = ws_server().await;
    let h = start(url, None, Arc::default()).await;
    h.handle
        .subscribe(SubscriptionSpec::Slot, None)
        .await
        .unwrap();
    let (mut ws, _) = conns.recv().await.unwrap();
    ack(&mut ws, 1).await;
    wait_state(&h.handle, ConnectionState::Connected).await;
    h.shutdown.cancel();
    wait_state(&h.handle, ConnectionState::ShuttingDown).await;
}
