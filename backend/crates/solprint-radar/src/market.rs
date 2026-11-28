//! Market screening and multi-bucket selection.
//!
//! Selection is deliberately spread across buckets with fixed scheduling
//! shares (active established, emerging, existing local activity, historical
//! yield, random exploration) so that no single provider ranking, launchpad
//! or market type absorbs all of RADAR's sampling capacity. A missing market
//! field is never read as zero: the market is deferred with a `MISSING_*`
//! reason instead.

use std::collections::HashSet;
use std::time::Duration;

use rust_decimal::Decimal;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::model::Bucket;

/// Thresholds applied to provider-reported market figures (USD values are
/// provider-reported decimals, never floats).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thresholds {
    pub min_liquidity_usd: Decimal,
    pub min_volume_24h_usd: Decimal,
    pub min_txns_1h: u32,
    pub max_observation_age: Duration,
    /// A pool sampled more recently than this is not sampled again yet.
    pub resample_after: Duration,
}

impl Thresholds {
    /// Emerging markets are young: they are held to a fifth of the
    /// established liquidity floor and half of the activity floor
    /// (documented policy, not a market statistic).
    fn emerging_liquidity(&self) -> Decimal {
        self.min_liquidity_usd / Decimal::from(5)
    }

    const fn emerging_txns(&self) -> u32 {
        self.min_txns_1h / 2
    }
}

/// A pool as selection sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolView {
    pub pool: String,
    pub supported: bool,
    pub liquidity_usd: Option<Decimal>,
    pub volume_24h_usd: Option<Decimal>,
    pub txns_1h: Option<u32>,
    pub observed_at: OffsetDateTime,
    pub last_sampled_at: Option<OffsetDateTime>,
}

/// A market as selection sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketCandidate {
    pub mint: String,
    pub bucket: Bucket,
    pub excluded_class: Option<String>,
    pub promotional_only: bool,
    /// The best supported pool (highest reported liquidity), if any.
    pub pool: Option<PoolView>,
    pub samples_completed: u32,
    pub wallets_found: u32,
    /// Local verified swaps of this mint (local-activity bucket weight).
    pub local_swaps: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Eligible,
    Deferred(Vec<String>),
    Excluded(Vec<String>),
}

fn push_threshold(
    reasons: &mut Vec<String>,
    value: Option<Decimal>,
    floor: Decimal,
    missing: &str,
    low: &str,
) {
    match value {
        None => reasons.push(missing.to_owned()),
        Some(v) if v < floor => reasons.push(low.to_owned()),
        Some(_) => {}
    }
}

/// Screen a market for sampling in `bucket` at `now`.
#[must_use]
pub fn screen(c: &MarketCandidate, bucket: Bucket, t: &Thresholds, now: OffsetDateTime) -> Screen {
    if let Some(class) = &c.excluded_class {
        return Screen::Excluded(vec![format!("CLASS_{class}")]);
    }
    let Some(pool) = c.pool.as_ref().filter(|p| p.supported) else {
        return Screen::Deferred(vec!["NO_SUPPORTED_POOL".to_owned()]);
    };
    let mut reasons = Vec::new();
    if pool
        .last_sampled_at
        .is_some_and(|s| now - s < t.resample_after)
    {
        reasons.push("RECENTLY_SAMPLED".to_owned());
    }
    match bucket {
        Bucket::Established | Bucket::Emerging => {
            if now - pool.observed_at > t.max_observation_age {
                reasons.push("STALE_MARKET_OBSERVATION".to_owned());
            }
            let (liq, txns) = if bucket == Bucket::Established {
                (t.min_liquidity_usd, t.min_txns_1h)
            } else {
                (t.emerging_liquidity(), t.emerging_txns())
            };
            push_threshold(
                &mut reasons,
                pool.liquidity_usd,
                liq,
                "MISSING_LIQUIDITY",
                "LOW_LIQUIDITY",
            );
            if bucket == Bucket::Established {
                push_threshold(
                    &mut reasons,
                    pool.volume_24h_usd,
                    t.min_volume_24h_usd,
                    "MISSING_VOLUME_24H",
                    "LOW_VOLUME_24H",
                );
            }
            match pool.txns_1h {
                None => reasons.push("MISSING_TXNS_1H".to_owned()),
                Some(n) if n < txns => reasons.push("LOW_ACTIVITY_1H".to_owned()),
                Some(_) => {}
            }
        }
        // Local, historical and exploration picks need only a supported pool:
        // their point is to look where provider rankings do not.
        Bucket::LocalActivity | Bucket::Historical | Bucket::Exploration => {}
    }
    if reasons.is_empty() {
        Screen::Eligible
    } else {
        Screen::Deferred(reasons)
    }
}

