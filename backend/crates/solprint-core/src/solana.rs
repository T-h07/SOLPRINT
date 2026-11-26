//! Solana identifiers.
//!
//! Addresses and signatures are validated by decoding their base58 form and
//! checking the exact byte length, so a malformed string can never become a
//! typed identifier. The canonical base58 string is kept for display and
//! serialization.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("not valid base58")]
    InvalidBase58,
    #[error("decoded length {actual} bytes, expected {expected}")]
    InvalidLength { expected: usize, actual: usize },
}

fn validate_base58(raw: &str, expected: usize) -> Result<(), AddressError> {
    let mut buf = [0_u8; 64];
    let actual = bs58::decode(raw).onto(&mut buf[..]).map_err(|e| match e {
        bs58::decode::Error::BufferTooSmall => AddressError::InvalidLength {
            expected,
            actual: usize::MAX,
        },
        _ => AddressError::InvalidBase58,
    })?;
    if actual == expected {
        Ok(())
    } else {
        Err(AddressError::InvalidLength { expected, actual })
    }
}

macro_rules! base58_id {
    ($(#[$meta:meta])* $name:ident, $len:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Decoded byte length.
            pub const BYTES: usize = $len;

            pub fn parse(raw: &str) -> Result<Self, AddressError> {
                validate_base58(raw, Self::BYTES)?;
                Ok(Self(raw.to_owned()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl FromStr for $name {
            type Err = AddressError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse(s)
            }
        }

        impl TryFrom<String> for $name {
            type Error = AddressError;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                validate_base58(&value, Self::BYTES)?;
                Ok(Self(value))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

base58_id!(
    /// A wallet (system account) public key.
    WalletAddress,
    32
);
base58_id!(
    /// An SPL token mint public key.
    TokenMint,
    32
);
base58_id!(
    /// A transaction signature (64-byte ed25519 signature).
    TransactionSignature,
    64
);

/// Solana slot number. Serialized as a JSON number: slots are far below
/// 2^53 for the foreseeable lifetime of the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Slot(pub u64);

/// Commitment level a piece of chain data was observed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommitmentLevel {
    Processed,
    Confirmed,
    Finalized,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    // Well-known public program and mint addresses (not secrets).
    const SYSTEM_PROGRAM: &str = "11111111111111111111111111111111";
    const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    #[test]
    fn valid_addresses_parse() {
        assert_eq!(
            WalletAddress::parse(SYSTEM_PROGRAM).unwrap().as_str(),
            SYSTEM_PROGRAM
        );
        assert!(TokenMint::parse(USDC_MINT).is_ok());
    }

    #[test]
    fn invalid_addresses_are_rejected() {
        assert_eq!(
            WalletAddress::parse("0OIl"),
            Err(AddressError::InvalidBase58)
        );
        assert!(matches!(
            WalletAddress::parse("1111"),
            Err(AddressError::InvalidLength {
                expected: 32,
                actual: 4
            })
        ));
        // A 32-byte key is not a 64-byte signature.
        assert!(TransactionSignature::parse(USDC_MINT).is_err());
        assert!(serde_json::from_str::<TokenMint>("\"not-a-mint\"").is_err());
    }

    #[test]
    fn signature_round_trips() {
        let sig = bs58::encode([7_u8; 64]).into_string();
        let parsed = TransactionSignature::parse(&sig).unwrap();
        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(
            serde_json::from_str::<TransactionSignature>(&json).unwrap(),
            parsed
        );
    }

    #[test]
    fn commitment_serializes_screaming_snake() {
        assert_eq!(
            serde_json::to_string(&CommitmentLevel::Finalized).unwrap(),
            "\"FINALIZED\""
        );
        assert_eq!(serde_json::to_string(&Slot(123)).unwrap(), "123");
    }
}
