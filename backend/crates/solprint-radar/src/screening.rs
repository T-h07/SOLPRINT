//! Preliminary screening: activity metrics and conservative FIFO matched
//! returns over a wallet's verified swaps.
//!
//! This is an investigation-priority input, not performance accounting:
//!
//! * amounts are exact raw integers (`u128` intermediates, checked);
//! * values in different quote denominations (SOL, USDC, USDT) are never
//!   added together, and there is no USD conversion;
//! * a disposal whose cost basis is unknown is **unmatched**, never matched
//!   at zero cost; inventory the wallet already held, or received outside an
//!   observed swap, has unknown cost;
//! * the transaction fee counts only when the wallet paid it, split evenly
//!   over its swaps in that transaction, and only against SOL legs (USDC and
//!   USDT outcomes are marked `INCOMPLETE_FEES`);
//! * an outcome is a **closed position cycle** (tracked inventory 0 → >0 →
//!   0) whose disposals were all fully matched and which no unobserved
//!   inflow or outflow touched.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use rust_decimal::Decimal;
use time::{Duration, OffsetDateTime};

use crate::model::Denomination;

/// One verified, attributed swap of the wallet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    pub swap_id: String,
    pub signature: String,
    pub slot: u64,
    pub order: (u64, i64, String),
    pub block_time: Option<OffsetDateTime>,
    pub input_mint: Option<String>,
    pub input_kind: String,
    pub input_amount: Option<u64>,
    pub output_mint: Option<String>,
    pub output_kind: String,
    pub output_amount: Option<u64>,
    /// The wallet paid this transaction's fee.
    pub wallet_paid_fee: bool,
    pub fee_lamports: u64,
    pub finalized: bool,
}

/// The wallet's balance of a token at the start of a transaction, summed
/// over the token accounts it owns there. `None`: not established.
pub type PreBalances = HashMap<(String, String), Option<u64>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
    /// Token for token: activity, not a quote-denominated position change.
    Rotate,
    /// Quote for quote (for example SOL for USDC): not memecoin activity.
    QuoteOnly,
}

