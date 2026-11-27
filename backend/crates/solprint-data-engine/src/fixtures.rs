//! SYNTHETIC FIXTURES — not real chain data.
//!
//! Builders for `getTransaction`-shaped JSON used by tests, benchmarks and
//! the `data-engine decode-fixture` CLI. Program ids and instruction layouts
//! are the real, verified ones; every wallet, mint, pool, token account and
//! signature is derived from a SHA-256 of a label, so no fixture can be
//! mistaken for (or collide with) a real account. Each document carries a
//! `"_fixture"` marker and is reported with provider `FIXTURE`.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::programs::{
    ANCHOR_EVENT_IX_TAG_LE, ASSOCIATED_TOKEN_PROGRAM, COMPUTE_BUDGET_PROGRAM, JUPITER_V6,
    ORCA_WHIRLPOOL, PUMP_FUN, PUMPSWAP, RAYDIUM_AMM_V4, RAYDIUM_CLMM, RAYDIUM_CPMM,
    SPL_TOKEN_PROGRAM, SYSTEM_PROGRAM, TOKEN_2022_PROGRAM, WSOL_MINT, jupiter, orca, pump,
    pumpswap, raydium,
};

pub const FIXTURE_MARKER: &str = "SYNTHETIC FIXTURE — not real chain data";

/// Deterministic synthetic 32-byte address for a label.
#[must_use]
pub fn addr(label: &str) -> String {
    let d = Sha256::digest(format!("solprint-fixture:{label}").as_bytes());
    bs58::encode(d).into_string()
}

/// Deterministic synthetic 64-byte signature for a label.
#[must_use]
pub fn sig(label: &str) -> String {
    let a = Sha256::digest(format!("solprint-fixture-sig-a:{label}").as_bytes());
    let b = Sha256::digest(format!("solprint-fixture-sig-b:{label}").as_bytes());
    let mut out = a.to_vec();
    out.extend_from_slice(&b);
    bs58::encode(out).into_string()
}

#[derive(Debug, Clone)]
struct IxSpec {
    program: String,
    accounts: Vec<String>,
    data: Vec<u8>,
    /// (stack height, program, accounts, data)
    inner: Vec<(u32, String, Vec<String>, Vec<u8>)>,
}

#[derive(Debug, Clone)]
struct TokenBal {
    account: String,
    mint: String,
    owner: String,
    program: String,
    decimals: u8,
    pre: Option<u64>,
    post: Option<u64>,
}

/// Builder for one synthetic transaction.
#[derive(Debug, Clone)]
pub struct TxBuilder {
    label: String,
    signers: Vec<String>,
    lookup_writable: Vec<String>,
    lookup_readonly: Vec<String>,
    ixs: Vec<IxSpec>,
    lamports: BTreeMap<String, (u64, u64)>,
    tokens: Vec<TokenBal>,
    fee: u64,
    failed: bool,
    record_inner: bool,
    record_stack_height: bool,
    slot: u64,
}

impl TxBuilder {
    #[must_use]
    pub fn new(label: &str, fee_payer: &str) -> Self {
        Self {
            label: label.to_owned(),
            signers: vec![fee_payer.to_owned()],
            lookup_writable: Vec::new(),
            lookup_readonly: Vec::new(),
            ixs: Vec::new(),
            lamports: BTreeMap::new(),
            tokens: Vec::new(),
            fee: 5_000,
            failed: false,
            record_inner: true,
            record_stack_height: true,
            slot: 300_000_000,
        }
    }

    #[must_use]
    pub fn signer(mut self, k: &str) -> Self {
        if !self.signers.iter().any(|s| s == k) {
            self.signers.push(k.to_owned());
        }
        self
    }

    /// Load `k` through an address lookup table (makes the tx v0).
    #[must_use]
    pub fn via_lookup(mut self, k: &str, writable: bool) -> Self {
        if writable {
            self.lookup_writable.push(k.to_owned())
        } else {
            self.lookup_readonly.push(k.to_owned())
        }
        self
    }

    #[must_use]
    pub fn fee(mut self, lamports: u64) -> Self {
        self.fee = lamports;
        self
    }

    #[must_use]
    pub fn failed(mut self) -> Self {
        self.failed = true;
        self
    }

    #[must_use]
    pub fn without_inner(mut self) -> Self {
        self.record_inner = false;
        self
    }

    #[must_use]
    pub fn without_stack_height(mut self) -> Self {
        self.record_stack_height = false;
        self
    }

    #[must_use]
    pub fn ix(mut self, program: &str, accounts: &[&str], data: Vec<u8>) -> Self {
        self.ixs.push(IxSpec {
            program: program.to_owned(),
            accounts: accounts.iter().map(|s| (*s).to_owned()).collect(),
            data,
            inner: Vec::new(),
        });
        self
    }

    /// Add a CPI under the last top-level instruction.
    #[must_use]
    pub fn cpi(mut self, height: u32, program: &str, accounts: &[&str], data: Vec<u8>) -> Self {
        if let Some(last) = self.ixs.last_mut() {
            last.inner.push((
                height,
                program.to_owned(),
                accounts.iter().map(|s| (*s).to_owned()).collect(),
                data,
            ));
        }
        self
    }

    #[must_use]
    pub fn lamports(mut self, k: &str, pre: u64, post: u64) -> Self {
        self.lamports.insert(k.to_owned(), (pre, post));
        self
    }

    /// Record a token balance (`None` side = account absent: created/closed).
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn token(
        mut self,
        account: &str,
        mint: &str,
        owner: &str,
        decimals: u8,
        pre: Option<u64>,
        post: Option<u64>,
    ) -> Self {
        self.tokens.push(TokenBal {
            account: account.to_owned(),
            mint: mint.to_owned(),
            owner: owner.to_owned(),
            program: SPL_TOKEN_PROGRAM.to_owned(),
            decimals,
            pre,
            post,
        });
        self
    }

    #[must_use]
    pub fn token_2022(
        mut self,
        account: &str,
        mint: &str,
        owner: &str,
        decimals: u8,
        pre: Option<u64>,
        post: Option<u64>,
    ) -> Self {
        self.tokens.push(TokenBal {
            account: account.to_owned(),
            mint: mint.to_owned(),
            owner: owner.to_owned(),
            program: TOKEN_2022_PROGRAM.to_owned(),
            decimals,
            pre,
            post,
        });
        self
    }

