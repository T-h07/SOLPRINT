//! `solprint data-engine <command>`: controlled, owner-initiated operations.
//!
//! Every command except `ingest --live` works on local state only; none of
//! them contacts a provider. Queued work is fetched by the running service
//! through GATEWAY under its budget admission and rate limits.
//!
//! ```text
//! data-engine status
//! data-engine targets list
//! data-engine targets add <ADDRESS> [--label L] [--priority P2..P5] [--commitment confirmed|finalized]
//! data-engine targets enable|disable <TARGET_ID>
//! data-engine ingest <SIGNATURE> [--finalized] [--live]
//! data-engine backfill create <ADDRESS> [--target ID] [--pages N] [--max-signatures N] [--min-slot S] [--until SIG]
//! data-engine backfill list
//! data-engine backfill cancel <BACKFILL_ID>
//! data-engine reprocess [--all] [--limit N]
//! data-engine decode-fixture [NAME]      (synthetic fixtures; no database)
//! ```

// This is a command-line tool: printing to stdout is its purpose.
#![allow(clippy::print_stdout)]

use std::process::ExitCode;
use std::sync::Arc;

use serde_json::json;
use solprint_config::Config;
use solprint_core::{CommitmentLevel, TransactionSignature, WalletAddress};
use solprint_data_engine::engine::{Engine, JobOutcome};
use solprint_data_engine::source::GatewaySource;
use solprint_data_engine::{DECODER_VERSION, Registry, decode, fixtures};
use solprint_gateway::{Gateway, GatewaySettings};
use solprint_storage::Database;
use solprint_storage::data_engine::{NewBackfill, NewTarget};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const USAGE: &str =
    "usage: solprint data-engine <status|targets|ingest|backfill|reprocess|decode-fixture> ...";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("solprint data-engine: {msg}");
    ExitCode::from(2)
}

pub fn run(args: &[String]) -> ExitCode {
    let Some(cmd) = args.first().map(String::as_str) else {
        return fail(USAGE);
    };
    if cmd == "decode-fixture" {
        return decode_fixture(args.get(1).map(String::as_str));
    }
    let config = match Config::load() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("solprint: {err}");
            return ExitCode::from(2);
        }
    };
    let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        return fail("could not build async runtime");
    };
    rt.block_on(async move {
        let db = match Database::connect(&config.database).await {
            Ok(db) => db,
            Err(e) => return fail(&format!("database unavailable: {e}")),
        };
        if let Err(e) = db.verify_migrations().await {
            return fail(&format!("migrations not applied: {e}"));
        }
        let code = match dispatch(cmd, args.get(1..).unwrap_or_default(), &config, &db).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => fail(&e),
        };
        db.close().await;
        code
    })
}

async fn dispatch(
    cmd: &str,
    rest: &[String],
    config: &Config,
    db: &Database,
) -> Result<(), String> {
    let s = |e: solprint_storage::StorageError| e.to_string();
    match cmd {
        "status" => {
            let c = db.de_counts().await.map_err(s)?;
            println!(
                "{}",
                json!({
                    "decoderVersion": DECODER_VERSION,
                    "enabled": config.data_engine.enabled,
                    "jobsByState": c.jobs_by_state,
                    "transactions": c.transactions,
                    "transactionsByFinality": c.transactions_by_finality,
                    "swapsCanonical": c.swaps_canonical,
                    "swapsVerified": c.swaps_verified,
                    "openIssuesBySeverity": c.open_issues_by_severity,
                    "outboxUndelivered": c.outbox_undelivered,
                    "targetsEnabled": c.targets_enabled,
                    "backfillsActive": c.backfills_active,
                })
            );
            Ok(())
        }
        "targets" => targets(rest, db).await,
        "ingest" => ingest(rest, config, db).await,
        "backfill" => backfill(rest, config, db).await,
        "reprocess" => {
            let limit = flag(rest, "--limit")
                .and_then(|v| v.parse().ok())
                .unwrap_or(10_000_i64);
            let only_stale = if has(rest, "--all") {
                None
            } else {
                Some(DECODER_VERSION)
            };
            let n = db
                .de_requeue_for_reprocess(only_stale, limit)
                .await
                .map_err(s)?;
            println!(
                "{}",
                json!({"requeued": n, "decoderVersion": DECODER_VERSION, "usesStoredEvidence": true})
            );
            Ok(())
        }
        _ => Err(USAGE.to_owned()),
    }
}

