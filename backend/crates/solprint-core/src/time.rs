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

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use time::format_description::BorrowedFormatItem;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// `2026-11-26T12:00:00.123Z`: fixed width, millisecond precision (truncated),
/// always UTC with a literal `Z`.
const RFC3339_MILLIS_Z: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

/// A UTC wall-clock instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UtcTimestamp(OffsetDateTime);

impl UtcTimestamp {
    #[must_use]
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }

    /// Wraps any instant, normalising its offset to UTC.
    #[must_use]
    pub fn from_offset_datetime(value: OffsetDateTime) -> Self {
        Self(value.to_offset(UtcOffset::UTC))
    }

    #[must_use]
    pub const fn as_offset_datetime(&self) -> &OffsetDateTime {
        &self.0
    }

    /// Parses any RFC 3339 timestamp (any offset) into UTC.
    ///
    /// # Errors
    /// Returns an error if `raw` is not valid RFC 3339.
    pub fn parse_rfc3339(raw: &str) -> Result<Self, time::error::Parse> {
        OffsetDateTime::parse(raw, &Rfc3339).map(Self::from_offset_datetime)
    }

    /// RFC 3339, millisecond precision, `Z` suffix.
    #[must_use]
    pub fn to_rfc3339(&self) -> String {
        // Only years outside 0000..=9999 can fail to format; those cannot
        // come from the system clock, PostgreSQL `timestamptz` or RFC 3339.
        self.0
            .format(RFC3339_MILLIS_Z)
            .unwrap_or_else(|_| String::from("invalid-timestamp"))
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
        Self::parse_rfc3339(&raw).map_err(serde::de::Error::custom)
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
        let ts = UtcTimestamp::parse_rfc3339("2026-11-26T12:00:00.123456+02:00").unwrap();
        assert_eq!(
            serde_json::to_string(&ts).unwrap(),
            "\"2026-11-26T10:00:00.123Z\""
        );
        let back: UtcTimestamp = serde_json::from_str("\"2026-11-26T10:00:00.123Z\"").unwrap();
        assert_eq!(back.to_rfc3339(), "2026-11-26T10:00:00.123Z");
    }

    #[test]
    fn keeps_fixed_width_and_utc() {
        let ts = UtcTimestamp::parse_rfc3339("2026-01-02T03:04:05Z").unwrap();
        assert_eq!(ts.to_rfc3339(), "2026-01-02T03:04:05.000Z");
        let late = UtcTimestamp::parse_rfc3339("2026-12-31T23:30:00.999999-01:00").unwrap();
        assert_eq!(late.to_rfc3339(), "2027-01-01T00:30:00.999Z");
        assert!(UtcTimestamp::parse_rfc3339("2026-11-26 12:00:00").is_err());
        assert!(serde_json::from_str::<UtcTimestamp>("\"not a time\"").is_err());
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
