//! Program identifiers and instruction discriminators.
//!
//! Every value here was taken from an official source at a recorded commit
//! (see `docs/architecture/DECODER_CAPABILITIES.md`), not from memory.
//! Anchor discriminators are the first 8 bytes of
//! `sha256("global:<instruction>")` / `sha256("event:<Event>")`; the unit
//! tests below recompute every one of them.

/// The network every Phase 02 record belongs to.
pub const NETWORK: &str = "solana-mainnet";

// ---- runtime / SPL programs (Solana documentation, solana-program/token-2022) ----
pub const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
pub const SPL_TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const TOKEN_2022_PROGRAM: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
pub const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
pub const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";
pub const MEMO_PROGRAM: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";
pub const MEMO_PROGRAM_V1: &str = "Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo";
/// The native mint (wrapped SOL). Native SOL legs of swaps are reported with
/// this mint and asset kind `NATIVE_SOL`.
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
/// `Pubkey::default()`, used by Pump.fun events for "SOL quote".
pub const DEFAULT_PUBKEY: &str = "11111111111111111111111111111111";

// ---- protocol programs ------------------------------------------------------
/// jup-ag/instruction-parser `src/constants.ts` (JUPITER_V6_PROGRAM_ID).
pub const JUPITER_V6: &str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
/// pump-fun/pump-public-docs `idl/pump.json` `address`.
pub const PUMP_FUN: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
/// pump-fun/pump-public-docs `idl/pump_amm.json` `address`.
pub const PUMPSWAP: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
/// raydium-io/raydium-amm `program/src/lib.rs` (`not(feature = "devnet")`).
pub const RAYDIUM_AMM_V4: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
/// raydium-io/raydium-cp-swap `programs/cp-swap/src/lib.rs`.
pub const RAYDIUM_CPMM: &str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
/// raydium-io/raydium-clmm `programs/amm/src/lib.rs`.
pub const RAYDIUM_CLMM: &str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
/// orca-so/whirlpools `programs/whirlpool/src/lib.rs`.
pub const ORCA_WHIRLPOOL: &str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

/// Anchor `EVENT_IX_TAG` (0x1d9acb512ea545e4) in little-endian order: the
/// prefix of every `emit_cpi!` self-invocation (anchor `lang/src/event.rs`).
pub const ANCHOR_EVENT_IX_TAG_LE: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

pub type Disc = [u8; 8];

pub mod jupiter {
    use super::Disc;
    pub const ROUTE: Disc = [229, 23, 203, 151, 122, 227, 173, 42];
    pub const ROUTE_WITH_TOKEN_LEDGER: Disc = [150, 86, 71, 116, 167, 93, 14, 104];
    pub const SHARED_ACCOUNTS_ROUTE: Disc = [193, 32, 155, 51, 65, 214, 156, 129];
    pub const SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER: Disc =
        [230, 121, 143, 80, 119, 159, 106, 170];
    pub const EXACT_OUT_ROUTE: Disc = [208, 51, 239, 151, 123, 43, 237, 92];
    pub const SHARED_ACCOUNTS_EXACT_OUT_ROUTE: Disc = [176, 209, 105, 168, 154, 125, 69, 62];
    pub const EVENT_SWAP: Disc = [64, 198, 205, 232, 38, 8, 113, 226];
    pub const EVENT_FEE: Disc = [73, 79, 78, 127, 184, 213, 13, 220];
}

pub mod pump {
    use super::Disc;
    pub const BUY: Disc = [102, 6, 61, 18, 1, 218, 235, 234];
    pub const BUY_EXACT_SOL_IN: Disc = [56, 252, 116, 8, 158, 223, 205, 95];
    pub const BUY_V2: Disc = [184, 23, 238, 97, 103, 197, 211, 61];
    pub const BUY_V3: Disc = [7, 5, 29, 196, 245, 23, 101, 80];
    pub const BUY_EXACT_QUOTE_IN_V2: Disc = [194, 171, 28, 70, 104, 77, 91, 47];
    pub const BUY_EXACT_QUOTE_IN_V3: Disc = [225, 247, 80, 30, 213, 179, 132, 136];
    pub const SELL: Disc = [51, 230, 133, 164, 1, 127, 131, 173];
    pub const SELL_V2: Disc = [93, 246, 130, 60, 231, 233, 64, 178];
    pub const SELL_V3: Disc = [28, 146, 222, 119, 38, 196, 105, 213];
    pub const CREATE: Disc = [24, 30, 200, 40, 5, 28, 7, 119];
    pub const CREATE_V2: Disc = [214, 144, 76, 236, 95, 139, 49, 180];
    pub const MIGRATE: Disc = [155, 234, 231, 146, 236, 158, 162, 30];
    pub const MIGRATE_V2: Disc = [187, 203, 18, 31, 206, 237, 254, 41];
    pub const EVENT_TRADE: Disc = [189, 219, 127, 211, 78, 230, 97, 238];
    pub const EVENT_POST_COMPLETE_BUY: Disc = [111, 176, 109, 139, 49, 108, 213, 251];
}

