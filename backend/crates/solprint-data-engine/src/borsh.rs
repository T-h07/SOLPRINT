//! A bounded little-endian (Borsh-layout) reader for instruction and event
//! data. Every read is checked: truncated data returns `None`, never panics
//! and never reads past the slice.

/// Strings inside decoded events are capped; anything longer is treated as
/// malformed rather than allocated.
pub const MAX_STRING_BYTES: usize = 256;

#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    #[must_use]
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    pub fn skip(&mut self, n: usize) -> Option<()> {
        self.take(n).map(|_| ())
    }

    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        let s = self.take(N)?;
        let mut out = [0_u8; N];
        out.copy_from_slice(s);
        Some(out)
    }

    pub fn u8(&mut self) -> Option<u8> {
        self.arr::<1>().map(|b| b[0])
    }

    pub fn bool(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    pub fn u16(&mut self) -> Option<u16> {
        self.arr().map(u16::from_le_bytes)
    }

    pub fn u32(&mut self) -> Option<u32> {
        self.arr().map(u32::from_le_bytes)
    }

    pub fn u64(&mut self) -> Option<u64> {
        self.arr().map(u64::from_le_bytes)
    }

    pub fn i64(&mut self) -> Option<i64> {
        self.arr().map(i64::from_le_bytes)
    }

    pub fn u128(&mut self) -> Option<u128> {
        self.arr().map(u128::from_le_bytes)
    }

    pub fn disc(&mut self) -> Option<[u8; 8]> {
        self.arr()
    }

    /// A 32-byte public key, returned in base58.
    pub fn pubkey(&mut self) -> Option<String> {
        let b = self.take(32)?;
        Some(bs58::encode(b).into_string())
    }

    /// Borsh string: u32 length then UTF-8 bytes.
    pub fn string(&mut self) -> Option<String> {
        let len = usize::try_from(self.u32()?).ok()?;
        if len > MAX_STRING_BYTES {
            return None;
        }
        let b = self.take(len)?;
        String::from_utf8(b.to_vec()).ok()
    }
}

/// Little-endian u64 at a fixed byte offset.
#[must_use]
pub fn u64_at(data: &[u8], offset: usize) -> Option<u64> {
    let mut r = Reader::new(data);
    r.skip(offset)?;
    r.u64()
}

/// Base58 pubkey at a fixed byte offset.
#[must_use]
pub fn pubkey_at(data: &[u8], offset: usize) -> Option<String> {
    let mut r = Reader::new(data);
    r.skip(offset)?;
    r.pubkey()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reads_little_endian_values() {
        let mut d = vec![1_u8];
        d.extend_from_slice(&7_u16.to_le_bytes());
        d.extend_from_slice(&u64::MAX.to_le_bytes());
        d.extend_from_slice(&3_u32.to_le_bytes());
        d.extend_from_slice(b"abc");
        let mut r = Reader::new(&d);
        assert_eq!(r.bool(), Some(true));
        assert_eq!(r.u16(), Some(7));
        assert_eq!(r.u64(), Some(u64::MAX));
        assert_eq!(r.string().as_deref(), Some("abc"));
        assert!(r.is_empty());
        assert_eq!(r.u8(), None);
    }

    #[test]
    fn truncated_and_invalid_data_is_rejected() {
        assert_eq!(Reader::new(&[1, 2, 3]).u64(), None);
        assert_eq!(Reader::new(&[2]).bool(), None);
        let mut long = 1_000_u32.to_le_bytes().to_vec();
        long.extend(std::iter::repeat_n(b'a', 1_000));
        assert_eq!(Reader::new(&long).string(), None);
        assert_eq!(u64_at(&[0; 10], 4), None);
    }

    proptest! {
        #[test]
        fn never_panics_on_arbitrary_input(data in proptest::collection::vec(any::<u8>(), 0..128), n in 0_usize..200) {
            let mut r = Reader::new(&data);
            let _ = r.skip(n % 64);
            let _ = r.string();
            let _ = r.pubkey();
            let _ = r.u128();
            let _ = u64_at(&data, n);
            prop_assert!(r.position() <= data.len());
        }
    }
}
