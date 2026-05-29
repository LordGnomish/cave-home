//! Multi-fragment payload reassembly.
//!
//! Clean-room (Charter §6.1 / ADR-002): modelled from the **public protocol
//! description** of how a Hoymiles inverter answers a request — as a sequence
//! of numbered radio fragments that the receiver concatenates in order. No GPL
//! `AhoyDTU` / `OpenDTU` source was read.
//!
//! Each fragment carries:
//! - a **sequence number** (1-based), and
//! - a **last-fragment marker** on the final fragment, and
//! - a per-fragment **CRC-8** ([`crate::crc::crc8`]) over its data bytes.
//!
//! The reassembler:
//! 1. validates each fragment's CRC-8 on insertion,
//! 2. concatenates fragment data in ascending sequence order regardless of
//!    arrival order, and
//! 3. once the last fragment has arrived, reports any **missing** sequence
//!    numbers and only yields the assembled payload when the run `1..=last`
//!    is complete.

use crate::crc::crc8;

/// One received radio fragment of an inverter response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fragment {
    /// 1-based position of this fragment in the response.
    pub seq: u8,
    /// `true` on the final fragment of the response.
    pub is_last: bool,
    /// The fragment's payload bytes (CRC-8 already stripped).
    pub data: Vec<u8>,
    /// The CRC-8 that accompanied this fragment on the wire.
    pub checksum: u8,
}

impl Fragment {
    /// Build a fragment, computing the CRC-8 that the wire would carry for
    /// `data`. Useful for constructing test fixtures and for the (deferred)
    /// transmit path.
    #[must_use]
    pub fn new(seq: u8, is_last: bool, data: Vec<u8>) -> Self {
        let checksum = crc8(&data);
        Self { seq, is_last, data, checksum }
    }

    /// Whether the carried [`checksum`](Self::checksum) matches the data.
    #[must_use]
    pub fn checksum_ok(&self) -> bool {
        crc8(&self.data) == self.checksum
    }
}

/// Why reassembly failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReassemblyError {
    /// A fragment's CRC-8 did not match its data — the fragment is corrupt.
    BadFragmentChecksum { seq: u8 },
    /// Two fragments claimed the same sequence number with different data.
    DuplicateSeq { seq: u8 },
    /// A sequence number of zero was supplied (sequences are 1-based).
    ZeroSeq,
    /// The last fragment never arrived, so the response is incomplete.
    NoLastFragment,
    /// One or more interior fragments are missing from the run `1..=last`.
    MissingFragments { missing: Vec<u8> },
}

impl core::fmt::Display for ReassemblyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadFragmentChecksum { seq } => {
                write!(f, "fragment {seq} failed its checksum")
            }
            Self::DuplicateSeq { seq } => write!(f, "fragment {seq} arrived twice"),
            Self::ZeroSeq => f.write_str("fragment sequence numbers start at 1"),
            Self::NoLastFragment => f.write_str("never saw the final fragment"),
            Self::MissingFragments { missing } => {
                write!(f, "missing fragments: {missing:?}")
            }
        }
    }
}

impl std::error::Error for ReassemblyError {}

/// Accumulates fragments of a single inverter response and reassembles them.
#[derive(Debug, Default)]
pub struct Reassembler {
    // Indexed by seq; `None` until that fragment arrives. Index 0 is unused so
    // that `slots[seq]` reads naturally for the 1-based sequence numbers.
    slots: Vec<Option<Vec<u8>>>,
    last_seq: Option<u8>,
}

impl Reassembler {
    /// A fresh, empty reassembler.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Accept one fragment, validating its checksum and recording it by
    /// sequence number. Idempotent for an identical re-delivery of the same
    /// fragment (radios retransmit).
    ///
    /// # Errors
    /// Returns [`ReassemblyError::ZeroSeq`] for a 0 sequence number,
    /// [`ReassemblyError::BadFragmentChecksum`] when the CRC-8 mismatches, and
    /// [`ReassemblyError::DuplicateSeq`] when a sequence number is re-used with
    /// conflicting data.
    pub fn accept(&mut self, fragment: &Fragment) -> Result<(), ReassemblyError> {
        if fragment.seq == 0 {
            return Err(ReassemblyError::ZeroSeq);
        }
        if !fragment.checksum_ok() {
            return Err(ReassemblyError::BadFragmentChecksum { seq: fragment.seq });
        }
        let idx = fragment.seq as usize;
        if idx >= self.slots.len() {
            self.slots.resize(idx + 1, None);
        }
        match &self.slots[idx] {
            Some(existing) if *existing != fragment.data => {
                return Err(ReassemblyError::DuplicateSeq { seq: fragment.seq });
            }
            _ => {}
        }
        self.slots[idx] = Some(fragment.data.clone());
        if fragment.is_last {
            self.last_seq = Some(fragment.seq);
        }
        Ok(())
    }

