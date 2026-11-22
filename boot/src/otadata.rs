//! OTA selection data, bit-compatible with ESP-IDF's `esp_ota_select_entry_t`.
//!
//! Layout (32 bytes, little endian):
//!
//! | offset | field                  |
//! |--------|------------------------|
//! | 0      | `ota_seq` (u32)        |
//! | 4      | `seq_label[20]`        |
//! | 24     | `ota_state` (u32)      |
//! | 28     | `crc` (u32)            |
//!
//! `crc` covers the four `ota_seq` bytes and uses the ROM convention in
//! [`crate::crc32`]. An entry is valid when the CRC matches, the sequence is not
//! the erased value, and the state is neither `INVALID` nor `ABORTED` — exactly
//! the rule `bootloader_common_ota_select_valid()` applies.

use crate::crc32::ota_seq_crc;

/// Size of one selection entry.
pub const ENTRY_SIZE: usize = 32;

/// Size of one otadata sector.
pub const SECTOR_SIZE: usize = 0x1000;

/// `ota_seq` value in an erased sector.
pub const ERASED_SEQ: u32 = u32::MAX;

/// Image state stored in an otadata entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum State {
    /// Newly written image; the bootloader turns this into `PendingVerify`.
    New = 0,
    /// First boot of a new image: the app must confirm itself or be rolled back.
    PendingVerify = 1,
    /// Confirmed working.
    Valid = 2,
    /// Confirmed non-working; never selected again.
    Invalid = 3,
    /// The image never confirmed itself; never selected again.
    Aborted = 4,
    /// Undefined (also what an erased entry reads as).
    Undefined = 0xFFFF_FFFF,
}

impl State {
    /// Map the raw `u32` to a state; unknown values become [`State::Undefined`].
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::New,
            1 => Self::PendingVerify,
            2 => Self::Valid,
            3 => Self::Invalid,
            4 => Self::Aborted,
            _ => Self::Undefined,
        }
    }

    /// Raw value as stored.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self as u32
    }
}

/// One parsed selection entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// OTA sequence number; the active slot is `seq - 1`.
    pub seq: u32,
    /// Image state.
    pub state: State,
    /// Whether the stored CRC matched.
    pub crc_ok: bool,
}

impl Entry {
    /// Parse the first [`ENTRY_SIZE`] bytes of `bytes`.
    ///
    /// Returns `None` when `bytes` is too short; a bad CRC is reported through
    /// [`Entry::crc_ok`] rather than rejected here, so callers can log why a
    /// slot was skipped.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < ENTRY_SIZE {
            return None;
        }
        let seq = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let state = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let crc = u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        Some(Self {
            seq,
            state: State::from_raw(state),
            crc_ok: crc == ota_seq_crc(seq),
        })
    }

    /// Whether the bootloader would consider this entry selectable.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.crc_ok
            && self.seq != ERASED_SEQ
            && !matches!(self.state, State::Invalid | State::Aborted)
    }

    /// Active slot index (`seq - 1`) if the entry has a usable sequence.
    #[must_use]
    pub fn slot(&self) -> Option<u32> {
        self.seq.checked_sub(1)
    }
}

/// Choose the active entry, following ESP-IDF's rule: among valid entries, the
/// higher sequence wins; on a tie, index 0 wins.
///
/// Returns `None` when neither entry is selectable.
#[must_use]
pub fn select(entries: &[Entry; 2]) -> Option<usize> {
    match (entries[0].is_valid(), entries[1].is_valid()) {
        (true, true) => Some(if entries[0].seq >= entries[1].seq {
            0
        } else {
            1
        }),
        (true, false) => Some(0),
        (false, true) => Some(1),
        (false, false) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(seq: u32, state: State) -> [u8; ENTRY_SIZE] {
        let mut bytes = [0u8; ENTRY_SIZE];
        bytes[0..4].copy_from_slice(&seq.to_le_bytes());
        bytes[24..28].copy_from_slice(&state.raw().to_le_bytes());
        bytes[28..32].copy_from_slice(&ota_seq_crc(seq).to_le_bytes());
        bytes
    }

    fn valid(seq: u32) -> Entry {
        Entry {
            seq,
            state: State::Valid,
            crc_ok: true,
        }
    }

    #[test]
    fn parses_a_valid_entry() {
        let entry = Entry::parse(&encoded(7, State::Valid)).unwrap();
        assert_eq!(entry.seq, 7);
        assert_eq!(entry.state, State::Valid);
        assert!(entry.crc_ok);
        assert!(entry.is_valid());
        assert_eq!(entry.slot(), Some(6));
    }

    #[test]
    fn detects_a_corrupt_crc() {
        let mut bytes = encoded(7, State::Valid);
        bytes[31] ^= 0xFF;
        let entry = Entry::parse(&bytes).unwrap();
        assert!(!entry.crc_ok);
        assert!(!entry.is_valid());
    }

    #[test]
    fn rejects_short_input() {
        assert!(Entry::parse(&[0u8; ENTRY_SIZE - 1]).is_none());
    }

    #[test]
    fn erased_sector_is_invalid() {
        let entry = Entry::parse(&[0xFF; ENTRY_SIZE]).unwrap();
        assert_eq!(entry.seq, ERASED_SEQ);
        assert_eq!(entry.state, State::Undefined);
        assert!(!entry.is_valid());
    }

    #[test]
    fn invalid_and_aborted_states_are_not_selectable() {
        assert!(
            !Entry::parse(&encoded(1, State::Invalid))
                .unwrap()
                .is_valid()
        );
        assert!(
            !Entry::parse(&encoded(1, State::Aborted))
                .unwrap()
                .is_valid()
        );
        assert!(Entry::parse(&encoded(1, State::New)).unwrap().is_valid());
        assert!(
            Entry::parse(&encoded(1, State::PendingVerify))
                .unwrap()
                .is_valid()
        );
    }

    #[test]
    fn higher_sequence_wins() {
        assert_eq!(select(&[valid(1), valid(2)]), Some(1));
        assert_eq!(select(&[valid(2), valid(1)]), Some(0));
    }

    #[test]
    fn ties_prefer_index_zero() {
        assert_eq!(select(&[valid(3), valid(3)]), Some(0));
    }

    #[test]
    fn one_valid_entry_is_chosen() {
        let broken = Entry {
            seq: 9,
            state: State::Valid,
            crc_ok: false,
        };
        assert_eq!(select(&[broken, valid(2)]), Some(1));
        assert_eq!(select(&[valid(2), broken]), Some(0));
    }

    #[test]
    fn no_valid_entry_selects_nothing() {
        let broken = Entry {
            seq: 1,
            state: State::Valid,
            crc_ok: false,
        };
        assert_eq!(select(&[broken, broken]), None);
    }

    #[test]
    fn slot_underflow_is_none() {
        let entry = Entry {
            seq: 0,
            state: State::Valid,
            crc_ok: true,
        };
        assert_eq!(entry.slot(), None);
    }
}
