//! `solprint gateway-diagnostics [--live] [--include-fallback]`
//!
//! Without `--live` this contacts nothing: it validates configuration and
//! prints GATEWAY's effective limits and which credentials are present (by
//! name, never value).
//!
//! With `--live` it performs a small, fixed, read-only check, at most:
//!
//! * Helius: `getHealth`, `getVersion`, `getSlot` (3 credits),
//! * Jupiter: one Price V3 request for wrapped SOL,
//! * DEX Screener: one `tokens/v1` request for wrapped SOL,
//! * public Solana RPC: one `getSlot`, only with `--include-fallback`.
//!
//! Credits are charged to the persisted ledger when PostgreSQL is reachable;
//! otherwise the report says they were not recorded. Results describe this
//! moment only and are not stored as current measurements.

// This is a command-line report: printing to stdout is its purpose.
#![allow(clippy::print_stdout)]

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use solprint_config::Config;
use solprint_core::TokenMint;
use solprint_gateway::{
    Gateway, GatewayError, GatewaySettings, GatewayStore, MemoryStore, Priority, RequestContext,
};
use solprint_storage::Database;

const WSOL: &str = "So11111111111111111111111111111111111111112";

pub fn run(live: bool, include_fallback: bool) -> ExitCode {
    let config = match Config::load() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("solprint: {err}");
            return ExitCode::from(2);
        }
    };
    let settings = GatewaySettings::from_config(&config);
    print_offline(&settings);
    if !live {
        println!();
        println!(
            "No provider was contacted. Re-run with --live to perform the bounded read-only check."
        );
        return ExitCode::SUCCESS;
    }
    let Ok(rt) = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    else {
        eprintln!("solprint: could not build async runtime");
        return ExitCode::FAILURE;
    };
    rt.block_on(run_live(&config, settings, include_fallback))
}

fn present(v: bool) -> &'static str {
    if v { "configured" } else { "absent" }
}

fn print_offline(s: &GatewaySettings) {
    println!("SOLPRINT GATEWAY diagnostics (configuration)");
    println!("  gateway enabled ............ {}", s.enabled);
    println!("  execution capability ....... NONE (read-only; no signing or submission)");
    println!(
        "  HELIUS_API_KEY ............. {}",
        present(s.helius.api_key.is_some())
    );
    println!(
        "  JUPITER_API_KEY ............ {}",
        present(s.jupiter.api_key.is_some())
    );
    println!(
        "  Helius budget .............. {} credits/cycle, {} % reserve, cycle starts day {}",
        s.helius.budget.monthly_credits,
        s.helius.budget.reserve_percent,
        s.helius.budget.cycle_start_day
    );
    println!(
        "  Helius limits .............. {} req/s, {} in flight, getBlock {}",
        s.helius.rps,
        s.helius.max_in_flight,
        if s.helius.get_block_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  Jupiter limits ............. {} req/min, {} in flight, quotes {}",
        s.jupiter.rpm,
        s.jupiter.max_in_flight,
        if s.jupiter.quote_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  DEX Screener limits ........ {} req/min, {} in flight, discovery {}",
        s.dexscreener.rpm,
        s.dexscreener.max_in_flight,
        if s.dexscreener.discovery_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!(
        "  public RPC fallback ........ {} ({} req/s)",
        if s.fallback.enabled {
            "enabled"
        } else {
            "disabled"
        },
        s.fallback.rps
    );
    println!(
        "  streams .................... {}",
        if s.streams_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
}

fn line<T>(
    name: &str,
    r: &Result<solprint_gateway::GatewayResponse<T>, GatewayError>,
    show: impl Fn(&T) -> String,
) -> bool {
    match r {
        Ok(resp) => {
            println!(
                "  {name:<26} OK    age={}ms provider={} credits={} {}",
                resp.age.as_millis(),
                resp.provider.as_str(),
                resp.credits_charged,
                show(&resp.value)
            );
            true
        }
        Err(e) => {
            println!("  {name:<26} FAIL  {}", e.code());
            false
        }
    }
}

async fn run_live(config: &Config, settings: GatewaySettings, include_fallback: bool) -> ExitCode {
    println!();
    println!("Live check (bounded, read-only). Results describe this moment only.");
    let mut db_cfg = config.database.clone();
    db_cfg.startup_timeout = Duration::from_secs(5);
    let (store, recorded): (Arc<dyn GatewayStore>, bool) = match Database::connect(&db_cfg).await {
        Ok(db) if db.verify_migrations().await.is_ok() => (Arc::new(db), true),
        _ => (Arc::new(MemoryStore::default()), false),
    };
    let gateway = match Gateway::new(settings, store) {
        Ok(g) => g,
        Err(e) => {
            println!("  GATEWAY could not start: {}", e.code());
            return ExitCode::FAILURE;
        }
    };
    gateway.load_ledger().await;
    // Diagnostics run at P1 so a conserving budget still admits them, but
    // never at P0 (reserved for safety-critical reads).
    let ctx =
        || RequestContext::new("diagnostics", Priority::P1).with_timeout(Duration::from_secs(10));
    let mut ok = true;
    ok &= line(
        "helius getHealth",
        &gateway.get_health(&ctx().allow_fallback(false)).await,
        |h| format!("{h:?}"),
    );
    ok &= line(
        "helius getVersion",
        &gateway.get_version(&ctx().allow_fallback(false)).await,
        |v| v.solana_core.clone(),
    );
    ok &= line(
        "helius getSlot",
        &gateway.get_slot(&ctx().allow_fallback(false)).await,
        |s| format!("slot={s}"),
    );
    let wsol = TokenMint::parse(WSOL).ok();
    if let Some(m) = &wsol {
        ok &= line(
            "jupiter price (wSOL)",
            &gateway
                .jupiter_prices(&ctx(), std::slice::from_ref(m))
                .await,
            |p| {
                p.first().map_or_else(
                    || "no price".to_owned(),
                    |p| format!("usd={}", p.usd_price.get()),
                )
            },
        );
        ok &= line(
            "dexscreener tokens (wSOL)",
            &gateway
                .dex_pairs_by_tokens(&ctx(), std::slice::from_ref(m))
                .await,
            |p| format!("pairs={}", p.len()),
        );
    }
    if include_fallback {
        let r = gateway.fallback_get_slot(&ctx()).await;
        ok &= line("public rpc getSlot", &r, |s| format!("slot={s}"));
    }
    gateway.flush().await;
    let b = gateway.snapshot().budget;
    if let Some(b) = b {
        println!(
            "  Helius budget now .......... {} / {} credits used ({})",
            b.credits_used,
            b.credit_limit,
            b.state.as_str()
        );
    }
    if !recorded {
        println!(
            "  NOTE: PostgreSQL was not reachable; credits used here were NOT recorded in the ledger."
        );
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