/// Classify a trade's direction and its token/denomination.
#[must_use]
pub fn side(t: &Trade) -> (Side, Option<Denomination>, Option<&str>) {
    let din = Denomination::of(t.input_mint.as_deref(), &t.input_kind);
    let dout = Denomination::of(t.output_mint.as_deref(), &t.output_kind);
    match (din, dout) {
        (Some(d), None) => (Side::Buy, Some(d), t.output_mint.as_deref()),
        (None, Some(d)) => (Side::Sell, Some(d), t.input_mint.as_deref()),
        (None, None) => (Side::Rotate, None, None),
        (Some(_), Some(_)) => (Side::QuoteOnly, None, None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Lot {
    qty: u128,
    /// Remaining cost and its denomination; `None` = unknown cost basis.
    cost: Option<(Denomination, u128)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Cycle {
    tainted: bool,
    denoms: BTreeSet<Denomination>,
    cost: u128,
    proceeds: u128,
    incomplete_fees: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TokenBook {
    lots: VecDeque<Lot>,
    tracked: u128,
    cycle: Option<Cycle>,
    ambiguous: bool,
    disposals: u32,
    disposals_matched: u32,
    disposed_qty: u128,
    matched_qty: u128,
}

impl TokenBook {
    fn open_cycle(&mut self) -> &mut Cycle {
        self.cycle.get_or_insert_with(Cycle::default)
    }

    /// Remove `qty` from the front of the inventory (FIFO).
    fn take(&mut self, mut qty: u128) -> Vec<(u128, Option<(Denomination, u128)>)> {
        let mut out = Vec::new();
        while qty > 0 {
            let Some(front) = self.lots.front_mut() else {
                break;
            };
            let take = qty.min(front.qty);
            let cost = match front.cost.as_mut() {
                Some((d, c)) => {
                    // Exact allocation; the remainder stays with the lot and
                    // goes to whichever portion takes the lot's last unit.
                    let part = if take == front.qty {
                        *c
                    } else {
                        *c * take / front.qty
                    };
                    *c -= part;
                    Some((*d, part))
                }
                None => None,
            };
            front.qty -= take;
            self.tracked -= take;
            qty -= take;
            out.push((take, cost));
            if front.qty == 0 {
                self.lots.pop_front();
            }
        }
        out
    }
}

/// One evaluated position cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub mint: String,
    pub denomination: Denomination,
    pub cost: u128,
    pub proceeds: u128,
    pub incomplete_fees: bool,
}

impl Outcome {
    #[must_use]
    pub fn is_win(&self) -> bool {
        self.proceeds > self.cost
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DenominationSummary {
    pub outcomes: u32,
    pub wins: u32,
    pub cost: u128,
    pub proceeds: u128,
    pub incomplete_fees: bool,
}

impl DenominationSummary {
    /// `(proceeds − cost) / cost`, 6 dp; `None` without cost.
    #[must_use]
    pub fn return_ratio(&self) -> Option<Decimal> {
        if self.cost == 0 {
            return None;
        }
        let c = Decimal::from_i128_with_scale(i128::try_from(self.cost).ok()?, 0);
        let p = Decimal::from_i128_with_scale(i128::try_from(self.proceeds).ok()?, 0);
        p.checked_sub(c)?.checked_div(c).map(|r| r.round_dp(6))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Screening {
    pub outcomes: Vec<Outcome>,
    pub by_denomination: BTreeMap<Denomination, DenominationSummary>,
    /// Disposal events and how many were fully matched to known cost.
    pub disposals: u32,
    pub disposals_matched: u32,
    /// Tokens whose inventory could not be reconciled (unknown balance).
    pub ambiguous_tokens: Vec<String>,
    pub unknown_cost_tokens: Vec<String>,
    pub open_positions: u32,
    pub unevaluated_cycles: u32,
    /// Per-token matched quantity basis points (not summed across tokens).
    pub token_coverage_bps: BTreeMap<String, u32>,
}

impl Screening {
    /// Fraction of disposal **events** fully matched, in basis points.
    #[must_use]
    pub fn matched_coverage_bps(&self) -> Option<u32> {
        (self.disposals > 0).then(|| {
            u32::try_from(u64::from(self.disposals_matched) * 10_000 / u64::from(self.disposals))
                .unwrap_or(10_000)
        })
    }

    /// The denomination with the most evaluated outcomes (SOL wins ties).
    #[must_use]
    pub fn primary(&self) -> Option<(Denomination, &DenominationSummary)> {
        self.by_denomination
            .iter()
            .filter(|(_, s)| s.outcomes > 0)
            .max_by(|a, b| a.1.outcomes.cmp(&b.1.outcomes).then(b.0.cmp(a.0)))
            .map(|(d, s)| (*d, s))
    }
}

/// The fee share of each of the wallet's swaps (lamports), by swap id.
fn fee_shares(trades: &[Trade]) -> HashMap<&str, u64> {
    let mut by_tx: BTreeMap<&str, Vec<&Trade>> = BTreeMap::new();
    for t in trades {
        by_tx.entry(t.signature.as_str()).or_default().push(t);
    }
    let mut out = HashMap::new();
    for ts in by_tx.values() {
        let Some(first) = ts.first() else { continue };
        if !first.wallet_paid_fee {
            continue;
        }
        let n = u64::try_from(ts.len()).unwrap_or(1).max(1);
        let each = first.fee_lamports / n;
        let rem = first.fee_lamports % n;
        for (i, t) in ts.iter().enumerate() {
            out.insert(t.swap_id.as_str(), if i == 0 { each + rem } else { each });
        }
    }
    out
}

/// Screen a wallet's trades (any order; sorted here by chain order).
#[must_use]
pub fn screen(trades: &[Trade], pre: &PreBalances) -> Screening {
    let mut ordered: Vec<&Trade> = trades.iter().collect();
    ordered.sort_by(|a, b| a.order.cmp(&b.order));
    let fees = fee_shares(trades);
    let mut books: BTreeMap<String, TokenBook> = BTreeMap::new();
    let mut reconciled: BTreeSet<(String, String)> = BTreeSet::new();
    let mut out = Screening::default();
    let mut unknown_cost: BTreeSet<String> = BTreeSet::new();

    for t in ordered {
        let (s, denom, token) = side(t);
        let (Some(denom), Some(token)) = (denom, token) else {
            continue;
        };
        let book = books.entry(token.to_owned()).or_default();
        // Reconcile tracked inventory with the observed pre-balance once per
        // (transaction, token).
        if reconciled.insert((t.signature.clone(), token.to_owned())) {
            match pre
                .get(&(t.signature.clone(), token.to_owned()))
                .copied()
                .flatten()
            {
                None => book.ambiguous = true,
                Some(p) => {
                    let p = u128::from(p);
                    if p > book.tracked {
                        let extra = p - book.tracked;
                        book.lots.push_back(Lot {
                            qty: extra,
                            cost: None,
                        });
                        book.tracked += extra;
                        unknown_cost.insert(token.to_owned());
                        book.open_cycle().tainted = true;
                    } else if p < book.tracked {
                        let _ = book.take(book.tracked - p);
                        if let Some(c) = book.cycle.as_mut() {
                            c.tainted = true;
                        }
                        if book.tracked == 0 {
                            book.cycle = None;
                            out.unevaluated_cycles += 1;
                        }
                    }
                }
            }
        }
        let fee = u128::from(fees.get(t.swap_id.as_str()).copied().unwrap_or(0));
        match s {
            Side::Buy => {
                let (Some(qty), Some(paid)) = (t.output_amount, t.input_amount) else {
                    book.ambiguous = true;
                    continue;
                };
                let qty = u128::from(qty);
                let mut cost = u128::from(paid);
                let c = book.open_cycle();
                if denom == Denomination::Sol {
                    cost += fee;
                } else if fee > 0 {
                    c.incomplete_fees = true;
                }
                c.denoms.insert(denom);
                if qty > 0 {
                    book.lots.push_back(Lot {
                        qty,
                        cost: Some((denom, cost)),
                    });
                    book.tracked += qty;
                }
            }
            Side::Sell => {
                let (Some(qty), Some(got)) = (t.input_amount, t.output_amount) else {
                    book.ambiguous = true;
                    continue;
                };
                let qty = u128::from(qty);
                let mut proceeds = u128::from(got);
                let incomplete = if denom == Denomination::Sol {
                    proceeds = proceeds.saturating_sub(fee);
                    false
                } else {
                    fee > 0
                };
                book.disposals += 1;
                book.disposed_qty += qty;
                let portions = book.take(qty);
                let taken: u128 = portions.iter().map(|(q, _)| *q).sum();
                let fully = taken == qty
                    && portions
                        .iter()
                        .all(|(_, c)| c.is_some_and(|(d, _)| d == denom));
                let matched: u128 = portions
                    .iter()
                    .filter(|(_, c)| c.is_some_and(|(d, _)| d == denom))
                    .map(|(q, _)| *q)
                    .sum();
                book.matched_qty += matched;
                let cost: u128 = portions.iter().filter_map(|(_, c)| c.map(|(_, v)| v)).sum();
                if fully {
                    book.disposals_matched += 1;
                }
                let c = book.open_cycle();
                c.denoms.insert(denom);
                c.incomplete_fees |= incomplete;
                if fully {
                    c.cost += cost;
                    c.proceeds += proceeds;
                } else {
                    c.tainted = true;
                }
                if book.tracked == 0
                    && let Some(c) = book.cycle.take()
                {
                    let single = (c.denoms.len() == 1)
                        .then(|| c.denoms.first().copied())
                        .flatten();
                    match single {
                        Some(d) if !c.tainted && c.cost > 0 => out.outcomes.push(Outcome {
                            mint: token.to_owned(),
                            denomination: d,
                            cost: c.cost,
                            proceeds: c.proceeds,
                            incomplete_fees: c.incomplete_fees,
                        }),
                        _ => out.unevaluated_cycles += 1,
                    }
                }
            }
            Side::Rotate | Side::QuoteOnly => {}
        }
    }
    for (mint, b) in &books {
        out.disposals += b.disposals;
        out.disposals_matched += b.disposals_matched;
        if b.ambiguous {
            out.ambiguous_tokens.push(mint.clone());
        }
        if b.tracked > 0 {
            out.open_positions += 1;
        }
        if let Some(bps) = b
            .matched_qty
            .saturating_mul(10_000)
            .checked_div(b.disposed_qty)
        {
            out.token_coverage_bps
                .insert(mint.clone(), u32::try_from(bps).unwrap_or(10_000));
        }
    }
    out.unknown_cost_tokens = unknown_cost.into_iter().collect();
    for o in &out.outcomes {
        let s = out.by_denomination.entry(o.denomination).or_default();
        s.outcomes += 1;
        s.wins += u32::from(o.is_win());
        s.cost += o.cost;
        s.proceeds += o.proceeds;
        s.incomplete_fees |= o.incomplete_fees;
    }
    out
}

/// Activity in one time window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowActivity {
    pub window: &'static str,
    pub swaps: u32,
    pub buys: u32,
    pub sells: u32,
    pub distinct_tokens: u32,
    pub trading_days: u32,
    /// Swaps without a block time (counted only in the `all` window).
    pub undated: u32,
}

pub const WINDOWS: [(&str, Option<i64>); 5] = [
    ("24h", Some(1)),
    ("7d", Some(7)),
    ("30d", Some(30)),
    ("90d", Some(90)),
    ("all", None),
];

/// Activity per window relative to `now` (UTC trading days).
#[must_use]
pub fn activity(trades: &[Trade], now: OffsetDateTime) -> Vec<WindowActivity> {
    WINDOWS
        .iter()
        .map(|&(name, days)| {
            let mut w = WindowActivity {
                window: name,
                ..WindowActivity::default()
            };
            let mut tokens = BTreeSet::new();
            let mut dates = BTreeSet::new();
            for t in trades {
                let inside = match (t.block_time, days) {
                    (_, None) => true,
                    (Some(bt), Some(d)) => bt <= now && now - bt <= Duration::days(d),
                    (None, Some(_)) => false,
                };
                if !inside {
                    continue;
                }
                w.swaps += 1;
                let (s, _, token) = side(t);
                match s {
                    Side::Buy => w.buys += 1,
                    Side::Sell => w.sells += 1,
                    Side::Rotate | Side::QuoteOnly => {}
                }
                if let Some(tk) = token {
                    tokens.insert(tk.to_owned());
                } else if s == Side::Rotate {
                    tokens.extend(t.input_mint.clone());
                    tokens.extend(t.output_mint.clone());
                }
                match t.block_time {
                    Some(bt) => {
                        dates.insert(bt.date());
                    }
                    None => w.undated += 1,
                }
            }
            w.distinct_tokens = u32::try_from(tokens.len()).unwrap_or(u32::MAX);
            w.trading_days = u32::try_from(dates.len()).unwrap_or(u32::MAX);
            w
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{USDC_MINT, WSOL_MINT};

    const TOK: &str = "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr";

    fn at(slot: u64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000 + i64::try_from(slot).unwrap_or(0) * 3600)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }

    fn buy(slot: u64, sol: u64, qty: u64) -> Trade {
        Trade {
            swap_id: format!("s{slot}"),
            signature: format!("sig{slot}"),
            slot,
            order: (slot, 0, "0".into()),
            block_time: Some(at(slot)),
            input_mint: Some(WSOL_MINT.into()),
            input_kind: "NATIVE_SOL".into(),
            input_amount: Some(sol),
            output_mint: Some(TOK.into()),
            output_kind: "SPL_TOKEN".into(),
            output_amount: Some(qty),
            wallet_paid_fee: true,
            fee_lamports: 5000,
            finalized: true,
        }
    }

    fn sell(slot: u64, qty: u64, sol: u64) -> Trade {
        let mut t = buy(slot, 0, 0);
        t.input_mint = Some(TOK.into());
        t.input_kind = "SPL_TOKEN".into();
        t.input_amount = Some(qty);
        t.output_mint = Some(WSOL_MINT.into());
        t.output_kind = "NATIVE_SOL".into();
        t.output_amount = Some(sol);
        t
    }

    fn pre(entries: &[(u64, Option<u64>)]) -> PreBalances {
        entries
            .iter()
            .map(|&(slot, v)| ((format!("sig{slot}"), TOK.to_owned()), v))
            .collect()
    }

    #[test]
    fn closed_cycle_with_fees_is_one_outcome() {
        let trades = [buy(1, 1_000_000, 100), sell(2, 100, 1_500_000)];
        let s = screen(&trades, &pre(&[(1, Some(0)), (2, Some(100))]));
        assert_eq!(s.outcomes.len(), 1);
        let o = &s.outcomes[0];
        assert_eq!(o.cost, 1_005_000);
        assert_eq!(o.proceeds, 1_495_000);
        assert!(o.is_win());
        assert_eq!(s.matched_coverage_bps(), Some(10_000));
        let primary = s.primary().map(|(d, sum)| (d, sum.return_ratio()));
        assert_eq!(
            primary,
            Some((Denomination::Sol, Some(Decimal::new(487_562, 6))))
        );
    }

    #[test]
    fn preexisting_inventory_has_unknown_cost_and_is_never_zero_cost() {
        // The wallet already held 50 before its first observed buy.
        let trades = [buy(1, 1_000_000, 100), sell(2, 150, 9_000_000)];
        let s = screen(&trades, &pre(&[(1, Some(50)), (2, Some(150))]));
        assert!(s.outcomes.is_empty());
        assert_eq!(s.unevaluated_cycles, 1);
        assert_eq!(s.matched_coverage_bps(), Some(0));
        assert_eq!(s.unknown_cost_tokens, vec![TOK.to_owned()]);
    }

    #[test]
    fn unknown_pre_balance_marks_inventory_ambiguous() {
        let trades = [buy(1, 1_000_000, 100), sell(2, 100, 900_000)];
        let s = screen(&trades, &pre(&[(1, None), (2, Some(100))]));
        assert_eq!(s.ambiguous_tokens, vec![TOK.to_owned()]);
        // The cycle itself still matched (inventory reconciled at the sell).
        assert_eq!(s.outcomes.len(), 1);
        assert!(!s.outcomes[0].is_win());
    }

    #[test]
    fn unobserved_outflow_taints_the_cycle() {
        // 100 bought, only 60 left at the sell: 40 left by transfer.
        let trades = [buy(1, 1_000_000, 100), sell(2, 60, 2_000_000)];
        let s = screen(&trades, &pre(&[(1, Some(0)), (2, Some(60))]));
        assert!(s.outcomes.is_empty());
        assert_eq!(s.unevaluated_cycles, 1);
    }

    #[test]
    fn partial_sells_allocate_cost_exactly() {
        let trades = [
            buy(1, 1_000_000, 3),
            sell(2, 1, 400_000),
            sell(3, 1, 400_000),
            sell(4, 1, 400_000),
        ];
        let s = screen(
            &trades,
            &pre(&[(1, Some(0)), (2, Some(3)), (3, Some(2)), (4, Some(1))]),
        );
        assert_eq!(s.outcomes.len(), 1);
        // Cost is the full buy plus its fee, exactly once.
        assert_eq!(s.outcomes[0].cost, 1_005_000);
        assert_eq!(s.outcomes[0].proceeds, 3 * 395_000);
        assert_eq!(s.disposals, 3);
    }

    #[test]
    fn denominations_are_never_mixed() {
        let mut usdc_sell = sell(2, 100, 2_000_000);
        usdc_sell.output_mint = Some(USDC_MINT.into());
        usdc_sell.output_kind = "SPL_TOKEN".into();
        let trades = [buy(1, 1_000_000, 100), usdc_sell];
        let s = screen(&trades, &pre(&[(1, Some(0)), (2, Some(100))]));
        assert!(s.outcomes.is_empty());
        assert_eq!(s.matched_coverage_bps(), Some(0));
    }

    #[test]
    fn usdc_cycles_flag_incomplete_fees() {
        let mut b = buy(1, 1_000_000, 100);
        b.input_mint = Some(USDC_MINT.into());
        b.input_kind = "SPL_TOKEN".into();
        let mut sl = sell(2, 100, 1_200_000);
        sl.output_mint = Some(USDC_MINT.into());
        sl.output_kind = "SPL_TOKEN".into();
        let s = screen(&[b, sl], &pre(&[(1, Some(0)), (2, Some(100))]));
        assert_eq!(s.outcomes.len(), 1);
        assert!(s.outcomes[0].incomplete_fees);
        assert_eq!(s.outcomes[0].cost, 1_000_000);
    }

    #[test]
    fn fee_counts_only_when_the_wallet_paid_it_and_is_split() {
        let mut a = buy(1, 1_000_000, 100);
        let mut b = buy(1, 1_000_000, 100);
        b.swap_id = "s1b".into();
        b.order.2 = "1".into();
        a.fee_lamports = 5001;
        b.fee_lamports = 5001;
        let both = [a.clone(), b.clone()];
        let f = fee_shares(&both);
        assert_eq!(f.get("s1"), Some(&2501));
        assert_eq!(f.get("s1b"), Some(&2500));
        a.wallet_paid_fee = false;
        b.wallet_paid_fee = false;
        let unpaid = [a, b];
        assert!(fee_shares(&unpaid).is_empty());
    }

    #[test]
    fn open_positions_are_not_outcomes() {
        let s = screen(&[buy(1, 1_000_000, 100)], &pre(&[(1, Some(0))]));
        assert!(s.outcomes.is_empty());
        assert_eq!(s.open_positions, 1);
        assert_eq!(s.matched_coverage_bps(), None);
    }

    #[test]
    fn activity_windows_count_days_and_tokens() {
        let trades = [buy(1, 1, 1), sell(30, 1, 1), buy(200, 1, 1)];
        let now = at(200) + Duration::hours(1);
        let w = activity(&trades, now);
        assert_eq!(w[0].window, "24h");
        assert_eq!(w[0].swaps, 1);
        assert_eq!(w[4].swaps, 3);
        assert_eq!(w[4].distinct_tokens, 1);
        assert!(w[4].trading_days >= 2);
    }
}
