//! Request context, priorities, providers and the normalized response envelope.

use std::fmt;
use std::time::Duration;

use solprint_core::{CommitmentLevel, CorrelationId, UtcTimestamp};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

/// External providers GATEWAY can reach. The set is closed: adding a
/// provider is a reviewed code change, never configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProviderId {
    Helius,
    Jupiter,
    DexScreener,
    SolanaRpc,
}

impl ProviderId {
    pub const ALL: [Self; 4] = [
        Self::Helius,
        Self::Jupiter,
        Self::DexScreener,
        Self::SolanaRpc,
    ];

    /// Stable lowercase label used in storage, metrics and logs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Helius => "helius",
            Self::Jupiter => "jupiter",
            Self::DexScreener => "dexscreener",
            Self::SolanaRpc => "solana_rpc",
        }
    }

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Helius => 0,
            Self::Jupiter => 1,
            Self::DexScreener => 2,
            Self::SolanaRpc => 3,
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Scheduling priority. `P0` is the most important. Budget states shed the
/// least important priorities first (see `docs/architecture/API_BUDGET_POLICY.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Priority {
    /// Safety-critical: protecting an open position or confirming state that
    /// a safety decision depends on.
    P0,
    /// Position and exposure monitoring.
    P1,
    /// Stream continuity and gap recovery.
    P2,
    /// Interactive, owner-initiated requests.
    P3,
    /// Background enrichment.
    P4,
    /// Discovery, diagnostics and anything else that can wait.
    P5,
}

impl Priority {
    pub const ALL: [Self; 6] = [Self::P0, Self::P1, Self::P2, Self::P3, Self::P4, Self::P5];

    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::P0 => 0,
            Self::P1 => 1,
            Self::P2 => 2,
            Self::P3 => 3,
            Self::P4 => 4,
            Self::P5 => 5,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::P0 => "P0",
            Self::P1 => "P1",
            Self::P2 => "P2",
            Self::P3 => "P3",
            Self::P4 => "P4",
            Self::P5 => "P5",
        }
    }
}

/// How the cache participates in one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Serve a cached value that satisfies `max_data_age`; otherwise fetch.
    PreferCache,
    /// Always fetch; store the result.
    Refresh,
    /// Never contact a provider. A miss is `GatewayError::Unavailable`.
    CacheOnly,
    /// Like `PreferCache`, but if the fetch fails, a stale (not expired)
    /// value may be served, labelled `STALE_CACHE`.
    AllowStaleOnError,
}

/// Everything GATEWAY needs to decide how (and whether) to serve a request.
#[derive(Debug, Clone)]
pub struct RequestContext {
    pub correlation_id: CorrelationId,
    /// Bounded label of the calling module (`diagnostics`, `api`, `stream`…).
    pub origin: &'static str,
    pub priority: Priority,
    pub deadline: Instant,
    /// Oldest acceptable data. `None` uses the operation's default freshness.
    pub max_data_age: Option<Duration>,
    pub cache: CachePolicy,
    pub commitment: CommitmentLevel,
    /// Whether the public Solana RPC fallback may answer this request.
    pub allow_fallback: bool,
    pub cancel: CancellationToken,
}

impl RequestContext {
    /// A context with conservative defaults: `confirmed` commitment, cache
    /// preferred, fallback disallowed, 10 s deadline.
    #[must_use]
    pub fn new(origin: &'static str, priority: Priority) -> Self {
        Self {
            correlation_id: CorrelationId::new(),
            origin,
            priority,
            deadline: Instant::now() + Duration::from_secs(10),
            max_data_age: None,
            cache: CachePolicy::PreferCache,
            commitment: CommitmentLevel::Confirmed,
            allow_fallback: false,
            cancel: CancellationToken::new(),
        }
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.deadline = Instant::now() + timeout;
        self
    }

