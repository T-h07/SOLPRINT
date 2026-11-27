//! The stream manager task: one WebSocket connection, many subscriptions.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use solprint_core::{CommitmentLevel, TransactionSignature, UtcTimestamp, WalletAddress};
use solprint_storage::StreamCheckpointRow;
use time::OffsetDateTime;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use super::protocol::{Incoming, SubscriptionSpec, parse_frame};
use super::{
    BACKOFF_MAX, BACKOFF_MIN, Command, ConnectionState, Continuity, DEDUP_CAPACITY,
    EVENT_CHANNEL_CAPACITY, RECOVERY_MAX_PAGES, RECOVERY_PAGE_LIMIT, Shared, StreamConfig,
    StreamEvent, StreamEventKind, StreamHandle, StreamSnapshot, SubscriptionId,
    SubscriptionSnapshot,
};
use crate::context::{Priority, Provenance, ProviderId, RequestContext};
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::models::{SignaturePage, SignatureQuery};
use crate::operations as op;
use crate::rpc::{commitment_str, parse_account};
use crate::transport::tls_config;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;
type Sink = SplitSink<Ws, Message>;
type Source = SplitStream<Ws>;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const BUDGET_RECHECK: Duration = Duration::from_secs(60);
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_LOG_LINES: usize = 64;
const MAX_LOG_LINE: usize = 512;

#[derive(Debug)]
struct Sub {
    id: SubscriptionId,
    spec: SubscriptionSpec,
    durable_key: Option<String>,
    server_id: Option<u64>,
    pending: Option<u64>,
    failed: bool,
    notifications: u64,
    dropped: u64,
    needs_marker: bool,
    continuity: Continuity,
    last_slot: Option<u64>,
    last_signature: Option<String>,
    recover_on_connect: bool,
}

#[derive(Debug)]
struct RecoveryOutcome {
    sub: SubscriptionId,
    from_slot: Option<u64>,
    result: Result<SignaturePage, GatewayError>,
}

