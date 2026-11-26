//! Internal identifiers.
//!
//! UUIDv7 is used for internally generated identifiers: it is time-ordered,
//! which keeps B-tree indexes compact and makes IDs roughly sortable by
//! creation time, while remaining globally unique.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generate a new time-ordered identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }
    };
}

uuid_id!(
    /// Identifier of a normalized SOLPRINT event.
    EventId
);
uuid_id!(
    /// Identifier tying together everything caused by one request or one
    /// upstream event (HTTP request, ingestion batch, decision chain).
    CorrelationId
);

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_ordered_and_parse() {
        let a = EventId::new();
        let b = EventId::new();
        assert_ne!(a, b);
        assert!(a <= b, "UUIDv7 should be monotonic within a process");
        assert_eq!(a.to_string().parse::<EventId>().unwrap(), a);
        assert!("nope".parse::<CorrelationId>().is_err());
    }
}