    #[must_use]
    pub const fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = deadline;
        self
    }

    #[must_use]
    pub const fn with_max_age(mut self, age: Duration) -> Self {
        self.max_data_age = Some(age);
        self
    }

    #[must_use]
    pub const fn with_cache(mut self, policy: CachePolicy) -> Self {
        self.cache = policy;
        self
    }

    #[must_use]
    pub const fn with_commitment(mut self, commitment: CommitmentLevel) -> Self {
        self.commitment = commitment;
        self
    }

    #[must_use]
    pub const fn allow_fallback(mut self, allow: bool) -> Self {
        self.allow_fallback = allow;
        self
    }

    #[must_use]
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Time left before the deadline (zero once passed).
    #[must_use]
    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// Where a returned value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provenance {
    /// Fetched from the primary provider for this request.
    Direct,
    /// Served from cache within the requested freshness.
    FreshCache,
    /// Served from cache beyond its freshness window (only on request).
    StaleCache,
    /// Answered by the public Solana RPC fallback.
    Fallback,
    /// Reconstructed by gap recovery after a stream interruption.
    Recovery,
    /// Synthetic test or demo data. Never produced by a live code path.
    Fixture,
}

impl Provenance {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "DIRECT",
            Self::FreshCache => "FRESH_CACHE",
            Self::StaleCache => "STALE_CACHE",
            Self::Fallback => "FALLBACK",
            Self::Recovery => "RECOVERY",
            Self::Fixture => "FIXTURE",
        }
    }
}

/// Freshness of a returned value at the moment it is handed to the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Freshness {
    Fresh,
    Stale,
    Expired,
    Unavailable,
    /// The provider gives no source timestamp; only the observation time is
    /// known (DEX Screener, Jupiter tokens).
    UnknownSourceAge,
}

impl Freshness {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fresh => "FRESH",
            Self::Stale => "STALE",
            Self::Expired => "EXPIRED",
            Self::Unavailable => "UNAVAILABLE",
            Self::UnknownSourceAge => "UNKNOWN_SOURCE_AGE",
        }
    }
}

/// Normalized envelope around every value GATEWAY returns.
#[derive(Debug, Clone)]
pub struct GatewayResponse<T> {
    pub value: T,
    pub provider: ProviderId,
    pub provenance: Provenance,
    pub freshness: Freshness,
    /// When SOLPRINT received the data from the provider.
    pub observed_at: UtcTimestamp,
    /// The provider's own timestamp for the data, when it supplies one.
    pub source_timestamp: Option<UtcTimestamp>,
    /// Slot the provider evaluated the request at, for RPC context responses.
    pub context_slot: Option<u64>,
    /// Age of the data when returned (monotonic since observation).
    pub age: Duration,
    pub correlation_id: CorrelationId,
    /// Credits charged to the provider budget for this call (0 for cache hits).
    pub credits_charged: u64,
}

impl<T> GatewayResponse<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> GatewayResponse<U> {
        GatewayResponse {
            value: f(self.value),
            provider: self.provider,
            provenance: self.provenance,
            freshness: self.freshness,
            observed_at: self.observed_at,
            source_timestamp: self.source_timestamp,
            context_slot: self.context_slot,
            age: self.age,
            correlation_id: self.correlation_id,
            credits_charged: self.credits_charged,
        }
    }
}

impl<T> GatewayResponse<Option<T>> {
    /// Turn an optional value into a required one, failing with `err` when
    /// the provider answered "not found" for an operation where that is not
    /// a legitimate answer.
    pub fn require<E>(self, err: impl FnOnce(ProviderId) -> E) -> Result<GatewayResponse<T>, E> {
        let provider = self.provider;
        match self.value {
            Some(value) => Ok(GatewayResponse {
                value,
                provider: self.provider,
                provenance: self.provenance,
                freshness: self.freshness,
                observed_at: self.observed_at,
                source_timestamp: self.source_timestamp,
                context_slot: self.context_slot,
                age: self.age,
                correlation_id: self.correlation_id,
                credits_charged: self.credits_charged,
            }),
            None => Err(err(provider)),
        }
    }
}
