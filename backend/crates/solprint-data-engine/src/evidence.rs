//! Parsing of the `getTransaction` (`encoding: "json"`,
//! `maxSupportedTransactionVersion: 0`) result object into a bounded, typed
//! evidence model. Nothing is inferred here; missing optional fields stay
//! `None` and are reported by reconstruction as quality issues.

use serde_json::Value;

/// Hard ceilings. Real transactions are far below these; anything above is
/// rejected as malformed instead of being allocated.
pub const MAX_ACCOUNTS: usize = 256;
pub const MAX_INSTRUCTIONS: usize = 4_096;
pub const MAX_IX_DATA_BYTES: usize = 10_240;
pub const MAX_LOG_LINES: usize = 2_048;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    #[error("evidence is not valid JSON")]
    NotJson,
    #[error("missing or invalid field `{0}`")]
    Field(&'static str),
    #[error("limit exceeded: {0}")]
    Limit(&'static str),
    #[error("unsupported transaction version `{0}`")]
    UnsupportedVersion(String),
    #[error("instruction data is not valid base58")]
    InstructionData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxVersion {
    Legacy,
    V0,
}

impl TxVersion {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::V0 => "0",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawInstruction {
    pub program_id_index: usize,
    pub accounts: Vec<usize>,
    pub data: Vec<u8>,
    /// `None` on old nodes / old transactions that do not report it.
    pub stack_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnerSet {
    pub index: usize,
    pub instructions: Vec<RawInstruction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTokenBalance {
    pub account_index: usize,
    pub mint: String,
    pub owner: Option<String>,
    pub program_id: Option<String>,
    pub amount: u64,
    pub decimals: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxEvidence {
    pub signatures: Vec<String>,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub version: TxVersion,
    pub num_required_signatures: usize,
    pub num_readonly_signed: usize,
    pub num_readonly_unsigned: usize,
    pub static_keys: Vec<String>,
    pub loaded_writable: Vec<String>,
    pub loaded_readonly: Vec<String>,
    /// Address lookup tables referenced by a v0 message (table keys only).
    pub lookup_tables: Vec<String>,
    pub instructions: Vec<RawInstruction>,
    /// `None` when the node did not record inner instructions.
    pub inner: Option<Vec<InnerSet>>,
    pub fee: u64,
    pub failed: bool,
    pub err_json: Option<String>,
    pub pre_balances: Vec<u64>,
    pub post_balances: Vec<u64>,
    pub pre_token_balances: Option<Vec<RawTokenBalance>>,
    pub post_token_balances: Option<Vec<RawTokenBalance>>,
    pub log_messages: Option<Vec<String>>,
    pub compute_units_consumed: Option<u64>,
    /// Position inside the block when the node reports it.
    pub transaction_index: Option<u32>,
}

fn u64_of(v: &Value) -> Option<u64> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn usize_of(v: &Value) -> Option<usize> {
    v.as_u64().and_then(|n| usize::try_from(n).ok())
}

fn str_list(
    v: Option<&Value>,
    field: &'static str,
    max: usize,
) -> Result<Vec<String>, EvidenceError> {
    let Some(v) = v else { return Ok(Vec::new()) };
    if v.is_null() {
        return Ok(Vec::new());
    }
    let arr = v.as_array().ok_or(EvidenceError::Field(field))?;
    if arr.len() > max {
        return Err(EvidenceError::Limit(field));
    }
    arr.iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or(EvidenceError::Field(field))
        })
        .collect()
}

fn u64_list(v: Option<&Value>, field: &'static str) -> Result<Vec<u64>, EvidenceError> {
    let arr = v
        .and_then(Value::as_array)
        .ok_or(EvidenceError::Field(field))?;
    if arr.len() > MAX_ACCOUNTS {
        return Err(EvidenceError::Limit(field));
    }
    arr.iter()
        .map(|x| u64_of(x).ok_or(EvidenceError::Field(field)))
        .collect()
}

fn instruction(v: &Value) -> Result<RawInstruction, EvidenceError> {
    let program_id_index = v
        .get("programIdIndex")
        .and_then(usize_of)
        .ok_or(EvidenceError::Field("programIdIndex"))?;
    let accounts = v
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or(EvidenceError::Field("accounts"))?;
    if accounts.len() > MAX_ACCOUNTS {
        return Err(EvidenceError::Limit("instruction accounts"));
    }
    let accounts = accounts
        .iter()
        .map(|a| usize_of(a).ok_or(EvidenceError::Field("accounts")))
        .collect::<Result<Vec<_>, _>>()?;
    let data_b58 = v.get("data").and_then(Value::as_str).unwrap_or("");
    if data_b58.len() > MAX_IX_DATA_BYTES * 2 {
        return Err(EvidenceError::Limit("instruction data"));
    }
    let data = bs58::decode(data_b58)
        .into_vec()
        .map_err(|_| EvidenceError::InstructionData)?;
    let stack_height = v
        .get("stackHeight")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok());
    Ok(RawInstruction {
        program_id_index,
        accounts,
        data,
        stack_height,
    })
}

fn token_balances(v: Option<&Value>) -> Result<Option<Vec<RawTokenBalance>>, EvidenceError> {
    let Some(v) = v.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let arr = v.as_array().ok_or(EvidenceError::Field("tokenBalances"))?;
    if arr.len() > MAX_ACCOUNTS {
        return Err(EvidenceError::Limit("tokenBalances"));
    }
    let mut out = Vec::with_capacity(arr.len());
    for b in arr {
        let ui = b
            .get("uiTokenAmount")
            .ok_or(EvidenceError::Field("uiTokenAmount"))?;
        out.push(RawTokenBalance {
            account_index: b
                .get("accountIndex")
                .and_then(usize_of)
                .ok_or(EvidenceError::Field("accountIndex"))?,
            mint: b
                .get("mint")
                .and_then(Value::as_str)
                .ok_or(EvidenceError::Field("mint"))?
                .to_owned(),
            owner: b.get("owner").and_then(Value::as_str).map(str::to_owned),
            program_id: b
                .get("programId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            amount: ui
                .get("amount")
                .and_then(u64_of)
                .ok_or(EvidenceError::Field("uiTokenAmount.amount"))?,
            decimals: ui
                .get("decimals")
                .and_then(Value::as_u64)
                .and_then(|d| u8::try_from(d).ok())
                .ok_or(EvidenceError::Field("uiTokenAmount.decimals"))?,
        });
    }
    Ok(Some(out))
}

/// Parse raw `getTransaction` result JSON.
pub fn parse(raw_json: &str) -> Result<TxEvidence, EvidenceError> {
    let v: Value = serde_json::from_str(raw_json).map_err(|_| EvidenceError::NotJson)?;
    parse_value(&v)
}

pub fn parse_value(v: &Value) -> Result<TxEvidence, EvidenceError> {
    let slot = v
        .get("slot")
        .and_then(Value::as_u64)
        .ok_or(EvidenceError::Field("slot"))?;
    let block_time = v.get("blockTime").and_then(Value::as_i64);
    let version = match v.get("version") {
        None | Some(Value::Null) => TxVersion::Legacy,
        Some(Value::String(s)) if s == "legacy" => TxVersion::Legacy,
        Some(Value::Number(n)) if n.as_u64() == Some(0) => TxVersion::V0,
        Some(other) => return Err(EvidenceError::UnsupportedVersion(other.to_string())),
    };
    let tx = v
        .get("transaction")
        .ok_or(EvidenceError::Field("transaction"))?;
    if tx.is_string() || tx.is_array() {
        // base58/base64 encodings are not what GATEWAY requests.
        return Err(EvidenceError::Field("transaction (expected json encoding)"));
    }
    let signatures = str_list(tx.get("signatures"), "signatures", 64)?;
    let msg = tx.get("message").ok_or(EvidenceError::Field("message"))?;
    let header = msg.get("header").ok_or(EvidenceError::Field("header"))?;
    let hdr = |k: &'static str| {
        header
            .get(k)
            .and_then(usize_of)
            .ok_or(EvidenceError::Field(k))
    };
    let static_keys = str_list(msg.get("accountKeys"), "accountKeys", MAX_ACCOUNTS)?;
    if static_keys.is_empty() {
        return Err(EvidenceError::Field("accountKeys"));
    }
    let ixs = msg
        .get("instructions")
        .and_then(Value::as_array)
        .ok_or(EvidenceError::Field("instructions"))?;
    if ixs.len() > MAX_INSTRUCTIONS {
        return Err(EvidenceError::Limit("instructions"));
    }
    let instructions = ixs.iter().map(instruction).collect::<Result<Vec<_>, _>>()?;
    let lookup_tables = msg
        .get("addressTableLookups")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|l| {
                    l.get("accountKey")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();

    let meta = v
        .get("meta")
        .filter(|m| !m.is_null())
        .ok_or(EvidenceError::Field("meta"))?;
    let err = meta.get("err").filter(|e| !e.is_null());
    let loaded = meta.get("loadedAddresses");
    let inner = match meta.get("innerInstructions") {
        None | Some(Value::Null) => None,
        Some(Value::Array(sets)) => {
            let mut total = instructions.len();
            let mut out = Vec::with_capacity(sets.len());
            for s in sets {
                let index = s
                    .get("index")
                    .and_then(usize_of)
                    .ok_or(EvidenceError::Field("inner.index"))?;
                let list = s
                    .get("instructions")
                    .and_then(Value::as_array)
                    .ok_or(EvidenceError::Field("inner.instructions"))?;
                total = total.saturating_add(list.len());
                if total > MAX_INSTRUCTIONS {
                    return Err(EvidenceError::Limit("instructions"));
                }
                out.push(InnerSet {
                    index,
                    instructions: list
                        .iter()
                        .map(instruction)
                        .collect::<Result<Vec<_>, _>>()?,
                });
            }
            Some(out)
        }
        Some(_) => return Err(EvidenceError::Field("innerInstructions")),
    };
    let log_messages = match meta.get("logMessages") {
        None | Some(Value::Null) => None,
        other => Some(str_list(other, "logMessages", MAX_LOG_LINES)?),
    };
    Ok(TxEvidence {
        signatures,
        slot,
        block_time,
        version,
        num_required_signatures: hdr("numRequiredSignatures")?,
        num_readonly_signed: hdr("numReadonlySignedAccounts")?,
        num_readonly_unsigned: hdr("numReadonlyUnsignedAccounts")?,
        static_keys,
        loaded_writable: str_list(
            loaded.and_then(|l| l.get("writable")),
            "loadedAddresses.writable",
            MAX_ACCOUNTS,
        )?,
        loaded_readonly: str_list(
            loaded.and_then(|l| l.get("readonly")),
            "loadedAddresses.readonly",
            MAX_ACCOUNTS,
        )?,
        lookup_tables,
        instructions,
        inner,
        fee: meta
            .get("fee")
            .and_then(u64_of)
            .ok_or(EvidenceError::Field("fee"))?,
        failed: err.is_some(),
        err_json: err.map(|e| e.to_string().chars().take(512).collect()),
        pre_balances: u64_list(meta.get("preBalances"), "preBalances")?,
        post_balances: u64_list(meta.get("postBalances"), "postBalances")?,
        pre_token_balances: token_balances(meta.get("preTokenBalances"))?,
        post_token_balances: token_balances(meta.get("postTokenBalances"))?,
        log_messages,
        compute_units_consumed: meta.get("computeUnitsConsumed").and_then(u64_of),
        transaction_index: v
            .get("transactionIndex")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn rejects_non_json_and_unknown_versions() {
        assert_eq!(parse("not json"), Err(EvidenceError::NotJson));
        let v = serde_json::json!({"slot": 1, "version": 1, "transaction": {}, "meta": {}});
        assert!(matches!(
            parse_value(&v),
            Err(EvidenceError::UnsupportedVersion(_))
        ));
    }

    #[test]
    fn rejects_missing_meta() {
        let v = serde_json::json!({"slot": 1, "transaction": {"signatures": [], "message": {
            "header": {"numRequiredSignatures":1,"numReadonlySignedAccounts":0,"numReadonlyUnsignedAccounts":0},
            "accountKeys": ["11111111111111111111111111111111"], "instructions": []}}, "meta": null});
        assert_eq!(parse_value(&v), Err(EvidenceError::Field("meta")));
    }

    proptest! {
        #[test]
        fn arbitrary_text_never_panics(s in ".{0,512}") {
            let _ = parse(&s);
        }
    }
}
