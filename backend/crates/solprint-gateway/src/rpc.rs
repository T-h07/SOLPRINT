//! Solana JSON-RPC methods (Helius primary, public RPC fallback).
//!
//! Fallback rules: the public endpoint answers only when the caller set
//! `allow_fallback`, the method has identical semantics there (all methods
//! here are standard Solana RPC), the deadline still has room, and Helius
//! failed for an availability reason (disabled, circuit open, budget
//! refused, connection/timeout/5xx/rate-limited). A Helius *answer* (even
//! "not found" or an RPC error) is never second-guessed by the fallback.

use std::time::Duration;

use serde_json::{Value, json};
use solprint_core::{
    CommitmentLevel, Lamports, TokenAmount, TokenMint, TransactionSignature, UtcTimestamp,
    WalletAddress,
};
use time::OffsetDateTime;

use crate::cache::CacheTtl;
use crate::context::{GatewayResponse, ProviderId, RequestContext};
use crate::engine::{Call, Fetched};
use crate::error::GatewayError;
use crate::gateway::Gateway;
use crate::jsonrpc::{self, RpcEndpoint};
use crate::models::{
    AccountData, BlockSummary, LargestTokenAccount, NodeHealth, NodeVersion, SignatureInfo,
    SignaturePage, SignatureQuery, TokenAccount, TokenAccountFilter, TransactionRecord,
};
use crate::numbers::u64_from_value;
use crate::operations as op;

const MIN_FALLBACK_BUDGET: Duration = Duration::from_millis(500);
/// getMultipleAccounts: documented maximum keys per call.
pub const MULTIPLE_ACCOUNTS_MAX: usize = 100;
/// JSON-RPC batch size used for getTransaction (Helius documents 100; a
/// smaller batch bounds response size).
pub const TRANSACTION_BATCH_MAX: usize = 20;

const fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}
const fn millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

const TTL_ACCOUNT: CacheTtl = CacheTtl::new(secs(2), secs(30), secs(5));
const TTL_VERSION: CacheTtl = CacheTtl::new(secs(600), secs(3_600), Duration::ZERO);
const TTL_SLOT: CacheTtl = CacheTtl::new(millis(400), secs(2), Duration::ZERO);
const TTL_BLOCK_TIME: CacheTtl = CacheTtl::new(secs(3_600), secs(86_400), secs(10));
const TTL_TX_FINAL: CacheTtl = CacheTtl::new(secs(86_400), secs(86_400), secs(2));
const TTL_TX_CONFIRMED: CacheTtl = CacheTtl::new(secs(30), secs(120), secs(2));
const TTL_SUPPLY: CacheTtl = CacheTtl::new(secs(30), secs(300), secs(30));

pub const fn commitment_str(c: CommitmentLevel) -> &'static str {
    match c {
        CommitmentLevel::Processed => "processed",
        CommitmentLevel::Confirmed => "confirmed",
        CommitmentLevel::Finalized => "finalized",
    }
}

fn decode(provider: ProviderId, detail: &'static str) -> GatewayError {
    GatewayError::Decode { provider, detail }
}

fn ts_from_unix(v: &Value) -> Option<UtcTimestamp> {
    v.as_i64()
        .and_then(|s| OffsetDateTime::from_unix_timestamp(s).ok())
        .map(UtcTimestamp::from_offset_datetime)
}

/// Errors after which the fallback may be tried.
const fn fallback_eligible(e: &GatewayError) -> bool {
    matches!(
        e,
        GatewayError::Disabled { .. }
            | GatewayError::CircuitOpen { .. }
            | GatewayError::BudgetDenied { .. }
            | GatewayError::Connect { .. }
            | GatewayError::Timeout { .. }
            | GatewayError::RateLimited { .. }
            | GatewayError::QueueFull { .. }
    ) || matches!(e, GatewayError::Http { status, .. } if *status >= 500)
}

type Parser<T> = fn(ProviderId, Value) -> Result<Fetched<T>, GatewayError>;

impl Gateway {
    fn endpoint(&self, p: ProviderId) -> Result<&RpcEndpoint, GatewayError> {
        let ep = match p {
            ProviderId::Helius => self.inner.helius.as_ref(),
            _ => self.inner.fallback.as_ref(),
        };
        ep.ok_or(GatewayError::Disabled {
            provider: p,
            reason: "endpoint not configured",
        })
    }