pub mod pumpswap {
    use super::Disc;
    pub const BUY: Disc = [102, 6, 61, 18, 1, 218, 235, 234];
    pub const BUY_V2: Disc = [184, 23, 238, 97, 103, 197, 211, 61];
    pub const BUY_EXACT_QUOTE_IN: Disc = [198, 46, 21, 82, 180, 217, 232, 112];
    pub const BUY_EXACT_QUOTE_IN_V2: Disc = [194, 171, 28, 70, 104, 77, 91, 47];
    pub const SELL: Disc = [51, 230, 133, 164, 1, 127, 131, 173];
    pub const SELL_V2: Disc = [93, 246, 130, 60, 231, 233, 64, 178];
    pub const CREATE_POOL: Disc = [233, 146, 209, 142, 207, 104, 64, 188];
    pub const DEPOSIT: Disc = [242, 35, 198, 137, 82, 225, 242, 182];
    pub const WITHDRAW: Disc = [183, 18, 70, 156, 148, 109, 161, 34];
    pub const EVENT_BUY: Disc = [103, 244, 82, 31, 44, 245, 119, 119];
    pub const EVENT_SELL: Disc = [62, 47, 55, 10, 165, 3, 220, 42];
}

pub mod raydium {
    use super::Disc;
    // AMM v4 is not an Anchor program: a one-byte tag (program/src/instruction.rs).
    pub const AMM_INITIALIZE2: u8 = 1;
    pub const AMM_DEPOSIT: u8 = 3;
    pub const AMM_WITHDRAW: u8 = 4;
    pub const AMM_SWAP_BASE_IN: u8 = 9;
    pub const AMM_SWAP_BASE_OUT: u8 = 11;
    pub const AMM_SWAP_BASE_IN_V2: u8 = 16;
    pub const AMM_SWAP_BASE_OUT_V2: u8 = 17;
    // CPMM (Anchor).
    pub const CPMM_SWAP_BASE_INPUT: Disc = [143, 190, 90, 218, 196, 30, 51, 222];
    pub const CPMM_SWAP_BASE_OUTPUT: Disc = [55, 217, 98, 86, 163, 74, 180, 173];
    pub const CPMM_INITIALIZE: Disc = [175, 175, 109, 31, 13, 152, 155, 237];
    pub const CPMM_DEPOSIT: Disc = [242, 35, 198, 137, 82, 225, 242, 182];
    pub const CPMM_WITHDRAW: Disc = [183, 18, 70, 156, 148, 109, 161, 34];
    // CLMM (Anchor).
    pub const CLMM_SWAP: Disc = [248, 198, 158, 145, 225, 117, 135, 200];
    pub const CLMM_SWAP_V2: Disc = [43, 4, 237, 11, 26, 201, 30, 98];
    pub const CLMM_SWAP_ROUTER_BASE_IN: Disc = [69, 125, 115, 218, 245, 186, 242, 196];
    pub const CLMM_CREATE_POOL: Disc = [233, 146, 209, 142, 207, 104, 64, 188];
    pub const CLMM_OPEN_POSITION: Disc = [135, 128, 47, 77, 15, 152, 240, 49];
    pub const CLMM_OPEN_POSITION_V2: Disc = [77, 184, 74, 214, 112, 86, 241, 199];
    pub const CLMM_OPEN_POSITION_WITH_TOKEN22_NFT: Disc = [77, 255, 174, 82, 125, 29, 201, 46];
    pub const CLMM_INCREASE_LIQUIDITY: Disc = [46, 156, 243, 118, 13, 205, 251, 178];
    pub const CLMM_INCREASE_LIQUIDITY_V2: Disc = [133, 29, 89, 223, 69, 238, 176, 10];
    pub const CLMM_DECREASE_LIQUIDITY: Disc = [160, 38, 208, 111, 104, 91, 44, 1];
    pub const CLMM_DECREASE_LIQUIDITY_V2: Disc = [58, 127, 188, 62, 79, 82, 196, 96];
}

