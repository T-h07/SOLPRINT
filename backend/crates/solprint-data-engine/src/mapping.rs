//! Domain model → storage write set, including the domain events committed
//! through the transactional outbox (see `docs/architecture/DOMAIN_EVENTS.md`).

use serde_json::json;
use sha2::{Digest, Sha256};
use solprint_core::UtcTimestamp;
use solprint_storage::data_engine::{
    ActivityWrite, BalanceWrite, EvidenceWrite, FeeWrite, IssueWrite, LegWrite, OutboxWrite,
    StoredTxState, SwapWrite, TransactionWrite,
};
use uuid::Uuid;

use crate::model::{DECODER_VERSION, DecodedTransaction, Swap};
use crate::quality::Issue;

/// Version of the stored evidence document layout.
pub const EVIDENCE_SCHEMA: i16 = 1;
/// Version of every outbox payload schema below.
pub const EVENT_SCHEMA: i16 = 1;

pub mod events {
    pub const TRANSACTION_DECODED: &str = "TransactionDecoded";
    pub const SWAP_RECORDED: &str = "SwapRecorded";
    pub const SWAP_RETRACTED: &str = "SwapRetracted";
    pub const TRANSACTION_FINALIZED: &str = "TransactionFinalized";
    pub const TRANSACTION_ORPHANED: &str = "TransactionOrphaned";
    pub const FINALITY_UNKNOWN: &str = "FinalityUnknown";
    pub const ALL: &[&str] = &[
        TRANSACTION_DECODED,
        SWAP_RECORDED,
        SWAP_RETRACTED,
        TRANSACTION_FINALIZED,
        TRANSACTION_ORPHANED,
        FINALITY_UNKNOWN,
    ];
}

