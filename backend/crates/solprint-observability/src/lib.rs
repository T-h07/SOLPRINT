//! Structured logging for SOLPRINT.
//!
//! * `pretty` format for local development, `json` (one object per line) for
//!   containers and log shipping. Both use UTC RFC 3339 timestamps.
//! * The level from `LOG_LEVEL` applies to SOLPRINT crates; noisy dependencies
//!   are capped so INFO stays readable.
//! * Redaction: secrets are wrapped in `solprint_config::Secret` (redacting
//!   `Debug`, no `Display`). HTTP request logging records method, matched
//!   route, status and latency only: never headers, query strings or bodies.

use solprint_config::{LogConfig, LogFormat};
use time::macros::format_description;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::UtcTime;

/// Header carrying the request correlation identifier.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    #[error("invalid log filter directive")]
    Filter,
    #[error("a global tracing subscriber is already installed")]
    AlreadyInitialized,
}

/// Build the filter directive: SOLPRINT crates at the configured level,
/// dependencies capped at `warn` (HTTP access logs from tower-http stay at
/// the configured level because they carry request outcomes).
#[must_use]
pub fn filter_directive(config: &LogConfig) -> String {
    let level = config.level.as_str();
    format!(
        "warn,solprint={level},solprint_app={level},solprint_api={level},solprint_storage={level},\
         solprint_config={level},solprint_observability={level},solprint_core={level},\
         tower_http={level},sqlx=warn"
    )
}

/// Install the global subscriber. Call once, from the composition root.
pub fn init(config: &LogConfig) -> Result<(), ObservabilityError> {
    let filter =
        EnvFilter::try_new(filter_directive(config)).map_err(|_| ObservabilityError::Filter)?;
    let timer = UtcTime::new(format_description!(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
    ));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_timer(timer)
        .with_target(true);
    let result = match config.format {
        LogFormat::Pretty => builder.compact().try_init(),
        LogFormat::Json => builder
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .try_init(),
    };
    result.map_err(|_| ObservabilityError::AlreadyInitialized)
}

#[cfg(test)]
mod tests {
    use solprint_config::LogLevel;

    use super::*;

    #[test]
    fn directive_is_valid_and_caps_dependencies() {
        let cfg = LogConfig {
            level: LogLevel::Debug,
            format: LogFormat::Json,
        };
        let d = filter_directive(&cfg);
        assert!(EnvFilter::try_new(&d).is_ok());
        assert!(d.starts_with("warn,"));
        assert!(d.contains("solprint_api=debug"));
        // The executable's module path is `solprint` (bin name), not the package name.
        assert!(d.contains(",solprint=debug,"));
        assert!(d.contains("sqlx=warn"));
    }
}
