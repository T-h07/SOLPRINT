//! Helius standard WebSocket subscriptions with recovery.
//!
//! See `docs/architecture/STREAM_RECOVERY.md` for the state machine, the
//! continuity guarantees and what is *not* guaranteed.

mod manager;
pub mod protocol;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use solprint_config::Secret;
use solprint_core::{CommitmentLevel, TransactionSignature, UtcTimestamp, WalletAddress};
use solprint_storage::StreamCheckpointRow;
use tokio::sync::{mpsc, oneshot};
use url::Url;

pub use manager::StreamManager;
pub use protocol::{ProgramFilter, SubscriptionSpec};

use crate::context::Provenance;
use crate::error::GatewayError;
use crate::models::AccountData;
use crate::settings::GatewaySettings;

/// Bounded event channel (documented capacity).
pub const EVENT_CHANNEL_CAPACITY: usize = 1_024;
pub const PING_INTERVAL: Duration = Duration::from_secs(30);
pub const PONG_TIMEOUT: Duration = Duration::from_secs(10);
pub const BACKOFF_MIN: Duration = Duration::from_secs(1);
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Recovery bound: pages × page size of `getSignaturesForAddress`.
pub const RECOVERY_PAGE_LIMIT: u16 = 100;
pub const RECOVERY_MAX_PAGES: u8 = 3;
/// Signatures remembered for de-duplication.
pub const DEDUP_CAPACITY: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Subscribing,
    Connected,
    /// Connected, but at least one subscription failed.
    Degraded,
    Reconnecting,
    /// Streams are turned off or no credential is configured.
    Disabled,
    /// The credit budget no longer admits streaming (P2).
    BudgetExhausted,
    ShuttingDown,
}

impl ConnectionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disconnected => "DISCONNECTED",
            Self::Connecting => "CONNECTING",
            Self::Subscribing => "SUBSCRIBING",
            Self::Connected => "CONNECTED",
            Self::Degraded => "DEGRADED",
            Self::Reconnecting => "RECONNECTING",
            Self::Disabled => "DISABLED",
            Self::BudgetExhausted => "BUDGET_EXHAUSTED",
            Self::ShuttingDown => "SHUTTING_DOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Continuity {
    Continuous,
    Recovered,
    Gap,
    Unknown,
}

impl Continuity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Continuous => "CONTINUOUS",
            Self::Recovered => "RECOVERED",
            Self::Gap => "GAP",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEventKind {
    Slot {
        slot: u64,
        parent: Option<u64>,
        root: Option<u64>,
    },
    Root {
        slot: u64,
    },
    Logs {
        signature: TransactionSignature,
        failed: bool,
        /// At most 64 lines of at most 512 characters each.
        logs: Vec<String>,
    },
    Account(AccountData),
    /// Terminal: the server ends a signature subscription after this.
    Signature {
        failed: bool,
        err_json: Option<String>,
    },
    Program {
        pubkey: WalletAddress,
        account: AccountData,
    },
    /// A signature found by gap recovery that the live stream did not deliver.
    Recovered {
        signature: TransactionSignature,
        slot: u64,
        block_time: Option<UtcTimestamp>,
        failed: bool,
    },
    /// Events may be missing between these slots (if known).
    Gap {
        reason: &'static str,
        from_slot: Option<u64>,
        to_slot: Option<u64>,
    },
    /// Continuity cannot be established (e.g. account streams after a
    /// reconnect, or events dropped under backpressure).
    ContinuityUnknown {
        reason: &'static str,
    },
    SubscriptionFailed {
        message: String,
    },
    ConnectionState(ConnectionState),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEvent {
    /// `None` for connection-wide events.
    pub subscription: Option<SubscriptionId>,
    /// Connection generation the event was received on (0 = none yet).
    pub generation: u64,
    pub received_at: UtcTimestamp,
    pub commitment: Option<CommitmentLevel>,
    pub context_slot: Option<u64>,
    pub provenance: Provenance,
    pub kind: StreamEventKind,
}

#[derive(Debug, Clone)]
pub struct SubscriptionSnapshot {
    pub id: SubscriptionId,
    pub kind: &'static str,
    pub commitment: Option<CommitmentLevel>,
    pub durable_key: Option<String>,
    pub active: bool,
    pub notifications: u64,
    pub dropped: u64,
    pub continuity: Continuity,
    pub last_slot: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct StreamSnapshot {
    pub state: ConnectionState,
    pub generation: u64,
    pub connected_since: Option<UtcTimestamp>,
    pub last_message_at: Option<UtcTimestamp>,
    pub reconnects: u64,
    pub bytes_received: u64,
    pub credits_charged: u64,
    pub events_emitted: u64,
    pub events_dropped: u64,
    pub gaps: u64,
    pub recovered: u64,
    pub duplicates_suppressed: u64,
    pub last_error: Option<&'static str>,
    pub max_subscriptions: usize,
    pub subscriptions: Vec<SubscriptionSnapshot>,
}

#[derive(Debug)]
pub(crate) struct Shared {
    pub snapshot: Mutex<StreamSnapshot>,
    pub events: Mutex<Option<mpsc::Receiver<StreamEvent>>>,
    pub dirty_checkpoints: Mutex<HashMap<String, StreamCheckpointRow>>,
}

impl Shared {
    pub fn update(&self, f: impl FnOnce(&mut StreamSnapshot)) {
        let mut s = self
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut s);
    }
}

#[derive(Debug)]
pub(crate) enum Command {
    Subscribe {
        spec: SubscriptionSpec,
        durable_key: Option<String>,
        reply: oneshot::Sender<Result<SubscriptionId, GatewayError>>,
    },
    Unsubscribe {
        id: SubscriptionId,
        reply: oneshot::Sender<Result<(), GatewayError>>,
    },
}

/// Settings for the stream manager.
#[derive(Clone)]
pub struct StreamConfig {
    pub url: Secret<Url>,
    pub max_subscriptions: usize,
    pub allow_insecure_loopback: bool,
    pub ping_interval: Duration,
    pub pong_timeout: Duration,
    pub max_message_bytes: usize,
}

impl std::fmt::Debug for StreamConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamConfig")
            .field("max_subscriptions", &self.max_subscriptions)
            .finish_non_exhaustive()
    }
}