/// Scheduling weight in 0..=1 (6 dp). Higher is sampled first within its
/// bucket. It is **not** a market-quality rating.
#[must_use]
pub fn market_priority(c: &MarketCandidate, bucket: Bucket, t: &Thresholds) -> Decimal {
    let one = Decimal::ONE;
    let ratio = |n: u32, d: u32| -> Decimal {
        if d == 0 {
            return one;
        }
        (Decimal::from(n) / Decimal::from(d)).min(one)
    };
    let novelty = if c.samples_completed == 0 {
        one
    } else {
        one / Decimal::from(c.samples_completed.saturating_add(1))
    };
    let yield_rate = if c.samples_completed == 0 {
        Decimal::new(5, 1)
    } else {
        ratio(c.wallets_found, c.samples_completed.saturating_mul(5))
    };
    let activity = match bucket {
        Bucket::LocalActivity => ratio(c.local_swaps, 50),
        _ => c
            .pool
            .as_ref()
            .and_then(|p| p.txns_1h)
            .map_or(Decimal::ZERO, |n| {
                ratio(n, t.min_txns_1h.saturating_mul(4).max(1))
            }),
    };
    let w = |v: u32| Decimal::new(i64::from(v), 1);
    (w(5) * activity + w(3) * novelty + w(2) * yield_rate)
        .round_dp(6)
        .clamp(Decimal::ZERO, one)
}

/// Split `slots` across buckets by basis-point `shares` (largest remainder),
/// never above what each bucket can use. Exploration keeps at least one slot
/// whenever there are two or more slots and it has a candidate. Capacity a
/// bucket cannot use flows to the others in share order.
#[must_use]
pub fn allocate(slots: u32, shares: [u32; 5], available: [usize; 5]) -> [u32; 5] {
    let total_bps: u64 = shares.iter().map(|&s| u64::from(s)).sum();
    let mut out = [0u32; 5];
    if slots == 0 || total_bps == 0 {
        return out;
    }
    let mut rems: Vec<(u64, usize)> = Vec::with_capacity(5);
    let mut used = 0u32;
    for (i, &s) in shares.iter().enumerate() {
        let exact = u64::from(slots) * u64::from(s);
        let q = u32::try_from(exact / total_bps).unwrap_or(u32::MAX);
        out[i] = q;
        used = used.saturating_add(q);
        rems.push((exact % total_bps, i));
    }
    // Largest remainder; ties go to the larger share, then the lower index.
    rems.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(shares[b.1].cmp(&shares[a.1]))
            .then(a.1.cmp(&b.1))
    });
    let extra = usize::try_from(slots.saturating_sub(used)).unwrap_or(0);
    let mut by_remainder = Vec::with_capacity(extra);
    for &(_, i) in rems.iter().cycle().take(extra) {
        out[i] += 1;
        by_remainder.push(i);
    }
    let ex = 4;
    if slots >= 2 && out[ex] == 0 && available[ex] > 0 {
        // Take the floor slot from the weakest claim: the last slot handed
        // out by remainder, else the largest bucket.
        let donor = by_remainder
            .iter()
            .rev()
            .copied()
            .find(|&i| i != ex)
            .or_else(|| (0..4).max_by_key(|&i| (out[i], shares[i])));
        if let Some(d) = donor.filter(|&d| out[d] > 0) {
            out[d] -= 1;
            out[ex] += 1;
        }
    }
    // Cap by availability and redistribute the excess.
    let mut spare = 0u32;
    for i in 0..5 {
        let cap = u32::try_from(available[i]).unwrap_or(u32::MAX);
        if out[i] > cap {
            spare += out[i] - cap;
            out[i] = cap;
        }
    }
    let mut order: Vec<usize> = (0..5).collect();
    order.sort_by(|&a, &b| shares[b].cmp(&shares[a]).then(a.cmp(&b)));
    while spare > 0 {
        let mut placed = false;
        for &i in &order {
            if spare == 0 {
                break;
            }
            if usize::try_from(out[i]).unwrap_or(usize::MAX) < available[i] {
                out[i] += 1;
                spare -= 1;
                placed = true;
            }
        }
        if !placed {
            break;
        }
    }
    out
}

