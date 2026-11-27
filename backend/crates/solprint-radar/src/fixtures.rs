//! Synthetic RADAR fixtures for tests and benchmarks. Every address, mint,
//! pool and signature is a deterministic hash of a label (see
//! `solprint_data_engine::fixtures`); nothing here is real chain data, and
//! nothing here is used outside tests and benchmarks.

use solprint_data_engine::fixtures::{TxBuilder, addr, sig};
use solprint_data_engine::programs::{
    COMPUTE_BUDGET_PROGRAM, RAYDIUM_AMM_V4, SPL_TOKEN_PROGRAM, WSOL_MINT, raydium,
};

pub const FIXTURE_MARKER: &str = solprint_data_engine::fixtures::FIXTURE_MARKER;

fn u64s(vals: &[u64]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn spl_transfer(amount: u64) -> Vec<u8> {
    let mut d = vec![3u8];
    d.extend(amount.to_le_bytes());
    d
}

/// One synthetic Raydium AMM v4 `SwapBaseIn` between WSOL and `mint`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapSpec {
    pub label: String,
    pub wallet: String,
    pub pool_label: String,
    pub mint: String,
    /// `true`: WSOL → token; `false`: token → WSOL.
    pub buy: bool,
    pub sol: u64,
    pub tokens: u64,
    /// The wallet's token balance before the swap (`None`: account created).
    pub token_pre: Option<u64>,
    pub slot: u64,
    pub block_time: i64,
}

impl SwapSpec {
    #[must_use]
    pub fn signature(&self) -> String {
        sig(&format!("{}:0", self.label))
    }

    /// Render the transaction as `getTransaction` JSON.
    #[must_use]
    pub fn build(&self) -> String {
        let pool = addr(&self.pool_label);
        let auth = addr(&format!("{}:authority", self.pool_label));
        let vc = addr(&format!("{}:coin_vault", self.pool_label));
        let vp = addr(&format!("{}:pc_vault", self.pool_label));
        let w = &self.wallet;
        let w_wsol = addr(&format!("{w}:wsol"));
        let w_tok = addr(&format!("{w}:{}", self.mint));
        let (src, dst) = if self.buy {
            (w_wsol.clone(), w_tok.clone())
        } else {
            (w_tok.clone(), w_wsol.clone())
        };
        let mut accts: Vec<String> = vec![
            SPL_TOKEN_PROGRAM.into(),
            pool,
            auth.clone(),
            addr(&format!("{}:open_orders", self.pool_label)),
            addr(&format!("{}:target", self.pool_label)),
            vc.clone(),
            vp.clone(),
        ];
        accts.extend((0..7).map(|k| addr(&format!("{}:serum{k}", self.pool_label))));
        accts.extend([src.clone(), dst.clone(), w.clone()]);
        let a: Vec<&str> = accts.iter().map(String::as_str).collect();
        let (amount_in, amount_out) = if self.buy {
            (self.sol, self.tokens)
        } else {
            (self.tokens, self.sol)
        };
        let mut data = vec![raydium::AMM_SWAP_BASE_IN];
        data.extend(u64s(&[amount_in, amount_out]));
        let (in_vault, out_vault) = if self.buy { (&vc, &vp) } else { (&vp, &vc) };
        let wsol_before: u64 = 1_000_000_000_000;
        let (wsol_pre, wsol_post) = if self.buy {
            (wsol_before, wsol_before - self.sol)
        } else {
            (wsol_before, wsol_before + self.sol)
        };
        let tok_pre = self.token_pre;
        let tok_post = if self.buy {
            tok_pre.unwrap_or(0) + self.tokens
        } else {
            tok_pre.unwrap_or(0).saturating_sub(self.tokens)
        };
        let vault_big: u64 = 1_000_000_000_000_000;
        let (vc_pre, vc_post, vp_pre, vp_post) = if self.buy {
            (
                vault_big,
                vault_big + self.sol,
                vault_big,
                vault_big - self.tokens,
            )
        } else {
            (
                vault_big,
                vault_big - self.sol,
                vault_big,
                vault_big + self.tokens,
            )
        };
        TxBuilder::new(&self.label, w)
            .at(self.slot, self.block_time)
            .ix(COMPUTE_BUDGET_PROGRAM, &[], vec![2, 0x40, 0x0d, 0x03, 0])
            .ix(RAYDIUM_AMM_V4, &a, data)
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&src, in_vault, w],
                spl_transfer(amount_in),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[out_vault, &dst, &auth],
                spl_transfer(amount_out),
            )
            .token(&w_wsol, WSOL_MINT, w, 9, Some(wsol_pre), Some(wsol_post))
            .token(&w_tok, &self.mint, w, 6, tok_pre, Some(tok_post))
            .token(&vc, WSOL_MINT, &auth, 9, Some(vc_pre), Some(vc_post))
            .token(&vp, &self.mint, &auth, 6, Some(vp_pre), Some(vp_post))
            .build()
    }
}

/// A profitable round-trip trader: `cycles` buy/sell pairs of `mint`, one
/// pair every `spacing_hours`, ending `end_hours_ago` before `now_unix`.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn round_trips(
    wallet_label: &str,
    pool_label: &str,
    mint: &str,
    cycles: u32,
    spacing_hours: i64,
    end_hours_ago: i64,
    now_unix: i64,
    base_slot: u64,
) -> Vec<SwapSpec> {
    let wallet = addr(wallet_label);
    let mut out = Vec::new();
    for c in 0..cycles {
        let back = end_hours_ago + i64::from(cycles - 1 - c) * spacing_hours;
        let t = now_unix - back * 3600;
        let slot = base_slot + u64::from(c) * 10;
        out.push(SwapSpec {
            label: format!("{wallet_label}:buy:{c}"),
            wallet: wallet.clone(),
            pool_label: pool_label.to_owned(),
            mint: mint.to_owned(),
            buy: true,
            sol: 1_000_000_000,
            tokens: 1_000_000,
            token_pre: None,
            slot,
            block_time: t,
        });
        out.push(SwapSpec {
            label: format!("{wallet_label}:sell:{c}"),
            wallet: wallet.clone(),
            pool_label: pool_label.to_owned(),
            mint: mint.to_owned(),
            buy: false,
            sol: 1_200_000_000,
            tokens: 1_000_000,
            token_pre: Some(1_000_000),
            slot: slot + 1,
            block_time: t + 600,
        });
    }
    out
}
