//! DEX Screener (no key): pairs by token, pools of a token, pair lookup,
//! search; optional on-demand profiles/boosts (disabled by default, never
//! scanned continuously).
//!
//! DEX Screener publishes no snapshot timestamp, so every result has
//! freshness `UNKNOWN_SOURCE_AGE`: only the time SOLPRINT observed it is
//! known. Only `chainId == "solana"` pairs are returned.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;
use solprint_core::{Price, TokenMint, UtcTimestamp};
use time::OffsetDateTime;
use url::Url;

use crate::cache::CacheTtl;
use crate::context::{GatewayResponse, ProviderId, RequestContext};
use crate::engine::{Call, Fetched};
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::http_json::{self, Accept};
use crate::models::{DEX_WINDOWS, DexBoost, DexPair, DexProfile, DexToken, DexWindowTxns};
use crate::numbers::decimal_from_raw;
use crate::operations as op;
use crate::ratelimit::RateLimiter;

pub const TOKENS_MAX: usize = 30;
pub const CHAIN: &str = "solana";
const P: ProviderId = ProviderId::DexScreener;
const SEARCH_MAX_LEN: usize = 100;

const TTL_PAIRS: CacheTtl = CacheTtl::new(
    Duration::from_secs(15),
    Duration::from_secs(120),
    Duration::from_secs(30),
);
const TTL_DISCOVERY: CacheTtl = CacheTtl::new(
    Duration::from_secs(60),
    Duration::from_secs(300),
    Duration::ZERO,
);

fn decode(detail: &'static str) -> GatewayError {
    GatewayError::Decode {
        provider: P,
        detail,
    }
}

fn bounded(s: Option<String>, n: usize) -> Option<String> {
    s.map(|s| s.chars().filter(|c| !c.is_control()).take(n).collect())
}

#[derive(Deserialize)]
struct RawToken {
    address: Option<String>,
    name: Option<String>,
    symbol: Option<String>,
}

#[derive(Deserialize)]
struct RawBuysSells {
    buys: Option<u64>,
    sells: Option<u64>,
}

#[derive(Deserialize)]
struct RawLiquidity {
    usd: Option<Box<RawValue>>,
    base: Option<Box<RawValue>>,
    quote: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
struct RawBoosts {
    active: Option<u64>,
}

#[derive(Deserialize)]
struct RawPair {
    #[serde(rename = "chainId")]
    chain_id: String,
    #[serde(rename = "dexId")]
    dex_id: Option<String>,
    url: Option<String>,
    #[serde(rename = "pairAddress")]
    pair_address: String,
    labels: Option<Vec<String>>,
    #[serde(rename = "baseToken")]
    base_token: RawToken,
    #[serde(rename = "quoteToken")]
    quote_token: Option<RawToken>,
    #[serde(rename = "priceNative")]
    price_native: Option<Box<RawValue>>,
    #[serde(rename = "priceUsd")]
    price_usd: Option<Box<RawValue>>,
    txns: Option<HashMap<String, RawBuysSells>>,
    volume: Option<HashMap<String, Box<RawValue>>>,
    #[serde(rename = "priceChange")]
    price_change: Option<HashMap<String, Box<RawValue>>>,
    liquidity: Option<RawLiquidity>,
    fdv: Option<Box<RawValue>>,
    #[serde(rename = "marketCap")]
    market_cap: Option<Box<RawValue>>,
    #[serde(rename = "pairCreatedAt")]
    pair_created_at: Option<i64>,
    boosts: Option<RawBoosts>,
}

#[derive(Deserialize)]
struct RawPairsEnvelope {
    pairs: Option<Vec<RawPair>>,
}

#[derive(Deserialize)]
struct RawBoost {
    #[serde(rename = "chainId")]
    chain_id: String,
    #[serde(rename = "tokenAddress")]
    token_address: String,
    url: Option<String>,
    amount: Option<Box<RawValue>>,
    #[serde(rename = "totalAmount")]
    total_amount: Option<Box<RawValue>>,
    description: Option<String>,
}

fn dec(v: Option<&RawValue>) -> Option<Decimal> {
    v.and_then(decimal_from_raw)
}

fn window_map(m: Option<HashMap<String, Box<RawValue>>>) -> Vec<(&'static str, Decimal)> {
    let m = m.unwrap_or_default();
    DEX_WINDOWS
        .iter()
        .filter_map(|w| m.get(*w).and_then(|v| decimal_from_raw(v)).map(|d| (*w, d)))
        .collect()
}

fn token(t: RawToken) -> DexToken {
    DexToken {
        address: bounded(t.address, 64).unwrap_or_default(),
        name: bounded(t.name, 64),
        symbol: bounded(t.symbol, 32),
    }
}

fn normalize(p: RawPair) -> Option<DexPair> {
    if p.chain_id != CHAIN {
        return None;
    }
    let txns = p.txns.unwrap_or_default();
    let liquidity = p.liquidity;
    Some(DexPair {
        chain_id: p.chain_id,
        dex_id: bounded(p.dex_id, 32).unwrap_or_default(),
        url: bounded(p.url, 256),
        pair_address: p.pair_address.chars().take(64).collect(),
        labels: p
            .labels
            .unwrap_or_default()
            .into_iter()
            .take(8)
            .map(|l| l.chars().take(24).collect())
            .collect(),
        base_token: token(p.base_token),
        quote_token: p.quote_token.map_or(
            DexToken {
                address: String::new(),
                name: None,
                symbol: None,
            },
            token,
        ),
        price_native: dec(p.price_native.as_deref()),
        price_usd: dec(p.price_usd.as_deref()).and_then(|d| Price::new(d).ok()),
        txns: DEX_WINDOWS
            .iter()
            .filter_map(|w| {
                txns.get(*w).map(|t| DexWindowTxns {
                    window: w,
                    buys: t.buys.unwrap_or(0),
                    sells: t.sells.unwrap_or(0),
                })
            })
            .collect(),
        volume_usd: window_map(p.volume),
        price_change_pct: window_map(p.price_change),
        liquidity_usd: liquidity.as_ref().and_then(|l| dec(l.usd.as_deref())),
        liquidity_base: liquidity.as_ref().and_then(|l| dec(l.base.as_deref())),
        liquidity_quote: liquidity.as_ref().and_then(|l| dec(l.quote.as_deref())),
        fdv_usd: dec(p.fdv.as_deref()),
        market_cap_usd: dec(p.market_cap.as_deref()),
        pair_created_at: p
            .pair_created_at
            .and_then(|ms| {
                OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000).ok()
            })
            .map(UtcTimestamp::from_offset_datetime),
        boosts_active: p.boosts.and_then(|b| b.active),
    })
}