    /// Run one RPC method with caching, budget and fallback.
    async fn rpc<T>(
        &self,
        ctx: &RequestContext,
        method: &'static str,
        params: Value,
        ttl: CacheTtl,
        parse: Parser<T>,
    ) -> Result<GatewayResponse<Option<T>>, GatewayError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let credits = op::helius_credits(method)
            .ok_or(GatewayError::Prohibited("RPC method is not registered"))?;
        let key = (!ttl.keep_for.is_zero() || !ttl.negative_for.is_zero())
            .then(|| format!("{}|{}", commitment_str(ctx.commitment), params));
        let primary = self
            .rpc_on(
                ctx,
                ProviderId::Helius,
                method,
                &params,
                key.clone(),
                ttl,
                credits,
                parse,
            )
            .await;
        match primary {
            Err(e)
                if ctx.allow_fallback
                    && fallback_eligible(&e)
                    && self.inner.fallback.is_some()
                    && ctx.remaining() >= MIN_FALLBACK_BUDGET =>
            {
                tracing::debug!(method, reason = e.code(), "using public RPC fallback");
                self.engine().metrics.note_fallback(method);
                self.rpc_on(
                    ctx,
                    ProviderId::SolanaRpc,
                    method,
                    &params,
                    key,
                    ttl,
                    0,
                    parse,
                )
                .await
            }
            other => other,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn rpc_on<T>(
        &self,
        ctx: &RequestContext,
        provider: ProviderId,
        method: &'static str,
        params: &Value,
        cache_key: Option<String>,
        ttl: CacheTtl,
        credits: u64,
        parse: Parser<T>,
    ) -> Result<GatewayResponse<Option<T>>, GatewayError>
    where
        T: Clone + Send + Sync + 'static,
    {
        let endpoint = self.endpoint(provider).cloned();
        let limiter = match provider {
            ProviderId::Helius => &self.inner.limiters.helius,
            _ => &self.inner.limiters.solana,
        };
        let call = Call {
            provider,
            operation: method,
            category: method,
            cache_key,
            ttl,
            credits: if provider == ProviderId::Helius {
                credits
            } else {
                0
            },
            source_age_known: true,
            fallback: provider == ProviderId::SolanaRpc,
        };
        let transport = &self.inner.transport;
        let ids = &self.inner.ids;
        self.engine()
            .execute(ctx, limiter, call, |timeout| {
                let endpoint = endpoint.clone();
                async move {
                    let endpoint = endpoint?;
                    let result =
                        jsonrpc::call(transport, ids, &endpoint, method, params.clone(), timeout)
                            .await?;
                    parse(provider, result)
                }
            })
            .await
    }

    fn require<T>(
        r: GatewayResponse<Option<T>>,
        detail: &'static str,
    ) -> Result<GatewayResponse<T>, GatewayError> {
        r.require(|provider| decode(provider, detail))
    }

    // ---- methods ----------------------------------------------------------

    /// `getHealth` (never cached).
    pub async fn get_health(
        &self,
        ctx: &RequestContext,
    ) -> Result<GatewayResponse<NodeHealth>, GatewayError> {
        let r = self
            .rpc(ctx, op::GET_HEALTH, json!([]), CacheTtl::NONE, |p, v| {
                if v.as_str() == Some("ok") {
                    Ok(Fetched::found(NodeHealth::Ok))
                } else {
                    Err(decode(p, "unexpected getHealth result"))
                }
            })
            .await;
        match r {
            Ok(r) => Self::require(r, "getHealth returned null"),
            // A node that is behind answers with -32005 and numSlotsBehind.
            Err(GatewayError::Rpc {
                provider,
                code: -32005,
                ..
            }) => Ok(GatewayResponse {
                value: NodeHealth::Behind { slots: None },
                provider,
                provenance: crate::context::Provenance::Direct,
                freshness: crate::context::Freshness::Fresh,
                observed_at: UtcTimestamp::now(),
                source_timestamp: None,
                context_slot: None,
                age: Duration::ZERO,
                correlation_id: ctx.correlation_id,
                credits_charged: u64::from(provider == ProviderId::Helius),
            }),
            Err(e) => Err(e),
        }
    }

    pub async fn get_version(
        &self,
        ctx: &RequestContext,
    ) -> Result<GatewayResponse<NodeVersion>, GatewayError> {
        let r = self
            .rpc(ctx, op::GET_VERSION, json!([]), TTL_VERSION, |p, v| {
                let core = v
                    .get("solana-core")
                    .and_then(Value::as_str)
                    .ok_or_else(|| decode(p, "getVersion missing solana-core"))?;
                Ok(Fetched::found(NodeVersion {
                    solana_core: core.chars().take(32).collect(),
                    feature_set: v
                        .get("feature-set")
                        .and_then(Value::as_u64)
                        .and_then(|f| u32::try_from(f).ok()),
                }))
            })
            .await?;
        Self::require(r, "getVersion returned null")
    }

    pub async fn get_slot(
        &self,
        ctx: &RequestContext,
    ) -> Result<GatewayResponse<u64>, GatewayError> {
        let params = json!([{"commitment": commitment_str(ctx.commitment)}]);
        let r = self
            .rpc(ctx, op::GET_SLOT, params, TTL_SLOT, |p, v| {
                let slot = v
                    .as_u64()
                    .ok_or_else(|| decode(p, "getSlot result is not a u64"))?;
                Ok(Fetched::found(slot).at_slot(Some(slot)))
            })
            .await?;
        Self::require(r, "getSlot returned null")
    }

    /// `getSlot` sent directly to the public RPC (diagnostics only; never
    /// cached, costs no Helius credits).
    pub async fn fallback_get_slot(
        &self,
        ctx: &RequestContext,
    ) -> Result<GatewayResponse<u64>, GatewayError> {
        let params = json!([{"commitment": commitment_str(ctx.commitment)}]);
        let r = self
            .rpc_on(
                ctx,
                ProviderId::SolanaRpc,
                op::GET_SLOT,
                &params,
                None,
                CacheTtl::NONE,
                0,
                |p, v| {
                    let slot = v
                        .as_u64()
                        .ok_or_else(|| decode(p, "getSlot result is not a u64"))?;
                    Ok(Fetched::found(slot).at_slot(Some(slot)))
                },
            )
            .await?;
        Self::require(r, "getSlot returned null")
    }

    /// `None` when the node has no block time for the slot.
    pub async fn get_block_time(
        &self,
        ctx: &RequestContext,
        slot: u64,
    ) -> Result<GatewayResponse<Option<UtcTimestamp>>, GatewayError> {
        self.rpc(
            ctx,
            op::GET_BLOCK_TIME,
            json!([slot]),
            TTL_BLOCK_TIME,
            |p, v| {
                if v.is_null() {
                    return Ok(Fetched::not_found());
                }
                let ts =
                    ts_from_unix(&v).ok_or_else(|| decode(p, "getBlockTime result invalid"))?;
                Ok(Fetched::found(ts).with_source_time(Some(ts)))
            },
        )
        .await
    }

    pub async fn get_balance(
        &self,
        ctx: &RequestContext,
        address: &WalletAddress,
    ) -> Result<GatewayResponse<Lamports>, GatewayError> {
        let params = json!([address.as_str(), {"commitment": commitment_str(ctx.commitment)}]);
        let r = self
            .rpc(ctx, op::GET_BALANCE, params, TTL_ACCOUNT, |p, v| {
                let (slot, value) = jsonrpc::split_context(v);
                let lamports = value
                    .as_u64()
                    .ok_or_else(|| decode(p, "getBalance value is not a u64"))?;
                Ok(Fetched::found(Lamports::new(lamports)).at_slot(slot))
            })
            .await?;
        Self::require(r, "getBalance returned null")
    }

    /// `None` when the account does not exist.
    pub async fn get_account_info(
        &self,
        ctx: &RequestContext,
        address: &WalletAddress,
    ) -> Result<GatewayResponse<Option<AccountData>>, GatewayError> {
        let params = json!([address.as_str(), {
            "commitment": commitment_str(ctx.commitment),
            "encoding": "base64",
        }]);
        self.rpc(ctx, op::GET_ACCOUNT_INFO, params, TTL_ACCOUNT, |p, v| {
            let (slot, value) = jsonrpc::split_context(v);
            if value.is_null() {
                return Ok(Fetched::not_found().at_slot(slot));
            }
            Ok(Fetched::found(parse_account(p, &value)?).at_slot(slot))
        })
        .await
    }

    /// Accounts in request order; chunks of 100 keys (one request each).
    pub async fn get_multiple_accounts(
        &self,
        ctx: &RequestContext,
        addresses: &[WalletAddress],
    ) -> Result<GatewayResponse<Vec<Option<AccountData>>>, GatewayError> {
        if addresses.is_empty() {
            return Err(GatewayError::InvalidRequest("no addresses"));
        }
        if addresses.len() > 10 * MULTIPLE_ACCOUNTS_MAX {
            return Err(GatewayError::InvalidRequest(
                "at most 1000 addresses per call",
            ));
        }
        let mut out: Vec<Option<AccountData>> = Vec::with_capacity(addresses.len());
        let mut last: Option<GatewayResponse<Option<Vec<Option<AccountData>>>>> = None;
        let mut credits = 0;
        for chunk in addresses.chunks(MULTIPLE_ACCOUNTS_MAX) {
            let keys: Vec<&str> = chunk.iter().map(WalletAddress::as_str).collect();
            let params = json!([keys, {
                "commitment": commitment_str(ctx.commitment),
                "encoding": "base64",
            }]);
            let r = self
                .rpc(
                    ctx,
                    op::GET_MULTIPLE_ACCOUNTS,
                    params,
                    TTL_ACCOUNT,
                    |p, v| {
                        let (slot, value) = jsonrpc::split_context(v);
                        let items = value.as_array().ok_or_else(|| {
                            decode(p, "getMultipleAccounts value is not an array")
                        })?;
                        let parsed = items
                            .iter()
                            .map(|a| {
                                if a.is_null() {
                                    Ok(None)
                                } else {
                                    parse_account(p, a).map(Some)
                                }
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(Fetched::found(parsed).at_slot(slot))
                    },
                )
                .await?;
            let got = r.value.clone().unwrap_or_default();
            if got.len() != chunk.len() {
                return Err(decode(r.provider, "getMultipleAccounts length mismatch"));
            }
            credits += r.credits_charged;
            out.extend(got);
            last = Some(r);
        }
        let Some(last) = last else {
            return Err(GatewayError::InvalidRequest("no addresses"));
        };
        let mut resp = last.map(|_| out);
        resp.credits_charged = credits;
        Ok(resp)
    }

    /// Bounded incremental pagination (newest first).
    pub async fn get_signatures_for_address(
        &self,
        ctx: &RequestContext,
        query: &SignatureQuery,
    ) -> Result<GatewayResponse<SignaturePage>, GatewayError> {
        if query.page_limit == 0 || query.page_limit > SignatureQuery::MAX_PAGE_LIMIT {
            return Err(GatewayError::InvalidRequest("page_limit must be 1..=1000"));
        }
        if query.max_pages == 0 || query.max_pages > SignatureQuery::MAX_PAGES {
            return Err(GatewayError::InvalidRequest("max_pages must be 1..=10"));
        }
        if ctx.commitment == CommitmentLevel::Processed {
            return Err(GatewayError::InvalidRequest(
                "getSignaturesForAddress requires confirmed or finalized",
            ));
        }
        let mut items = Vec::new();
        let mut before = query.before.clone();
        let mut pages = 0_u8;
        let mut complete = false;
        let mut last = None;
        let mut credits = 0;
        while pages < query.max_pages {
            let mut cfg = serde_json::Map::new();
            cfg.insert("limit".into(), json!(query.page_limit));
            cfg.insert("commitment".into(), json!(commitment_str(ctx.commitment)));
            if let Some(b) = &before {
                cfg.insert("before".into(), json!(b.as_str()));
            }
            if let Some(u) = &query.until {
                cfg.insert("until".into(), json!(u.as_str()));
            }
            let params = json!([query.address.as_str(), cfg]);
            let r = self
                .rpc(
                    ctx,
                    op::GET_SIGNATURES_FOR_ADDRESS,
                    params,
                    CacheTtl::NONE,
                    |p, v| {
                        let arr = v.as_array().ok_or_else(|| {
                            decode(p, "getSignaturesForAddress result is not an array")
                        })?;
                        arr.iter()
                            .map(|s| parse_signature_info(p, s))
                            .collect::<Result<Vec<_>, _>>()
                            .map(Fetched::found)
                    },
                )
                .await?;
            pages += 1;
            credits += r.credits_charged;
            let page = r.value.clone().unwrap_or_default();
            let n = page.len();
            before = page.last().map(|s| s.signature.clone());
            items.extend(page);
            last = Some(r);
            if n < usize::from(query.page_limit) {
                complete = true;
                break;
            }
        }
        let Some(last) = last else {
            return Err(GatewayError::InvalidRequest("no pages fetched"));
        };
        let mut resp = last.map(|_| SignaturePage {
            items,
            complete,
            next_before: if complete { None } else { before },
            pages_fetched: pages,
        });
        resp.credits_charged = credits;
        Ok(resp)
    }

    /// `None` when the transaction is unknown or not yet at the requested
    /// commitment. Requires `confirmed` or `finalized`.
    pub async fn get_transaction(
        &self,
        ctx: &RequestContext,
        signature: &TransactionSignature,
    ) -> Result<GatewayResponse<Option<TransactionRecord>>, GatewayError> {
        if ctx.commitment == CommitmentLevel::Processed {
            return Err(GatewayError::InvalidRequest(
                "getTransaction does not accept processed",
            ));
        }
        let ttl = if ctx.commitment == CommitmentLevel::Finalized {
            TTL_TX_FINAL
        } else {
            TTL_TX_CONFIRMED
        };
        let params = json!([signature.as_str(), {
            "commitment": commitment_str(ctx.commitment),
            "encoding": "json",
            "maxSupportedTransactionVersion": 0,
        }]);
        self.rpc(ctx, op::GET_TRANSACTION, params, ttl, |p, v| {
            if v.is_null() {
                return Ok(Fetched::not_found());
            }
            let rec = parse_transaction(p, &v)?;
            let ts = rec.block_time;
            let slot = rec.slot;
            Ok(Fetched::found(rec).at_slot(Some(slot)).with_source_time(ts))
        })
        .await
    }

    /// Several transactions via JSON-RPC batches of [`TRANSACTION_BATCH_MAX`].
    /// Each item costs one credit (batch accounting is undocumented, so every
    /// item is charged). Not cached; items keep request order.
    pub async fn get_transactions(
        &self,
        ctx: &RequestContext,
        signatures: &[TransactionSignature],
    ) -> Result<GatewayResponse<Vec<Result<Option<TransactionRecord>, GatewayError>>>, GatewayError>
    {
        if ctx.commitment == CommitmentLevel::Processed {
            return Err(GatewayError::InvalidRequest(
                "getTransaction does not accept processed",
            ));
        }
        if signatures.is_empty() || signatures.len() > 200 {
            return Err(GatewayError::InvalidRequest("1..=200 signatures per call"));
        }
        let mut out = Vec::with_capacity(signatures.len());
        let mut last = None;
        let mut credits = 0;
        for chunk in signatures.chunks(TRANSACTION_BATCH_MAX) {
            let params: Vec<Value> = chunk
                .iter()
                .map(|s| {
                    json!([s.as_str(), {
                        "commitment": commitment_str(ctx.commitment),
                        "encoding": "json",
                        "maxSupportedTransactionVersion": 0,
                    }])
                })
                .collect();
            let n = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            let endpoint = self.endpoint(ProviderId::Helius).cloned();
            let call = Call {
                provider: ProviderId::Helius,
                operation: op::GET_TRANSACTION,
                category: op::GET_TRANSACTION,
                cache_key: None,
                ttl: CacheTtl::NONE,
                credits: n,
                source_age_known: true,
                fallback: false,
            };
            let transport = &self.inner.transport;
            let ids = &self.inner.ids;
            let r = self
                .engine()
                .execute(ctx, &self.inner.limiters.helius, call, |timeout| {
                    let endpoint = endpoint.clone();
                    let params = params.clone();
                    async move {
                        let endpoint = endpoint?;
                        let results = jsonrpc::batch(
                            transport,
                            ids,
                            &endpoint,
                            op::GET_TRANSACTION,
                            params,
                            timeout,
                        )
                        .await?;
                        let parsed: Vec<Result<Option<TransactionRecord>, GatewayError>> = results
                            .into_iter()
                            .map(|r| {
                                r.and_then(|v| {
                                    if v.is_null() {
                                        Ok(None)
                                    } else {
                                        parse_transaction(ProviderId::Helius, &v).map(Some)
                                    }
                                })
                            })
                            .collect();
                        Ok(Fetched::found(parsed))
                    }
                })
                .await?;
            credits += r.credits_charged;
            out.extend(r.value.clone().unwrap_or_default());
            last = Some(r);
        }
        let Some(last) = last else {
            return Err(GatewayError::InvalidRequest("no signatures"));
        };
        let mut resp = last.map(|_| out);
        resp.credits_charged = credits;
        Ok(resp)
    }

    pub async fn get_token_account_balance(
        &self,
        ctx: &RequestContext,
        token_account: &WalletAddress,
    ) -> Result<GatewayResponse<Option<TokenAmount>>, GatewayError> {
        let params =
            json!([token_account.as_str(), {"commitment": commitment_str(ctx.commitment)}]);
        self.rpc(
            ctx,
            op::GET_TOKEN_ACCOUNT_BALANCE,
            params,
            TTL_ACCOUNT,
            |p, v| {
                let (slot, value) = jsonrpc::split_context(v);
                if value.is_null() {
                    return Ok(Fetched::not_found().at_slot(slot));
                }
                Ok(Fetched::found(parse_ui_token_amount(p, &value)?).at_slot(slot))
            },
        )
        .await
    }

    /// SPL token accounts of `owner`, filtered by mint or by token program.
    pub async fn get_token_accounts_by_owner(
        &self,
        ctx: &RequestContext,
        owner: &WalletAddress,
        filter: TokenAccountFilter,
        filter_address: &WalletAddress,
    ) -> Result<GatewayResponse<Vec<TokenAccount>>, GatewayError> {
        let f = match filter {
            TokenAccountFilter::Mint => json!({"mint": filter_address.as_str()}),
            TokenAccountFilter::Program => json!({"programId": filter_address.as_str()}),
        };
        let params = json!([owner.as_str(), f, {
            "commitment": commitment_str(ctx.commitment),
            "encoding": "jsonParsed",
        }]);
        let r = self
            .rpc(
                ctx,
                op::GET_TOKEN_ACCOUNTS_BY_OWNER,
                params,
                TTL_ACCOUNT,
                |p, v| {
                    let (slot, value) = jsonrpc::split_context(v);
                    let arr = value.as_array().ok_or_else(|| {
                        decode(p, "getTokenAccountsByOwner value is not an array")
                    })?;
                    let accounts = arr
                        .iter()
                        .map(|a| parse_token_account(p, a))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Fetched::found(accounts).at_slot(slot))
                },
            )
            .await?;
        Self::require(r, "getTokenAccountsByOwner returned null")
    }

    pub async fn get_token_supply(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> Result<GatewayResponse<Option<TokenAmount>>, GatewayError> {
        let params = json!([mint.as_str(), {"commitment": commitment_str(ctx.commitment)}]);
        self.rpc(ctx, op::GET_TOKEN_SUPPLY, params, TTL_SUPPLY, |p, v| {
            let (slot, value) = jsonrpc::split_context(v);
            if value.is_null() {
                return Ok(Fetched::not_found().at_slot(slot));
            }
            Ok(Fetched::found(parse_ui_token_amount(p, &value)?).at_slot(slot))
        })
        .await
    }

    pub async fn get_token_largest_accounts(
        &self,
        ctx: &RequestContext,
        mint: &TokenMint,
    ) -> Result<GatewayResponse<Vec<LargestTokenAccount>>, GatewayError> {
        let params = json!([mint.as_str(), {"commitment": commitment_str(ctx.commitment)}]);
        let r = self
            .rpc(
                ctx,
                op::GET_TOKEN_LARGEST_ACCOUNTS,
                params,
                TTL_SUPPLY,
                |p, v| {
                    let (slot, value) = jsonrpc::split_context(v);
                    let arr = value.as_array().ok_or_else(|| {
                        decode(p, "getTokenLargestAccounts value is not an array")
                    })?;
                    let items = arr
                        .iter()
                        .map(|a| {
                            let address = a
                                .get("address")
                                .and_then(Value::as_str)
                                .and_then(|s| WalletAddress::parse(s).ok())
                                .ok_or_else(|| decode(p, "invalid token account address"))?;
                            Ok(LargestTokenAccount {
                                address,
                                amount: parse_ui_token_amount(p, a)?,
                            })
                        })
                        .collect::<Result<Vec<_>, GatewayError>>()?;
                    Ok(Fetched::found(items).at_slot(slot))
                },
            )
            .await?;
        Self::require(r, "getTokenLargestAccounts returned null")
    }

    /// `getBlock` with signatures only and no rewards (bounded size).
    /// Disabled unless `HELIUS_GET_BLOCK_ENABLED=true`; never polled.
    pub async fn get_block(
        &self,
        ctx: &RequestContext,
        slot: u64,
    ) -> Result<GatewayResponse<Option<BlockSummary>>, GatewayError> {
        if !self.inner.settings.helius.get_block_enabled {
            return Err(GatewayError::Disabled {
                provider: ProviderId::Helius,
                reason: "HELIUS_GET_BLOCK_ENABLED=false",
            });
        }
        if ctx.commitment == CommitmentLevel::Processed {
            return Err(GatewayError::InvalidRequest(
                "getBlock does not accept processed",
            ));
        }
        let ttl = if ctx.commitment == CommitmentLevel::Finalized {
            TTL_TX_FINAL
        } else {
            TTL_TX_CONFIRMED
        };
        let params = json!([slot, {
            "commitment": commitment_str(ctx.commitment),
            "encoding": "json",
            "transactionDetails": "signatures",
            "rewards": false,
            "maxSupportedTransactionVersion": 0,
        }]);
        self.rpc(ctx, op::GET_BLOCK, params, ttl, move |p, v| {
            if v.is_null() {
                return Ok(Fetched::not_found());
            }
            let signatures = v
                .get("signatures")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter_map(|s| TransactionSignature::parse(s).ok())
                        .collect()
                })
                .unwrap_or_default();
            let block_time = v.get("blockTime").and_then(ts_from_unix);
            let summary = BlockSummary {
                slot: 0,
                blockhash: v
                    .get("blockhash")
                    .and_then(Value::as_str)
                    .ok_or_else(|| decode(p, "getBlock missing blockhash"))?
                    .chars()
                    .take(64)
                    .collect(),
                parent_slot: v
                    .get("parentSlot")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| decode(p, "getBlock missing parentSlot"))?,
                block_time,
                block_height: v.get("blockHeight").and_then(Value::as_u64),
                signatures,
            };
            Ok(Fetched::found(summary).with_source_time(block_time))
        })
        .await
        .map(|mut r| {
            if let Some(b) = r.value.as_mut() {
                b.slot = slot;
            }
            r.context_slot = Some(slot);
            r
        })
    }
}