/// Deterministic exploration rank: the same seed and mint always give the
/// same rank, so a cycle's random picks are reproducible from its seed.
#[must_use]
pub fn exploration_rank(seed: i64, mint: &str) -> u64 {
    let mut h = Sha256::new();
    h.update(seed.to_le_bytes());
    h.update(mint.as_bytes());
    let d = h.finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b)
}

/// One selected market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub mint: String,
    pub pool: String,
    pub bucket: Bucket,
    pub priority: Decimal,
}

/// Per-market selection outcome (recorded on the market and the observation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub mint: String,
    pub state: &'static str,
    pub reasons: Vec<String>,
    pub priority: Option<Decimal>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    pub picks: Vec<Pick>,
    pub decisions: Vec<Decision>,
    pub allocation: [u32; 5],
}

fn idx(b: Bucket) -> usize {
    match b {
        Bucket::Established => 0,
        Bucket::Emerging => 1,
        Bucket::LocalActivity => 2,
        Bucket::Historical => 3,
        Bucket::Exploration => 4,
    }
}

/// Select up to `slots` markets across buckets. A market is picked at most
/// once per cycle. Markets with earlier wallet yield are also candidates in
/// the historical bucket; every screened market is an exploration
/// candidate.
#[must_use]
pub fn select(
    markets: &[MarketCandidate],
    slots: u32,
    shares: [u32; 5],
    t: &Thresholds,
    seed: i64,
    now: OffsetDateTime,
) -> Selection {
    let mut pools: [Vec<(Decimal, &MarketCandidate)>; 5] = Default::default();
    let mut decisions = Vec::new();
    for m in markets {
        let home = screen(m, m.bucket, t, now);
        match &home {
            Screen::Excluded(r) => {
                decisions.push(Decision {
                    mint: m.mint.clone(),
                    state: "EXCLUDED",
                    reasons: r.clone(),
                    priority: None,
                });
                continue;
            }
            Screen::Eligible if !(m.promotional_only && m.bucket != Bucket::Exploration) => {
                pools[idx(m.bucket)].push((market_priority(m, m.bucket, t), m));
            }
            _ => {}
        }
        if m.wallets_found > 0 && screen(m, Bucket::Historical, t, now) == Screen::Eligible {
            pools[3].push((market_priority(m, Bucket::Historical, t), m));
        }
        if screen(m, Bucket::Exploration, t, now) == Screen::Eligible {
            let r = exploration_rank(seed, &m.mint);
            let p = Decimal::from(r >> 44) / Decimal::from(1u64 << 20);
            pools[4].push((p.round_dp(6), m));
        }
        if let Screen::Deferred(r) = home {
            decisions.push(Decision {
                mint: m.mint.clone(),
                state: "DEFERRED",
                reasons: r,
                priority: None,
            });
        }
    }
    for (i, p) in pools.iter_mut().enumerate() {
        if i == 4 {
            p.sort_by_key(|(_, m)| exploration_rank(seed, &m.mint));
        } else {
            p.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.mint.cmp(&b.1.mint)));
        }
    }
    let available: [usize; 5] = std::array::from_fn(|i| pools[i].len());
    let allocation = allocate(slots, shares, available);
    let mut chosen: HashSet<&str> = HashSet::new();
    let mut picks = Vec::new();
    for (i, bucket) in Bucket::ALL.into_iter().enumerate() {
        let mut want = allocation[i];
        for (prio, m) in &pools[i] {
            if want == 0 {
                break;
            }
            if !chosen.insert(m.mint.as_str()) {
                continue;
            }
            let Some(pool) = &m.pool else { continue };
            picks.push(Pick {
                mint: m.mint.clone(),
                pool: pool.pool.clone(),
                bucket,
                priority: *prio,
            });
            want -= 1;
        }
    }
    for p in &picks {
        decisions.retain(|d| d.mint != p.mint);
        decisions.push(Decision {
            mint: p.mint.clone(),
            state: "SELECTED",
            reasons: vec![format!("BUCKET_{}", p.bucket.as_str())],
            priority: Some(p.priority),
        });
    }
    for m in markets {
        if !decisions.iter().any(|d| d.mint == m.mint) {
            decisions.push(Decision {
                mint: m.mint.clone(),
                state: "CANDIDATE",
                reasons: vec!["NOT_SELECTED_THIS_CYCLE".to_owned()],
                priority: None,
            });
        }
    }
    Selection {
        picks,
        decisions,
        allocation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHARES: [u32; 5] = [3500, 2500, 2000, 1000, 1000];

    fn t() -> Thresholds {
        Thresholds {
            min_liquidity_usd: Decimal::from(5000),
            min_volume_24h_usd: Decimal::from(10_000),
            min_txns_1h: 20,
            max_observation_age: Duration::from_secs(300),
            resample_after: Duration::from_secs(3600),
        }
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    fn market(mint: &str, bucket: Bucket, liq: Option<i64>, txns: Option<u32>) -> MarketCandidate {
        MarketCandidate {
            mint: mint.to_owned(),
            bucket,
            excluded_class: None,
            promotional_only: false,
            pool: Some(PoolView {
                pool: format!("pool-{mint}"),
                supported: true,
                liquidity_usd: liq.map(Decimal::from),
                volume_24h_usd: Some(Decimal::from(50_000)),
                txns_1h: txns,
                observed_at: now(),
                last_sampled_at: None,
            }),
            samples_completed: 0,
            wallets_found: 0,
            local_swaps: 0,
        }
    }

    #[test]
    fn allocation_follows_shares_with_largest_remainder() {
        assert_eq!(allocate(5, SHARES, [9; 5]), [2, 1, 1, 0, 1]);
        assert_eq!(allocate(10, SHARES, [9; 5]), [4, 2, 2, 1, 1]);
        let total: u32 = allocate(7, SHARES, [9; 5]).iter().sum();
        assert_eq!(total, 7);
    }

    #[test]
    fn unused_capacity_flows_to_other_buckets_and_exploration_keeps_a_floor() {
        // No emerging or local candidates: their slots go elsewhere.
        let a = allocate(5, SHARES, [9, 0, 0, 9, 9]);
        assert_eq!(a.iter().sum::<u32>(), 5);
        assert_eq!(a[1] + a[2], 0);
        assert!(a[4] >= 1);
        // Everything empty except exploration.
        assert_eq!(allocate(3, SHARES, [0, 0, 0, 0, 2]), [0, 0, 0, 0, 2]);
        assert_eq!(allocate(0, SHARES, [9; 5]), [0; 5]);
    }

    #[test]
    fn missing_fields_defer_instead_of_reading_as_zero() {
        let m = market("A", Bucket::Established, None, None);
        let Screen::Deferred(r) = screen(&m, Bucket::Established, &t(), now()) else {
            panic!("expected deferral");
        };
        assert!(r.contains(&"MISSING_LIQUIDITY".to_owned()));
        assert!(r.contains(&"MISSING_TXNS_1H".to_owned()));
        // The same market is a valid exploration candidate.
        assert_eq!(
            screen(&m, Bucket::Exploration, &t(), now()),
            Screen::Eligible
        );
    }

    #[test]
    fn stale_observations_and_recent_samples_defer() {
        let mut m = market("A", Bucket::Established, Some(9000), Some(50));
        assert_eq!(
            screen(&m, Bucket::Established, &t(), now()),
            Screen::Eligible
        );
        if let Some(p) = m.pool.as_mut() {
            p.observed_at = now() - Duration::from_secs(600);
            p.last_sampled_at = Some(now() - Duration::from_secs(60));
        }
        let Screen::Deferred(r) = screen(&m, Bucket::Established, &t(), now()) else {
            panic!("expected deferral");
        };
        assert_eq!(
            r,
            vec![
                "RECENTLY_SAMPLED".to_owned(),
                "STALE_MARKET_OBSERVATION".to_owned()
            ]
        );
    }

    #[test]
    fn emerging_uses_lower_floors() {
        let m = market("A", Bucket::Emerging, Some(1500), Some(12));
        assert_eq!(screen(&m, Bucket::Emerging, &t(), now()), Screen::Eligible);
        assert!(matches!(
            screen(&m, Bucket::Established, &t(), now()),
            Screen::Deferred(_)
        ));
    }

    #[test]
    fn selection_spreads_across_buckets_and_never_picks_twice() {
        let mut ms = Vec::new();
        for i in 0..20 {
            ms.push(market(
                &format!("E{i:02}"),
                Bucket::Established,
                Some(90_000),
                Some(500),
            ));
        }
        ms.push(market("M1", Bucket::Emerging, Some(2000), Some(15)));
        let mut local = market("L1", Bucket::LocalActivity, None, None);
        local.local_swaps = 10;
        ms.push(local);
        let s = select(&ms, 5, SHARES, &t(), 42, now());
        assert_eq!(s.picks.len(), 5);
        let buckets: HashSet<Bucket> = s.picks.iter().map(|p| p.bucket).collect();
        assert!(buckets.contains(&Bucket::Emerging));
        assert!(buckets.contains(&Bucket::LocalActivity));
        assert!(buckets.contains(&Bucket::Exploration));
        let mints: HashSet<&str> = s.picks.iter().map(|p| p.mint.as_str()).collect();
        assert_eq!(mints.len(), 5);
        // Same seed, same picks.
        assert_eq!(select(&ms, 5, SHARES, &t(), 42, now()).picks, s.picks);
    }

    #[test]
    fn promotional_only_markets_are_exploration_only() {
        let mut m = market("P", Bucket::Established, Some(90_000), Some(500));
        m.promotional_only = true;
        let s = select(&[m], 5, SHARES, &t(), 1, now());
        assert!(s.picks.iter().all(|p| p.bucket == Bucket::Exploration));
    }

    #[test]
    fn excluded_classes_are_never_selected() {
        let mut m = market("S", Bucket::Established, Some(90_000), Some(500));
        m.excluded_class = Some("STABLECOIN".to_owned());
        let s = select(&[m], 5, SHARES, &t(), 1, now());
        assert!(s.picks.is_empty());
        assert_eq!(s.decisions[0].state, "EXCLUDED");
    }

    #[test]
    fn priority_is_bounded() {
        let m = market("A", Bucket::Established, Some(90_000), Some(10_000));
        let p = market_priority(&m, Bucket::Established, &t());
        assert!(p <= Decimal::ONE && p >= Decimal::ZERO);
    }
}