/// Bare `Pair[]` (tokens/v1, token-pairs/v1).
pub(crate) fn parse_pair_array(body: &[u8]) -> Result<Vec<DexPair>, GatewayError> {
    let raw: Vec<RawPair> = http_json::decode_json(P, body, "pairs response is not an array")?;
    Ok(raw.into_iter().filter_map(normalize).collect())
}

/// `{schemaVersion, pairs}` (latest/dex/pairs, latest/dex/search).
pub(crate) fn parse_pair_envelope(body: &[u8]) -> Result<Vec<DexPair>, GatewayError> {
    let raw: RawPairsEnvelope = http_json::decode_json(P, body, "pairs envelope invalid")?;
    Ok(raw
        .pairs
        .unwrap_or_default()
        .into_iter()
        .filter_map(normalize)
        .collect())
}

pub(crate) fn parse_boosts(body: &[u8]) -> Result<Vec<DexBoost>, GatewayError> {
    // Endpoints return either one object or an array.
    let v: serde_json::Value = http_json::decode_json(P, body, "boosts response invalid")?;
    let items: Vec<RawBoost> = match v {
        serde_json::Value::Array(_) => {
            serde_json::from_value(v).map_err(|_| decode("boosts invalid"))?
        }
        serde_json::Value::Object(_) => {
            vec![serde_json::from_value(v).map_err(|_| decode("boost invalid"))?]
        }
        _ => return Err(decode("boosts response invalid")),
    };
    Ok(items
        .into_iter()
        .filter(|b| b.chain_id == CHAIN)
        .take(100)
        .map(|b| DexBoost {
            chain_id: b.chain_id,
            token_address: b.token_address.chars().take(64).collect(),
            url: bounded(b.url, 256),
            amount: dec(b.amount.as_deref()),
            total_amount: dec(b.total_amount.as_deref()),
            description: bounded(b.description, 280),
        })
        .collect())
}

pub(crate) fn parse_profiles(body: &[u8]) -> Result<Vec<DexProfile>, GatewayError> {
    let v: serde_json::Value = http_json::decode_json(P, body, "profiles response invalid")?;
    let items = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(_) => vec![v],
        _ => return Err(decode("profiles response invalid")),
    };
    Ok(items
        .into_iter()
        .filter(|p| p.get("chainId").and_then(serde_json::Value::as_str) == Some(CHAIN))
        .take(100)
        .filter_map(|p| {
            Some(DexProfile {
                chain_id: CHAIN.to_owned(),
                token_address: p.get("tokenAddress")?.as_str()?.chars().take(64).collect(),
                url: bounded(
                    p.get("url").and_then(|u| u.as_str()).map(str::to_owned),
                    256,
                ),
                description: bounded(
                    p.get("description")
                        .and_then(|u| u.as_str())
                        .map(str::to_owned),
                    280,
                ),
            })
        })
        .collect())
}