pub mod orca {
    use super::Disc;
    pub const SWAP: Disc = [248, 198, 158, 145, 225, 117, 135, 200];
    pub const SWAP_V2: Disc = [43, 4, 237, 11, 26, 201, 30, 98];
    pub const TWO_HOP_SWAP: Disc = [195, 96, 237, 108, 68, 162, 219, 230];
    pub const TWO_HOP_SWAP_V2: Disc = [186, 143, 209, 29, 254, 2, 194, 117];
    pub const INITIALIZE_POOL: Disc = [95, 180, 10, 172, 84, 174, 232, 40];
    pub const INITIALIZE_POOL_V2: Disc = [207, 45, 87, 242, 27, 63, 204, 67];
    pub const INCREASE_LIQUIDITY: Disc = [46, 156, 243, 118, 13, 205, 251, 178];
    pub const INCREASE_LIQUIDITY_V2: Disc = [133, 29, 89, 223, 69, 238, 176, 10];
    pub const DECREASE_LIQUIDITY: Disc = [160, 38, 208, 111, 104, 91, 44, 1];
    pub const DECREASE_LIQUIDITY_V2: Disc = [58, 127, 188, 62, 79, 82, 196, 96];
}

/// Display label of a program for route legs and unsupported-program
/// reports. `None` for programs SOLPRINT has no verified identity for.
#[must_use]
pub fn protocol_label(program: &str) -> Option<&'static str> {
    Some(match program {
        JUPITER_V6 => "JUPITER",
        PUMP_FUN => "PUMP_FUN",
        PUMPSWAP => "PUMPSWAP",
        RAYDIUM_AMM_V4 => "RAYDIUM_AMM_V4",
        RAYDIUM_CPMM => "RAYDIUM_CPMM",
        RAYDIUM_CLMM => "RAYDIUM_CLMM",
        ORCA_WHIRLPOOL => "ORCA_WHIRLPOOL",
        _ => return None,
    })
}

/// Programs whose instructions are infrastructure, never economic activity
/// by themselves (fees and compute budget are reported separately).
#[must_use]
pub fn is_infrastructure(program: &str) -> bool {
    matches!(
        program,
        COMPUTE_BUDGET_PROGRAM | MEMO_PROGRAM | MEMO_PROGRAM_V1
    )
}

