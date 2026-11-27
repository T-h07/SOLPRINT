//! Normalized internal models returned by GATEWAY.
//!
//! These are SOLPRINT's types, not provider wire types: amounts are integer
//! base units (`u64`, `TokenAmount`, `Lamports`), prices are `Price` /
//! `Decimal`, times are `UtcTimestamp`, identifiers are validated.

use rust_decimal::Decimal;
use solprint_core::{
    CommitmentLevel, Lamports, Price, TokenAmount, TokenMint, TransactionSignature, UtcTimestamp,
    WalletAddress,
};

// ---- Solana RPC ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeHealth {
    Ok,
    /// The node reports it is behind the cluster.
    Behind {
        slots: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVersion {
    pub solana_core: String,
    pub feature_set: Option<u32>,
}

/// An account as returned with `base64` encoding. `owner` is the owning
/// program's address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountData {
    pub lamports: Lamports,
    pub owner: WalletAddress,
    pub data_base64: String,
    pub executable: bool,
    pub rent_epoch: u64,
    pub space: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureInfo {
    pub signature: TransactionSignature,
    pub slot: u64,
    pub block_time: Option<UtcTimestamp>,
    pub failed: bool,
    /// Compact JSON of the on-chain error, when the transaction failed.
    pub err_json: Option<String>,
    pub memo: Option<String>,
    pub confirmation_status: Option<CommitmentLevel>,
}

/// Bounded incremental pagination for `getSignaturesForAddress`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureQuery {
    pub address: WalletAddress,
    /// Start searching backwards from this signature (exclusive).
    pub before: Option<TransactionSignature>,
    /// Stop at this signature (exclusive), e.g. the last checkpoint.
    pub until: Option<TransactionSignature>,
    /// Items per page, 1–1000.
    pub page_limit: u16,
    /// Pages to fetch at most, 1–10. Each page costs one request.
    pub max_pages: u8,
}

impl SignatureQuery {
    pub const MAX_PAGE_LIMIT: u16 = 1_000;
    pub const MAX_PAGES: u8 = 10;

    #[must_use]
    pub const fn latest(address: WalletAddress, page_limit: u16) -> Self {
        Self {
            address,
            before: None,
            until: None,
            page_limit,
            max_pages: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignaturePage {
    /// Newest first, as the RPC returns them.
    pub items: Vec<SignatureInfo>,
    /// `true` when history was exhausted or `until` was reached; `false`
    /// means the page bound stopped the walk and older items may exist.
    pub complete: bool,
    /// Continue with `before = next_before` to walk further back.
    pub next_before: Option<TransactionSignature>,
    pub pages_fetched: u8,
}

/// A transaction kept as raw evidence for Phase 02 decoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRecord {
    pub signature: TransactionSignature,
    pub slot: u64,
    pub block_time: Option<UtcTimestamp>,
    /// `"legacy"` or the numeric version as text.
    pub version: Option<String>,
    pub failed: bool,
    /// The complete `result` object exactly as received (JSON text).
    pub raw_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAccountFilter {
    Mint,
    Program,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenAccount {
    pub address: WalletAddress,
    pub mint: TokenMint,
    pub owner: WalletAddress,
    pub amount: TokenAmount,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LargestTokenAccount {
    pub address: WalletAddress,
    pub amount: TokenAmount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSummary {
    pub slot: u64,
    pub blockhash: String,
    pub parent_slot: u64,
    pub block_time: Option<UtcTimestamp>,
    pub block_height: Option<u64>,
    pub signatures: Vec<TransactionSignature>,
}

// ---- Jupiter -------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JupiterPrice {
    pub mint: TokenMint,
    pub usd_price: Price,
    /// Block the price was derived at, when the provider says.
    pub block_id: Option<u64>,
    pub decimals: Option<u8>,
    /// Percentage points, as published.
    pub price_change_24h_pct: Option<Decimal>,
    pub liquidity_usd: Option<Decimal>,
    pub created_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInfo {
    pub mint: TokenMint,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<u8>,
    pub token_program: Option<String>,
    pub icon_url: Option<String>,
    pub is_verified: Option<bool>,
    pub tags: Vec<String>,
    /// 0–100 as published by Jupiter.
    pub organic_score: Option<Decimal>,
    pub usd_price: Option<Price>,
    pub liquidity_usd: Option<Decimal>,
    pub holder_count: Option<u64>,
    pub fdv_usd: Option<Decimal>,
    pub market_cap_usd: Option<Decimal>,
    pub updated_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteRequest {
    pub input_mint: TokenMint,
    pub output_mint: TokenMint,
    /// Input amount in base units.
    pub amount: u64,
    pub slippage_bps: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteLeg {
    pub amm_key: String,
    pub label: Option<String>,
    pub input_mint: TokenMint,
    pub output_mint: TokenMint,
    pub in_amount: u64,
    pub out_amount: u64,
    pub percent: Option<u8>,
    pub bps: Option<u16>,
}

/// A quote. It is information only: it carries no transaction and nothing
/// in SOLPRINT can execute it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapQuote {
    pub input_mint: TokenMint,
    pub output_mint: TokenMint,
    pub in_amount: u64,
    pub out_amount: u64,
    pub other_amount_threshold: u64,
    pub swap_mode: String,
    pub slippage_bps: Option<u16>,
    /// Percentage points, as published (`priceImpact`).
    pub price_impact_pct: Option<Decimal>,
    pub route: Vec<RouteLeg>,
    pub fee_bps: Option<u16>,
    pub router: Option<String>,
    pub request_id: Option<String>,
    pub expires_at: Option<String>,
}

// ---- DEX Screener --------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexToken {
    pub address: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
}

/// Activity windows DEX Screener publishes. Any other key is ignored.
pub const DEX_WINDOWS: [&str; 4] = ["m5", "h1", "h6", "h24"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexWindowTxns {
    pub window: &'static str,
    pub buys: u64,
    pub sells: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexPair {
    pub chain_id: String,
    pub dex_id: String,
    pub url: Option<String>,
    pub pair_address: String,
    pub labels: Vec<String>,
    pub base_token: DexToken,
    pub quote_token: DexToken,
    pub price_native: Option<Decimal>,
    pub price_usd: Option<Price>,
    pub txns: Vec<DexWindowTxns>,
    pub volume_usd: Vec<(&'static str, Decimal)>,
    pub price_change_pct: Vec<(&'static str, Decimal)>,
    pub liquidity_usd: Option<Decimal>,
    pub liquidity_base: Option<Decimal>,
    pub liquidity_quote: Option<Decimal>,
    pub fdv_usd: Option<Decimal>,
    pub market_cap_usd: Option<Decimal>,
    /// When the pair was created (not when this data was produced).
    pub pair_created_at: Option<UtcTimestamp>,
    pub boosts_active: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexBoost {
    pub chain_id: String,
    pub token_address: String,
    pub url: Option<String>,
    pub amount: Option<Decimal>,
    pub total_amount: Option<Decimal>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DexProfile {
    pub chain_id: String,
    pub token_address: String,
    pub url: Option<String>,
    pub description: Option<String>,
}