#[must_use]
pub fn sha256_hex(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    let mut out = String::with_capacity(64);
    for b in d {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[derive(Debug, Clone)]
pub struct EvidenceMeta {
    pub commitment: &'static str,
    pub provider: &'static str,
    pub fetched_at: UtcTimestamp,
    pub raw_json: String,
    pub first_observed_at: UtcTimestamp,
    pub next_finality_check_at: Option<UtcTimestamp>,
    pub correlation_id: Uuid,
}

fn opt_u64(v: Option<u64>) -> Option<String> {
    v.map(|x| x.to_string())
}

fn issue_write(sig: &str, i: &Issue) -> IssueWrite {
    IssueWrite {
        signature: Some(sig.to_owned()),
        target_id: None,
        backfill_id: None,
        code: i.code.as_str().to_owned(),
        severity: i.severity.as_str().to_owned(),
        detail: Some(i.detail.clone()),
        decoder_version: Some(DECODER_VERSION.to_owned()),
        dedup_key: i.dedup_key(sig),
    }
}

fn swap_write(sig: &str, d: &DecodedTransaction, s: &Swap) -> SwapWrite {
    let activity_id = d
        .activities
        .get(s.activity_index as usize)
        .map(|a| a.id(sig))
        .unwrap_or_default();
    SwapWrite {
        swap_id: s.id(sig),
        activity_id,
        wallet: if s.attribution.is_verified() {
            s.wallet.clone()
        } else {
            None
        },
        attribution: s.attribution.as_str().to_owned(),
        authority: s.authority.clone(),
        beneficiary: s.beneficiary.clone(),
        input_mint: s.input.mint.clone(),
        input_amount: opt_u64(s.input.amount),
        input_decimals: s.input.decimals.map(i16::from),
        input_asset_kind: s.input.asset_kind.as_str().to_owned(),
        output_mint: s.output.mint.clone(),
        output_amount: opt_u64(s.output.amount),
        output_decimals: s.output.decimals.map(i16::from),
        output_asset_kind: s.output.asset_kind.as_str().to_owned(),
        exchange_ratio: s.exchange_ratio.map(|r| r.to_string()),
        protocol: s.protocol.to_owned(),
        venue_kind: s.venue_kind.as_str().to_owned(),
        aggregator: s.aggregator.map(str::to_owned),
        route_complete: s.route_complete,
        instruction_ref: s.instruction_ref.clone(),
        classification_status: s.status.as_str().to_owned(),
        reasons: s.reasons.clone(),
        legs: s
            .legs
            .iter()
            .enumerate()
            .map(|(i, l)| LegWrite {
                leg_index: i16::try_from(i).unwrap_or(i16::MAX),
                protocol: l.protocol.clone(),
                program_id: l.program_id.clone(),
                pool: l.pool.clone(),
                input_mint: l.input_mint.clone(),
                input_amount: l.input_amount.to_string(),
                output_mint: l.output_mint.clone(),
                output_amount: l.output_amount.to_string(),
                instruction_ref: l.instruction_ref.clone(),
                evidence_kind: l.evidence.as_str().to_owned(),
            })
            .collect(),
        fees: s
            .fees
            .iter()
            .enumerate()
            .map(|(i, f)| FeeWrite {
                fee_index: i16::try_from(i).unwrap_or(i16::MAX),
                kind: f.kind.as_str().to_owned(),
                mint: f.mint.clone(),
                amount: opt_u64(f.amount),
                provenance: f.provenance.as_str().to_owned(),
                recipient: f.recipient.clone(),
            })
            .collect(),
    }
}

fn block_time(d: &DecodedTransaction) -> Option<UtcTimestamp> {
    d.block_time
        .and_then(|t| time::OffsetDateTime::from_unix_timestamp(t).ok())
        .map(UtcTimestamp::from_offset_datetime)
}

/// Build the write set for a new revision (`state.current_revision + 1`).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn transaction_write(
    d: &DecodedTransaction,
    meta: &EvidenceMeta,
    state: &StoredTxState,
) -> TransactionWrite {
    let sig = d.signature.as_str();
    let revision = state.current_revision + 1;
    let now = UtcTimestamp::now();
    let finality = if meta.commitment == "finalized" {
        "FINALIZED"
    } else {
        "OBSERVED_CONFIRMED"
    };
    let mut balances = Vec::new();
    for c in &d.balances.native {
        balances.push(BalanceWrite {
            account_index: i16::try_from(c.account_index).unwrap_or(i16::MAX),
            asset: "SOL".to_owned(),
            account: c.account.clone(),
            owner: None,
            token_program: None,
            decimals: Some(9),
            pre_amount: Some(c.pre.to_string()),
            post_amount: Some(c.post.to_string()),
            delta: Some(c.delta.to_string()),
            pre_evidence: "REPORTED".to_owned(),
            post_evidence: "REPORTED".to_owned(),
            ownership_evidence: "NATIVE_ACCOUNT".to_owned(),
        });
    }
    for c in &d.balances.token {
        balances.push(BalanceWrite {
            account_index: i16::try_from(c.account_index).unwrap_or(i16::MAX),
            asset: c.mint.clone(),
            account: c.account.clone(),
            owner: c.owner.clone(),
            token_program: c.program_id.clone(),
            decimals: Some(i16::from(c.decimals)),
            pre_amount: Some(c.pre.to_string()),
            post_amount: Some(c.post.to_string()),
            delta: Some(c.delta.to_string()),
            pre_evidence: if c.pre_reported {
                "REPORTED"
            } else {
                "INFERRED_CREATED"
            }
            .to_owned(),
            post_evidence: if c.post_reported {
                "REPORTED"
            } else {
                "INFERRED_CLOSED"
            }
            .to_owned(),
            ownership_evidence: d
                .ownership
                .get(&c.account)
                .copied()
                .unwrap_or("UNKNOWN")
                .to_owned(),
        });
    }
    let activities = d
        .activities
        .iter()
        .map(|a| ActivityWrite {
            activity_id: a.id(sig),
            activity_index: i32::try_from(a.index).unwrap_or(i32::MAX),
            kind: a.kind.as_str().to_owned(),
            status: a.status.as_str().to_owned(),
            protocol: a.protocol.map(str::to_owned),
            program_id: a.program_id.clone(),
            instruction_ref: a.instruction_ref.clone(),
            actor: a.actor.clone(),
            attribution: a.attribution.map(|x| x.as_str().to_owned()),
            mint: a.mint.clone(),
            amount: opt_u64(a.amount),
            source_account: a.source_account.clone(),
            destination_account: a.destination_account.clone(),
            counterparty: a.counterparty.clone(),
            reasons: a.reasons.clone(),
            decoder: a.decoder.to_owned(),
        })
        .collect();
    let swaps: Vec<SwapWrite> = d.swaps.iter().map(|s| swap_write(sig, d, s)).collect();
    let mut kinds: Vec<String> = d
        .activities
        .iter()
        .map(|a| a.kind.as_str().to_owned())
        .collect();
    kinds.sort_unstable();
    kinds.dedup();

    let event = |event_type: &str,
                 aggregate_type: &str,
                 aggregate_id: String,
                 quality: &str,
                 payload: serde_json::Value| OutboxWrite {
        event_id: Uuid::new_v4(),
        event_type: event_type.to_owned(),
        schema_version: EVENT_SCHEMA,
        aggregate_type: aggregate_type.to_owned(),
        aggregate_id,
        aggregate_revision: revision,
        source: meta.provider.to_owned(),
        source_event_time: block_time(d),
        observed_at: meta.fetched_at,
        correlation_id: meta.correlation_id,
        causation_id: None,
        commitment: Some(meta.commitment.to_owned()),
        quality_state: quality.to_owned(),
        payload_json: payload.to_string(),
    };
    let mut outbox = vec![event(
        events::TRANSACTION_DECODED,
        "TRANSACTION",
        sig.to_owned(),
        d.status.as_str(),
        json!({
            "signature": sig,
            "slot": d.slot,
            "executionStatus": if d.failed { "FAILED" } else { "SUCCESS" },
            "decodeStatus": d.status.as_str(),
            "decoderVersion": DECODER_VERSION,
            "protocols": d.protocols,
            "swapCount": d.swaps.len(),
            "evidenceRevision": revision,
            "finality": finality,
        }),
    )];
    for (s, w) in d.swaps.iter().zip(&swaps) {
        outbox.push(event(
            events::SWAP_RECORDED,
            "SWAP",
            w.swap_id.clone(),
            s.status.as_str(),
            json!({
                "swapId": w.swap_id,
                "signature": sig,
                "slot": d.slot,
                "protocol": s.protocol,
                "wallet": w.wallet,
                "attribution": w.attribution,
                "inputMint": w.input_mint,
                "inputAmount": w.input_amount,
                "outputMint": w.output_mint,
                "outputAmount": w.output_amount,
                "exchangeRatio": w.exchange_ratio,
                "classificationStatus": w.classification_status,
                "verifiedTrade": s.is_verified_trade(),
                "finality": finality,
            }),
        ));
    }
    for gone in state
        .canonical_swap_ids
        .iter()
        .filter(|id| !swaps.iter().any(|w| &w.swap_id == *id))
    {
        outbox.push(event(
            events::SWAP_RETRACTED,
            "SWAP",
            gone.clone(),
            "NOT_APPLICABLE",
            json!({"swapId": gone, "signature": sig, "reason": "DECODER_OR_EVIDENCE_REVISION"}),
        ));
    }
    TransactionWrite {
        signature: sig.to_owned(),
        slot: d.slot,
        block_time: block_time(d),
        transaction_index: d.transaction_index.and_then(|i| i32::try_from(i).ok()),
        version: d.version.to_owned(),
        execution_status: if d.failed { "FAILED" } else { "SUCCESS" }.to_owned(),
        err_json: d.err_json.clone(),
        fee_lamports: d.fee.to_string(),
        fee_payer: d.fee_payer.clone(),
        signers: d.signers.clone(),
        compute_units: opt_u64(d.compute_units),
        finality: finality.to_owned(),
        next_finality_check_at: if finality == "FINALIZED" {
            None
        } else {
            meta.next_finality_check_at
        },
        decode_status: d.status.as_str().to_owned(),
        decoder_version: DECODER_VERSION.to_owned(),
        protocols: d.protocols.iter().map(|p| (*p).to_owned()).collect(),
        activity_kinds: kinds,
        first_observed_at: state.first_observed_at.unwrap_or(meta.first_observed_at),
        decoded_at: now,
        evidence: EvidenceWrite {
            commitment: meta.commitment.to_owned(),
            provider: meta.provider.to_owned(),
            slot: d.slot,
            fetched_at: meta.fetched_at,
            evidence_schema: EVIDENCE_SCHEMA,
            content_sha256: sha256_hex(&meta.raw_json),
            byte_len: i32::try_from(meta.raw_json.len()).unwrap_or(i32::MAX),
            raw_json: meta.raw_json.clone(),
        },
        balances,
        activities,
        swaps,
        issues: d.issues.iter().map(|i| issue_write(sig, i)).collect(),
        outbox,
    }
}

/// Token registry observations from a decoded transaction.
#[must_use]
pub fn token_observations(d: &DecodedTransaction) -> Vec<(String, Option<String>, i16, u64)> {
    let mut out: Vec<(String, Option<String>, i16, u64)> = Vec::new();
    for c in &d.balances.token {
        if !out.iter().any(|t| t.0 == c.mint) {
            out.push((
                c.mint.clone(),
                c.program_id.clone(),
                i16::from(c.decimals),
                d.slot,
            ));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::{Registry, decode, fixtures};

    #[test]
    fn write_set_satisfies_schema_shapes() {
        let reg = Registry::standard();
        for f in fixtures::catalog() {
            let d = decode(&f.json, &reg).unwrap();
            let meta = EvidenceMeta {
                commitment: "confirmed",
                provider: "FIXTURE",
                fetched_at: UtcTimestamp::now(),
                raw_json: f.json.clone(),
                first_observed_at: UtcTimestamp::now(),
                next_finality_check_at: None,
                correlation_id: Uuid::new_v4(),
            };
            let w = transaction_write(&d, &meta, &StoredTxState::default());
            assert_eq!(w.evidence.content_sha256.len(), 64);
            for a in &w.activities {
                assert!(a.activity_id.len() <= 160, "{}", a.activity_id);
                assert!(
                    a.decoder
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                );
                if let Some(r) = &a.instruction_ref {
                    assert!(
                        r.split('.')
                            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                    );
                }
            }
            for o in &w.outbox {
                assert!(o.payload_json.len() <= 16_384);
                assert!(events::ALL.contains(&o.event_type.as_str()));
            }
            assert_eq!(w.outbox.len(), 1 + w.swaps.len(), "{}", f.name);
            // Wallets are only set for verified attribution.
            for s in &w.swaps {
                if s.wallet.is_some() {
                    assert!(s.attribution.starts_with("VERIFIED_"));
                }
            }
        }
    }

    #[test]
    fn retracted_swaps_emit_events() {
        let f = fixtures::catalog()
            .into_iter()
            .find(|f| f.name == "failed_swap")
            .unwrap();
        let d = decode(&f.json, &Registry::standard()).unwrap();
        let meta = EvidenceMeta {
            commitment: "finalized",
            provider: "FIXTURE",
            fetched_at: UtcTimestamp::now(),
            raw_json: f.json,
            first_observed_at: UtcTimestamp::now(),
            next_finality_check_at: None,
            correlation_id: Uuid::new_v4(),
        };
        let state = StoredTxState {
            exists: true,
            current_revision: 1,
            canonical_swap_ids: vec!["swap:x:0".into()],
            ..StoredTxState::default()
        };
        let w = transaction_write(&d, &meta, &state);
        assert_eq!(w.finality, "FINALIZED");
        assert!(
            w.outbox
                .iter()
                .any(|o| o.event_type == events::SWAP_RETRACTED && o.aggregate_revision == 2)
        );
    }
}
