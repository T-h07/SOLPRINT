//! Market discovery inputs. Production uses [`GatewayFeed`]: every provider
//! request goes through GATEWAY's typed capability traits (budget admission,
//! rate limits, priorities, provenance). Tests and the fixture CLI use
//! [`FixtureFeed`], so automated tests never contact a provider.
//!
//! Listings name **tokens and pools, never wallets**: RADAR finds wallets
//! only from swaps the DATA ENGINE verified.

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rust_decimal::Decimal;
use solprint_core::{TokenMint, UtcTimestamp, WalletAddress};
use solprint_gateway::models::{DexPair, TokenCategory, TokenInfo, TokenInterval};
use solprint_gateway::{
    BlockchainProvider, Gateway, GatewayError, MarketDataProvider, MarketDiscoveryProvider,
    Priority, RequestContext,
};

use crate::budget::BudgetView;
use crate::model::source;

pub const ORIGIN: &str = "radar";
const TIMEOUT: Duration = Duration::from_secs(20);
/// DEX Screener `tokens/v1` accepts up to 30 mints per request.
pub const PAIRS_CHUNK: usize = 30;
/// `getMultipleAccounts` accepts up to 100 accounts per request.
pub const ACCOUNTS_CHUNK: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    JupiterTopTraded,
    JupiterTopOrganic,
    JupiterRecent,
    /// Paid promotion; enabled only with `DEXSCREENER_DISCOVERY_ENABLED`.
    DexBoosts,
}

impl Listing {
    #[must_use]
    pub const fn source(self) -> &'static str {
        match self {
            Self::JupiterTopTraded => source::JUPITER_TOP_TRADED,
            Self::JupiterTopOrganic => source::JUPITER_TOP_ORGANIC,
            Self::JupiterRecent => source::JUPITER_RECENT,
            Self::DexBoosts => source::DEXSCREENER_BOOSTS,
        }
    }

    #[must_use]
    pub const fn promotional(self) -> bool {
        matches!(self, Self::DexBoosts)
    }
}

/// A token named by a listing (provider observation).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListedToken {
    pub mint: String,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub token_program: Option<String>,
    pub verified: Option<bool>,
    pub tags: Vec<String>,
    pub organic_score: Option<Decimal>,
    pub launchpad: Option<String>,
    pub first_pool_created_at: Option<UtcTimestamp>,
    pub created_at: Option<UtcTimestamp>,
    /// Reported 1-hour buys + sells, when both are published.
    pub txns_1h: Option<u64>,
    pub rank: u32,
}

/// A pool as reported by market data (provider observation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedPool {
    pub pool: String,
    pub base_mint: String,
    pub quote_mint: Option<String>,
    pub dex_id: Option<String>,
    pub labels: Vec<String>,
    pub liquidity_usd: Option<Decimal>,
    pub volume_24h_usd: Option<Decimal>,
    pub txns_1h: Option<u64>,
    pub txns_24h: Option<u64>,
    pub pair_created_at: Option<UtcTimestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched<T> {
    pub value: T,
    pub observed_at: UtcTimestamp,
    pub requests: u32,
    pub credits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FeedError {
    #[error("budget denied")]
    Budget,
    #[error("provider cooling down ({0})")]
    Cooldown(&'static str),
    #[error("disabled ({0})")]
    Disabled(&'static str),
    #[error("request failed ({0})")]
    Failed(&'static str),
}

impl FeedError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Budget => "BUDGET_DENIED",
            Self::Cooldown(c) | Self::Disabled(c) | Self::Failed(c) => c,
        }
    }

    /// The source-run status this error is recorded as.
    #[must_use]
    pub const fn run_status(&self) -> &'static str {
        match self {
            Self::Budget => "BUDGET_DEFERRED",
            Self::Cooldown(_) => "PROVIDER_COOLDOWN",
            Self::Disabled(_) => "DISABLED",
            Self::Failed(_) => "FAILED",
        }
    }
}