enum Exit {
    Shutdown,
    Reconnect(&'static str),
}

/// Bounded FIFO set for duplicate suppression.
#[derive(Debug, Default)]
struct Dedup {
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl Dedup {
    /// `true` if newly inserted.
    fn insert(&mut self, sig: &str) -> bool {
        if self.set.contains(sig) {
            return false;
        }
        if self.order.len() == DEDUP_CAPACITY
            && let Some(old) = self.order.pop_front()
        {
            self.set.remove(&old);
        }
        self.order.push_back(sig.to_owned());
        self.set.insert(sig.to_owned());
        true
    }
}

/// Spawns and owns the stream task.
#[derive(Debug)]
pub struct StreamManager;

impl StreamManager {
    #[must_use]
    pub fn spawn(cfg: StreamConfig, gateway: Gateway, shutdown: CancellationToken) -> StreamHandle {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let (ev_tx, ev_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let (rec_tx, rec_rx) = mpsc::channel(16);
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(StreamSnapshot {
                state: ConnectionState::Disconnected,
                generation: 0,
                connected_since: None,
                last_message_at: None,
                reconnects: 0,
                bytes_received: 0,
                credits_charged: 0,
                events_emitted: 0,
                events_dropped: 0,
                gaps: 0,
                recovered: 0,
                duplicates_suppressed: 0,
                last_error: None,
                max_subscriptions: cfg.max_subscriptions,
                subscriptions: Vec::new(),
            }),
            events: Mutex::new(Some(ev_rx)),
            dirty_checkpoints: Mutex::new(HashMap::new()),
        });
        let runner = Runner {
            cfg,
            gateway,
            shared: Arc::clone(&shared),
            cmd_rx,
            events: ev_tx,
            rec_tx,
            rec_rx,
            subs: Vec::new(),
            next_sub: 0,
            next_req: 0,
            generation: 0,
            dedup: Dedup::default(),
            stored: HashMap::new(),
        };
        tokio::spawn(runner.run(shutdown));
        StreamHandle {
            cmd: cmd_tx,
            shared,
        }
    }
}

struct Runner {
    cfg: StreamConfig,
    gateway: Gateway,
    shared: Arc<Shared>,
    cmd_rx: mpsc::Receiver<Command>,
    events: mpsc::Sender<StreamEvent>,
    rec_tx: mpsc::Sender<RecoveryOutcome>,
    rec_rx: mpsc::Receiver<RecoveryOutcome>,
    subs: Vec<Sub>,
    next_sub: u64,
    next_req: u64,
    generation: u64,
    dedup: Dedup,
    stored: HashMap<String, StreamCheckpointRow>,
}

impl Runner {
    async fn run(mut self, shutdown: CancellationToken) {
        if let Ok(rows) = self.gateway.stored_checkpoints().await {
            self.stored = rows
                .into_iter()
                .map(|r| (r.stream_key.clone(), r))
                .collect();
        }
        let mut attempt: u32 = 0;
        loop {
            if shutdown.is_cancelled() {
                break;
            }
            if !self.budget_admits_streaming() {
                self.set_state(ConnectionState::BudgetExhausted);
                if self.idle(BUDGET_RECHECK, &shutdown).await {
                    break;
                }
                continue;
            }
            self.set_state(ConnectionState::Connecting);
            if !self.charge_connection() {
                self.set_state(ConnectionState::BudgetExhausted);
                if self.idle(BUDGET_RECHECK, &shutdown).await {
                    break;
                }
                continue;
            }
            match self.connect().await {
                Err(reason) => {
                    self.record_failure(reason);
                    self.set_state(ConnectionState::Reconnecting);
                    let wait = self.backoff(attempt);
                    attempt = attempt.saturating_add(1);
                    if self.idle(wait, &shutdown).await {
                        break;
                    }
                }
                Ok(ws) => {
                    self.generation += 1;
                    let generation = self.generation;
                    self.shared.update(|s| {
                        s.generation = generation;
                        s.connected_since = Some(UtcTimestamp::now());
                        if generation > 1 {
                            s.reconnects = s.reconnects.saturating_add(1);
                        }
                    });
                    match self.session(ws, &shutdown).await {
                        Exit::Shutdown => break,
                        Exit::Reconnect(reason) => {
                            self.record_failure(reason);
                            for s in &mut self.subs {
                                s.server_id = None;
                                s.pending = None;
                            }
                            self.set_state(ConnectionState::Reconnecting);
                            self.shared.update(|s| s.connected_since = None);
                            let wait = self.backoff(0);
                            attempt = 1;
                            if self.idle(wait, &shutdown).await {
                                break;
                            }
                        }
                    }
                }
            }
        }
        self.set_state(ConnectionState::ShuttingDown);
        self.publish_subs();
    }

    fn budget_admits_streaming(&self) -> bool {
        self.gateway
            .budget()
            .is_none_or(|b| b.state(OffsetDateTime::now_utc()).admits(Priority::P2))
    }

    /// One credit per connection opened (documented Helius metering).
    fn charge_connection(&self) -> bool {
        let Some(b) = self.gateway.budget() else {
            return true;
        };
        match b.try_reserve(
            Priority::P2,
            crate::budget::STREAM_CONNECTION_CREDITS,
            OffsetDateTime::now_utc(),
        ) {
            Ok(r) => {
                self.gateway.engine().metrics.record_usage(
                    ProviderId::Helius,
                    op::WS_CONNECT,
                    1,
                    r.credits,
                    0,
                );
                self.shared
                    .update(|s| s.credits_charged = s.credits_charged.saturating_add(r.credits));
                true
            }
            Err(_) => false,
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let exp = BACKOFF_MIN.saturating_mul(1_u32 << attempt.min(6));
        let base = exp.min(BACKOFF_MAX);
        base / 2 + self.gateway.engine().jitter(base / 2)
    }

    fn record_failure(&self, reason: &'static str) {
        let rt = self.gateway.engine().runtime(ProviderId::Helius);
        rt.health.failure("STREAM");
        self.shared.update(|s| s.last_error = Some(reason));
        tracing::warn!(reason, "stream connection lost or failed");
    }

    /// Wait (processing commands) until `dur` passes. Returns `true` on shutdown.
    async fn idle(&mut self, dur: Duration, shutdown: &CancellationToken) -> bool {
        let until = Instant::now() + dur;
        loop {
            tokio::select! {
                () = shutdown.cancelled() => return true,
                () = tokio::time::sleep_until(until) => return false,
                cmd = self.cmd_rx.recv() => match cmd {
                    Some(c) => { let _ = self.handle_cmd(c, None).await; }
                    None => { tokio::time::sleep_until(until).await; return shutdown.is_cancelled(); }
                },
            }
        }
    }

    async fn connect(&self) -> Result<Ws, &'static str> {
        let url = self.cfg.url.expose();
        let host = url.host_str().unwrap_or_default();
        let loopback = matches!(host, "127.0.0.1" | "localhost");
        let connector = match url.scheme() {
            "wss" => Connector::Rustls(Arc::new(
                tls_config(false).map_err(|_| "TLS configuration failed")?,
            )),
            "ws" if self.cfg.allow_insecure_loopback && loopback => Connector::Plain,
            _ => return Err("stream URL not allowed"),
        };
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(self.cfg.max_message_bytes);
        config.max_frame_size = Some(self.cfg.max_message_bytes);
        let connect = tokio_tungstenite::connect_async_tls_with_config(
            url.as_str(),
            Some(config),
            true,
            Some(connector),
        );
        // Errors are classified, never formatted: they may describe the
        // request, and the request URL carries the key.
        match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
            Ok(Ok((ws, _response))) => Ok(ws),
            Ok(Err(_)) => Err("connect failed"),
            Err(_) => Err("connect timed out"),
        }
    }

