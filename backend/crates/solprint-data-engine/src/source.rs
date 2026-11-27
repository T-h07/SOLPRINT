//! The chain-data boundary of the Data Engine. Production uses
//! [`GatewaySource`]: every request goes through GATEWAY (budget admission,
//! rate limits, priorities, provenance). Tests use an in-memory source, so
//! automated tests never consume provider credits.

use std::future::Future;
use std::time::Duration;

use solprint_core::{CommitmentLevel, TransactionSignature, UtcTimestamp, WalletAddress};
use solprint_gateway::models::{SignaturePage, SignatureQuery, SignatureStatus};
use solprint_gateway::{BlockchainProvider, Gateway, GatewayError, Priority, RequestContext};
use tokio_util::sync::CancellationToken;

/// Origin label for GATEWAY metrics and logs.
pub const ORIGIN: &str = "data_engine";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedTx {
    pub raw_json: String,
    pub slot: u64,
    /// `HELIUS` | `SOLANA_RPC` | `FIXTURE` (evidence provider label).
    pub provider: &'static str,
    pub fetched_at: UtcTimestamp,
    pub credits: u64,
    /// `true` when GATEWAY served the value from its cache.
    pub cached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    /// Budget admission denied this priority: pause, do not consume attempts.
    #[error("budget denied")]
    Budget,
    /// Transient provider condition; retry after the hint.
    #[error("provider unavailable ({code})")]
    Transient {
        code: &'static str,
        retry_after: Option<Duration>,
    },
    /// Configuration prevents the request (disabled, no credential).
    #[error("source disabled ({code})")]
    Disabled { code: &'static str },
    #[error("unsupported transaction version")]
    UnsupportedVersion,
    #[error("cancelled")]
    Cancelled,
    /// Permanent request/decode failure.
    #[error("request failed ({code})")]
    Permanent { code: &'static str },
}

impl SourceError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Budget => "BUDGET_DENIED",
            Self::Transient { code, .. } | Self::Disabled { code } | Self::Permanent { code } => {
                code
            }
            Self::UnsupportedVersion => "UNSUPPORTED_TX_VERSION",
            Self::Cancelled => "CANCELLED",
        }
    }
}

impl From<GatewayError> for SourceError {
    fn from(e: GatewayError) -> Self {
        match e {
            GatewayError::BudgetDenied { .. } => Self::Budget,
            GatewayError::Cancelled => Self::Cancelled,
            GatewayError::UnsupportedTransactionVersion => Self::UnsupportedVersion,
            GatewayError::Disabled { .. }
            | GatewayError::MissingCredential(_)
            | GatewayError::HostNotAllowed => Self::Disabled { code: e.code() },
            GatewayError::RateLimited { retry_after, .. } => Self::Transient {
                code: e.code(),
                retry_after,
            },
            GatewayError::CircuitOpen { retry_in, .. } => Self::Transient {
                code: e.code(),
                retry_after: Some(retry_in),
            },
            GatewayError::QueueFull { .. }
            | GatewayError::DeadlineExceeded
            | GatewayError::NoRoute
            | GatewayError::Unavailable => Self::Transient {
                code: e.code(),
                retry_after: None,
            },
            ref other if other.is_retryable() || other.is_provider_failure() => Self::Transient {
                code: other.code(),
                retry_after: None,
            },
            other => Self::Permanent { code: other.code() },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusesResult {
    pub statuses: Vec<Option<SignatureStatus>>,
    pub credits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageResult {
    pub page: SignaturePage,
    pub credits: u64,
}

/// Read-only chain access needed by the Data Engine.
pub trait ChainSource: Send + Sync + 'static {
    fn transaction(
        &self,
        signature: &TransactionSignature,
        commitment: CommitmentLevel,
        priority: Priority,
        refresh: bool,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<Option<FetchedTx>, SourceError>> + Send;

    fn signature_statuses(
        &self,
        signatures: &[TransactionSignature],
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<StatusesResult, SourceError>> + Send;

    fn signatures_for_address(
        &self,
        query: &SignatureQuery,
        commitment: CommitmentLevel,
        cancel: &CancellationToken,
    ) -> impl Future<Output = Result<PageResult, SourceError>> + Send;

    /// Whether requests of `priority` are currently admissible, from local
    /// budget state only. Never triggers provider traffic.
    fn admits(&self, priority: Priority) -> bool;
}

/// Production source: GATEWAY.
#[derive(Debug, Clone)]
pub struct GatewaySource {
    gateway: Gateway,
}

impl GatewaySource {
    #[must_use]
    pub const fn new(gateway: Gateway) -> Self {
        Self { gateway }
    }

    fn ctx(
        priority: Priority,
        commitment: CommitmentLevel,
        cancel: &CancellationToken,
    ) -> RequestContext {
        RequestContext::new(ORIGIN, priority)
            .with_commitment(commitment)
            .with_timeout(REQUEST_TIMEOUT)
            .with_cancel(cancel.clone())
    }
}

fn provider_label(p: solprint_gateway::ProviderId) -> &'static str {
    match p {
        solprint_gateway::ProviderId::Helius => "HELIUS",
        _ => "SOLANA_RPC",
    }
}

impl ChainSource for GatewaySource {
    async fn transaction(
        &self,
        signature: &TransactionSignature,
        commitment: CommitmentLevel,
        priority: Priority,
        refresh: bool,
        cancel: &CancellationToken,
    ) -> Result<Option<FetchedTx>, SourceError> {
        let mut ctx = Self::ctx(priority, commitment, cancel);
        if refresh {
            ctx = ctx.with_cache(solprint_gateway::CachePolicy::Refresh);
        }
        let r = self.gateway.transaction(&ctx, signature).await?;
        let cached = !matches!(
            r.provenance,
            solprint_gateway::Provenance::Direct | solprint_gateway::Provenance::Fallback
        );
        let provider = provider_label(r.provider);
        let (observed_at, credits) = (r.observed_at, r.credits_charged);
        Ok(r.value.map(|t| FetchedTx {
            raw_json: t.raw_json,
            slot: t.slot,
            provider,
            fetched_at: observed_at,
            credits,
            cached,
        }))
    }

    async fn signature_statuses(
        &self,
        signatures: &[TransactionSignature],
        cancel: &CancellationToken,
    ) -> Result<StatusesResult, SourceError> {
        let ctx = Self::ctx(Priority::P4, CommitmentLevel::Confirmed, cancel);
        // Recent signatures are in the status cache; history search is not
        // needed for finality tracking of recently ingested transactions.
        let r = self
            .gateway
            .signature_statuses(&ctx, signatures, false)
            .await?;
        Ok(StatusesResult {
            statuses: r.value,
            credits: r.credits_charged,
        })
    }

    async fn signatures_for_address(
        &self,
        query: &SignatureQuery,
        commitment: CommitmentLevel,
        cancel: &CancellationToken,
    ) -> Result<PageResult, SourceError> {
        let ctx = Self::ctx(Priority::P5, commitment, cancel);
        let r = self.gateway.signatures(&ctx, query).await?;
        Ok(PageResult {
            page: r.value,
            credits: r.credits_charged,
        })
    }

    fn admits(&self, priority: Priority) -> bool {
        let snap = self.gateway.snapshot();
        snap.budget.as_ref().is_none_or(|b| {
            b.admits_up_to
                .is_some_and(|max| priority.rank() <= max.rank())
        })
    }
}

/// Parse a base58 address for an RPC query.
pub fn wallet(address: &str) -> Result<WalletAddress, SourceError> {
    WalletAddress::parse(address).map_err(|_| SourceError::Permanent {
        code: "INVALID_ADDRESS",
    })
}