pub(crate) fn parse_account(p: ProviderId, v: &Value) -> Result<AccountData, GatewayError> {
    let lamports = v
        .get("lamports")
        .and_then(Value::as_u64)
        .ok_or_else(|| decode(p, "account lamports missing"))?;
    let owner = v
        .get("owner")
        .and_then(Value::as_str)
        .and_then(|s| WalletAddress::parse(s).ok())
        .ok_or_else(|| decode(p, "account owner invalid"))?;
    let data = v
        .get("data")
        .and_then(Value::as_array)
        .and_then(|a| match (a.first(), a.get(1)) {
            (Some(Value::String(d)), Some(Value::String(enc))) if enc == "base64" => {
                Some(d.clone())
            }
            _ => None,
        })
        .ok_or_else(|| decode(p, "account data is not base64"))?;
    Ok(AccountData {
        lamports: Lamports::new(lamports),
        owner,
        data_base64: data,
        executable: v
            .get("executable")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        rent_epoch: v.get("rentEpoch").and_then(Value::as_u64).unwrap_or(0),
        space: v.get("space").and_then(Value::as_u64),
    })
}

pub(crate) fn parse_signature_info(
    p: ProviderId,
    v: &Value,
) -> Result<SignatureInfo, GatewayError> {
    let signature = v
        .get("signature")
        .and_then(Value::as_str)
        .and_then(|s| TransactionSignature::parse(s).ok())
        .ok_or_else(|| decode(p, "signature invalid"))?;
    let slot = v
        .get("slot")
        .and_then(Value::as_u64)
        .ok_or_else(|| decode(p, "signature slot missing"))?;
    let err = v.get("err").filter(|e| !e.is_null());
    let confirmation_status = match v.get("confirmationStatus").and_then(Value::as_str) {
        Some("processed") => Some(CommitmentLevel::Processed),
        Some("confirmed") => Some(CommitmentLevel::Confirmed),
        Some("finalized") => Some(CommitmentLevel::Finalized),
        _ => None,
    };
    Ok(SignatureInfo {
        signature,
        slot,
        block_time: v.get("blockTime").and_then(ts_from_unix),
        failed: err.is_some(),
        err_json: err.map(|e| {
            let s = e.to_string();
            s.chars().take(512).collect()
        }),
        memo: v
            .get("memo")
            .and_then(Value::as_str)
            .map(|m| m.chars().take(256).collect()),
        confirmation_status,
    })
}