    async fn session(&mut self, ws: Ws, shutdown: &CancellationToken) -> Exit {
        let (mut sink, mut source): (Sink, Source) = ws.split();
        self.set_state(ConnectionState::Subscribing);
        let reconnected = self.generation > 1;
        for i in 0..self.subs.len() {
            if self.send_subscribe(&mut sink, i).await.is_err() {
                return Exit::Reconnect("write failed");
            }
            self.after_resubscribe(i, reconnected);
        }
        self.refresh_state();
        let mut ping = tokio::time::interval(self.cfg.ping_interval);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ping.tick().await;
        let mut pong_deadline: Option<Instant> = None;
        loop {
            let pong_wait =
                pong_deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(86_400));
            tokio::select! {
                () = shutdown.cancelled() => {
                    self.set_state(ConnectionState::ShuttingDown);
                    let _ = sink.send(Message::Close(None)).await;
                    return Exit::Shutdown;
                }
                msg = source.next() => {
                    let Some(msg) = msg else { return Exit::Reconnect("connection closed") };
                    let Ok(msg) = msg else { return Exit::Reconnect("read error") };
                    self.meter(msg.len());
                    match msg {
                        Message::Text(text) => self.on_text(&mut sink, text.as_str()).await,
                        Message::Pong(_) => pong_deadline = None,
                        Message::Close(_) => return Exit::Reconnect("server closed the connection"),
                        Message::Ping(_) | Message::Binary(_) | Message::Frame(_) => {}
                    }
                }
                () = tokio::time::sleep_until(pong_wait), if pong_deadline.is_some() => {
                    return Exit::Reconnect("heartbeat timeout");
                }
                _ = ping.tick() => {
                    if pong_deadline.is_none() {
                        if sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                            return Exit::Reconnect("write failed");
                        }
                        pong_deadline = Some(Instant::now() + self.cfg.pong_timeout);
                    }
                }
                cmd = self.cmd_rx.recv() => {
                    if let Some(c) = cmd && self.handle_cmd(c, Some(&mut sink)).await.is_err() {
                        return Exit::Reconnect("write failed");
                    }
                }
                Some(rec) = self.rec_rx.recv() => self.on_recovery(rec),
            }
        }
    }

    fn meter(&self, bytes: usize) {
        let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
        let credits = self.gateway.budget().map_or(0, |b| {
            b.charge_stream_bytes(bytes, OffsetDateTime::now_utc())
        });
        self.gateway.engine().metrics.record_usage(
            ProviderId::Helius,
            op::WS_BYTES,
            0,
            credits,
            bytes,
        );
        self.shared.update(|s| {
            s.bytes_received = s.bytes_received.saturating_add(bytes);
            s.credits_charged = s.credits_charged.saturating_add(credits);
            s.last_message_at = Some(UtcTimestamp::now());
        });
    }

    fn next_request_id(&mut self) -> u64 {
        self.next_req = self.next_req.wrapping_add(1);
        self.next_req
    }

    async fn send_subscribe(&mut self, sink: &mut Sink, i: usize) -> Result<(), ()> {
        let id = self.next_request_id();
        let msg = self.subs[i].spec.subscribe_message(id);
        self.subs[i].pending = Some(id);
        self.subs[i].failed = false;
        self.gateway
            .engine()
            .metrics
            .record_usage(ProviderId::Helius, op::WS_SUBSCRIBE, 1, 0, 0);
        sink.send(Message::text(msg.to_string()))
            .await
            .map_err(|_| ())
    }

    /// Continuity bookkeeping when a subscription (re)starts on a connection.
    fn after_resubscribe(&mut self, i: usize, reconnected: bool) {
        let recoverable = matches!(self.subs[i].spec, SubscriptionSpec::Logs { .. })
            && self.subs[i].durable_key.is_some();
        if recoverable && (reconnected || self.subs[i].recover_on_connect) {
            self.subs[i].recover_on_connect = false;
            if self.subs[i].last_signature.is_some() {
                self.spawn_recovery(i);
                return;
            }
        }
        if reconnected {
            self.subs[i].continuity = Continuity::Unknown;
            let id = self.subs[i].id;
            self.emit(
                Some(id),
                StreamEventKind::ContinuityUnknown {
                    reason: "reconnected; events during the interruption cannot be recovered for this subscription",
                },
                None,
                Provenance::Direct,
            );
        }
    }

    fn spawn_recovery(&mut self, i: usize) {
        let sub = &self.subs[i];
        let SubscriptionSpec::Logs {
            mentions,
            commitment,
        } = &sub.spec
        else {
            return;
        };
        let Some(until) = sub
            .last_signature
            .as_deref()
            .and_then(|s| TransactionSignature::parse(s).ok())
        else {
            return;
        };
        let commitment = if *commitment == CommitmentLevel::Processed {
            CommitmentLevel::Confirmed
        } else {
            *commitment
        };
        let query = SignatureQuery {
            address: mentions.clone(),
            before: None,
            until: Some(until),
            page_limit: RECOVERY_PAGE_LIMIT,
            max_pages: RECOVERY_MAX_PAGES,
        };
        let id = sub.id;
        let from_slot = sub.last_slot;
        let gateway = self.gateway.clone();
        let tx = self.rec_tx.clone();
        tokio::spawn(async move {
            let ctx = RequestContext::new("stream.recovery", Priority::P2)
                .with_timeout(RECOVERY_TIMEOUT)
                .with_commitment(commitment);
            let result = gateway
                .get_signatures_for_address(&ctx, &query)
                .await
                .map(|r| r.value);
            let _ = tx
                .send(RecoveryOutcome {
                    sub: id,
                    from_slot,
                    result,
                })
                .await;
        });
    }

    fn on_recovery(&mut self, rec: RecoveryOutcome) {
        let Some(i) = self.subs.iter().position(|s| s.id == rec.sub) else {
            return;
        };
        let id = self.subs[i].id;
        let commitment = self.subs[i].spec.commitment();
        match rec.result {
            Ok(page) => {
                let mut recovered = 0_u64;
                let mut duplicates = 0_u64;
                for item in page.items.iter().rev() {
                    if !self.dedup.insert(item.signature.as_str()) {
                        duplicates += 1;
                        continue;
                    }
                    recovered += 1;
                    self.emit_with(
                        Some(id),
                        StreamEventKind::Recovered {
                            signature: item.signature.clone(),
                            slot: item.slot,
                            block_time: item.block_time,
                            failed: item.failed,
                        },
                        Some(item.slot),
                        commitment,
                        Provenance::Recovery,
                    );
                }
                if let Some(newest) = page.items.first() {
                    self.advance_checkpoint(i, newest.slot, newest.signature.as_str());
                }
                if page.complete {
                    self.subs[i].continuity = Continuity::Recovered;
                } else {
                    self.subs[i].continuity = Continuity::Gap;
                    let to_slot = page.items.last().map(|s| s.slot);
                    self.emit(
                        Some(id),
                        StreamEventKind::Gap {
                            reason: "recovery bound reached; older events may be missing",
                            from_slot: rec.from_slot,
                            to_slot,
                        },
                        None,
                        Provenance::Recovery,
                    );
                    self.shared.update(|s| s.gaps = s.gaps.saturating_add(1));
                }
                self.shared.update(|s| {
                    s.recovered = s.recovered.saturating_add(recovered);
                    s.duplicates_suppressed = s.duplicates_suppressed.saturating_add(duplicates);
                });
                self.mark_checkpoint(i);
            }
            Err(e) => {
                self.subs[i].continuity = Continuity::Gap;
                tracing::warn!(error = e.code(), "stream gap recovery failed");
                self.emit(
                    Some(id),
                    StreamEventKind::Gap {
                        reason: "recovery request failed",
                        from_slot: rec.from_slot,
                        to_slot: None,
                    },
                    None,
                    Provenance::Recovery,
                );
                self.shared.update(|s| s.gaps = s.gaps.saturating_add(1));
                self.mark_checkpoint(i);
            }
        }
        self.publish_subs();
    }

    async fn handle_cmd(&mut self, cmd: Command, sink: Option<&mut Sink>) -> Result<(), ()> {
        match cmd {
            Command::Subscribe {
                spec,
                durable_key,
                reply,
            } => {
                if self.subs.len() >= self.cfg.max_subscriptions {
                    let _ = reply.send(Err(GatewayError::InvalidRequest(
                        "subscription limit reached (HELIUS_WS_MAX_SUBSCRIPTIONS)",
                    )));
                    return Ok(());
                }
                if durable_key
                    .as_ref()
                    .is_some_and(|k| self.subs.iter().any(|s| s.durable_key.as_ref() == Some(k)))
                {
                    let _ = reply.send(Err(GatewayError::InvalidRequest(
                        "durable key already in use",
                    )));
                    return Ok(());
                }
                self.next_sub += 1;
                let id = SubscriptionId(self.next_sub);
                let stored = durable_key.as_ref().and_then(|k| self.stored.get(k));
                let sub = Sub {
                    id,
                    durable_key: durable_key.clone(),
                    server_id: None,
                    pending: None,
                    failed: false,
                    notifications: 0,
                    dropped: 0,
                    needs_marker: false,
                    continuity: if stored.is_some() {
                        Continuity::Unknown
                    } else {
                        Continuity::Continuous
                    },
                    last_slot: stored.and_then(|c| c.last_slot),
                    last_signature: stored.and_then(|c| c.last_signature.clone()),
                    recover_on_connect: stored.is_some(),
                    spec,
                };
                self.subs.push(sub);
                let _ = reply.send(Ok(id));
                let i = self.subs.len() - 1;
                if let Some(sink) = sink {
                    self.send_subscribe(sink, i).await?;
                    self.after_resubscribe(i, false);
                    self.refresh_state();
                }
                self.publish_subs();
                Ok(())
            }
            Command::Unsubscribe { id, reply } => {
                let Some(i) = self.subs.iter().position(|s| s.id == id) else {
                    let _ = reply.send(Err(GatewayError::InvalidRequest("unknown subscription")));
                    return Ok(());
                };
                let sub = self.subs.remove(i);
                let _ = reply.send(Ok(()));
                let mut result = Ok(());
                if let (Some(sink), Some(server_id)) = (sink, sub.server_id) {
                    let req = self.next_request_id();
                    let msg = sub.spec.unsubscribe_message(req, server_id);
                    result = sink
                        .send(Message::text(msg.to_string()))
                        .await
                        .map_err(|_| ());
                }
                self.refresh_state();
                self.publish_subs();
                result
            }
        }
    }

    async fn on_text(&mut self, _sink: &mut Sink, text: &str) {
        match parse_frame(text) {
            Incoming::Reply { id, result } => {
                if let Some(s) = self.subs.iter_mut().find(|s| s.pending == Some(id)) {
                    s.pending = None;
                    match result.as_u64() {
                        Some(server_id) => s.server_id = Some(server_id),
                        None => s.failed = true,
                    }
                    self.refresh_state();
                    self.publish_subs();
                }
            }
            Incoming::Error { id, message, .. } => {
                if let Some(i) =
                    id.and_then(|id| self.subs.iter().position(|s| s.pending == Some(id)))
                {
                    self.subs[i].pending = None;
                    self.subs[i].failed = true;
                    let sid = self.subs[i].id;
                    self.emit(
                        Some(sid),
                        StreamEventKind::SubscriptionFailed { message },
                        None,
                        Provenance::Direct,
                    );
                    self.refresh_state();
                    self.publish_subs();
                }
            }
            Incoming::Notification {
                method,
                subscription,
                result,
            } => self.on_notification(&method, subscription, &result),
            Incoming::Unknown => {}
        }
    }

    fn on_notification(&mut self, method: &str, server_id: u64, result: &Value) {
        let Some(i) = self
            .subs
            .iter()
            .position(|s| s.server_id == Some(server_id))
        else {
            return;
        };
        self.subs[i].notifications = self.subs[i].notifications.saturating_add(1);
        let id = self.subs[i].id;
        let ctx_slot = result
            .get("context")
            .and_then(|c| c.get("slot"))
            .and_then(Value::as_u64);
        let value = result.get("value");
        let p = ProviderId::Helius;
        let kind = match method {
            "slotNotification" => {
                result
                    .get("slot")
                    .and_then(Value::as_u64)
                    .map(|slot| StreamEventKind::Slot {
                        slot,
                        parent: result.get("parent").and_then(Value::as_u64),
                        root: result.get("root").and_then(Value::as_u64),
                    })
            }
            "rootNotification" => result.as_u64().map(|slot| StreamEventKind::Root { slot }),
            "logsNotification" => {
                let v = value.unwrap_or(&Value::Null);
                match v
                    .get("signature")
                    .and_then(Value::as_str)
                    .and_then(|s| TransactionSignature::parse(s).ok())
                {
                    Some(signature) => {
                        if !self.dedup.insert(signature.as_str()) {
                            self.shared.update(|s| {
                                s.duplicates_suppressed = s.duplicates_suppressed.saturating_add(1)
                            });
                            return;
                        }
                        if let Some(slot) = ctx_slot {
                            self.advance_checkpoint(i, slot, signature.as_str());
                            self.mark_checkpoint(i);
                        }
                        let logs = v
                            .get("logs")
                            .and_then(Value::as_array)
                            .map(|a| {
                                a.iter()
                                    .take(MAX_LOG_LINES)
                                    .filter_map(Value::as_str)
                                    .map(|l| l.chars().take(MAX_LOG_LINE).collect())
                                    .collect()
                            })
                            .unwrap_or_default();
                        Some(StreamEventKind::Logs {
                            signature,
                            failed: v.get("err").is_some_and(|e| !e.is_null()),
                            logs,
                        })
                    }
                    None => None,
                }
            }
            "accountNotification" => value
                .and_then(|v| parse_account(p, v).ok())
                .map(StreamEventKind::Account),
            "programNotification" => value.and_then(|v| {
                let pubkey = v
                    .get("pubkey")
                    .and_then(Value::as_str)
                    .and_then(|s| WalletAddress::parse(s).ok())?;
                let account = parse_account(p, v.get("account")?).ok()?;
                Some(StreamEventKind::Program { pubkey, account })
            }),
            "signatureNotification" => {
                let v = value.unwrap_or(&Value::Null);
                if v.as_str() == Some("receivedSignature") {
                    None
                } else {
                    let err = v.get("err").filter(|e| !e.is_null());
                    Some(StreamEventKind::Signature {
                        failed: err.is_some(),
                        err_json: err.map(|e| e.to_string().chars().take(512).collect()),
                    })
                }
            }
            _ => None,
        };
        let event_slot = match &kind {
            Some(StreamEventKind::Slot { slot, .. } | StreamEventKind::Root { slot }) => {
                Some(*slot)
            }
            _ => None,
        };
        if let Some(slot) = ctx_slot.or(event_slot) {
            self.subs[i].last_slot = Some(self.subs[i].last_slot.map_or(slot, |s| s.max(slot)));
        }
        let terminal = matches!(kind, Some(StreamEventKind::Signature { .. }));
        if let Some(kind) = kind {
            let commitment = self.subs[i].spec.commitment();
            self.emit_with(Some(id), kind, ctx_slot, commitment, Provenance::Direct);
        }
        if terminal {
            // The server ended this subscription after its single notification.
            self.subs.remove(i);
            self.refresh_state();
            self.publish_subs();
        }
    }

    fn advance_checkpoint(&mut self, i: usize, slot: u64, signature: &str) {
        let s = &mut self.subs[i];
        if s.durable_key.is_none() {
            return;
        }
        if s.last_slot.is_none_or(|old| slot >= old) {
            s.last_slot = Some(slot);
            s.last_signature = Some(signature.to_owned());
        }
    }

    fn mark_checkpoint(&mut self, i: usize) {
        let s = &self.subs[i];
        let Some(key) = &s.durable_key else { return };
        let commitment = s.spec.commitment().unwrap_or(CommitmentLevel::Confirmed);
        let row = StreamCheckpointRow {
            stream_key: key.clone(),
            kind: s.spec.kind().to_owned(),
            commitment: commitment_str(commitment).to_owned(),
            last_slot: s.last_slot,
            last_signature: s.last_signature.clone(),
            continuity: s.continuity.as_str().to_owned(),
            updated_at: UtcTimestamp::now(),
        };
        self.stored.insert(key.clone(), row.clone());
        self.shared
            .dirty_checkpoints
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.clone(), row);
    }

    fn emit(
        &mut self,
        sub: Option<SubscriptionId>,
        kind: StreamEventKind,
        slot: Option<u64>,
        provenance: Provenance,
    ) {
        let commitment = sub
            .and_then(|id| self.subs.iter().find(|s| s.id == id))
            .and_then(|s| s.spec.commitment());
        self.emit_with(sub, kind, slot, commitment, provenance);
    }

    /// Send an event without ever blocking the connection. When the channel
    /// is full the event is dropped, counted, and the subscription is marked
    /// so that its next delivered event is preceded by `CONTINUITY_UNKNOWN`.
    fn emit_with(
        &mut self,
        sub: Option<SubscriptionId>,
        kind: StreamEventKind,
        slot: Option<u64>,
        commitment: Option<CommitmentLevel>,
        provenance: Provenance,
    ) {
        let idx = sub.and_then(|id| self.subs.iter().position(|s| s.id == id));
        let make = |kind| StreamEvent {
            subscription: sub,
            generation: self.generation,
            received_at: UtcTimestamp::now(),
            commitment,
            context_slot: slot,
            provenance,
            kind,
        };
        if let Some(i) = idx
            && self.subs[i].needs_marker
        {
            let marker = make(StreamEventKind::ContinuityUnknown {
                reason: "events were dropped because the consumer fell behind",
            });
            if self.events.try_send(marker).is_ok() {
                self.subs[i].needs_marker = false;
                self.subs[i].continuity = Continuity::Unknown;
            }
        }
        let event = make(kind);
        match self.events.try_send(event) {
            Ok(()) => self
                .shared
                .update(|s| s.events_emitted = s.events_emitted.saturating_add(1)),
            Err(_) => {
                if let Some(i) = idx {
                    self.subs[i].dropped = self.subs[i].dropped.saturating_add(1);
                    self.subs[i].needs_marker = true;
                }
                self.shared
                    .update(|s| s.events_dropped = s.events_dropped.saturating_add(1));
            }
        }
    }

    fn set_state(&mut self, state: ConnectionState) {
        let mut changed = false;
        self.shared.update(|s| {
            changed = s.state != state;
            s.state = state;
        });
        if changed {
            tracing::info!(
                state = state.as_str(),
                generation = self.generation,
                "stream state"
            );
            self.emit_with(
                None,
                StreamEventKind::ConnectionState(state),
                None,
                None,
                Provenance::Direct,
            );
        }
    }

    /// CONNECTED once every subscription is acknowledged; DEGRADED if any failed.
    fn refresh_state(&mut self) {
        let current = self
            .shared
            .snapshot
            .lock()
            .map_or(ConnectionState::Disconnected, |s| s.state);
        if !matches!(
            current,
            ConnectionState::Subscribing | ConnectionState::Connected | ConnectionState::Degraded
        ) {
            return;
        }
        let next = if self.subs.iter().any(|s| s.failed) {
            ConnectionState::Degraded
        } else if self.subs.iter().any(|s| s.pending.is_some()) {
            ConnectionState::Subscribing
        } else {
            ConnectionState::Connected
        };
        if next == ConnectionState::Connected {
            self.gateway
                .engine()
                .runtime(ProviderId::Helius)
                .health
                .success();
        }
        self.set_state(next);
    }

    fn publish_subs(&self) {
        let subs = self
            .subs
            .iter()
            .map(|s| SubscriptionSnapshot {
                id: s.id,
                kind: s.spec.kind(),
                commitment: s.spec.commitment(),
                durable_key: s.durable_key.clone(),
                active: s.server_id.is_some(),
                notifications: s.notifications,
                dropped: s.dropped,
                continuity: s.continuity,
                last_slot: s.last_slot,
            })
            .collect();
        self.shared.update(|s| s.subscriptions = subs);
    }
}