async fn targets(rest: &[String], db: &Database) -> Result<(), String> {
    let s = |e: solprint_storage::StorageError| e.to_string();
    match rest.first().map(String::as_str) {
        Some("list") | None => {
            for t in db.de_targets().await.map_err(s)? {
                println!(
                    "{}",
                    json!({"targetId": t.target_id, "type": t.target_type, "address": t.address, "label": t.label,
                           "enabled": t.enabled, "priority": t.priority, "commitment": t.commitment, "coverage": t.coverage_state})
                );
            }
            Ok(())
        }
        Some("add") => {
            let address = rest.get(1).ok_or("address required")?;
            WalletAddress::parse(address).map_err(|_| "invalid address")?;
            let priority = flag(rest, "--priority").unwrap_or_else(|| "P3".to_owned());
            if !matches!(priority.as_str(), "P2" | "P3" | "P4" | "P5") {
                return Err("priority must be P2..P5".to_owned());
            }
            let commitment = flag(rest, "--commitment").unwrap_or_else(|| "confirmed".to_owned());
            if !matches!(commitment.as_str(), "confirmed" | "finalized") {
                return Err("commitment must be confirmed or finalized".to_owned());
            }
            let t = db
                .de_add_target(&NewTarget {
                    target_id: Uuid::new_v4(),
                    target_type: "WALLET_ADDRESS".to_owned(),
                    address: address.clone(),
                    label: flag(rest, "--label"),
                    priority,
                    commitment,
                    discovery_source: "MANUAL".to_owned(),
                })
                .await
                .map_err(s)?;
            println!(
                "{}",
                json!({"targetId": t.target_id, "address": t.address, "enabled": t.enabled})
            );
            Ok(())
        }
        Some(op @ ("enable" | "disable")) => {
            let id: Uuid = rest
                .get(1)
                .and_then(|v| v.parse().ok())
                .ok_or("target id required")?;
            let ok = db
                .de_set_target_enabled(id, op == "enable")
                .await
                .map_err(s)?;
            println!("{}", json!({"targetId": id, "updated": ok}));
            Ok(())
        }
        _ => Err("targets list|add|enable|disable".to_owned()),
    }
}

