//! The closed set of operation labels. Metrics, usage categories and cache
//! namespaces use only these, which bounds cardinality by construction.

pub const GET_HEALTH: &str = "getHealth";
pub const GET_VERSION: &str = "getVersion";
pub const GET_SLOT: &str = "getSlot";
pub const GET_BLOCK_TIME: &str = "getBlockTime";
pub const GET_BALANCE: &str = "getBalance";
pub const GET_ACCOUNT_INFO: &str = "getAccountInfo";
pub const GET_MULTIPLE_ACCOUNTS: &str = "getMultipleAccounts";
pub const GET_SIGNATURES_FOR_ADDRESS: &str = "getSignaturesForAddress";
pub const GET_TRANSACTION: &str = "getTransaction";
pub const GET_TOKEN_ACCOUNT_BALANCE: &str = "getTokenAccountBalance";
pub const GET_TOKEN_ACCOUNTS_BY_OWNER: &str = "getTokenAccountsByOwner";
pub const GET_TOKEN_SUPPLY: &str = "getTokenSupply";
pub const GET_TOKEN_LARGEST_ACCOUNTS: &str = "getTokenLargestAccounts";
pub const GET_BLOCK: &str = "getBlock";
pub const GET_SIGNATURE_STATUSES: &str = "getSignatureStatuses";

pub const JUP_PRICE: &str = "jupiter.price";
pub const JUP_TOKENS_SEARCH: &str = "jupiter.tokens.search";
pub const JUP_QUOTE: &str = "jupiter.quote";
pub const JUP_TOKENS_CATEGORY: &str = "jupiter.tokens.category";
pub const JUP_TOKENS_RECENT: &str = "jupiter.tokens.recent";

pub const DEX_TOKENS: &str = "dex.tokens";
pub const DEX_TOKEN_PAIRS: &str = "dex.token_pairs";
pub const DEX_PAIR: &str = "dex.pair";
pub const DEX_SEARCH: &str = "dex.search";
pub const DEX_PROFILES: &str = "dex.profiles";
pub const DEX_BOOSTS_LATEST: &str = "dex.boosts.latest";
pub const DEX_BOOSTS_TOP: &str = "dex.boosts.top";

pub const WS_CONNECT: &str = "ws.connect";
pub const WS_BYTES: &str = "ws.bytes";
pub const WS_SUBSCRIBE: &str = "ws.subscribe";

pub const ALL: &[&str] = &[
    GET_HEALTH,
    GET_VERSION,
    GET_SLOT,
    GET_BLOCK_TIME,
    GET_BALANCE,
    GET_ACCOUNT_INFO,
    GET_MULTIPLE_ACCOUNTS,
    GET_SIGNATURES_FOR_ADDRESS,
    GET_TRANSACTION,
    GET_TOKEN_ACCOUNT_BALANCE,
    GET_TOKEN_ACCOUNTS_BY_OWNER,
    GET_TOKEN_SUPPLY,
    GET_TOKEN_LARGEST_ACCOUNTS,
    GET_BLOCK,
    GET_SIGNATURE_STATUSES,
    JUP_PRICE,
    JUP_TOKENS_SEARCH,
    JUP_QUOTE,
    JUP_TOKENS_CATEGORY,
    JUP_TOKENS_RECENT,
    DEX_TOKENS,
    DEX_TOKEN_PAIRS,
    DEX_PAIR,
    DEX_SEARCH,
    DEX_PROFILES,
    DEX_BOOSTS_LATEST,
    DEX_BOOSTS_TOP,
    WS_CONNECT,
    WS_BYTES,
    WS_SUBSCRIBE,
];

/// Helius credit cost per call, from the documented credit table
/// (`docs/architecture/PROVIDER_CAPABILITIES.md`). `None` means the method is
/// not registered and must not be sent: GATEWAY never sends a method whose
/// cost it does not know.
#[must_use]
pub fn helius_credits(method: &str) -> Option<u64> {
    match method {
        GET_HEALTH
        | GET_VERSION
        | GET_SLOT
        | GET_BLOCK_TIME
        | GET_BALANCE
        | GET_ACCOUNT_INFO
        | GET_MULTIPLE_ACCOUNTS
        | GET_SIGNATURES_FOR_ADDRESS
        | GET_TRANSACTION
        | GET_TOKEN_ACCOUNT_BALANCE
        | GET_TOKEN_ACCOUNTS_BY_OWNER
        | GET_TOKEN_SUPPLY
        | GET_TOKEN_LARGEST_ACCOUNTS
        | GET_BLOCK
        | GET_SIGNATURE_STATUSES => Some(1),
        _ => None,
    }
}

/// JSON-RPC methods GATEWAY refuses to send under any circumstances.
pub const PROHIBITED_RPC_METHODS: &[&str] = &[
    "sendTransaction",
    "simulateTransaction",
    "requestAirdrop",
    "sendBundle",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rpc_operation_has_a_cost_and_nothing_else_does() {
        for m in &ALL[..15] {
            assert_eq!(helius_credits(m), Some(1), "{m}");
        }
        for m in PROHIBITED_RPC_METHODS {
            assert_eq!(helius_credits(m), None);
        }
        assert_eq!(helius_credits("getProgramAccounts"), None);
    }
}