impl From<GatewayError> for FeedError {
    fn from(e: GatewayError) -> Self {
        match e {
            GatewayError::BudgetDenied { .. } => Self::Budget,
            GatewayError::Disabled { .. }
            | GatewayError::MissingCredential(_)
            | GatewayError::HostNotAllowed => Self::Disabled(e.code()),
            GatewayError::RateLimited { .. }
            | GatewayError::CircuitOpen { .. }
            | GatewayError::QueueFull { .. }
            | GatewayError::Unavailable => Self::Cooldown(e.code()),
            other => Self::Failed(other.code()),
        }
    }
}

type FR<T> = Result<Fetched<T>, FeedError>;

/// Market discovery inputs RADAR needs.
pub trait MarketFeed: Send + Sync + 'static {
    /// GATEWAY's budget from local state (never provider traffic).
    fn budget(&self) -> Option<BudgetView>;
    /// Whether DATA ENGINE live subscriptions can be used for monitoring.
    fn streams_available(&self) -> bool;
    fn listing(&self, l: Listing) -> impl Future<Output = FR<Vec<ListedToken>>> + Send;
    fn pools(&self, mints: &[String]) -> impl Future<Output = FR<Vec<ListedPool>>> + Send;
    /// On-chain owner program of each account (`None`: account not found).
    fn owners(&self, accounts: &[String]) -> impl Future<Output = FR<Vec<Option<String>>>> + Send;
}

fn ctx() -> RequestContext {
    RequestContext::new(ORIGIN, Priority::P5).with_timeout(TIMEOUT)
}

fn token(t: TokenInfo, rank: u32) -> ListedToken {
    let txns_1h = t
        .discovery
        .stats_1h
        .as_ref()
        .and_then(|s| Some(s.num_buys?.saturating_add(s.num_sells?)));
    ListedToken {
        mint: t.mint.as_str().to_owned(),
        symbol: t.symbol,
        name: t.name,
        token_program: t.token_program,
        verified: t.is_verified,
        tags: t.tags,
        organic_score: t.organic_score,
        launchpad: t.discovery.launchpad,
        first_pool_created_at: t.discovery.first_pool_created_at,
        created_at: t.discovery.created_at,
        txns_1h,
        rank,
    }
}

/// Map a DEX Screener pair. Only Solana pairs are kept.
#[must_use]
pub fn pool_of(p: &DexPair) -> Option<ListedPool> {
    if p.chain_id != "solana" {
        return None;
    }
    let window = |w: &str| {
        p.txns
            .iter()
            .find(|x| x.window == w)
            .map(|x| x.buys.saturating_add(x.sells))
    };
    Some(ListedPool {
        pool: p.pair_address.clone(),
        base_mint: p.base_token.address.clone(),
        quote_mint: Some(p.quote_token.address.clone()),
        dex_id: Some(p.dex_id.clone()).filter(|d| !d.is_empty()),
        labels: p.labels.clone(),
        liquidity_usd: p.liquidity_usd,
        volume_24h_usd: p
            .volume_usd
            .iter()
            .find(|(w, _)| *w == "h24")
            .map(|(_, v)| *v),
        txns_1h: window("h1"),
        txns_24h: window("h24"),
        pair_created_at: p.pair_created_at,
    })
}

/// Production feed over GATEWAY.
#[derive(Debug, Clone)]
pub struct GatewayFeed {
    gateway: Gateway,
    dex_discovery_enabled: bool,
}

impl GatewayFeed {
    #[must_use]
    pub const fn new(gateway: Gateway, dex_discovery_enabled: bool) -> Self {
        Self {
            gateway,
            dex_discovery_enabled,
        }
    }
}

impl MarketFeed for GatewayFeed {
    fn budget(&self) -> Option<BudgetView> {
        self.gateway.snapshot().budget.map(|b| BudgetView {
            state: b.state.as_str().to_owned(),
            normal_credits: b.normal_credits,
            credits_used: b.credits_used,
            ledger_loaded: b.ledger_loaded,
        })
    }