    /// Render `getTransaction` JSON (`encoding: "json"`).
    #[must_use]
    pub fn build(&self) -> String {
        // Account ordering: writable signers, readonly signers (none here),
        // writable non-signers, readonly non-signers (programs and accounts
        // only ever read), then lookup-loaded writable, lookup readonly.
        let lookup: Vec<&String> = self
            .lookup_writable
            .iter()
            .chain(&self.lookup_readonly)
            .collect();
        let mut writable = Vec::<String>::new();
        let mut readonly = Vec::<String>::new();
        let programs: Vec<&str> = self
            .ixs
            .iter()
            .map(|i| i.program.as_str())
            .chain(
                self.ixs
                    .iter()
                    .flat_map(|i| i.inner.iter().map(|x| x.1.as_str())),
            )
            .collect();
        let seen = |k: &String, w: &mut Vec<String>, r: &mut Vec<String>| {
            if self.signers.contains(k) || lookup.contains(&k) || w.contains(k) || r.contains(k) {
                return;
            }
            if programs.contains(&k.as_str()) || *k == WSOL_MINT || k.starts_with("Sysvar") {
                r.push(k.clone());
            } else {
                w.push(k.clone());
            }
        };
        for i in &self.ixs {
            for a in &i.accounts {
                seen(a, &mut writable, &mut readonly);
            }
            for (_, _, accts, _) in &i.inner {
                for a in accts {
                    seen(a, &mut writable, &mut readonly);
                }
            }
        }
        for i in &self.ixs {
            seen(&i.program, &mut writable, &mut readonly);
            for (_, p, _, _) in &i.inner {
                seen(p, &mut writable, &mut readonly);
            }
        }
        for t in &self.tokens {
            seen(&t.account, &mut writable, &mut readonly);
        }
        let static_keys: Vec<String> = self
            .signers
            .iter()
            .cloned()
            .chain(writable)
            .chain(readonly.iter().cloned())
            .collect();
        let all: Vec<String> = static_keys
            .iter()
            .cloned()
            .chain(self.lookup_writable.iter().cloned())
            .chain(self.lookup_readonly.iter().cloned())
            .collect();
        let idx = |k: &str| all.iter().position(|a| a == k).unwrap_or(0);
        let ix_json = |p: &str, accts: &[String], data: &[u8]| {
            json!({
                "programIdIndex": idx(p),
                "accounts": accts.iter().map(|a| idx(a)).collect::<Vec<_>>(),
                "data": bs58::encode(data).into_string(),
            })
        };
        let instructions: Vec<Value> = self
            .ixs
            .iter()
            .map(|i| ix_json(&i.program, &i.accounts, &i.data))
            .collect();
        let inner: Vec<Value> = self
            .ixs
            .iter()
            .enumerate()
            .filter(|(_, i)| !i.inner.is_empty())
            .map(|(n, i)| {
                json!({
                    "index": n,
                    "instructions": i.inner.iter().map(|(h, p, a, d)| {
                        let mut v = ix_json(p, a, d);
                        if self.record_stack_height {
                            v["stackHeight"] = json!(h);
                        }
                        v
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        let pre: Vec<u64> = all
            .iter()
            .map(|k| self.lamports.get(k).map_or(1_000_000, |v| v.0))
            .collect();
        let mut post: Vec<u64> = all
            .iter()
            .map(|k| self.lamports.get(k).map_or(1_000_000, |v| v.1))
            .collect();
        if !self.lamports.contains_key(&self.signers[0]) {
            post[0] = post[0].saturating_sub(self.fee);
        }
        let tb = |pick: fn(&TokenBal) -> Option<u64>| -> Vec<Value> {
            self.tokens
                .iter()
                .filter_map(|t| {
                    let amount = pick(t)?;
                    Some(json!({
                        "accountIndex": idx(&t.account),
                        "mint": t.mint,
                        "owner": t.owner,
                        "programId": t.program,
                        "uiTokenAmount": {"amount": amount.to_string(), "decimals": t.decimals, "uiAmountString": "fixture"},
                    }))
                })
                .collect()
        };
        let v0 = !lookup.is_empty();
        let mut message = json!({
            "header": {
                "numRequiredSignatures": self.signers.len(),
                "numReadonlySignedAccounts": 0,
                "numReadonlyUnsignedAccounts": readonly.len(),
            },
            "accountKeys": static_keys,
            "recentBlockhash": addr(&format!("{}:blockhash", self.label)),
            "instructions": instructions,
        });
        if v0 {
            message["addressTableLookups"] = json!([{
                "accountKey": addr(&format!("{}:alt", self.label)),
                "writableIndexes": (0..self.lookup_writable.len()).collect::<Vec<_>>(),
                "readonlyIndexes": (self.lookup_writable.len()..lookup.len()).collect::<Vec<_>>(),
            }]);
        }
        let mut meta = json!({
            "err": if self.failed { json!({"InstructionError": [0, {"Custom": 6001}]}) } else { Value::Null },
            "fee": self.fee,
            "preBalances": pre,
            "postBalances": post,
            "preTokenBalances": tb(|t| t.pre),
            "postTokenBalances": tb(|t| t.post),
            "logMessages": ["Program log: synthetic fixture"],
            "computeUnitsConsumed": 150_000,
            "loadedAddresses": {"writable": self.lookup_writable, "readonly": self.lookup_readonly},
        });
        if self.record_inner {
            meta["innerInstructions"] = json!(inner);
        } else {
            meta["innerInstructions"] = Value::Null;
        }
        let doc = json!({
            "_fixture": FIXTURE_MARKER,
            "slot": self.slot,
            "blockTime": 1_790_000_000,
            "version": if v0 { json!(0) } else { json!("legacy") },
            "transaction": {"signatures": self.signers.iter().enumerate().map(|(n, _)| sig(&format!("{}:{n}", self.label))).collect::<Vec<_>>(), "message": message},
            "meta": meta,
        });
        doc.to_string()
    }
}

// ---- instruction data helpers ----------------------------------------------------

#[must_use]
pub fn spl_transfer(amount: u64) -> Vec<u8> {
    let mut d = vec![3];
    d.extend_from_slice(&amount.to_le_bytes());
    d
}

#[must_use]
pub fn spl_transfer_checked(amount: u64, decimals: u8) -> Vec<u8> {
    let mut d = vec![12];
    d.extend_from_slice(&amount.to_le_bytes());
    d.push(decimals);
    d
}

#[must_use]
pub fn transfer_checked_with_fee(amount: u64, decimals: u8, fee: u64) -> Vec<u8> {
    let mut d = vec![26, 1];
    d.extend_from_slice(&amount.to_le_bytes());
    d.push(decimals);
    d.extend_from_slice(&fee.to_le_bytes());
    d
}

#[must_use]
pub fn system_transfer(lamports: u64) -> Vec<u8> {
    let mut d = 2_u32.to_le_bytes().to_vec();
    d.extend_from_slice(&lamports.to_le_bytes());
    d
}

#[must_use]
pub fn anchor(disc: [u8; 8], args: &[u8]) -> Vec<u8> {
    let mut d = disc.to_vec();
    d.extend_from_slice(args);
    d
}

#[must_use]
pub fn event(disc: [u8; 8], body: &[u8]) -> Vec<u8> {
    let mut d = ANCHOR_EVENT_IX_TAG_LE.to_vec();
    d.extend_from_slice(&disc);
    d.extend_from_slice(body);
    d
}

fn pk(a: &str) -> Vec<u8> {
    bs58::decode(a).into_vec().unwrap_or_else(|_| vec![0; 32])
}

fn u64s(vals: &[u64]) -> Vec<u8> {
    vals.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A Pump.fun `TradeEvent` body (current layout, SOL quote).
#[must_use]
pub fn pump_trade_event(
    mint: &str,
    sol: u64,
    tokens: u64,
    is_buy: bool,
    user: &str,
    fee: u64,
    creator_fee: u64,
) -> Vec<u8> {
    let mut d = pk(mint);
    d.extend(u64s(&[sol, tokens]));
    d.push(u8::from(is_buy));
    d.extend(pk(user));
    d.extend(u64s(&[
        1_790_000_000,
        30_000_000_000,
        1_000_000_000_000_000,
        5_000_000_000,
        800_000_000_000_000,
    ]));
    d.extend(pk(&addr("pump:fee_recipient")));
    d.extend(u64s(&[95, fee]));
    d.extend(pk(&addr("pump:creator")));
    d.extend(u64s(&[5, creator_fee]));
    d.push(0); // track_volume
    d.extend(u64s(&[0, 0, 0, 0]));
    let name: &[u8] = if is_buy { b"buy" } else { b"sell" };
    d.extend_from_slice(&u32::try_from(name.len()).unwrap_or(0).to_le_bytes());
    d.extend_from_slice(name);
    d.push(0); // mayhem_mode
    d.extend(u64s(&[0, 0, 0, 0]));
    d.extend_from_slice(&0_u32.to_le_bytes()); // shareholders
    d.extend([0_u8; 32]); // quote_mint = default (SOL)
    d.extend(u64s(&[sol, 0, 0, 0, 0, 0]));
    d
}

/// A PumpSwap Buy/SellEvent body with the offsets the decoder reads.
#[must_use]
pub fn pumpswap_event(
    base_amount: u64,
    user_quote_amount: u64,
    pool: &str,
    user: &str,
    lp_fee: u64,
    protocol_fee: u64,
    creator_fee: u64,
) -> Vec<u8> {
    let mut d = u64s(&[
        1_790_000_000,
        base_amount,
        0,
        0,
        0,
        0,
        0,
        user_quote_amount,
        20,
        lp_fee,
        5,
        protocol_fee,
        0,
        user_quote_amount,
    ]);
    d.extend(pk(pool));
    d.extend(pk(user));
    d.extend(pk(&addr("ps:user_base")));
    d.extend(pk(&addr("ps:user_quote")));
    d.extend(pk(&addr("ps:protocol_fee_recipient")));
    d.extend(pk(&addr("ps:protocol_fee_recipient_ata")));
    d.extend(pk(&addr("ps:coin_creator")));
    d.extend(u64s(&[5, creator_fee]));
    d
}

/// A Jupiter `SwapEvent` body.
#[must_use]
pub fn jupiter_swap_event(
    amm: &str,
    input_mint: &str,
    input: u64,
    output_mint: &str,
    output: u64,
) -> Vec<u8> {
    let mut d = pk(amm);
    d.extend(pk(input_mint));
    d.extend(u64s(&[input]));
    d.extend(pk(output_mint));
    d.extend(u64s(&[output]));
    d
}

/// Expected outcome of decoding a fixture, asserted by the tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expect {
    pub swaps: usize,
    pub protocol: Option<&'static str>,
    pub input_amount: Option<u64>,
    pub output_amount: Option<u64>,
    pub swap_status: Option<&'static str>,
    pub tx_status: &'static str,
    pub verified_wallet: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Fixture {
    pub name: &'static str,
    pub description: &'static str,
    pub json: String,
    pub expect: Expect,
}

const USDC_DECIMALS: u8 = 6;

/// The full synthetic catalogue.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn catalog() -> Vec<Fixture> {
    let user = addr("user");
    let usdc = addr("mint:usdc");
    let meme = addr("mint:meme");
    let mid = addr("mint:mid");
    let user_wsol = addr("user:wsol");
    let user_usdc = addr("user:usdc");
    let user_meme = addr("user:meme");
    let mut out = Vec::new();
    let exp = |swaps, protocol, i, o, s, t, w: Option<&String>| Expect {
        swaps,
        protocol,
        input_amount: i,
        output_amount: o,
        swap_status: s,
        tx_status: t,
        verified_wallet: w.cloned(),
    };

    // --- Raydium AMM v4 SwapBaseIn (17 accounts) -----------------------------
    {
        let pool = addr("ray:amm");
        let auth = addr("ray:amm_authority");
        let (vc, vp) = (addr("ray:coin_vault"), addr("ray:pc_vault"));
        let mut accts: Vec<String> = vec![
            SPL_TOKEN_PROGRAM.into(),
            pool.clone(),
            auth.clone(),
            addr("ray:open_orders"),
            addr("ray:target"),
            vc.clone(),
            vp.clone(),
        ];
        accts.extend((0..7).map(|k| addr(&format!("ray:serum{k}"))));
        accts.extend([user_wsol.clone(), user_usdc.clone(), user.clone()]);
        let a: Vec<&str> = accts.iter().map(String::as_str).collect();
        let mut data = vec![raydium::AMM_SWAP_BASE_IN];
        data.extend(u64s(&[1_000_000_000, 140_000_000]));
        let json = TxBuilder::new("ray-amm", &user)
            .ix(COMPUTE_BUDGET_PROGRAM, &[], vec![2, 0x40, 0x0d, 0x03, 0])
            .ix(RAYDIUM_AMM_V4, &a, data)
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_wsol, &vc, &user],
                spl_transfer(1_000_000_000),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&vp, &user_usdc, &auth],
                spl_transfer(150_000_000),
            )
            .token(
                &user_wsol,
                WSOL_MINT,
                &user,
                9,
                Some(2_000_000_000),
                Some(1_000_000_000),
            )
            .token(
                &user_usdc,
                &usdc,
                &user,
                USDC_DECIMALS,
                Some(0),
                Some(150_000_000),
            )
            .token(
                &vc,
                WSOL_MINT,
                &auth,
                9,
                Some(50_000_000_000),
                Some(51_000_000_000),
            )
            .token(
                &vp,
                &usdc,
                &auth,
                USDC_DECIMALS,
                Some(9_000_000_000),
                Some(8_850_000_000),
            )
            .build();
        out.push(Fixture {
            name: "raydium_amm_v4_swap",
            description: "Raydium AMM v4 SwapBaseIn, WSOL -> USDC, user signs",
            json,
            expect: exp(
                1,
                Some("RAYDIUM_AMM_V4"),
                Some(1_000_000_000),
                Some(150_000_000),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Raydium CPMM swap_base_input -----------------------------------------
    {
        let pool = addr("cpmm:pool");
        let auth = addr("cpmm:authority");
        let (vi, vo) = (addr("cpmm:vault_in"), addr("cpmm:vault_out"));
        let accts = [
            user.as_str(),
            &auth,
            &addr("cpmm:config"),
            &pool,
            &user_usdc,
            &user_meme,
            &vi,
            &vo,
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("cpmm", &user)
            .ix(
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[25_000_000, 1])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &usdc, &vi, &user],
                spl_transfer_checked(25_000_000, 6),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&vo, &meme, &user_meme, &auth],
                spl_transfer_checked(4_200_000_000, 6),
            )
            .token(
                &user_usdc,
                &usdc,
                &user,
                6,
                Some(30_000_000),
                Some(5_000_000),
            )
            .token(&user_meme, &meme, &user, 6, None, Some(4_200_000_000))
            .build();
        out.push(Fixture {
            name: "raydium_cpmm_swap",
            description: "Raydium CPMM swap_base_input USDC -> MEME into a new token account",
            json,
            expect: exp(
                1,
                Some("RAYDIUM_CPMM"),
                Some(25_000_000),
                Some(4_200_000_000),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Raydium CLMM swap_v2 via an address lookup table (v0) --------------
    {
        let pool = addr("clmm:pool");
        let (vi, vo) = (addr("clmm:vault_in"), addr("clmm:vault_out"));
        let accts = [
            user.as_str(),
            &addr("clmm:config"),
            &pool,
            &user_usdc,
            &user_wsol,
            &vi,
            &vo,
            &addr("clmm:observation"),
            SPL_TOKEN_PROGRAM,
            TOKEN_2022_PROGRAM,
            &addr("clmm:memo"),
            &usdc,
            WSOL_MINT,
        ];
        let mut args = u64s(&[10_000_000, 0]);
        args.extend([0_u8; 16]);
        args.push(1);
        let json = TxBuilder::new("clmm", &user)
            .via_lookup(&vi, true)
            .via_lookup(&vo, true)
            .ix(RAYDIUM_CLMM, &accts, anchor(raydium::CLMM_SWAP_V2, &args))
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &usdc, &vi, &user],
                spl_transfer_checked(10_000_000, 6),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&vo, WSOL_MINT, &user_wsol, &pool],
                spl_transfer_checked(66_000_000, 9),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(10_000_000), Some(0))
            .token(&user_wsol, WSOL_MINT, &user, 9, Some(0), Some(66_000_000))
            .build();
        out.push(Fixture {
            name: "raydium_clmm_swap_v2_alt",
            description: "Raydium CLMM swap_v2, vaults loaded from an address lookup table",
            json,
            expect: exp(
                1,
                Some("RAYDIUM_CLMM"),
                Some(10_000_000),
                Some(66_000_000),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Orca Whirlpool swap (b -> a) --------------------------------------------
    {
        let pool = addr("orca:pool");
        let (va, vb) = (addr("orca:vault_a"), addr("orca:vault_b"));
        let accts = [
            SPL_TOKEN_PROGRAM,
            user.as_str(),
            &pool,
            &user_wsol,
            &va,
            &user_usdc,
            &vb,
            &addr("orca:ta0"),
            &addr("orca:ta1"),
            &addr("orca:ta2"),
            &addr("orca:oracle"),
        ];
        let mut args = u64s(&[3_000_000, 0]);
        args.extend([0_u8; 16]);
        args.extend([1, 0]); // amount_specified_is_input, a_to_b = false
        let json = TxBuilder::new("orca", &user)
            .ix(ORCA_WHIRLPOOL, &accts, anchor(orca::SWAP, &args))
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &vb, &user],
                spl_transfer(3_000_000),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&va, &user_wsol, &pool],
                spl_transfer(20_000_000),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(3_000_000), Some(0))
            .token(&user_wsol, WSOL_MINT, &user, 9, Some(5), Some(20_000_005))
            .token(
                &va,
                WSOL_MINT,
                &pool,
                9,
                Some(900_000_000),
                Some(880_000_000),
            )
            .token(&vb, &usdc, &pool, 6, Some(0), Some(3_000_000))
            .build();
        out.push(Fixture {
            name: "orca_whirlpool_swap",
            description: "Orca Whirlpool swap with a_to_b = false (USDC -> WSOL)",
            json,
            expect: exp(
                1,
                Some("ORCA_WHIRLPOOL"),
                Some(3_000_000),
                Some(20_000_000),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Orca two_hop_swap ------------------------------------------------------------
    {
        let (p1, p2) = (addr("orca2:pool1"), addr("orca2:pool2"));
        let (v1a, v1b, v2a, v2b) = (
            addr("orca2:v1a"),
            addr("orca2:v1b"),
            addr("orca2:v2a"),
            addr("orca2:v2b"),
        );
        let user_mid = addr("user:mid");
        let accts = [
            SPL_TOKEN_PROGRAM,
            user.as_str(),
            &p1,
            &p2,
            &user_usdc,
            &v1a,
            &user_mid,
            &v1b,
            &user_mid,
            &v2a,
            &user_meme,
            &v2b,
            &addr("orca2:t0"),
            &addr("orca2:t1"),
            &addr("orca2:t2"),
            &addr("orca2:t3"),
            &addr("orca2:t4"),
            &addr("orca2:t5"),
            &addr("orca2:o1"),
            &addr("orca2:o2"),
        ];
        let mut args = u64s(&[7_000_000, 0]);
        args.extend([1, 1, 1]); // is_input, a_to_b_one, a_to_b_two
        args.extend([0_u8; 32]);
        let json = TxBuilder::new("orca-2hop", &user)
            .ix(ORCA_WHIRLPOOL, &accts, anchor(orca::TWO_HOP_SWAP, &args))
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &v1a, &user],
                spl_transfer(7_000_000),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&v1b, &user_mid, &p1],
                spl_transfer(900),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_mid, &v2a, &user],
                spl_transfer(900),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&v2b, &user_meme, &p2],
                spl_transfer(12_345),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(7_000_000), Some(0))
            .token(&user_mid, &mid, &user, 4, Some(0), Some(0))
            .token(&v1b, &mid, &p1, 4, Some(100_000), Some(99_100))
            .token(&user_meme, &meme, &user, 6, Some(0), Some(12_345))
            .build();
        out.push(Fixture {
            name: "orca_two_hop_swap",
            description: "Orca two_hop_swap USDC -> MID -> MEME with two reconciled legs",
            json,
            expect: exp(
                1,
                Some("ORCA_WHIRLPOOL"),
                Some(7_000_000),
                Some(12_345),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Pump.fun buy with TradeEvent self-CPI -------------------------------------------
    {
        let curve = addr("pump:curve");
        let curve_ata = addr("pump:curve_ata");
        let accts = [
            addr("pump:global"),
            addr("pump:fee_recipient"),
            meme.clone(),
            curve.clone(),
            curve_ata.clone(),
            user_meme.clone(),
            user.clone(),
            SYSTEM_PROGRAM.into(),
            SPL_TOKEN_PROGRAM.into(),
            addr("pump:creator_vault"),
            addr("pump:event_authority"),
            PUMP_FUN.into(),
        ];
        let a: Vec<&str> = accts.iter().map(String::as_str).collect();
        let ev = pump_trade_event(
            &meme,
            500_000_000,
            17_000_000_000_000,
            true,
            &user,
            5_000_000,
            250_000,
        );
        let json = TxBuilder::new("pump-buy", &user)
            .ix(
                PUMP_FUN,
                &a,
                anchor(pump::BUY, &u64s(&[17_000_000_000_000, 600_000_000])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&curve_ata, &user_meme, &curve],
                spl_transfer(17_000_000_000_000),
            )
            .cpi(
                2,
                SYSTEM_PROGRAM,
                &[&user, &curve],
                system_transfer(500_000_000),
            )
            .cpi(
                2,
                SYSTEM_PROGRAM,
                &[&user, &addr("pump:fee_recipient")],
                system_transfer(5_000_000),
            )
            .cpi(
                2,
                PUMP_FUN,
                &[&addr("pump:event_authority")],
                event(pump::EVENT_TRADE, &ev),
            )
            .token(&user_meme, &meme, &user, 6, None, Some(17_000_000_000_000))
            .build();
        out.push(Fixture {
            name: "pump_fun_buy",
            description: "Pump.fun buy: SOL -> MEME, amounts from the TradeEvent self-CPI",
            json,
            expect: exp(
                1,
                Some("PUMP_FUN"),
                Some(500_000_000),
                Some(17_000_000_000_000),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- PumpSwap sell with SellEvent -------------------------------------------------------
    {
        let pool = addr("ps:pool");
        let (pb, pq) = (addr("ps:pool_base"), addr("ps:pool_quote"));
        let accts = [
            pool.as_str(),
            &user,
            &addr("ps:config"),
            &meme,
            WSOL_MINT,
            &user_meme,
            &user_wsol,
            &pb,
            &pq,
            &addr("ps:protocol_fee_recipient"),
        ];
        let ev = pumpswap_event(1_000_000_000, 2_490_000, &pool, &user, 5_000, 1_000, 500);
        let json = TxBuilder::new("pumpswap-sell", &user)
            .ix(
                PUMPSWAP,
                &accts,
                anchor(pumpswap::SELL, &u64s(&[1_000_000_000, 1])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_meme, &meme, &pb, &user],
                spl_transfer_checked(1_000_000_000, 6),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&pq, WSOL_MINT, &user_wsol, &pool],
                spl_transfer_checked(2_490_000, 9),
            )
            .cpi(
                2,
                PUMPSWAP,
                &[&addr("ps:event_authority")],
                event(pumpswap::EVENT_SELL, &ev),
            )
            .token(&user_meme, &meme, &user, 6, Some(1_000_000_000), Some(0))
            .token(&user_wsol, WSOL_MINT, &user, 9, Some(0), Some(2_490_000))
            .build();
        out.push(Fixture {
            name: "pumpswap_sell",
            description: "PumpSwap sell: MEME -> WSOL with explicit LP/protocol/creator fees",
            json,
            expect: exp(
                1,
                Some("PUMPSWAP"),
                Some(1_000_000_000),
                Some(2_490_000),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Jupiter route through two venues (route legs, not separate swaps) ---------------
    {
        let user_mid = addr("user:mid");
        let ray_pool = addr("jup:ray_pool");
        let (rv_in, rv_out) = (addr("jup:ray_vin"), addr("jup:ray_vout"));
        let orca_pool = addr("jup:orca_pool");
        let (ov_a, ov_b) = (addr("jup:orca_va"), addr("jup:orca_vb"));
        // route: token_program, user_transfer_authority, user_source, user_destination,
        // destination_token_account (None = program id), destination_mint, platform_fee_account (None), event_authority, program
        let accts = [
            SPL_TOKEN_PROGRAM,
            user.as_str(),
            &user_usdc,
            &user_meme,
            JUPITER_V6,
            &meme,
            JUPITER_V6,
            &addr("jup:event_authority"),
            JUPITER_V6,
        ];
        let mut args = 2_u32.to_le_bytes().to_vec(); // route_plan len (opaque to the decoder)
        args.extend([7_u8; 6]);
        args.extend(u64s(&[50_000_000, 900_000]));
        args.extend(50_u16.to_le_bytes());
        args.push(0);
        let mut ray: Vec<String> = vec![
            SPL_TOKEN_PROGRAM.into(),
            ray_pool.clone(),
            addr("jup:ray_auth"),
        ];
        ray.extend((0..11).map(|k| addr(&format!("jup:ray{k}"))));
        ray.extend([user_usdc.clone(), user_mid.clone(), user.clone()]);
        let ray_a: Vec<&str> = ray.iter().map(String::as_str).collect();
        let mut ray_data = vec![raydium::AMM_SWAP_BASE_IN];
        ray_data.extend(u64s(&[50_000_000, 0]));
        let orca_accts = [
            SPL_TOKEN_PROGRAM,
            user.as_str(),
            &orca_pool,
            &user_mid,
            &ov_a,
            &user_meme,
            &ov_b,
            &addr("jup:o0"),
            &addr("jup:o1"),
            &addr("jup:o2"),
            &addr("jup:oo"),
        ];
        let mut orca_args = u64s(&[0, 0]);
        orca_args.extend([0_u8; 16]);
        orca_args.extend([1, 1]);
        let json = TxBuilder::new("jupiter-route", &user)
            .ix(JUPITER_V6, &accts, anchor(jupiter::ROUTE, &args))
            .cpi(2, RAYDIUM_AMM_V4, &ray_a, ray_data)
            .cpi(
                3,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &rv_in, &user],
                spl_transfer(50_000_000),
            )
            .cpi(
                3,
                SPL_TOKEN_PROGRAM,
                &[&rv_out, &user_mid, &addr("jup:ray_auth")],
                spl_transfer(4_000),
            )
            .cpi(
                2,
                JUPITER_V6,
                &[&addr("jup:event_authority")],
                event(
                    jupiter::EVENT_SWAP,
                    &jupiter_swap_event(RAYDIUM_AMM_V4, &usdc, 50_000_000, &mid, 4_000),
                ),
            )
            .cpi(
                2,
                ORCA_WHIRLPOOL,
                &orca_accts,
                anchor(orca::SWAP, &orca_args),
            )
            .cpi(
                3,
                SPL_TOKEN_PROGRAM,
                &[&user_mid, &ov_a, &user],
                spl_transfer(4_000),
            )
            .cpi(
                3,
                SPL_TOKEN_PROGRAM,
                &[&ov_b, &user_meme, &orca_pool],
                spl_transfer(1_000_000),
            )
            .cpi(
                2,
                JUPITER_V6,
                &[&addr("jup:event_authority")],
                event(
                    jupiter::EVENT_SWAP,
                    &jupiter_swap_event(ORCA_WHIRLPOOL, &mid, 4_000, &meme, 1_000_000),
                ),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(50_000_000), Some(0))
            .token(&user_mid, &mid, &user, 4, Some(0), Some(0))
            .token(&user_meme, &meme, &user, 6, Some(0), Some(1_000_000))
            .build();
        out.push(Fixture { name: "jupiter_route_two_legs", description: "Jupiter route USDC -> MID (Raydium) -> MEME (Orca): one user swap, two legs", json, expect: exp(1, Some("JUPITER"), Some(50_000_000), Some(1_000_000), Some("VERIFIED"), "VERIFIED", Some(&user)) });
    }

    // --- Relayer: fee payer differs from the swapping user ------------------------------
    {
        let relayer = addr("relayer");
        let pool = addr("cpmm:pool");
        let auth = addr("cpmm:authority");
        let (vi, vo) = (addr("cpmm:vault_in"), addr("cpmm:vault_out"));
        let accts = [
            user.as_str(),
            &auth,
            &addr("cpmm:config"),
            &pool,
            &user_usdc,
            &user_meme,
            &vi,
            &vo,
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("relayed", &relayer)
            .signer(&user)
            .ix(
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[1_000_000, 1])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &usdc, &vi, &user],
                spl_transfer_checked(1_000_000, 6),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&vo, &meme, &user_meme, &auth],
                spl_transfer_checked(2_000, 6),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(1_000_000), Some(0))
            .token(&user_meme, &meme, &user, 6, Some(0), Some(2_000))
            .build();
        out.push(Fixture { name: "relayed_swap_fee_payer_differs", description: "Relayer pays the fee; the swap is attributed to the signing user, not the fee payer", json, expect: exp(1, Some("RAYDIUM_CPMM"), Some(1_000_000), Some(2_000), Some("VERIFIED"), "VERIFIED", Some(&user)) });
    }

    // --- Program-controlled authority (CPI from an unknown program) --------------------
    {
        let bot = addr("program:bot");
        let vault = addr("bot:vault_pda");
        let (src, dst) = (addr("bot:src"), addr("bot:dst"));
        let pool = addr("cpmm:pool");
        let auth = addr("cpmm:authority");
        let (vi, vo) = (addr("cpmm:vault_in"), addr("cpmm:vault_out"));
        let accts = [
            vault.as_str(),
            &auth,
            &addr("cpmm:config"),
            &pool,
            &src,
            &dst,
            &vi,
            &vo,
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("pda", &user)
            .ix(&bot, &[&vault], vec![1, 2, 3])
            .cpi(
                2,
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[1_000, 1])),
            )
            .cpi(
                3,
                SPL_TOKEN_PROGRAM,
                &[&src, &usdc, &vi, &vault],
                spl_transfer_checked(1_000, 6),
            )
            .cpi(
                3,
                SPL_TOKEN_PROGRAM,
                &[&vo, &meme, &dst, &auth],
                spl_transfer_checked(10, 6),
            )
            .token(&src, &usdc, &vault, 6, Some(1_000), Some(0))
            .token(&dst, &meme, &vault, 6, Some(0), Some(10))
            .build();
        out.push(Fixture { name: "program_controlled_swap", description: "A swap signed by a program address inside an unsupported program: PROGRAM_CONTROLLED, no wallet", json, expect: exp(1, Some("RAYDIUM_CPMM"), Some(1_000), Some(10), Some("VERIFIED"), "PARTIALLY_DECODED", None) });
    }

    // --- Token-2022 transfer with transfer fee -------------------------------------------
    {
        let t22 = addr("mint:t22");
        let (a, b) = (addr("user:t22"), addr("friend:t22"));
        let friend = addr("friend");
        let json = TxBuilder::new("t22-fee", &user)
            .ix(
                TOKEN_2022_PROGRAM,
                &[&a, &t22, &b, &user],
                transfer_checked_with_fee(1_000_000, 6, 10_000),
            )
            .token_2022(&a, &t22, &user, 6, Some(5_000_000), Some(4_000_000))
            .token_2022(&b, &t22, &friend, 6, Some(0), Some(990_000))
            .build();
        out.push(Fixture {
            name: "token_2022_transfer_fee",
            description: "Token-2022 TransferCheckedWithFee: 1.0 sent, 0.01 withheld",
            json,
            expect: exp(0, None, None, None, None, "VERIFIED", None),
        });
    }

    // --- Wrap SOL, ATA create and close ----------------------------------------------------
    {
        let wsol = addr("user:wsol_temp");
        let json = TxBuilder::new("wrap", &user)
            .ix(
                ASSOCIATED_TOKEN_PROGRAM,
                &[
                    &user,
                    &wsol,
                    &user,
                    WSOL_MINT,
                    SYSTEM_PROGRAM,
                    SPL_TOKEN_PROGRAM,
                ],
                vec![1],
            )
            .cpi(2, SYSTEM_PROGRAM, &[&user, &wsol], {
                let mut d = 0_u32.to_le_bytes().to_vec();
                d.extend(u64s(&[2_039_280, 165]));
                d.extend(pk(SPL_TOKEN_PROGRAM));
                d
            })
            .ix(
                SYSTEM_PROGRAM,
                &[&user, &wsol],
                system_transfer(100_000_000),
            )
            .ix(SPL_TOKEN_PROGRAM, &[&wsol], vec![17])
            .ix(SPL_TOKEN_PROGRAM, &[&wsol, &user, &user], vec![9])
            .token(&wsol, WSOL_MINT, &user, 9, None, None)
            .build();
        out.push(Fixture {
            name: "wrap_and_close_wsol",
            description: "ATA create, wrap 0.1 SOL, sync, close (unwrap)",
            json,
            expect: exp(0, None, None, None, None, "VERIFIED", None),
        });
    }

    // --- Failed transaction: no swap, fee still charged ------------------------------------
    {
        let pool = addr("cpmm:pool");
        let accts = [
            user.as_str(),
            &addr("cpmm:authority"),
            &addr("cpmm:config"),
            &pool,
            &user_usdc,
            &user_meme,
            &addr("cpmm:vault_in"),
            &addr("cpmm:vault_out"),
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("failed", &user)
            .failed()
            .ix(
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[1, 1])),
            )
            .build();
        out.push(Fixture {
            name: "failed_swap",
            description: "Failed swap attempt: FAILED_TRANSACTION + FEE, never a swap",
            json,
            expect: exp(0, None, None, None, None, "VERIFIED", None),
        });
    }

    // --- Missing inner instructions --------------------------------------------------------
    {
        let pool = addr("cpmm:pool");
        let accts = [
            user.as_str(),
            &addr("cpmm:authority"),
            &addr("cpmm:config"),
            &pool,
            &user_usdc,
            &user_meme,
            &addr("cpmm:vault_in"),
            &addr("cpmm:vault_out"),
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("no-inner", &user)
            .without_inner()
            .ix(
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[1, 1])),
            )
            .build();
        out.push(Fixture { name: "missing_inner_instructions", description: "Node did not record inner instructions: INSUFFICIENT_EVIDENCE, never guessed", json, expect: exp(1, Some("RAYDIUM_CPMM"), None, None, Some("INSUFFICIENT_EVIDENCE"), "INSUFFICIENT_EVIDENCE", Some(&user)) });
    }

    // --- Unsupported program only -------------------------------------------------------------
    {
        let json = TxBuilder::new("unknown", &user)
            .ix(&addr("program:unknown"), &[&user], vec![9, 9])
            .build();
        out.push(Fixture {
            name: "unsupported_program",
            description: "Only an unknown program: UNKNOWN_ACTIVITY / UNSUPPORTED",
            json,
            expect: exp(0, None, None, None, None, "UNSUPPORTED", None),
        });
    }

    // --- Old node without stackHeight: CPIs still attached to their top-level ix ---------
    {
        let pool = addr("cpmm:pool");
        let auth = addr("cpmm:authority");
        let (vi, vo) = (addr("cpmm:vault_in"), addr("cpmm:vault_out"));
        let accts = [
            user.as_str(),
            &auth,
            &addr("cpmm:config"),
            &pool,
            &user_usdc,
            &user_meme,
            &vi,
            &vo,
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("no-stack-height", &user)
            .without_stack_height()
            .ix(
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[8, 1])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &usdc, &vi, &user],
                spl_transfer_checked(8, 6),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&vo, &meme, &user_meme, &auth],
                spl_transfer_checked(9, 6),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(8), Some(0))
            .token(&user_meme, &meme, &user, 6, Some(0), Some(9))
            .build();
        out.push(Fixture {
            name: "no_stack_height",
            description: "Legacy metadata without stackHeight: decoded with an INFO issue",
            json,
            expect: exp(
                1,
                Some("RAYDIUM_CPMM"),
                Some(8),
                Some(9),
                Some("VERIFIED"),
                "VERIFIED",
                Some(&user),
            ),
        });
    }

    // --- Amount mismatch: balance delta contradicts the transfer -----------------------------
    {
        let pool = addr("cpmm:pool");
        let auth = addr("cpmm:authority");
        let (vi, vo) = (addr("cpmm:vault_in"), addr("cpmm:vault_out"));
        let accts = [
            user.as_str(),
            &auth,
            &addr("cpmm:config"),
            &pool,
            &user_usdc,
            &user_meme,
            &vi,
            &vo,
            SPL_TOKEN_PROGRAM,
            SPL_TOKEN_PROGRAM,
            &usdc,
            &meme,
            &addr("cpmm:observation"),
        ];
        let json = TxBuilder::new("mismatch", &user)
            .ix(
                RAYDIUM_CPMM,
                &accts,
                anchor(raydium::CPMM_SWAP_BASE_INPUT, &u64s(&[100, 1])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&user_usdc, &usdc, &vi, &user],
                spl_transfer_checked(100, 6),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&vo, &meme, &user_meme, &auth],
                spl_transfer_checked(50, 6),
            )
            .token(&user_usdc, &usdc, &user, 6, Some(100), Some(0))
            .token(&user_meme, &meme, &user, 6, Some(0), Some(49))
            .build();
        out.push(Fixture { name: "amount_mismatch", description: "Destination delta disagrees with the observed transfer: PARTIALLY_DECODED + issue", json, expect: exp(1, Some("RAYDIUM_CPMM"), Some(100), Some(50), Some("PARTIALLY_DECODED"), "PARTIALLY_DECODED", Some(&user)) });
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::model::{ActivityKind, Attribution, DecodeStatus};
    use crate::{Registry, decode};

    #[test]
    fn every_fixture_decodes_to_its_expected_outcome() {
        let reg = Registry::standard();
        for f in catalog() {
            let d = decode(&f.json, &reg).unwrap_or_else(|e| panic!("{}: {e}", f.name));
            assert_eq!(
                d.swaps.len(),
                f.expect.swaps,
                "{}: swaps {:#?}",
                f.name,
                d.swaps
            );
            assert_eq!(
                d.status.as_str(),
                f.expect.tx_status,
                "{}: tx status; issues {:#?}",
                f.name,
                d.issues
            );
            if let Some(s) = d.swaps.first() {
                assert_eq!(Some(s.protocol), f.expect.protocol, "{}", f.name);
                assert_eq!(s.input.amount, f.expect.input_amount, "{}: input", f.name);
                assert_eq!(
                    s.output.amount, f.expect.output_amount,
                    "{}: output",
                    f.name
                );
                assert_eq!(
                    Some(s.status.as_str()),
                    f.expect.swap_status,
                    "{}: swap status {:?}",
                    f.name,
                    s.reasons
                );
                let wallet = if s.attribution.is_verified() {
                    s.wallet.clone()
                } else {
                    None
                };
                assert_eq!(
                    wallet, f.expect.verified_wallet,
                    "{}: wallet ({:?})",
                    f.name, s.attribution
                );
            }
            // Determinism: decoding twice yields identical ids.
            let again = decode(&f.json, &reg).unwrap();
            let ids = |x: &crate::DecodedTransaction| {
                x.activities
                    .iter()
                    .map(|a| a.id(&x.signature))
                    .collect::<Vec<_>>()
            };
            assert_eq!(ids(&d), ids(&again), "{}", f.name);
        }
    }

    fn get(name: &str) -> crate::DecodedTransaction {
        let f = catalog().into_iter().find(|f| f.name == name).unwrap();
        decode(&f.json, &Registry::standard()).unwrap()
    }

    #[test]
    fn jupiter_route_has_two_legs_and_no_nested_swaps() {
        let d = get("jupiter_route_two_legs");
        let s = &d.swaps[0];
        assert_eq!(s.legs.len(), 2);
        assert_eq!(s.legs[0].protocol, "RAYDIUM_AMM_V4");
        assert_eq!(s.legs[1].protocol, "ORCA_WHIRLPOOL");
        assert!(s.route_complete);
        assert_eq!(s.aggregator, Some("JUPITER"));
        assert!(
            !d.activities
                .iter()
                .any(|a| a.kind == ActivityKind::TokenTransfer),
            "legs are not separate transfers"
        );
    }

    #[test]
    fn relayer_fee_payer_is_not_the_wallet() {
        let d = get("relayed_swap_fee_payer_differs");
        assert_ne!(d.fee_payer, d.swaps[0].wallet.clone().unwrap());
        assert_eq!(d.swaps[0].attribution, Attribution::VerifiedOwner);
    }

    #[test]
    fn program_controlled_swaps_have_no_wallet() {
        let d = get("program_controlled_swap");
        assert_eq!(d.swaps[0].attribution, Attribution::ProgramControlled);
        assert!(d.swaps[0].wallet.is_none());
        assert!(!d.swaps[0].is_verified_trade());
    }

    #[test]
    fn failed_transactions_produce_no_swaps_but_a_fee() {
        let d = get("failed_swap");
        assert!(d.failed);
        let kinds: Vec<_> = d.activities.iter().map(|a| a.kind).collect();
        assert_eq!(
            kinds,
            vec![ActivityKind::Fee, ActivityKind::FailedTransaction]
        );
    }

    #[test]
    fn token_2022_fee_is_netted_and_reported() {
        let d = get("token_2022_transfer_fee");
        let t = d
            .activities
            .iter()
            .find(|a| a.kind == ActivityKind::TokenTransfer)
            .unwrap();
        assert_eq!(t.amount, Some(1_000_000));
        assert!(
            t.reasons
                .iter()
                .any(|r| r == "TOKEN_2022_TRANSFER_FEE_10000")
        );
        let recv = d
            .balances
            .token
            .iter()
            .find(|c| c.owner.as_deref() == Some(addr("friend").as_str()))
            .unwrap();
        assert_eq!(recv.delta, 990_000);
    }

    #[test]
    fn plain_transfers_are_not_swaps() {
        let user = addr("user");
        let (src, dst) = (addr("user:usdc"), addr("friend:usdc"));
        let usdc = addr("mint:usdc");
        let json = TxBuilder::new("plain-transfers", &user)
            .ix(
                SYSTEM_PROGRAM,
                &[&user, &addr("friend")],
                system_transfer(1_000),
            )
            .ix(SPL_TOKEN_PROGRAM, &[&src, &dst, &user], spl_transfer(250))
            .token(&src, &usdc, &user, 6, Some(1_000), Some(750))
            .token(&dst, &usdc, &addr("friend"), 6, Some(0), Some(250))
            .build();
        let d = decode(&json, &Registry::standard()).unwrap();
        assert!(d.swaps.is_empty());
        let kinds: Vec<_> = d.activities.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&ActivityKind::NativeTransfer), "{kinds:?}");
        assert!(kinds.contains(&ActivityKind::TokenTransfer), "{kinds:?}");
    }

    #[test]
    fn liquidity_operations_are_not_swaps() {
        let user = addr("user");
        let pool = addr("orca:pool");
        let (ua, ub) = (addr("user:usdc"), addr("user:meme"));
        let (va, vb) = (addr("orca:vault_a"), addr("orca:vault_b"));
        // increase_liquidity: [whirlpool, token_program, position_authority, position,
        // position_token_account, token_owner_account_a, token_owner_account_b,
        // token_vault_a, token_vault_b, tick_array_lower, tick_array_upper]
        let json = TxBuilder::new("orca-increase", &user)
            .ix(
                ORCA_WHIRLPOOL,
                &[
                    &pool,
                    SPL_TOKEN_PROGRAM,
                    &user,
                    &addr("orca:position"),
                    &addr("orca:position_ta"),
                    &ua,
                    &ub,
                    &va,
                    &vb,
                    &addr("orca:tick_lo"),
                    &addr("orca:tick_hi"),
                ],
                anchor(orca::INCREASE_LIQUIDITY, &u64s(&[1, 0, 1_000, 2_000])),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&ua, &va, &user],
                spl_transfer(1_000),
            )
            .cpi(
                2,
                SPL_TOKEN_PROGRAM,
                &[&ub, &vb, &user],
                spl_transfer(2_000),
            )
            .build();
        let d = decode(&json, &Registry::standard()).unwrap();
        assert!(d.swaps.is_empty(), "a deposit into a pool is not a swap");
        let lp = d
            .activities
            .iter()
            .find(|a| a.kind == ActivityKind::LiquidityDeposit)
            .unwrap();
        assert_eq!(lp.status, DecodeStatus::PartiallyDecoded);
        assert_eq!(lp.attribution, Some(Attribution::VerifiedAuthority));
    }

    #[test]
    fn unsupported_token_2022_extensions_are_flagged() {
        let user = addr("user");
        let t22 = addr("mint:t22");
        // ConfidentialTransferExtension (tag 27): amounts are encrypted.
        let json = TxBuilder::new("t22-confidential", &user)
            .ix(
                TOKEN_2022_PROGRAM,
                &[&addr("user:t22"), &t22, &user],
                vec![27, 7],
            )
            .build();
        let d = decode(&json, &Registry::standard()).unwrap();
        assert!(d.swaps.is_empty());
        assert!(
            d.issues
                .iter()
                .any(|i| i.code == crate::quality::IssueCode::UnsupportedTokenExtension),
            "{:?}",
            d.issues
        );
    }

    #[test]
    fn wrap_flow_is_classified() {
        let d = get("wrap_and_close_wsol");
        let kinds: Vec<_> = d.activities.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&ActivityKind::AccountCreation));
        assert!(kinds.contains(&ActivityKind::WrapSol));
        assert!(kinds.contains(&ActivityKind::UnwrapSol));
    }

    #[test]
    fn pump_fees_are_explicit() {
        let d = get("pump_fun_buy");
        let s = &d.swaps[0];
        assert_eq!(s.input.asset_kind, crate::model::AssetKind::NativeSol);
        assert_eq!(s.fees.len(), 2);
        assert_eq!(s.fees[0].amount, Some(5_000_000));
        assert_eq!(
            d.activities[s.activity_index as usize].kind,
            ActivityKind::BondingCurveTrade
        );
    }

    #[test]
    fn missing_inner_is_insufficient_not_guessed() {
        let d = get("missing_inner_instructions");
        assert_eq!(d.swaps[0].status, DecodeStatus::InsufficientEvidence);
        assert_eq!(d.swaps[0].input.amount, None);
    }

    #[test]
    fn exchange_ratio_uses_decimals() {
        let d = get("raydium_amm_v4_swap");
        assert_eq!(
            d.swaps[0].exchange_ratio,
            Some(rust_decimal::Decimal::from(150))
        );
    }
}