impl StreamConfig {
    #[must_use]
    pub fn from_settings(s: &GatewaySettings, key: &Secret<String>) -> Self {
        let mut url = s.helius.ws_url.clone();
        url.query_pairs_mut()
            .clear()
            .append_pair("api-key", key.expose());
        Self {
            url: Secret::new(url),
            max_subscriptions: s.helius.ws_max_subscriptions,
            allow_insecure_loopback: s.allow_insecure_loopback,
            ping_interval: s.stream_ping_interval,
            pong_timeout: s.stream_pong_timeout,
            max_message_bytes: s.max_response_bytes.min(16 * 1024 * 1024),
        }
    }
}

/// Handle to the running stream manager.
#[derive(Debug, Clone)]
pub struct StreamHandle {
    pub(crate) cmd: mpsc::Sender<Command>,
    pub(crate) shared: Arc<Shared>,
}

impl StreamHandle {
    /// Add a subscription. A `durable_key` (`[A-Za-z0-9_.:-]{1,128}`) makes
    /// it checkpointed and recoverable across reconnects and restarts
    /// (logs subscriptions only; others report continuity `UNKNOWN`).
    pub async fn subscribe(
        &self,
        spec: SubscriptionSpec,
        durable_key: Option<String>,
    ) -> Result<SubscriptionId, GatewayError> {
        spec.validate().map_err(GatewayError::InvalidRequest)?;
        if let Some(k) = &durable_key
            && (k.is_empty()
                || k.len() > 128
                || !k
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-')))
        {
            return Err(GatewayError::InvalidRequest("invalid durable key"));
        }
        let (reply, rx) = oneshot::channel();
        self.cmd
            .send(Command::Subscribe {
                spec,
                durable_key,
                reply,
            })
            .await
            .map_err(|_| GatewayError::Stream("stream manager stopped"))?;
        rx.await
            .map_err(|_| GatewayError::Stream("stream manager stopped"))?
    }

    pub async fn unsubscribe(&self, id: SubscriptionId) -> Result<(), GatewayError> {
        let (reply, rx) = oneshot::channel();
        self.cmd
            .send(Command::Unsubscribe { id, reply })
            .await
            .map_err(|_| GatewayError::Stream("stream manager stopped"))?;
        rx.await
            .map_err(|_| GatewayError::Stream("stream manager stopped"))?
    }

    /// Take the event receiver (once). Until taken, events beyond the
    /// channel capacity are dropped and counted.
    #[must_use]
    pub fn take_events(&self) -> Option<mpsc::Receiver<StreamEvent>> {
        self.shared
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    #[must_use]
    pub fn snapshot(&self) -> StreamSnapshot {
        self.shared
            .snapshot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Checkpoints changed since the last call (for persistence).
    #[must_use]
    pub fn take_checkpoints(&self) -> Vec<StreamCheckpointRow> {
        let mut map = self
            .shared
            .dirty_checkpoints
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.drain().map(|(_, v)| v).collect()
    }

    /// Put back a checkpoint whose write failed (unless a newer one exists).
    pub fn restore_checkpoint(&self, cp: StreamCheckpointRow) {
        let mut map = self
            .shared
            .dirty_checkpoints
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.entry(cp.stream_key.clone()).or_insert(cp);
    }
}