    fn streams_available(&self) -> bool {
        self.gateway.snapshot().streams.is_some()
    }

    async fn listing(&self, l: Listing) -> FR<Vec<ListedToken>> {
        let c = ctx();
        let r = match l {
            Listing::JupiterTopTraded => {
                self.gateway
                    .token_category(&c, TokenCategory::TopTraded, TokenInterval::H1, 50)
                    .await?
            }
            Listing::JupiterTopOrganic => {
                self.gateway
                    .token_category(&c, TokenCategory::TopOrganicScore, TokenInterval::H1, 50)
                    .await?
            }
            Listing::JupiterRecent => self.gateway.recent_tokens(&c).await?,
            Listing::DexBoosts => {
                if !self.dex_discovery_enabled {
                    return Err(FeedError::Disabled("DEXSCREENER_DISCOVERY_DISABLED"));
                }
                let b = self.gateway.boosts(&c, true).await?;
                let tokens = b
                    .value
                    .iter()
                    .filter(|x| x.chain_id == "solana")
                    .zip(0u32..)
                    .filter_map(|(x, i)| {
                        TokenMint::parse(&x.token_address)
                            .ok()
                            .map(|m| ListedToken {
                                mint: m.as_str().to_owned(),
                                rank: i,
                                ..ListedToken::default()
                            })
                    })
                    .collect();
                return Ok(Fetched {
                    value: tokens,
                    observed_at: b.observed_at,
                    requests: 1,
                    credits: b.credits_charged,
                });
            }
        };
        Ok(Fetched {
            value: r
                .value
                .into_iter()
                .zip(0u32..)
                .map(|(t, i)| token(t, i))
                .collect(),
            observed_at: r.observed_at,
            requests: 1,
            credits: r.credits_charged,
        })
    }

    async fn pools(&self, mints: &[String]) -> FR<Vec<ListedPool>> {
        let c = ctx();
        let mut out = Vec::new();
        let (mut requests, mut credits) = (0u32, 0u64);
        let mut observed_at = UtcTimestamp::now();
        for chunk in mints.chunks(PAIRS_CHUNK) {
            let parsed: Vec<TokenMint> = chunk
                .iter()
                .filter_map(|m| TokenMint::parse(m).ok())
                .collect();
            if parsed.is_empty() {
                continue;
            }
            let r = self.gateway.pairs_by_tokens(&c, &parsed).await?;
            requests += 1;
            credits += r.credits_charged;
            observed_at = r.observed_at;
            out.extend(r.value.iter().filter_map(pool_of));
        }
        Ok(Fetched {
            value: out,
            observed_at,
            requests,
            credits,
        })
    }

    async fn owners(&self, accounts: &[String]) -> FR<Vec<Option<String>>> {
        let c = ctx();
        let mut out = Vec::with_capacity(accounts.len());
        let (mut requests, mut credits) = (0u32, 0u64);
        let mut observed_at = UtcTimestamp::now();
        for chunk in accounts.chunks(ACCOUNTS_CHUNK) {
            let parsed: Vec<Option<WalletAddress>> =
                chunk.iter().map(|a| WalletAddress::parse(a).ok()).collect();
            let valid: Vec<WalletAddress> = parsed.iter().flatten().cloned().collect();
            let r = self.gateway.accounts(&c, &valid).await?;
            requests += 1;
            credits += r.credits_charged;
            observed_at = r.observed_at;
            let mut it = r.value.into_iter();
            for p in parsed {
                out.push(match p {
                    Some(_) => it.next().flatten().map(|a| a.owner.as_str().to_owned()),
                    None => None,
                });
            }
        }
        Ok(Fetched {
            value: out,
            observed_at,
            requests,
            credits,
        })
    }
}

