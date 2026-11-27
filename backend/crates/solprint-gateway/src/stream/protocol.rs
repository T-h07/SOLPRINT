//! Solana WebSocket subscription protocol (standard methods only).

use serde_json::{Value, json};
use solprint_core::{CommitmentLevel, TransactionSignature, WalletAddress};

use crate::rpc::commitment_str;

/// Program-subscription filters. At least one is required, so a program
/// subscription can never stream every account of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramFilter {
    DataSize(u64),
    /// `bytes` is base58, at most 128 characters.
    Memcmp {
        offset: u64,
        bytes: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionSpec {
    Slot,
    Root,
    /// Logs mentioning exactly one address (the documented limit).
    Logs {
        mentions: WalletAddress,
        commitment: CommitmentLevel,
    },
    Account {
        address: WalletAddress,
        commitment: CommitmentLevel,
    },
    /// Fires once at the requested commitment, then the server ends it.
    Signature {
        signature: TransactionSignature,
        commitment: CommitmentLevel,
    },
    Program {
        program_id: WalletAddress,
        commitment: CommitmentLevel,
        filters: Vec<ProgramFilter>,
    },
}

impl SubscriptionSpec {
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Root => "root",
            Self::Logs { .. } => "logs",
            Self::Account { .. } => "account",
            Self::Signature { .. } => "signature",
            Self::Program { .. } => "program",
        }
    }

    #[must_use]
    pub const fn commitment(&self) -> Option<CommitmentLevel> {
        match self {
            Self::Slot | Self::Root => None,
            Self::Logs { commitment, .. }
            | Self::Account { commitment, .. }
            | Self::Signature { commitment, .. }
            | Self::Program { commitment, .. } => Some(*commitment),
        }
    }

    const fn methods(&self) -> (&'static str, &'static str) {
        match self {
            Self::Slot => ("slotSubscribe", "slotUnsubscribe"),
            Self::Root => ("rootSubscribe", "rootUnsubscribe"),
            Self::Logs { .. } => ("logsSubscribe", "logsUnsubscribe"),
            Self::Account { .. } => ("accountSubscribe", "accountUnsubscribe"),
            Self::Signature { .. } => ("signatureSubscribe", "signatureUnsubscribe"),
            Self::Program { .. } => ("programSubscribe", "programUnsubscribe"),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if let Self::Program { filters, .. } = self {
            if filters.is_empty() {
                return Err("program subscriptions require at least one filter");
            }
            if filters.len() > 4 {
                return Err("at most 4 program filters");
            }
            for f in filters {
                if let ProgramFilter::Memcmp { bytes, .. } = f
                    && (bytes.is_empty() || bytes.len() > 128 || bs58_invalid(bytes))
                {
                    return Err("memcmp bytes must be 1..=128 base58 characters");
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn subscribe_message(&self, id: u64) -> Value {
        let (method, _) = self.methods();
        let params = match self {
            Self::Slot | Self::Root => json!([]),
            Self::Logs {
                mentions,
                commitment,
            } => {
                json!([{"mentions": [mentions.as_str()]}, {"commitment": commitment_str(*commitment)}])
            }
            Self::Account {
                address,
                commitment,
            } => {
                json!([address.as_str(), {"commitment": commitment_str(*commitment), "encoding": "base64"}])
            }
            Self::Signature {
                signature,
                commitment,
            } => {
                json!([signature.as_str(), {"commitment": commitment_str(*commitment), "enableReceivedNotification": false}])
            }
            Self::Program {
                program_id,
                commitment,
                filters,
            } => {
                let filters: Vec<Value> = filters
                    .iter()
                    .map(|f| match f {
                        ProgramFilter::DataSize(n) => json!({"dataSize": n}),
                        ProgramFilter::Memcmp { offset, bytes } => {
                            json!({"memcmp": {"offset": offset, "bytes": bytes}})
                        }
                    })
                    .collect();
                json!([program_id.as_str(), {
                    "commitment": commitment_str(*commitment),
                    "encoding": "base64",
                    "filters": filters,
                }])
            }
        };
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    }

    #[must_use]
    pub fn unsubscribe_message(&self, id: u64, server_id: u64) -> Value {
        let (_, method) = self.methods();
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": [server_id]})
    }
}

fn bs58_invalid(s: &str) -> bool {
    const ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    s.chars().any(|c| !ALPHABET.contains(c))
}

/// One parsed server frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// Reply to a request: `result` is the server subscription id (subscribe)
    /// or `true` (unsubscribe).
    Reply {
        id: u64,
        result: Value,
    },
    Error {
        id: Option<u64>,
        code: i64,
        message: String,
    },
    Notification {
        method: String,
        subscription: u64,
        result: Value,
    },
    Unknown,
}

#[must_use]
pub fn parse_frame(text: &str) -> Incoming {
    let Ok(v) = serde_json::from_str::<Value>(text) else {
        return Incoming::Unknown;
    };
    if let Some(method) = v.get("method").and_then(Value::as_str) {
        let params = v.get("params");
        let subscription = params
            .and_then(|p| p.get("subscription"))
            .and_then(Value::as_u64);
        let result = params.and_then(|p| p.get("result")).cloned();
        return match (subscription, result) {
            (Some(subscription), Some(result)) => Incoming::Notification {
                method: method.chars().take(48).collect(),
                subscription,
                result,
            },
            _ => Incoming::Unknown,
        };
    }
    let id = v.get("id").and_then(Value::as_u64);
    if let Some(err) = v.get("error") {
        return Incoming::Error {
            id,
            code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
            message: crate::error::sanitize_message(
                err.get("message").and_then(Value::as_str).unwrap_or(""),
            ),
        };
    }
    match (id, v.get("result")) {
        (Some(id), Some(result)) => Incoming::Reply {
            id,
            result: result.clone(),
        },
        _ => Incoming::Unknown,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const ADDR: &str = "11111111111111111111111111111111";

    #[test]
    fn logs_subscription_mentions_exactly_one_address_with_explicit_commitment() {
        let spec = SubscriptionSpec::Logs {
            mentions: WalletAddress::parse(ADDR).unwrap(),
            commitment: CommitmentLevel::Confirmed,
        };
        let m = spec.subscribe_message(3);
        assert_eq!(m["method"], "logsSubscribe");
        assert_eq!(m["params"][0]["mentions"].as_array().unwrap().len(), 1);
        assert_eq!(m["params"][1]["commitment"], "confirmed");
        assert_eq!(spec.unsubscribe_message(4, 99)["params"][0], 99);
    }

    #[test]
    fn program_subscriptions_must_be_filtered() {
        let p = |filters| SubscriptionSpec::Program {
            program_id: WalletAddress::parse(ADDR).unwrap(),
            commitment: CommitmentLevel::Confirmed,
            filters,
        };
        assert!(p(vec![]).validate().is_err());
        assert!(
            p(vec![ProgramFilter::Memcmp {
                offset: 0,
                bytes: "0OIl".into()
            }])
            .validate()
            .is_err()
        );
        assert!(p(vec![ProgramFilter::DataSize(165)]).validate().is_ok());
    }

    #[test]
    fn frames_are_classified() {
        assert_eq!(
            parse_frame(r#"{"jsonrpc":"2.0","result":23784,"id":1}"#),
            Incoming::Reply {
                id: 1,
                result: json!(23784)
            }
        );
        assert!(matches!(
            parse_frame(
                r#"{"jsonrpc":"2.0","method":"slotNotification","params":{"result":{"parent":1,"root":0,"slot":2},"subscription":5}}"#
            ),
            Incoming::Notification {
                subscription: 5,
                ..
            }
        ));
        assert!(matches!(
            parse_frame(
                r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid params"},"id":2}"#
            ),
            Incoming::Error {
                id: Some(2),
                code: -32602,
                ..
            }
        ));
        assert_eq!(parse_frame("not json"), Incoming::Unknown);
    }
}
