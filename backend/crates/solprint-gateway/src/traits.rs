//! Capability traits. Consumers depend on the capability they need, not on
//! a provider; [`Gateway`] implements every capability by routing to the
//! configured provider (and, where allowed, the public RPC fallback).
//!
//! No trait here can build, sign or submit a transaction.

use std::future::Future;

use solprint_core::{
    Lamports, TokenAmount, TokenMint, TransactionSignature, UtcTimestamp, WalletAddress,
};

use crate::context::{GatewayResponse, RequestContext};
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::models::{
    AccountData, BlockSummary, DexBoost, DexPair, DexProfile, JupiterPrice, LargestTokenAccount, NodeHealth, NodeVersion,
    QuoteRequest, SignaturePage, SignatureQuery, SignatureStatus, SwapQuote, TokenAccount,
    TokenAccountFilter, TokenCategory, TokenInfo, TokenInterval, TransactionRecord,
};
use crate::stream::{StreamHandle, StreamSnapshot, SubscriptionId, SubscriptionSpec};

type R<T> = Result<GatewayResponse<T>, GatewayError>;

/// Read-only chain state over JSON-RPC.
pub trait BlockchainProvider: Send + Sync {
    fn health(&self, ctx: &RequestContext) -> impl Future<Output = R<NodeHealth>> + Send;
    fn version(&self, ctx: &RequestContext) -> impl Future<Output = R<NodeVersion>> + Send;
    fn slot(&self, ctx: &RequestContext) -> impl Future<Output = R<u64>> + Send;
    fn block_time(
        &self,
        ctx: &RequestContext,
        slot: u64,
    ) -> impl Future<Output = R<Option<UtcTimestamp>>> + Send;
    fn balance(
        &self,
        ctx: &RequestContext,
        address: &WalletAddress,
    ) -> impl Future<Output = R<Lamports>> + Send;
    fn account(
        &self,
        ctx: &RequestContext,
        address: &WalletAddress,
    ) -> impl Future<Output = R<Option<AccountData>>> + Send;
    fn accounts(
        &self,
        ctx: &RequestContext,
        addresses: &[WalletAddress],
    ) -> impl Future<Output = R<Vec<Option<AccountData>>>> + Send;
    fn signatures(
        &self,
        ctx: &RequestContext,
        query: &SignatureQuery,
    ) -> impl Future<Output = R<SignaturePage>> + Send;
    fn transaction(
        &self,
        ctx: &RequestContext,
        signature: &TransactionSignature,
    ) -> impl Future<Output = R<Option<TransactionRecord>>> + Send;
    /// `getSignatureStatuses`; `None` items are unknown, not failed.
    fn signature_statuses(
        &self,
        ctx: &RequestContext,
        signatures: &[TransactionSignature],
        search_transaction_history: bool,
    ) -> impl Future<Output = R<Vec<Option<SignatureStatus>>>> + Send;
    fn token_account_balance(
        &self,
        ctx: &RequestContext,
        account: &WalletAddress,
    ) -> impl Future<Output = R<Option<TokenAmount>>> + Send;
    fn token_accounts_by_owner(
        &self,
        ctx: &RequestContext,
        owner: &WalletAddress,
        filter: TokenAccountFilter,
        filter_address: &WalletAddress,
    ) -> impl Future<Output = R<Vec<TokenAccount>>> + Send;
    fn token_supply(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> impl Future<Output = R<Option<TokenAmount>>> + Send;
    fn token_largest_accounts(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> impl Future<Output = R<Vec<LargestTokenAccount>>> + Send;
    /// Budget-gated (`HELIUS_GET_BLOCK_ENABLED`); never polled.
    fn block(
        &self,
        ctx: &RequestContext,
        slot: u64,
    ) -> impl Future<Output = R<Option<BlockSummary>>> + Send;
}

/// Push subscriptions (standard Solana WebSocket methods).
pub trait BlockchainStreamProvider: Send + Sync {
    fn subscribe(
        &self,
        spec: SubscriptionSpec,
        durable_key: Option<String>,
    ) -> impl Future<Output = Result<SubscriptionId, GatewayError>> + Send;
    fn unsubscribe(
        &self,
        id: SubscriptionId,
    ) -> impl Future<Output = Result<(), GatewayError>> + Send;
    fn stream_snapshot(&self) -> Option<StreamSnapshot>;
}

pub trait TokenPriceProvider: Send + Sync {
    fn prices(
        &self,
        ctx: &RequestContext,
        mints: &[TokenMint],
    ) -> impl Future<Output = R<Vec<JupiterPrice>>> + Send;
}

pub trait TokenMetadataProvider: Send + Sync {
    fn token_info(
        &self,
        ctx: &RequestContext,
        mints: &[TokenMint],
    ) -> impl Future<Output = R<Vec<TokenInfo>>> + Send;
}

pub trait MarketDataProvider: Send + Sync {
    fn pairs_by_tokens(
        &self,
        ctx: &RequestContext,
        mints: &[TokenMint],
    ) -> impl Future<Output = R<Vec<DexPair>>> + Send;
    fn token_pairs(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> impl Future<Output = R<Vec<DexPair>>> + Send;
    fn pair(
        &self,
        ctx: &RequestContext,
        pair_address: &WalletAddress,
    ) -> impl Future<Output = R<Option<DexPair>>> + Send;
    fn search_pairs(
        &self,
        ctx: &RequestContext,
        query: &str,
    ) -> impl Future<Output = R<Vec<DexPair>>> + Send;
}

/// Market discovery listings (Phase 03 RADAR). Every listing is a provider
/// observation: it names tokens, never wallets, and promotional listings
/// (DEX Screener boosts) are paid visibility, not organic demand.
pub trait MarketDiscoveryProvider: Send + Sync {
    /// Jupiter Tokens V2 `/{category}/{interval}`.
    fn token_category(
        &self,
        ctx: &RequestContext,
        category: TokenCategory,
        interval: TokenInterval,
        limit: u16,
    ) -> impl Future<Output = R<Vec<TokenInfo>>> + Send;
    /// Jupiter Tokens V2 `/recent` (first pool recently created).
    fn recent_tokens(&self, ctx: &RequestContext) -> impl Future<Output = R<Vec<TokenInfo>>> + Send;
    /// DEX Screener latest token profiles (disabled unless
    /// `DEXSCREENER_DISCOVERY_ENABLED=true`).
    fn latest_profiles(
        &self,
        ctx: &RequestContext,
    ) -> impl Future<Output = R<Vec<DexProfile>>> + Send;
    /// DEX Screener boosts: **paid promotion** (disabled unless
    /// `DEXSCREENER_DISCOVERY_ENABLED=true`).
    fn boosts(&self, ctx: &RequestContext, top: bool)
    -> impl Future<Output = R<Vec<DexBoost>>> + Send;
}

/// Quote only. There is deliberately no method that returns, builds or
/// executes a transaction.
pub trait SwapQuoteProvider: Send + Sync {
    fn quote(
        &self,
        ctx: &RequestContext,
        req: &QuoteRequest,
    ) -> impl Future<Output = R<SwapQuote>> + Send;
}

impl BlockchainProvider for Gateway {
    async fn health(&self, ctx: &RequestContext) -> R<NodeHealth> {
        self.get_health(ctx).await
    }
    async fn version(&self, ctx: &RequestContext) -> R<NodeVersion> {
        self.get_version(ctx).await
    }
    async fn slot(&self, ctx: &RequestContext) -> R<u64> {
        self.get_slot(ctx).await
    }
    async fn block_time(&self, ctx: &RequestContext, slot: u64) -> R<Option<UtcTimestamp>> {
        self.get_block_time(ctx, slot).await
    }
    async fn balance(&self, ctx: &RequestContext, address: &WalletAddress) -> R<Lamports> {
        self.get_balance(ctx, address).await
    }
    async fn account(
        &self,
        ctx: &RequestContext,
        address: &WalletAddress,
    ) -> R<Option<AccountData>> {
        self.get_account_info(ctx, address).await
    }
    async fn accounts(
        &self,
        ctx: &RequestContext,
        addresses: &[WalletAddress],
    ) -> R<Vec<Option<AccountData>>> {
        self.get_multiple_accounts(ctx, addresses).await
    }
    async fn signatures(&self, ctx: &RequestContext, query: &SignatureQuery) -> R<SignaturePage> {
        self.get_signatures_for_address(ctx, query).await
    }
    async fn transaction(
        &self,
        ctx: &RequestContext,
        signature: &TransactionSignature,
    ) -> R<Option<TransactionRecord>> {
        self.get_transaction(ctx, signature).await
    }
    async fn signature_statuses(
        &self,
        ctx: &RequestContext,
        signatures: &[TransactionSignature],
        search_transaction_history: bool,
    ) -> R<Vec<Option<SignatureStatus>>> {
        self.get_signature_statuses(ctx, signatures, search_transaction_history)
            .await
    }
    async fn token_account_balance(
        &self,
        ctx: &RequestContext,
        account: &WalletAddress,
    ) -> R<Option<TokenAmount>> {
        self.get_token_account_balance(ctx, account).await
    }
    async fn token_accounts_by_owner(
        &self,
        ctx: &RequestContext,
        owner: &WalletAddress,
        filter: TokenAccountFilter,
        filter_address: &WalletAddress,
    ) -> R<Vec<TokenAccount>> {
        self.get_token_accounts_by_owner(ctx, owner, filter, filter_address)
            .await
    }
    async fn token_supply(&self, ctx: &RequestContext, mint: &TokenMint) -> R<Option<TokenAmount>> {
        self.get_token_supply(ctx, mint).await
    }
    async fn token_largest_accounts(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> R<Vec<LargestTokenAccount>> {
        self.get_token_largest_accounts(ctx, mint).await
    }
    async fn block(&self, ctx: &RequestContext, slot: u64) -> R<Option<BlockSummary>> {
        self.get_block(ctx, slot).await
    }
}

fn streams(g: &Gateway) -> Result<&StreamHandle, GatewayError> {
    g.streams()
        .ok_or(GatewayError::Stream("streams are disabled"))
}

impl BlockchainStreamProvider for Gateway {
    async fn subscribe(
        &self,
        spec: SubscriptionSpec,
        durable_key: Option<String>,
    ) -> Result<SubscriptionId, GatewayError> {
        streams(self)?.subscribe(spec, durable_key).await
    }
    async fn unsubscribe(&self, id: SubscriptionId) -> Result<(), GatewayError> {
        streams(self)?.unsubscribe(id).await
    }
    fn stream_snapshot(&self) -> Option<StreamSnapshot> {
        self.streams().map(StreamHandle::snapshot)
    }
}

impl TokenPriceProvider for Gateway {
    async fn prices(&self, ctx: &RequestContext, mints: &[TokenMint]) -> R<Vec<JupiterPrice>> {
        self.jupiter_prices(ctx, mints).await
    }
}

impl TokenMetadataProvider for Gateway {
    async fn token_info(&self, ctx: &RequestContext, mints: &[TokenMint]) -> R<Vec<TokenInfo>> {
        self.jupiter_tokens(ctx, mints).await
    }
}

impl MarketDataProvider for Gateway {
    async fn pairs_by_tokens(&self, ctx: &RequestContext, mints: &[TokenMint]) -> R<Vec<DexPair>> {
        self.dex_pairs_by_tokens(ctx, mints).await
    }
    async fn token_pairs(&self, ctx: &RequestContext, mint: &TokenMint) -> R<Vec<DexPair>> {
        self.dex_token_pairs(ctx, mint).await
    }
    async fn pair(&self, ctx: &RequestContext, pair_address: &WalletAddress) -> R<Option<DexPair>> {
        self.dex_pair(ctx, pair_address).await
    }
    async fn search_pairs(&self, ctx: &RequestContext, query: &str) -> R<Vec<DexPair>> {
        self.dex_search(ctx, query).await
    }
}

impl MarketDiscoveryProvider for Gateway {
    async fn token_category(
        &self,
        ctx: &RequestContext,
        category: TokenCategory,
        interval: TokenInterval,
        limit: u16,
    ) -> R<Vec<TokenInfo>> {
        self.jupiter_token_category(ctx, category, interval, limit)
            .await
    }
    async fn recent_tokens(&self, ctx: &RequestContext) -> R<Vec<TokenInfo>> {
        self.jupiter_recent_tokens(ctx).await
    }
    async fn latest_profiles(&self, ctx: &RequestContext) -> R<Vec<DexProfile>> {
        self.dex_latest_profiles(ctx).await
    }
    async fn boosts(&self, ctx: &RequestContext, top: bool) -> R<Vec<DexBoost>> {
        self.dex_boosts(ctx, top).await
    }
}

impl SwapQuoteProvider for Gateway {
    async fn quote(&self, ctx: &RequestContext, req: &QuoteRequest) -> R<SwapQuote> {
        self.jupiter_quote(ctx, req).await
    }
}
