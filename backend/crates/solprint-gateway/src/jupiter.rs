//! Jupiter: Price V3, Tokens V2 search, Swap V2 `/order` **quote only**.
//!
//! The order endpoint is called without a `taker`, so Jupiter returns a quote
//! with `transaction: null`. If a response ever carries a transaction it is
//! discarded and reported as `PROHIBITED`: SOLPRINT holds no keys and cannot
//! sign, and keeping an unsigned transaction around serves no purpose.
//! `/swap/v2/execute`, `/swap/v2/build` and transaction submission are
//! blocklisted in the transport.

use std::collections::{BTreeSet, HashMap};
use std::time::Duration;

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::value::RawValue;
use solprint_core::{Price, TokenMint, UtcTimestamp};
use url::Url;

use crate::cache::CacheTtl;
use crate::context::{GatewayResponse, ProviderId, RequestContext};
use crate::engine::{Call, Fetched};
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::http_json::{self, Accept};
use crate::models::{JupiterPrice, QuoteRequest, RouteLeg, SwapQuote, TokenInfo};
use crate::numbers::decimal_from_raw;
use crate::operations as op;

pub const PRICE_IDS_MAX: usize = 50;
pub const TOKENS_SEARCH_MAX: usize = 100;
const P: ProviderId = ProviderId::Jupiter;

const TTL_PRICE: CacheTtl = CacheTtl::new(
    Duration::from_secs(5),
    Duration::from_secs(60),
    Duration::ZERO,
);
const TTL_TOKENS: CacheTtl = CacheTtl::new(
    Duration::from_secs(300),
    Duration::from_secs(3_600),
    Duration::from_secs(60),
);

fn decode(detail: &'static str) -> GatewayError {
    GatewayError::Decode {
        provider: P,
        detail,
    }
}

fn parse_ts(s: Option<&str>) -> Option<UtcTimestamp> {
    s.and_then(|s| UtcTimestamp::parse_rfc3339(s).ok())
}

fn dec(v: Option<&RawValue>) -> Option<Decimal> {
    v.and_then(decimal_from_raw)
}

#[derive(Deserialize)]
struct RawPrice {
    #[serde(rename = "usdPrice")]
    usd_price: Option<Box<RawValue>>,
    #[serde(rename = "blockId")]
    block_id: Option<u64>,
    decimals: Option<u8>,
    #[serde(rename = "priceChange24h")]
    price_change_24h: Option<Box<RawValue>>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
    liquidity: Option<Box<RawValue>>,
}

#[derive(Deserialize)]
struct RawToken {
    id: String,
    name: Option<String>,
    symbol: Option<String>,
    icon: Option<String>,
    decimals: Option<u8>,
    #[serde(rename = "tokenProgram")]
    token_program: Option<String>,
    #[serde(rename = "isVerified")]
    is_verified: Option<bool>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(rename = "organicScore")]
    organic_score: Option<Box<RawValue>>,
    #[serde(rename = "usdPrice")]
    usd_price: Option<Box<RawValue>>,
    liquidity: Option<Box<RawValue>>,
    #[serde(rename = "holderCount")]
    holder_count: Option<u64>,
    fdv: Option<Box<RawValue>>,
    mcap: Option<Box<RawValue>>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
}

fn bounded(s: Option<String>, n: usize) -> Option<String> {
    s.map(|s| s.chars().filter(|c| !c.is_control()).take(n).collect())
}

pub(crate) fn parse_prices(body: &[u8]) -> Result<Vec<JupiterPrice>, GatewayError> {
    let map: HashMap<String, Option<RawPrice>> =
        http_json::decode_json(P, body, "price response is not an object keyed by mint")?;
    let mut out = Vec::with_capacity(map.len());
    for (mint, raw) in map {
        let (Ok(mint), Some(raw)) = (TokenMint::parse(&mint), raw) else {
            continue; // not a mint, or explicitly null: no price
        };
        let Some(usd) = dec(raw.usd_price.as_deref()).and_then(|d| Price::new(d).ok()) else {
            continue;
        };
        out.push(JupiterPrice {
            mint,
            usd_price: usd,
            block_id: raw.block_id,
            decimals: raw.decimals,
            price_change_24h_pct: dec(raw.price_change_24h.as_deref()),
            liquidity_usd: dec(raw.liquidity.as_deref()),
            created_at: parse_ts(raw.created_at.as_deref()),
        });
    }
    out.sort_by(|a, b| a.mint.cmp(&b.mint));
    Ok(out)
}