/// In-memory feed for tests and `solprint radar fixture-discovery`. Every
/// value it returns is labelled as fixture data by the caller.
#[derive(Debug, Clone, Default)]
pub struct FixtureFeed {
    inner: Arc<Mutex<FixtureState>>,
}

#[derive(Debug, Clone, Default)]
struct FixtureState {
    listings: Vec<(Listing, Vec<ListedToken>)>,
    pools: Vec<ListedPool>,
    owners: Vec<(String, String)>,
    budget: Option<BudgetView>,
    streams: bool,
    fail: Option<FeedError>,
    calls: u32,
}

impl FixtureFeed {
    /// A feed with a NORMAL budget of `normal_credits` (none used).
    #[must_use]
    pub fn with_budget(normal_credits: u64) -> Self {
        let f = Self::default();
        f.set_budget(Some(BudgetView {
            state: "NORMAL".to_owned(),
            normal_credits,
            credits_used: 0,
            ledger_loaded: true,
        }));
        f
    }

    fn with<R>(&self, f: impl FnOnce(&mut FixtureState) -> R) -> R {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn set_budget(&self, b: Option<BudgetView>) {
        self.with(|s| s.budget = b);
    }

    pub fn set_listing(&self, l: Listing, tokens: Vec<ListedToken>) {
        self.with(|s| {
            s.listings.retain(|(x, _)| *x != l);
            s.listings.push((l, tokens));
        });
    }

    pub fn add_pool(&self, p: ListedPool, owner_program: &str) {
        self.with(|s| {
            s.owners.push((p.pool.clone(), owner_program.to_owned()));
            s.pools.push(p);
        });
    }

    pub fn set_streams(&self, on: bool) {
        self.with(|s| s.streams = on);
    }

    /// Make every provider call fail with `e` (provider outage tests).
    pub fn fail_with(&self, e: Option<FeedError>) {
        self.with(|s| s.fail = e);
    }

    /// Provider calls made so far.
    #[must_use]
    pub fn calls(&self) -> u32 {
        self.with(|s| s.calls)
    }

    fn call(&self) -> Result<(), FeedError> {
        self.with(|s| {
            s.calls += 1;
            s.fail.clone().map_or(Ok(()), Err)
        })
    }
}

impl MarketFeed for FixtureFeed {
    fn budget(&self) -> Option<BudgetView> {
        self.with(|s| s.budget.clone())
    }

    fn streams_available(&self) -> bool {
        self.with(|s| s.streams)
    }

    async fn listing(&self, l: Listing) -> FR<Vec<ListedToken>> {
        self.call()?;
        let v = self.with(|s| {
            s.listings
                .iter()
                .find(|(x, _)| *x == l)
                .map(|(_, t)| t.clone())
                .unwrap_or_default()
        });
        Ok(Fetched {
            value: v,
            observed_at: UtcTimestamp::now(),
            requests: 1,
            credits: 0,
        })
    }

    async fn pools(&self, mints: &[String]) -> FR<Vec<ListedPool>> {
        self.call()?;
        let v = self.with(|s| {
            s.pools
                .iter()
                .filter(|p| mints.contains(&p.base_mint))
                .cloned()
                .collect()
        });
        Ok(Fetched {
            value: v,
            observed_at: UtcTimestamp::now(),
            requests: 1,
            credits: 0,
        })
    }

    async fn owners(&self, accounts: &[String]) -> FR<Vec<Option<String>>> {
        self.call()?;
        let v = self.with(|s| {
            accounts
                .iter()
                .map(|a| {
                    s.owners
                        .iter()
                        .find(|(p, _)| p == a)
                        .map(|(_, o)| o.clone())
                })
                .collect()
        });
        let credits = u64::try_from(accounts.len().div_ceil(ACCOUNTS_CHUNK)).unwrap_or(0);
        Ok(Fetched {
            value: v,
            observed_at: UtcTimestamp::now(),
            requests: 1,
            credits,
        })
    }
}