async fn ingest(rest: &[String], config: &Config, db: &Database) -> Result<(), String> {
    let sig = rest.first().ok_or("signature required")?;
    TransactionSignature::parse(sig).map_err(|_| "invalid signature")?;
    let mut de = config.data_engine.clone();
    if has(rest, "--finalized") {
        de.default_commitment = solprint_config::RetrievalCommitment::Finalized;
    }
    let live = has(rest, "--live");
    let gateway = Gateway::new(GatewaySettings::from_config(config), Arc::new(db.clone()))
        .map_err(|e| e.to_string())?;
    let engine = Arc::new(Engine::new(
        db.clone(),
        Arc::new(GatewaySource::new(gateway)),
        de,
    ));
    let outcome = engine
        .observe(
            sig,
            "MANUAL",
            None,
            Some(CommitmentLevel::Confirmed),
            None,
            None,
            3,
        )
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "{}",
        json!({"signature": sig, "enqueue": format!("{outcome:?}")})
    );
    if !live {
        println!(
            "Queued only; no provider was contacted. The running service fetches it through GATEWAY."
        );
        return Ok(());
    }
    // Explicit opt-in: process this one signature now through GATEWAY.
    let cancel = CancellationToken::new();
    let jobs = db
        .de_claim_jobs(engine.owner(), 1, config.data_engine.job_lease)
        .await
        .map_err(|e| e.to_string())?;
    let Some(job) = jobs.into_iter().find(|j| &j.signature == sig) else {
        db.de_release_owner(engine.owner())
            .await
            .map_err(|e| e.to_string())?;
        println!("The job is not claimable now (already processed or leased).");
        return Ok(());
    };
    let r = engine
        .process_job(&job, &cancel)
        .await
        .map_err(|e| e.to_string())?;
    let m = engine.metrics().snapshot();
    println!(
        "{}",
        json!({"outcome": match r {
            JobOutcome::Persisted { revision } => format!("PERSISTED r{revision}"),
            JobOutcome::Unchanged { revision } => format!("UNCHANGED r{revision}"),
            other => format!("{other:?}").to_uppercase(),
        }, "creditsCharged": m.credits_charged, "lastErrorCode": m.last_error_code})
    );
    db.de_release_owner(engine.owner())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn backfill(rest: &[String], config: &Config, db: &Database) -> Result<(), String> {
    let s = |e: solprint_storage::StorageError| e.to_string();
    let de = &config.data_engine;
    match rest.first().map(String::as_str) {
        Some("create") => {
            let address = rest.get(1).ok_or("address required")?;
            WalletAddress::parse(address).map_err(|_| "invalid address")?;
            let num = |name: &str, default: u32, max: u32| -> Result<i32, String> {
                let v = flag(rest, name).map_or(Ok(default), |v| {
                    v.parse::<u32>()
                        .map_err(|_| format!("{name} must be a number"))
                })?;
                i32::try_from(v.min(max)).map_err(|_| format!("{name} out of range"))
            };
            let pages = num("--pages", de.backfill_max_pages, de.backfill_max_pages)?;
            let page_size = i32::from(de.backfill_page_size);
            let b = db
                .de_create_backfill(&NewBackfill {
                    backfill_id: Uuid::new_v4(),
                    target_id: flag(rest, "--target").and_then(|v| v.parse().ok()),
                    address: address.clone(),
                    commitment: de.default_commitment.as_str().to_owned(),
                    start_before_signature: flag(rest, "--before"),
                    until_signature: flag(rest, "--until"),
                    min_slot: flag(rest, "--min-slot").and_then(|v| v.parse().ok()),
                    page_size,
                    max_pages: pages,
                    max_signatures: num(
                        "--max-signatures",
                        de.backfill_max_signatures,
                        de.backfill_max_signatures,
                    )?,
                    // One getSignaturesForAddress request per page; the
                    // per-transaction fetches are budgeted separately.
                    credits_estimated: pages,
                })
                .await
                .map_err(s)?;
            println!(
                "{}",
                json!({"backfillId": b.backfill_id, "status": b.status, "maxPages": b.max_pages, "maxSignatures": b.max_signatures,
                       "enabled": de.backfill_enabled})
            );
            if !de.backfill_enabled {
                println!(
                    "DATA_ENGINE_BACKFILL_ENABLED=false: the backfill stays PENDING until enabled."
                );
            }
            Ok(())
        }
        Some("list") | None => {
            for b in db.de_backfills(50).await.map_err(s)? {
                println!(
                    "{}",
                    json!({"backfillId": b.backfill_id, "address": b.address, "status": b.status, "pages": b.pages_fetched,
                           "discovered": b.discovered_count, "queued": b.queued_count, "coverage": b.coverage_state,
                           "stopReason": b.stop_reason})
                );
            }
            Ok(())
        }
        Some("cancel") => {
            let id: Uuid = rest
                .get(1)
                .and_then(|v| v.parse().ok())
                .ok_or("backfill id required")?;
            let ok = db.de_request_backfill_cancel(id).await.map_err(s)?;
            println!("{}", json!({"backfillId": id, "cancelRequested": ok}));
            Ok(())
        }
        _ => Err("backfill create|list|cancel".to_owned()),
    }
}

fn decode_fixture(name: Option<&str>) -> ExitCode {
    let reg = Registry::standard();
    let mut found = false;
    for f in fixtures::catalog() {
        if name.is_some_and(|n| n != f.name) {
            continue;
        }
        found = true;
        match decode(&f.json, &reg) {
            Ok(d) => println!(
                "{}",
                json!({
                    "fixture": f.name,
                    "dataLabel": "FIXTURE",
                    "status": d.status.as_str(),
                    "protocols": d.protocols,
                    "activities": d.activities.iter().map(|a| a.kind.as_str()).collect::<Vec<_>>(),
                    "swaps": d.swaps.iter().map(|s| json!({
                        "protocol": s.protocol,
                        "attribution": s.attribution.as_str(),
                        "inputAmount": s.input.amount.map(|v| v.to_string()),
                        "outputAmount": s.output.amount.map(|v| v.to_string()),
                        "status": s.status.as_str(),
                    })).collect::<Vec<_>>(),
                    "issues": d.issues.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(),
                })
            ),
            Err(e) => println!("{}", json!({"fixture": f.name, "error": e.to_string()})),
        }
    }
    if found {
        ExitCode::SUCCESS
    } else {
        fail("unknown fixture")
    }
}
