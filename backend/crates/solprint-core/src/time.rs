//! Time conventions.
//!
//! * All wall-clock timestamps are UTC ([`UtcTimestamp`]) and serialize as
//!   RFC 3339 with a `Z` suffix and millisecond precision.
//! * Elapsed durations and latencies are measured with the monotonic clock
//!   (`std::time::Instant` / `tokio::time::Instant`), never by subtracting
//!   wall-clock timestamps.
//! * A piece of data can carry several distinct times; [`EventTimestamps`]
//!   keeps them apart so "when did it happen" is never confused with "when
//!   did we see it".

use std::fmt;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A UTC wall-clock instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp(DateTime<Utc>);

impl UtcTimestamp {
    #[must_use]
    pub fn now() -> Self {
        Self(Utc::now())
    }

    #[must_use]
    pub const fn from_datetime(value: DateTime<Utc>) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn as_datetime(&self) -> &DateTime<Utc> {
        &self.0
    }

    /// RFC 3339, millisecond precision, `Z` suffix.
    #[must_use]
    pub fn to_rfc3339(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

impl fmt::Display for UtcTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

impl Serialize for UtcTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_rfc3339())
    }
}

impl<'de> Deserialize<'de> for UtcTimestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&raw)
            .map(|dt| Self(dt.with_timezone(&Utc)))
            .map_err(serde::de::Error::custom)
    }
}

/// The distinct times attached to an observed piece of data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTimestamps {
    /// When the event happened at its source (e.g. block time). `None` when
    /// the source does not provide it; never back-filled with observation time.
    pub source_at: Option<UtcTimestamp>,
    /// When SOLPRINT received the data.
    pub observed_at: UtcTimestamp,
    /// When SOLPRINT finished processing it, if it has.
    pub processed_at: Option<UtcTimestamp>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_utc_rfc3339_millis() {
        let ts = UtcTimestamp::from_datetime(
            DateTime::parse_from_rfc3339("2026-11-26T12:00:00.123456+02:00")
                .unwrap()
                .with_timezone(&Utc),
        );
        assert_eq!(
            serde_json::to_string(&ts).unwrap(),
            "\"2026-11-26T10:00:00.123Z\""
        );
        let back: UtcTimestamp = serde_json::from_str("\"2026-11-26T10:00:00.123Z\"").unwrap();
        assert_eq!(back.to_rfc3339(), "2026-11-26T10:00:00.123Z");
    }

    #[test]
    fn missing_source_time_stays_absent() {
        let t = EventTimestamps {
            source_at: None,
            observed_at: UtcTimestamp::now(),
            processed_at: None,
        };
        let json = serde_json::to_value(t).unwrap();
        assert!(json["source_at"].is_null());
    }
}