pub(crate) fn parse_transaction(
    p: ProviderId,
    v: &Value,
) -> Result<TransactionRecord, GatewayError> {
    let slot = v
        .get("slot")
        .and_then(Value::as_u64)
        .ok_or_else(|| decode(p, "transaction slot missing"))?;
    let signature = v
        .get("transaction")
        .and_then(|t| t.get("signatures"))
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(Value::as_str)
        .and_then(|s| TransactionSignature::parse(s).ok())
        .ok_or_else(|| decode(p, "transaction signature missing"))?;
    let version = match v.get("version") {
        Some(Value::String(s)) => Some(s.chars().take(16).collect()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };
    let failed = v
        .get("meta")
        .and_then(|m| m.get("err"))
        .is_some_and(|e| !e.is_null());
    Ok(TransactionRecord {
        signature,
        slot,
        block_time: v.get("blockTime").and_then(ts_from_unix),
        version,
        failed,
        raw_json: v.to_string(),
    })
}

/// `{amount: "…", decimals: n, uiAmountString: "…"}` → exact `TokenAmount`.
/// `uiAmount` (a float) is ignored on purpose.
pub(crate) fn parse_ui_token_amount(p: ProviderId, v: &Value) -> Result<TokenAmount, GatewayError> {
    let raw = v
        .get("amount")
        .and_then(u64_from_value)
        .ok_or_else(|| decode(p, "token amount missing"))?;
    let decimals = v
        .get("decimals")
        .and_then(Value::as_u64)
        .and_then(|d| u8::try_from(d).ok())
        .ok_or_else(|| decode(p, "token decimals missing"))?;
    TokenAmount::new(raw, decimals).map_err(|_| decode(p, "token decimals unsupported"))
}

fn parse_token_account(p: ProviderId, v: &Value) -> Result<TokenAccount, GatewayError> {
    let address = v
        .get("pubkey")
        .and_then(Value::as_str)
        .and_then(|s| WalletAddress::parse(s).ok())
        .ok_or_else(|| decode(p, "token account pubkey invalid"))?;
    let info = v
        .get("account")
        .and_then(|a| a.get("data"))
        .and_then(|d| d.get("parsed"))
        .and_then(|d| d.get("info"))
        .ok_or_else(|| decode(p, "token account is not jsonParsed"))?;
    let mint = info
        .get("mint")
        .and_then(Value::as_str)
        .and_then(|s| TokenMint::parse(s).ok())
        .ok_or_else(|| decode(p, "token account mint invalid"))?;
    let owner = info
        .get("owner")
        .and_then(Value::as_str)
        .and_then(|s| WalletAddress::parse(s).ok())
        .ok_or_else(|| decode(p, "token account owner invalid"))?;
    let amount = info
        .get("tokenAmount")
        .ok_or_else(|| decode(p, "token account amount missing"))
        .and_then(|a| parse_ui_token_amount(p, a))?;
    Ok(TokenAccount {
        address,
        mint,
        owner,
        amount,
        state: info
            .get("state")
            .and_then(Value::as_str)
            .map(|s| s.chars().take(32).collect()),
    })
}