pub(crate) fn parse_tokens(body: &[u8]) -> Result<Vec<TokenInfo>, GatewayError> {
    let items: Vec<RawToken> = http_json::decode_json(P, body, "tokens response is not an array")?;
    Ok(items
        .into_iter()
        .filter_map(|t| {
            let mint = TokenMint::parse(&t.id).ok()?;
            Some(TokenInfo {
                mint,
                name: bounded(t.name, 64),
                symbol: bounded(t.symbol, 32),
                decimals: t.decimals,
                token_program: bounded(t.token_program, 64),
                icon_url: bounded(t.icon, 512),
                is_verified: t.is_verified,
                tags: t
                    .tags
                    .unwrap_or_default()
                    .into_iter()
                    .take(16)
                    .map(|s| s.chars().take(32).collect())
                    .collect(),
                organic_score: dec(t.organic_score.as_deref()),
                usd_price: dec(t.usd_price.as_deref()).and_then(|d| Price::new(d).ok()),
                liquidity_usd: dec(t.liquidity.as_deref()),
                holder_count: t.holder_count,
                fdv_usd: dec(t.fdv.as_deref()),
                market_cap_usd: dec(t.mcap.as_deref()),
                updated_at: parse_ts(t.updated_at.as_deref()),
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct RawSwapInfo {
    #[serde(rename = "ammKey")]
    amm_key: String,
    label: Option<String>,
    #[serde(rename = "inputMint")]
    input_mint: String,
    #[serde(rename = "outputMint")]
    output_mint: String,
    #[serde(rename = "inAmount")]
    in_amount: String,
    #[serde(rename = "outAmount")]
    out_amount: String,
}

#[derive(Deserialize)]
struct RawLeg {
    #[serde(rename = "swapInfo")]
    swap_info: RawSwapInfo,
    percent: Option<u8>,
    bps: Option<u16>,
}

#[derive(Deserialize)]
struct RawOrder {
    #[serde(rename = "inputMint")]
    input_mint: Option<String>,
    #[serde(rename = "outputMint")]
    output_mint: Option<String>,
    #[serde(rename = "inAmount")]
    in_amount: Option<String>,
    #[serde(rename = "outAmount")]
    out_amount: Option<String>,
    #[serde(rename = "otherAmountThreshold")]
    other_amount_threshold: Option<String>,
    #[serde(rename = "swapMode")]
    swap_mode: Option<String>,
    #[serde(rename = "slippageBps")]
    slippage_bps: Option<u16>,
    #[serde(rename = "priceImpact")]
    price_impact: Option<Box<RawValue>>,
    #[serde(rename = "routePlan", default)]
    route_plan: Option<Vec<RawLeg>>,
    #[serde(rename = "feeBps")]
    fee_bps: Option<u16>,
    router: Option<String>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "expireAt")]
    expire_at: Option<Box<RawValue>>,
    transaction: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
    #[serde(rename = "errorCode")]
    error_code: Option<serde_json::Value>,
}

fn amount(s: Option<&str>, what: &'static str) -> Result<u64, GatewayError> {
    s.and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| decode(what))
}

pub(crate) fn parse_order(
    status: u16,
    body: &[u8],
    req: &QuoteRequest,
) -> Result<SwapQuote, GatewayError> {
    let raw: RawOrder = http_json::decode_json(P, body, "order response is not JSON")?;
    if raw.transaction.as_ref().is_some_and(|t| !t.is_null()) {
        tracing::warn!("Jupiter order response carried a transaction; discarded");
        return Err(GatewayError::Prohibited(
            "quote response carried a transaction; SOLPRINT does not handle transactions",
        ));
    }
    let has_error = raw.error.as_ref().is_some_and(|e| !e.is_null())
        || raw.error_code.as_ref().is_some_and(|e| !e.is_null());
    if status == 400 || status == 404 || has_error || raw.out_amount.is_none() {
        return Err(GatewayError::NoRoute);
    }
    // The quote must be for exactly what was asked.
    if raw
        .input_mint
        .as_deref()
        .is_some_and(|m| m != req.input_mint.as_str())
        || raw
            .output_mint
            .as_deref()
            .is_some_and(|m| m != req.output_mint.as_str())
    {
        return Err(decode("order response mints do not match the request"));
    }
    let route = raw
        .route_plan
        .unwrap_or_default()
        .into_iter()
        .take(16)
        .map(|leg| {
            Ok(RouteLeg {
                amm_key: leg.swap_info.amm_key.chars().take(64).collect(),
                label: bounded(leg.swap_info.label, 64),
                input_mint: TokenMint::parse(&leg.swap_info.input_mint)
                    .map_err(|_| decode("route input mint invalid"))?,
                output_mint: TokenMint::parse(&leg.swap_info.output_mint)
                    .map_err(|_| decode("route output mint invalid"))?,
                in_amount: amount(Some(&leg.swap_info.in_amount), "route inAmount invalid")?,
                out_amount: amount(Some(&leg.swap_info.out_amount), "route outAmount invalid")?,
                percent: leg.percent,
                bps: leg.bps,
            })
        })
        .collect::<Result<Vec<_>, GatewayError>>()?;
    Ok(SwapQuote {
        input_mint: req.input_mint.clone(),
        output_mint: req.output_mint.clone(),
        in_amount: amount(raw.in_amount.as_deref(), "inAmount invalid")?,
        out_amount: amount(raw.out_amount.as_deref(), "outAmount invalid")?,
        other_amount_threshold: amount(
            raw.other_amount_threshold.as_deref(),
            "otherAmountThreshold invalid",
        )?,
        swap_mode: bounded(raw.swap_mode, 16).unwrap_or_else(|| "ExactIn".to_owned()),
        slippage_bps: raw.slippage_bps,
        price_impact_pct: dec(raw.price_impact.as_deref()),
        route,
        fee_bps: raw.fee_bps,
        router: bounded(raw.router, 32),
        request_id: bounded(raw.request_id, 128),
        expires_at: raw
            .expire_at
            .map(|r| r.get().trim_matches('"').chars().take(40).collect()),
    })
}

fn dedup_mints(mints: &[TokenMint]) -> Vec<TokenMint> {
    mints
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl Gateway {
    fn jupiter_url(&self, path: &str) -> Result<Url, GatewayError> {
        self.inner
            .settings
            .jupiter
            .base_url
            .join(path)
            .map_err(|_| GatewayError::InvalidRequest("invalid Jupiter path"))
    }

    async fn jupiter_get<T>(
        &self,
        ctx: &RequestContext,
        operation: &'static str,
        url: Url,
        cache_key: Option<String>,
        ttl: CacheTtl,
        parse: impl Fn(u16, &[u8]) -> Result<T, GatewayError>,
    ) -> Result<GatewayResponse<T>, GatewayError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let call = Call {
            provider: P,
            operation,
            category: operation,
            cache_key,
            ttl,
            credits: 0,
            source_age_known: false,
            fallback: false,
        };
        let key = self.inner.settings.jupiter.api_key.as_ref();
        let transport = &self.inner.transport;
        let r = self
            .engine()
            .execute(ctx, &self.inner.limiters.jupiter, call, |timeout| {
                let url = url.clone();
                let parse = &parse;
                async move {
                    let resp = http_json::get(
                        transport,
                        P,
                        url,
                        key.map(|k| ("x-api-key", k)),
                        timeout,
                        Accept::SuccessOrClientAnswer,
                    )
                    .await?;
                    parse(resp.status, &resp.body).map(Fetched::found)
                }
            })
            .await?;
        r.require(|_| decode("empty response"))
    }

    /// USD prices for up to 50 mints per request (chunked). Mints Jupiter
    /// cannot price are simply absent from the result.
    pub async fn jupiter_prices(
        &self,
        ctx: &RequestContext,
        mints: &[TokenMint],
    ) -> Result<GatewayResponse<Vec<JupiterPrice>>, GatewayError> {
        let mints = dedup_mints(mints);
        if mints.is_empty() || mints.len() > 4 * PRICE_IDS_MAX {
            return Err(GatewayError::InvalidRequest("1..=200 mints per call"));
        }
        let mut all = Vec::new();
        let mut last = None;
        for chunk in mints.chunks(PRICE_IDS_MAX) {
            let ids = chunk
                .iter()
                .map(TokenMint::as_str)
                .collect::<Vec<_>>()
                .join(",");
            let mut url = self.jupiter_url("price/v3")?;
            url.query_pairs_mut().append_pair("ids", &ids);
            let r = self
                .jupiter_get(
                    ctx,
                    op::JUP_PRICE,
                    url,
                    Some(ids.clone()),
                    TTL_PRICE,
                    |status, body| {
                        if status != 200 {
                            return Err(GatewayError::Http {
                                provider: P,
                                status,
                            });
                        }
                        parse_prices(body)
                    },
                )
                .await?;
            all.extend(r.value.iter().cloned());
            last = Some(r);
        }
        let last = last.ok_or(GatewayError::InvalidRequest("no mints"))?;
        Ok(last.map(|_| all))
    }

    /// Token metadata for up to 100 mints per request (chunked).
    pub async fn jupiter_tokens(
        &self,
        ctx: &RequestContext,
        mints: &[TokenMint],
    ) -> Result<GatewayResponse<Vec<TokenInfo>>, GatewayError> {
        let mints = dedup_mints(mints);
        if mints.is_empty() || mints.len() > 4 * TOKENS_SEARCH_MAX {
            return Err(GatewayError::InvalidRequest("1..=400 mints per call"));
        }
        let mut all = Vec::new();
        let mut last = None;
        for chunk in mints.chunks(TOKENS_SEARCH_MAX) {
            let query = chunk
                .iter()
                .map(TokenMint::as_str)
                .collect::<Vec<_>>()
                .join(",");
            let mut url = self.jupiter_url("tokens/v2/search")?;
            url.query_pairs_mut().append_pair("query", &query);
            let r = self
                .jupiter_get(
                    ctx,
                    op::JUP_TOKENS_SEARCH,
                    url,
                    Some(query.clone()),
                    TTL_TOKENS,
                    |status, body| {
                        if status != 200 {
                            return Err(GatewayError::Http {
                                provider: P,
                                status,
                            });
                        }
                        parse_tokens(body)
                    },
                )
                .await?;
            all.extend(r.value.iter().cloned());
            last = Some(r);
        }
        let last = last.ok_or(GatewayError::InvalidRequest("no mints"))?;
        Ok(last.map(|_| all))
    }

    /// A swap quote (information only, never executable). Not cached.
    pub async fn jupiter_quote(
        &self,
        ctx: &RequestContext,
        req: &QuoteRequest,
    ) -> Result<GatewayResponse<SwapQuote>, GatewayError> {
        if !self.inner.settings.jupiter.quote_enabled {
            return Err(GatewayError::Disabled {
                provider: P,
                reason: "JUPITER_QUOTE_ENABLED=false",
            });
        }
        if req.input_mint == req.output_mint {
            return Err(GatewayError::InvalidRequest(
                "input and output mint are the same",
            ));
        }
        if req.amount == 0 {
            return Err(GatewayError::InvalidRequest("amount must be positive"));
        }
        if req.slippage_bps.is_some_and(|b| b > 10_000) {
            return Err(GatewayError::InvalidRequest(
                "slippage_bps must be 0..=10000",
            ));
        }
        let mut url = self.jupiter_url("swap/v2/order")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("inputMint", req.input_mint.as_str())
                .append_pair("outputMint", req.output_mint.as_str())
                .append_pair("amount", &req.amount.to_string());
            if let Some(bps) = req.slippage_bps {
                q.append_pair("slippageBps", &bps.to_string());
            }
            // Deliberately no `taker`: without it Jupiter returns a quote
            // and no transaction.
        }
        let req = req.clone();
        self.jupiter_get(
            ctx,
            op::JUP_QUOTE,
            url,
            None,
            CacheTtl::NONE,
            move |status, body| parse_order(status, body, &req),
        )
        .await
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const SOL: &str = "So11111111111111111111111111111111111111112";
    const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    #[test]
    fn prices_keep_exact_digits_and_skip_unpriced() {
        let body = format!(
            r#"{{"{SOL}":{{"usdPrice":147.12345678901234567,"blockId":348004023,"decimals":9,"priceChange24h":-1.25,"createdAt":"2024-06-05T08:55:25.527Z","liquidity":621679197.67}},
               "{USDC}":null,
               "not-a-mint":{{"usdPrice":1}}}}"#
        );
        let prices = parse_prices(body.as_bytes()).unwrap();
        assert_eq!(prices.len(), 1);
        assert_eq!(
            prices[0].usd_price.get().to_string(),
            "147.12345678901234567"
        );
        assert_eq!(prices[0].price_change_24h_pct.unwrap().to_string(), "-1.25");
        assert_eq!(prices[0].block_id, Some(348_004_023));
    }

    #[test]
    fn negative_prices_are_rejected() {
        let body = format!(r#"{{"{SOL}":{{"usdPrice":-1}}}}"#);
        assert!(parse_prices(body.as_bytes()).unwrap().is_empty());
    }

    fn req() -> QuoteRequest {
        QuoteRequest {
            input_mint: TokenMint::parse(SOL).unwrap(),
            output_mint: TokenMint::parse(USDC).unwrap(),
            amount: 1_000_000_000,
            slippage_bps: Some(50),
        }
    }

    #[test]
    fn quote_without_transaction_parses() {
        let body = format!(
            r#"{{"inputMint":"{SOL}","outputMint":"{USDC}","inAmount":"1000000000","outAmount":"147120000","otherAmountThreshold":"146384400",
                "swapMode":"ExactIn","slippageBps":50,"priceImpact":-0.0012,"feeBps":2,"router":"iris","requestId":"r-1","transaction":null,
                "routePlan":[{{"swapInfo":{{"ammKey":"AMM1","label":"Pool","inputMint":"{SOL}","outputMint":"{USDC}","inAmount":"1000000000","outAmount":"147120000"}},"percent":100,"bps":10000}}]}}"#
        );
        let q = parse_order(200, body.as_bytes(), &req()).unwrap();
        assert_eq!(q.out_amount, 147_120_000);
        assert_eq!(q.route.len(), 1);
        assert_eq!(q.price_impact_pct.unwrap().to_string(), "-0.0012");
    }

    #[test]
    fn quote_with_a_transaction_is_refused() {
        let body = format!(
            r#"{{"inputMint":"{SOL}","outputMint":"{USDC}","inAmount":"1","outAmount":"1","otherAmountThreshold":"1","transaction":"AQAB..."}}"#
        );
        assert!(matches!(
            parse_order(200, body.as_bytes(), &req()),
            Err(GatewayError::Prohibited(_))
        ));
    }

    #[test]
    fn no_route_and_mismatched_quotes() {
        assert_eq!(
            parse_order(
                400,
                br#"{"requestId":"x","error":"No routes found"}"#,
                &req()
            ),
            Err(GatewayError::NoRoute)
        );
        let body = format!(
            r#"{{"inputMint":"{USDC}","outputMint":"{SOL}","inAmount":"1","outAmount":"1","otherAmountThreshold":"1"}}"#
        );
        assert!(matches!(
            parse_order(200, body.as_bytes(), &req()),
            Err(GatewayError::Decode { .. })
        ));
    }

    #[test]
    fn tokens_are_bounded_and_validated() {
        let body = format!(
            r#"[{{"id":"{SOL}","name":"Wrapped SOL","symbol":"SOL","decimals":9,"isVerified":true,"tags":["verified"],"organicScore":98.5,"usdPrice":147.1,"holderCount":100,"updatedAt":"2026-11-26T00:00:00Z"}},
                {{"id":"bad","name":"x"}}]"#
        );
        let t = parse_tokens(body.as_bytes()).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].organic_score.unwrap().to_string(), "98.5");
        assert!(t[0].updated_at.is_some());
    }
}
