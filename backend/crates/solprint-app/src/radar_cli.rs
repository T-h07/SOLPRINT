//! `solprint radar <command>`: owner controls for RADAR.
//!
//! Every command reads or writes local PostgreSQL state only; none of them
//! contacts a provider. Work they request (a discovery cycle, an
//! investigation) is carried out by the running service through the DATA
//! ENGINE and GATEWAY, under RADAR's budget share and GATEWAY's admission.
//! Each change is recorded in `radar_operator_actions`.
//!
//! ```text
//! radar status
//! radar pause [--reason CODE] | radar resume
//! radar cycle                          (request one discovery cycle)
//! radar investigate <ADDRESS>          (add a wallet by operator request)
//! radar reevaluate <ADDRESS>
//! radar exclude <ADDRESS> | radar include <ADDRESS>
//! radar reactivate <ADDRESS>
//! radar cancel <JOB_ID>
//! radar wallets [--lifecycle L] [--limit N]
//! radar wallet <ADDRESS>
//! radar markets [--selection S] [--limit N]
//! radar jobs [STATE] [--limit N]
//! ```

// This is a command-line tool: printing to stdout is its purpose.
#![allow(clippy::print_stdout)]

use std::process::ExitCode;

use serde_json::json;
use solprint_config::Config;
use solprint_core::{UtcTimestamp, WalletAddress};
use solprint_storage::Database;
use solprint_storage::radar::{JobFilter, MarketFilter, WalletFilter};
use uuid::Uuid;

const USAGE: &str = "usage: solprint radar <status|pause|resume|cycle|investigate|reevaluate|exclude|include|reactivate|cancel|wallets|wallet|markets|jobs> ...";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("solprint radar: {msg}");
    ExitCode::from(2)
}

fn code(v: &str) -> Result<String, String> {
    if !v.is_empty()
        && v.len() <= 40
        && v.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        Ok(v.to_owned())
    } else {
        Err(format!(
            "invalid code {v:?} (expected SCREAMING_SNAKE_CASE)"
        ))
    }
}

fn address(rest: &[String]) -> Result<String, String> {
    let a = rest.first().ok_or("missing <ADDRESS>")?;
    WalletAddress::parse(a).map_err(|_| "invalid address".to_owned())?;
    Ok(a.clone())
}

fn limit(rest: &[String]) -> Result<i64, String> {
    flag(rest, "--limit")
        .map(|v| v.parse::<i64>().map_err(|_| "invalid --limit".to_owned()))
        .transpose()
        .map(|v| v.unwrap_or(25).clamp(1, 200))
}

fn ots(t: Option<UtcTimestamp>) -> Option<String> {
    t.map(|t| t.to_rfc3339())
}