fn sorted_mints(mints: &[TokenMint]) -> Vec<TokenMint> {
    mints
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl Gateway {
    fn dex_url(&self, segments: &[&str]) -> Result<Url, GatewayError> {
        let mut url = self.inner.settings.dexscreener.base_url.clone();
        url.path_segments_mut()
            .map_err(|_| GatewayError::InvalidRequest("invalid DEX Screener base URL"))?
            .pop_if_empty()
            .extend(segments);
        Ok(url)
    }

    #[allow(clippy::too_many_arguments)]
    async fn dex_get<T>(
        &self,
        ctx: &RequestContext,
        limiter: &RateLimiter,
        operation: &'static str,
        url: Url,
        cache_key: String,
        ttl: CacheTtl,
        parse: fn(&[u8]) -> Result<T, GatewayError>,
    ) -> Result<GatewayResponse<T>, GatewayError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let call = Call {
            provider: P,
            operation,
            category: operation,
            cache_key: Some(cache_key),
            ttl,
            credits: 0,
            source_age_known: false,
            fallback: false,
        };
        let transport = &self.inner.transport;
        let r = self
            .engine()
            .execute(ctx, limiter, call, |timeout| {
                let url = url.clone();
                async move {
                    let resp =
                        http_json::get(transport, P, url, None, timeout, Accept::Success).await?;
                    parse(&resp.body).map(Fetched::found)
                }
            })
            .await?;
        r.require(|_| decode("empty response"))
    }

    /// Pairs for up to 30 token mints per request (chunked).
    pub async fn dex_pairs_by_tokens(
        &self,
        ctx: &RequestContext,
        mints: &[TokenMint],
    ) -> Result<GatewayResponse<Vec<DexPair>>, GatewayError> {
        let mints = sorted_mints(mints);
        if mints.is_empty() || mints.len() > 4 * TOKENS_MAX {
            return Err(GatewayError::InvalidRequest("1..=120 mints per call"));
        }
        let mut all = Vec::new();
        let mut last = None;
        for chunk in mints.chunks(TOKENS_MAX) {
            let joined = chunk
                .iter()
                .map(TokenMint::as_str)
                .collect::<Vec<_>>()
                .join(",");
            // Mints are validated base58, so they cannot inject path segments.
            let url = self.dex_url(&["tokens", "v1", CHAIN, &joined])?;
            let r = self
                .dex_get(
                    ctx,
                    &self.inner.limiters.dex_pairs,
                    op::DEX_TOKENS,
                    url,
                    joined.clone(),
                    TTL_PAIRS,
                    parse_pair_array,
                )
                .await?;
            all.extend(r.value.iter().cloned());
            last = Some(r);
        }
        let last = last.ok_or(GatewayError::InvalidRequest("no mints"))?;
        Ok(last.map(|_| all))
    }

    /// All pools of one token.
    pub async fn dex_token_pairs(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> Result<GatewayResponse<Vec<DexPair>>, GatewayError> {
        let url = self.dex_url(&["token-pairs", "v1", CHAIN, mint.as_str()])?;
        self.dex_get(
            ctx,
            &self.inner.limiters.dex_pairs,
            op::DEX_TOKEN_PAIRS,
            url,
            mint.as_str().to_owned(),
            TTL_PAIRS,
            parse_pair_array,
        )
        .await
    }

    /// One pair by address; `None` when DEX Screener does not know it.
    pub async fn dex_pair(
        &self,
        ctx: &RequestContext,
        pair_address: &solprint_core::WalletAddress,
    ) -> Result<GatewayResponse<Option<DexPair>>, GatewayError> {
        let url = self.dex_url(&["latest", "dex", "pairs", CHAIN, pair_address.as_str()])?;
        let r = self
            .dex_get(
                ctx,
                &self.inner.limiters.dex_pairs,
                op::DEX_PAIR,
                url,
                pair_address.as_str().to_owned(),
                TTL_PAIRS,
                parse_pair_envelope,
            )
            .await?;
        let want = pair_address.as_str().to_owned();
        Ok(r.map(|pairs| pairs.into_iter().find(|p| p.pair_address == want)))
    }

    /// Free-text search (Solana pairs only), query 1–100 printable characters.
    pub async fn dex_search(
        &self,
        ctx: &RequestContext,
        query: &str,
    ) -> Result<GatewayResponse<Vec<DexPair>>, GatewayError> {
        let q = query.trim();
        if q.is_empty() || q.chars().count() > SEARCH_MAX_LEN || q.chars().any(char::is_control) {
            return Err(GatewayError::InvalidRequest(
                "query must be 1..=100 printable characters",
            ));
        }
        let mut url = self.dex_url(&["latest", "dex", "search"])?;
        url.query_pairs_mut().append_pair("q", q);
        self.dex_get(
            ctx,
            &self.inner.limiters.dex_pairs,
            op::DEX_SEARCH,
            url,
            q.to_lowercase(),
            TTL_PAIRS,
            parse_pair_envelope,
        )
        .await
    }

    fn discovery_enabled(&self) -> Result<(), GatewayError> {
        if self.inner.settings.dexscreener.discovery_enabled {
            Ok(())
        } else {
            Err(GatewayError::Disabled {
                provider: P,
                reason: "DEXSCREENER_DISCOVERY_ENABLED=false",
            })
        }
    }

    /// Latest token profiles (on demand only; disabled by default).
    pub async fn dex_latest_profiles(
        &self,
        ctx: &RequestContext,
    ) -> Result<GatewayResponse<Vec<DexProfile>>, GatewayError> {
        self.discovery_enabled()?;
        let url = self.dex_url(&["token-profiles", "latest", "v1"])?;
        self.dex_get(
            ctx,
            &self.inner.limiters.dex_discovery,
            op::DEX_PROFILES,
            url,
            "latest".into(),
            TTL_DISCOVERY,
            parse_profiles,
        )
        .await
    }

    /// Latest or top boosted tokens (on demand only; disabled by default).
    pub async fn dex_boosts(
        &self,
        ctx: &RequestContext,
        top: bool,
    ) -> Result<GatewayResponse<Vec<DexBoost>>, GatewayError> {
        self.discovery_enabled()?;
        let (kind, operation) = if top {
            ("top", op::DEX_BOOSTS_TOP)
        } else {
            ("latest", op::DEX_BOOSTS_LATEST)
        };
        let url = self.dex_url(&["token-boosts", kind, "v1"])?;
        self.dex_get(
            ctx,
            &self.inner.limiters.dex_discovery,
            operation,
            url,
            kind.into(),
            TTL_DISCOVERY,
            parse_boosts,
        )
        .await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const PAIR: &str = r#"{"chainId":"solana","dexId":"raydium","url":"https://dexscreener.com/solana/x","pairAddress":"Pair111",
        "baseToken":{"address":"So11111111111111111111111111111111111111112","name":"Wrapped SOL","symbol":"SOL"},
        "quoteToken":{"address":null,"name":null,"symbol":null},
        "priceNative":"1.0","priceUsd":"147.000000000000000001",
        "txns":{"m5":{"buys":3,"sells":4},"h24":{"buys":100,"sells":90},"weird":{"buys":1,"sells":1}},
        "volume":{"h24":123456.789,"m5":0.5},"priceChange":{"h1":-2.5},
        "liquidity":{"usd":1000000.5,"base":10,"quote":20},"fdv":5e9,"marketCap":4000000000,
        "pairCreatedAt":1700000000000,"boosts":{"active":2}}"#;

    #[test]
    fn pairs_are_normalized_exactly_and_bounded() {
        let body =
            format!(r#"[{PAIR},{{"chainId":"ethereum","pairAddress":"0x1","baseToken":{{}}}}]"#);
        let pairs = parse_pair_array(body.as_bytes()).unwrap();
        assert_eq!(pairs.len(), 1, "non-Solana pairs are dropped");
        let p = &pairs[0];
        assert_eq!(
            p.price_usd.unwrap().get().to_string(),
            "147.000000000000000001"
        );
        assert_eq!(p.txns.len(), 2, "unknown windows ignored");
        assert_eq!(p.volume_usd[0], ("m5", Decimal::new(5, 1)));
        assert_eq!(p.fdv_usd.unwrap().to_string(), "5000000000");
        assert_eq!(
            p.pair_created_at.unwrap().to_rfc3339(),
            "2023-11-14T22:13:20.000Z"
        );
        assert_eq!(p.quote_token.address, "");
        assert_eq!(p.boosts_active, Some(2));
    }

    #[test]
    fn envelopes_with_null_pairs_are_empty() {
        assert!(
            parse_pair_envelope(br#"{"schemaVersion":"1.0.0","pairs":null}"#)
                .unwrap()
                .is_empty()
        );
        let body = format!(r#"{{"schemaVersion":"1.0.0","pairs":[{PAIR}]}}"#);
        assert_eq!(parse_pair_envelope(body.as_bytes()).unwrap().len(), 1);
        assert!(parse_pair_envelope(b"<html>").is_err());
    }

    #[test]
    fn boosts_accept_object_or_array() {
        let one =
            br#"{"url":"u","chainId":"solana","tokenAddress":"T","amount":10,"totalAmount":20}"#;
        assert_eq!(parse_boosts(one).unwrap().len(), 1);
        let many = br#"[{"url":"u","chainId":"solana","tokenAddress":"T","amount":10,"totalAmount":20},{"url":"u","chainId":"bsc","tokenAddress":"X"}]"#;
        assert_eq!(parse_boosts(many).unwrap().len(), 1);
    }
}
