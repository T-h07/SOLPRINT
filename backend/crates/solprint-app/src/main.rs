//! SOLPRINT backend — composition root.
//!
//! Lifecycle:
//!
//! 1. Load and validate configuration (fail closed; live trading rejected).
//! 2. Initialise structured logging.
//! 3. Connect to PostgreSQL within `DATABASE_STARTUP_TIMEOUT_SECS`.
//! 4. Apply (or verify) migrations and record this start.
//! 5. Build GATEWAY (contacts nothing until asked) and start its
//!    maintenance task (ledger load, usage flush, retention; streams only if
//!    `GATEWAY_STREAMS_ENABLED=true`).
//! 6. Bind the HTTP listener and serve.
//! 7. On SIGINT / SIGTERM (Linux) or Ctrl+C / Ctrl+Break / close (Windows):
//!    stop accepting, drain in-flight requests, stop GATEWAY (final usage
//!    flush), close the pool, exit 0.
//!
//! Any failure in 1–6 exits non-zero before the listener exists, so the
//! process can never look healthy after a failed initialisation.

mod diagnostics;

use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use solprint_api::{AppState, BuildInfo};
use solprint_config::Config;
use solprint_gateway::{Gateway, GatewaySettings};
use solprint_storage::Database;
use tokio::net::TcpListener;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Upper bound for draining in-flight requests after a shutdown signal.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    // `solprint healthcheck` probes the local readiness endpoint and exits
    // 0/1. Used by the container HEALTHCHECK so the runtime image needs no
    // curl/wget/shell. Reads only APP_PORT; never loads secrets.
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        return healthcheck();
    }
    // `solprint gateway-diagnostics [--live]`: offline configuration report;
    // with `--live`, a small, bounded, read-only provider check.
    if std::env::args().nth(1).as_deref() == Some("gateway-diagnostics") {
        let live = std::env::args().skip(2).any(|a| a == "--live");
        let fallback = std::env::args().skip(2).any(|a| a == "--include-fallback");
        return diagnostics::run(live, fallback);
    }

    // Configuration errors are reported before logging exists, on stderr,
    // by key name only (values are never echoed).
    let config = match Config::load() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("solprint: {err}");
            return ExitCode::from(2);
        }
    };
    if let Err(err) = solprint_observability::init(&config.log) {
        eprintln!("solprint: logging initialisation failed: {err}");
        return ExitCode::from(2);
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("solprint-worker")
        .build()
    {
        Ok(rt) => rt,
        Err(err) => {
            tracing::error!(error = %err, "failed to build async runtime");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(config)) {
        Ok(()) => {
            tracing::info!("shutdown complete");
            ExitCode::SUCCESS
        }
        Err(err) => {
            tracing::error!(error = format!("{err:#}"), "fatal startup or runtime error");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> anyhow::Result<()> {
    let boot = Instant::now();
    tracing::info!(version = VERSION, config = %config.safe_summary(), "starting SOLPRINT");

    let database = Database::connect(&config.database)
        .await
        .context("database initialisation failed")?;
    if config.database.run_migrations {
        database
            .migrate()
            .await
            .context("database migration failed")?;
    } else {
        database
            .verify_migrations()
            .await
            .context("database migration verification failed")?;
        tracing::info!("migrations verified (this process does not own migrations)");
    }
    let instance = database
        .register_start(VERSION)
        .await
        .context("could not record instance start")?;
    tracing::info!(
        instance_id = %instance.instance_id,
        start_count = instance.start_count,
        first_started_at = %instance.created_at.to_rfc3339(),
        "instance registered"
    );

    let gateway = Gateway::new(
        GatewaySettings::from_config(&config),
        Arc::new(database.clone()),
    )
    .context("GATEWAY initialisation failed")?;
    let gateway_shutdown = CancellationToken::new();
    let gateway_task = gateway.start(gateway_shutdown.clone());

    let addr = SocketAddr::new(config.app.host, config.app.port);
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("could not bind {addr}"))?;
    let state = AppState::new(
        &config,
        database.clone(),
        gateway.clone(),
        BuildInfo::current(VERSION),
    );

    let shutdown = CancellationToken::new();
    let server = tokio::spawn(solprint_api::serve(listener, state, shutdown.clone()));
    tracing::info!(
        %addr,
        trading_mode = %config.trading.mode,
        startup_ms = u64::try_from(boot.elapsed().as_millis()).unwrap_or(u64::MAX),
        "SOLPRINT ready"
    );

    let mut server = server;
    tokio::select! {
        signal = shutdown_signal() => {
            tracing::info!(signal, "shutdown signal received; draining requests");
            shutdown.cancel();
            match tokio::time::timeout(SHUTDOWN_GRACE, &mut server).await {
                Ok(joined) => joined.context("server task panicked")?.context("server error")?,
                Err(_) => {
                    tracing::warn!(grace_s = SHUTDOWN_GRACE.as_secs(), "drain timed out; aborting");
                    server.abort();
                }
            }
        }
        joined = &mut server => {
            // The server should only stop after cancellation.
            joined.context("server task panicked")?.context("server error")?;
            anyhow::bail!("HTTP server stopped unexpectedly");
        }
    }

    // Stop GATEWAY after HTTP has drained; its task runs a final flush so
    // credits used by this process reach the ledger.
    gateway_shutdown.cancel();
    if tokio::time::timeout(SHUTDOWN_GRACE, gateway_task)
        .await
        .is_err()
    {
        tracing::warn!("GATEWAY final flush timed out");
    }
    database.close().await;
    Ok(())
}

fn healthcheck() -> ExitCode {
    use std::io::{Read, Write};
    let port = std::env::var("APP_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = Duration::from_secs(3);
    let probe = || -> std::io::Result<bool> {
        let mut stream = std::net::TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.write_all(b"GET /health/ready HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut head = [0_u8; 12];
        stream.read_exact(&mut head)?;
        Ok(head.starts_with(b"HTTP/1.1 200") || head.starts_with(b"HTTP/1.0 200"))
    };
    match probe() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

#[cfg(unix)]
async fn shutdown_signal() -> &'static str {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        tracing::error!("could not install signal handlers; falling back to ctrl_c");
        let _ = tokio::signal::ctrl_c().await;
        return "ctrl_c";
    };
    tokio::select! {
        _ = term.recv() => "SIGTERM",
        _ = int.recv() => "SIGINT",
    }
}

#[cfg(windows)]
async fn shutdown_signal() -> &'static str {
    use tokio::signal::windows;
    let (Ok(mut ctrl_c), Ok(mut brk), Ok(mut close)) = (
        windows::ctrl_c(),
        windows::ctrl_break(),
        windows::ctrl_close(),
    ) else {
        tracing::error!("could not install console handlers; falling back to ctrl_c");
        let _ = tokio::signal::ctrl_c().await;
        return "ctrl_c";
    };
    tokio::select! {
        _ = ctrl_c.recv() => "CTRL_C",
        _ = brk.recv() => "CTRL_BREAK",
        _ = close.recv() => "CTRL_CLOSE",
    }
}