pub fn run(args: &[String]) -> ExitCode {
    let Some(cmd) = args.first().map(String::as_str) else {
        return fail(USAGE);
    };
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

async fn record(
    db: &Database,
    action: &str,
    subject: Option<&str>,
    ok: bool,
) -> Result<(), String> {
    db.radar_record_operator_action(action, subject, if ok { "APPLIED" } else { "NO_CHANGE" })
        .await
        .map_err(|e| e.to_string())
}

#[allow(clippy::too_many_lines)]
async fn dispatch(
    cmd: &str,
    rest: &[String],
    config: &Config,
    db: &Database,
) -> Result<(), String> {
    let s = |e: solprint_storage::StorageError| e.to_string();
    match cmd {
        "status" => {
            let c = db.radar_counts().await.map_err(s)?;
            let control = db.radar_control().await.map_err(s)?;
            let jobs = db.radar_job_counts().await.map_err(s)?;
            println!(
                "{}",
                json!({
                    "enabled": config.radar.enabled,
                    "autoDiscoveryEnabled": config.radar.auto_discovery_enabled,
                    "dataEngineEnabled": config.data_engine.enabled,
                    "paused": control.paused,
                    "pauseReason": control.pause_reason,
                    "cycleRequested": control.cycle_requested,
                    "wallets": c.wallets_total,
                    "walletsByLifecycle": c.wallets_by_lifecycle,
                    "evidence": c.evidence_total,
                    "markets": c.markets_total,
                    "poolsSupported": c.pools_supported,
                    "monitoringActive": c.monitoring_active,
                    "jobsOpen": jobs.open_total,
                    "jobsDeferredByReason": jobs.deferred_by_reason,
                    "lastCycleAt": ots(c.last_cycle_at),
                })
            );
            Ok(())
        }
        "pause" => {
            let reason = flag(rest, "--reason")
                .map(|r| code(&r))
                .transpose()?
                .unwrap_or_else(|| "OPERATOR_PAUSE".into());
            db.radar_set_paused(true, Some(&reason)).await.map_err(s)?;
            record(db, "PAUSE", None, true).await?;
            println!("{}", json!({"paused": true, "reason": reason}));
            Ok(())
        }
        "resume" => {
            db.radar_set_paused(false, None).await.map_err(s)?;
            record(db, "RESUME", None, true).await?;
            println!("{}", json!({"paused": false}));
            Ok(())
        }
        "cycle" => {
            db.radar_request_cycle(true).await.map_err(s)?;
            record(db, "REQUEST_CYCLE", None, true).await?;
            println!(
                "{}",
                json!({
                    "cycleRequested": true,
                    "note": if config.radar.enabled {
                        "the running service starts the cycle within ~15 s; provider work stays under RADAR's budget share"
                    } else {
                        "RADAR_ENABLED=false: the request waits until RADAR runs"
                    },
                })
            );
            Ok(())
        }
        "investigate" => {
            let a = address(rest)?;
            let new = db
                .radar_register_operator_wallet(Uuid::new_v4(), &a)
                .await
                .map_err(s)?;
            let dirty = db.radar_mark_dirty(&a).await.map_err(s)?;
            record(db, "INVESTIGATE", Some(&a), new || dirty).await?;
            println!(
                "{}",
                json!({"address": a, "added": new, "queuedForEvaluation": dirty})
            );
            Ok(())
        }
        "reevaluate" => {
            let a = address(rest)?;
            let ok = db.radar_mark_dirty(&a).await.map_err(s)?;
            record(db, "REEVALUATE", Some(&a), ok).await?;
            if !ok {
                return Err("wallet is not in the registry".into());
            }
            println!("{}", json!({"address": a, "queuedForEvaluation": true}));
            Ok(())
        }
        "exclude" | "include" => {
            let a = address(rest)?;
            let excluded = cmd == "exclude";
            let ok = db
                .radar_set_operator_excluded(&a, excluded)
                .await
                .map_err(s)?;
            record(
                db,
                if excluded { "EXCLUDE" } else { "INCLUDE" },
                Some(&a),
                ok,
            )
            .await?;
            if !ok {
                return Err("wallet is not in the registry".into());
            }
            println!("{}", json!({"address": a, "operatorExcluded": excluded}));
            Ok(())
        }
        "reactivate" => {
            let a = address(rest)?;
            let ok = db
                .radar_reactivate(&a, "OPERATOR_REQUEST", None)
                .await
                .map_err(s)?;
            record(db, "REACTIVATE", Some(&a), ok).await?;
            println!("{}", json!({"address": a, "reactivated": ok}));
            Ok(())
        }
        "cancel" => {
            let id = rest
                .first()
                .and_then(|v| Uuid::parse_str(v).ok())
                .ok_or("missing or invalid <JOB_ID>")?;
            let ok = db.radar_cancel_job(id).await.map_err(s)?;
            record(db, "CANCEL_JOB", Some(&id.to_string()), ok).await?;
            println!(
                "{}",
                json!({"jobId": id.to_string(), "cancelRequested": ok})
            );
            Ok(())
        }
        "wallets" => {
            let f = WalletFilter {
                lifecycle: flag(rest, "--lifecycle").map(|v| code(&v)).transpose()?,
                limit: limit(rest)?,
                ..WalletFilter::default()
            };
            for w in db.radar_wallets(&f).await.map_err(s)? {
                println!(
                    "{}",
                    json!({
                        "address": w.address, "lifecycle": w.lifecycle, "eligibility": w.eligibility,
                        "qualification": w.qualification, "priority": w.priority_score,
                        "stage": w.history_stage, "coverage": w.history_coverage_state,
                        "monitoring": w.monitoring_mode, "verifiedSwaps": w.verified_swap_count,
                        "lastTradeAt": ots(w.last_verified_trade_at),
                    })
                );
            }
            Ok(())
        }
        "wallet" => {
            let a = address(rest)?;
            let w = db
                .radar_wallet(&a)
                .await
                .map_err(s)?
                .ok_or("wallet is not in the registry")?;
            let transitions = db.radar_wallet_transitions(&a, 10).await.map_err(s)?;
            println!(
                "{}",
                json!({
                    "address": w.address, "lifecycle": w.lifecycle, "eligibility": w.eligibility,
                    "eligibilityReasons": w.eligibility_reasons, "qualification": w.qualification,
                    "qualificationReasons": w.qualification_reasons, "priority": w.priority_score,
                    "stage": w.history_stage, "coverage": w.history_coverage_state,
                    "monitoring": w.monitoring_mode, "operatorExcluded": w.operator_excluded,
                    "firstSource": w.first_discovery_source,
                    "transitions": transitions.iter().map(|t| json!({
                        "from": t.from_state, "to": t.to_state, "trigger": t.trigger,
                        "reasons": t.reasons, "at": t.transitioned_at.to_rfc3339(),
                    })).collect::<Vec<_>>(),
                })
            );
            Ok(())
        }
        "markets" => {
            let f = MarketFilter {
                selection_state: flag(rest, "--selection").map(|v| code(&v)).transpose()?,
                limit: limit(rest)?,
                ..MarketFilter::default()
            };
            for m in db.radar_markets(&f).await.map_err(s)? {
                println!(
                    "{}",
                    json!({
                        "mint": m.mint, "symbol": m.display_symbol, "classification": m.classification,
                        "bucket": m.bucket, "selection": m.selection_state, "reasons": m.selection_reasons,
                        "priority": m.market_priority, "walletsFound": m.wallets_found,
                        "observedAt": ots(m.last_market_observation_at),
                    })
                );
            }
            Ok(())
        }
        "jobs" => {
            let state = rest
                .first()
                .filter(|v| !v.starts_with("--"))
                .map(|v| code(v))
                .transpose()?;
            let f = JobFilter {
                state,
                limit: limit(rest)?,
                ..JobFilter::default()
            };
            for j in db.radar_jobs(&f).await.map_err(s)? {
                println!(
                    "{}",
                    json!({
                        "jobId": j.job_id.to_string(), "class": j.work_class, "subject": j.subject,
                        "origin": j.origin, "state": j.state, "deferReason": j.defer_reason,
                        "result": j.result_code, "estimatedCredits": j.estimated_credits,
                        "creditsSpent": j.credits_spent, "signaturesFound": j.signatures_found,
                    })
                );
            }
            Ok(())
        }
        _ => Err(USAGE.into()),
    }
}