#[must_use]
pub fn is_token_program(program: &str) -> bool {
    program == SPL_TOKEN_PROGRAM || program == TOKEN_2022_PROGRAM
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn anchor(prefix: &str, name: &str) -> Disc {
        let d = Sha256::digest(format!("{prefix}:{name}").as_bytes());
        let mut out = [0_u8; 8];
        out.copy_from_slice(&d[..8]);
        out
    }

    #[test]
    fn anchor_discriminators_match_their_preimages() {
        let cases: &[(&str, &str, Disc)] = &[
            ("global", "route", jupiter::ROUTE),
            (
                "global",
                "route_with_token_ledger",
                jupiter::ROUTE_WITH_TOKEN_LEDGER,
            ),
            (
                "global",
                "shared_accounts_route",
                jupiter::SHARED_ACCOUNTS_ROUTE,
            ),
            (
                "global",
                "shared_accounts_route_with_token_ledger",
                jupiter::SHARED_ACCOUNTS_ROUTE_WITH_TOKEN_LEDGER,
            ),
            ("global", "exact_out_route", jupiter::EXACT_OUT_ROUTE),
            (
                "global",
                "shared_accounts_exact_out_route",
                jupiter::SHARED_ACCOUNTS_EXACT_OUT_ROUTE,
            ),
            ("event", "SwapEvent", jupiter::EVENT_SWAP),
            ("event", "FeeEvent", jupiter::EVENT_FEE),
            ("global", "buy", pump::BUY),
            ("global", "buy_exact_sol_in", pump::BUY_EXACT_SOL_IN),
            ("global", "buy_v2", pump::BUY_V2),
            ("global", "buy_v3", pump::BUY_V3),
            (
                "global",
                "buy_exact_quote_in_v2",
                pump::BUY_EXACT_QUOTE_IN_V2,
            ),
            (
                "global",
                "buy_exact_quote_in_v3",
                pump::BUY_EXACT_QUOTE_IN_V3,
            ),
            ("global", "sell", pump::SELL),
            ("global", "sell_v2", pump::SELL_V2),
            ("global", "sell_v3", pump::SELL_V3),
            ("global", "create", pump::CREATE),
            ("global", "create_v2", pump::CREATE_V2),
            ("global", "migrate", pump::MIGRATE),
            ("global", "migrate_v2", pump::MIGRATE_V2),
            ("event", "TradeEvent", pump::EVENT_TRADE),
            (
                "event",
                "PostCompleteBuyEvent",
                pump::EVENT_POST_COMPLETE_BUY,
            ),
            ("global", "buy_exact_quote_in", pumpswap::BUY_EXACT_QUOTE_IN),
            ("global", "create_pool", pumpswap::CREATE_POOL),
            ("global", "deposit", pumpswap::DEPOSIT),
            ("global", "withdraw", pumpswap::WITHDRAW),
            ("event", "BuyEvent", pumpswap::EVENT_BUY),
            ("event", "SellEvent", pumpswap::EVENT_SELL),
            ("global", "swap_base_input", raydium::CPMM_SWAP_BASE_INPUT),
            ("global", "swap_base_output", raydium::CPMM_SWAP_BASE_OUTPUT),
            ("global", "initialize", raydium::CPMM_INITIALIZE),
            ("global", "swap", raydium::CLMM_SWAP),
            ("global", "swap_v2", raydium::CLMM_SWAP_V2),
            (
                "global",
                "swap_router_base_in",
                raydium::CLMM_SWAP_ROUTER_BASE_IN,
            ),
            ("global", "open_position", raydium::CLMM_OPEN_POSITION),
            ("global", "open_position_v2", raydium::CLMM_OPEN_POSITION_V2),
            (
                "global",
                "open_position_with_token22_nft",
                raydium::CLMM_OPEN_POSITION_WITH_TOKEN22_NFT,
            ),
            (
                "global",
                "increase_liquidity",
                raydium::CLMM_INCREASE_LIQUIDITY,
            ),
            (
                "global",
                "increase_liquidity_v2",
                raydium::CLMM_INCREASE_LIQUIDITY_V2,
            ),
            (
                "global",
                "decrease_liquidity",
                raydium::CLMM_DECREASE_LIQUIDITY,
            ),
            (
                "global",
                "decrease_liquidity_v2",
                raydium::CLMM_DECREASE_LIQUIDITY_V2,
            ),
            ("global", "two_hop_swap", orca::TWO_HOP_SWAP),
            ("global", "two_hop_swap_v2", orca::TWO_HOP_SWAP_V2),
            ("global", "initialize_pool", orca::INITIALIZE_POOL),
            ("global", "initialize_pool_v2", orca::INITIALIZE_POOL_V2),
        ];
        for (prefix, name, disc) in cases {
            assert_eq!(anchor(prefix, name), *disc, "{prefix}:{name}");
        }
    }

    #[test]
    fn pump_discriminators_match_the_official_idl_arrays() {
        // Arrays copied from pump-public-docs idl/pump.json and pump_amm.json.
        assert_eq!(pump::BUY, [102, 6, 61, 18, 1, 218, 235, 234]);
        assert_eq!(pump::SELL, [51, 230, 133, 164, 1, 127, 131, 173]);
        assert_eq!(pump::EVENT_TRADE, [189, 219, 127, 211, 78, 230, 97, 238]);
        assert_eq!(pumpswap::EVENT_BUY, [103, 244, 82, 31, 44, 245, 119, 119]);
        assert_eq!(pumpswap::EVENT_SELL, [62, 47, 55, 10, 165, 3, 220, 42]);
    }

    #[test]
    fn event_tag_is_the_little_endian_anchor_constant() {
        assert_eq!(
            ANCHOR_EVENT_IX_TAG_LE,
            0x1d9a_cb51_2ea5_45e4_u64.to_le_bytes()
        );
    }

    #[test]
    fn program_ids_are_valid_32_byte_keys() {
        for id in [
            SYSTEM_PROGRAM,
            SPL_TOKEN_PROGRAM,
            TOKEN_2022_PROGRAM,
            ASSOCIATED_TOKEN_PROGRAM,
            COMPUTE_BUDGET_PROGRAM,
            WSOL_MINT,
            JUPITER_V6,
            PUMP_FUN,
            PUMPSWAP,
            RAYDIUM_AMM_V4,
            RAYDIUM_CPMM,
            RAYDIUM_CLMM,
            ORCA_WHIRLPOOL,
        ] {
            assert_eq!(bs58::decode(id).into_vec().unwrap().len(), 32, "{id}");
        }
    }
}