    /// Sequence numbers in `1..=last` that have not yet arrived. Empty when the
    /// last fragment is known and every interior fragment is present.
    #[must_use]
    pub fn missing(&self) -> Vec<u8> {
        let Some(last) = self.last_seq else {
            return Vec::new();
        };
        (1..=last)
            .filter(|&seq| {
                self.slots
                    .get(seq as usize)
                    .is_none_or(Option::is_none)
            })
            .collect()
    }

    /// Whether every fragment of the response has arrived.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.last_seq.is_some() && self.missing().is_empty()
    }

    /// Concatenate the fragments into the assembled payload.
    ///
    /// # Errors
    /// Returns [`ReassemblyError::NoLastFragment`] if the final fragment has
    /// not arrived, or [`ReassemblyError::MissingFragments`] listing any gaps.
    pub fn assemble(&self) -> Result<Vec<u8>, ReassemblyError> {
        let Some(last) = self.last_seq else {
            return Err(ReassemblyError::NoLastFragment);
        };
        let missing = self.missing();
        if !missing.is_empty() {
            return Err(ReassemblyError::MissingFragments { missing });
        }
        let mut out = Vec::new();
        for seq in 1..=last {
            if let Some(Some(data)) = self.slots.get(seq as usize) {
                out.extend_from_slice(data);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_in_order_fragments() {
        let mut r = Reassembler::new();
        r.accept(&Fragment::new(1, false, vec![0xAA, 0xBB])).unwrap();
        r.accept(&Fragment::new(2, true, vec![0xCC])).unwrap();
        assert!(r.is_complete());
        assert_eq!(r.assemble().unwrap(), vec![0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn reorders_out_of_order_fragments() {
        let mut r = Reassembler::new();
        // Last fragment arrives first, middle fragment second.
        r.accept(&Fragment::new(3, true, vec![0x33])).unwrap();
        r.accept(&Fragment::new(1, false, vec![0x11])).unwrap();
        r.accept(&Fragment::new(2, false, vec![0x22])).unwrap();
        assert!(r.is_complete());
        assert_eq!(r.assemble().unwrap(), vec![0x11, 0x22, 0x33]);
    }

    #[test]
    fn detects_missing_interior_fragment() {
        let mut r = Reassembler::new();
        r.accept(&Fragment::new(1, false, vec![0x11])).unwrap();
        r.accept(&Fragment::new(3, true, vec![0x33])).unwrap();
        assert!(!r.is_complete());
        assert_eq!(r.missing(), vec![2]);
        assert_eq!(
            r.assemble(),
            Err(ReassemblyError::MissingFragments { missing: vec![2] })
        );
    }

    #[test]
    fn missing_last_fragment_is_incomplete() {
        let mut r = Reassembler::new();
        r.accept(&Fragment::new(1, false, vec![0x11])).unwrap();
        r.accept(&Fragment::new(2, false, vec![0x22])).unwrap();
        assert!(!r.is_complete());
        assert_eq!(r.assemble(), Err(ReassemblyError::NoLastFragment));
    }

    #[test]
    fn rejects_corrupt_fragment_checksum() {
        let mut r = Reassembler::new();
        let mut frag = Fragment::new(1, true, vec![0x11, 0x22]);
        frag.checksum ^= 0xFF; // corrupt the carried checksum
        assert_eq!(
            r.accept(&frag),
            Err(ReassemblyError::BadFragmentChecksum { seq: 1 })
        );
    }

    #[test]
    fn rejects_zero_sequence() {
        let mut r = Reassembler::new();
        assert_eq!(
            r.accept(&Fragment::new(0, true, vec![0x00])),
            Err(ReassemblyError::ZeroSeq)
        );
    }

    #[test]
    fn idempotent_retransmit_is_accepted() {
        let mut r = Reassembler::new();
        let f = Fragment::new(1, true, vec![0x11]);
        r.accept(&f).unwrap();
        r.accept(&f).unwrap(); // radio retransmit of the same fragment
        assert_eq!(r.assemble().unwrap(), vec![0x11]);
    }

    #[test]
    fn conflicting_duplicate_is_rejected() {
        let mut r = Reassembler::new();
        r.accept(&Fragment::new(1, false, vec![0x11])).unwrap();
        assert_eq!(
            r.accept(&Fragment::new(1, true, vec![0x99])),
            Err(ReassemblyError::DuplicateSeq { seq: 1 })
        );
    }

    #[test]
    fn lists_multiple_missing_fragments() {
        let mut r = Reassembler::new();
        r.accept(&Fragment::new(2, false, vec![0x22])).unwrap();
        r.accept(&Fragment::new(5, true, vec![0x55])).unwrap();
        assert_eq!(r.missing(), vec![1, 3, 4]);
    }
}
