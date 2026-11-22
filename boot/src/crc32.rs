//! CRC-32 as ESP-IDF's otadata format computes it.
//!
//! ESP-IDF calls `esp_rom_crc32_le(UINT32_MAX, &ota_seq, 4)`, and the ROM
//! routine applies `~` before and after the reflected CRC update. The result is
//! therefore *not* the zlib CRC of the bytes taken in isolation; it is the ROM
//! convention, and matching it is what makes our entries readable by the IDF
//! bootloader (and vice versa).

/// CRC-32 in the ROM convention: `crc32_le(crc, data) = ~update(~crc, data)`.
///
/// `crc32_le(0, ..)` equals the standard zlib CRC-32.
#[must_use]
pub fn crc32_le(crc: u32, data: &[u8]) -> u32 {
    let mut state = !crc;
    for &byte in data {
        state ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (state & 1).wrapping_neg();
            state = (state >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !state
}

/// The CRC stored in an otadata entry: CRC over the 4 little-endian bytes of
/// `ota_seq`, using the ROM convention ESP-IDF uses.
#[must_use]
pub fn ota_seq_crc(seq: u32) -> u32 {
    crc32_le(u32::MAX, &seq.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zlib_convention_matches_the_standard_vector() {
        // The classic check value for CRC-32/ISO-HDLC.
        assert_eq!(crc32_le(0, b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn rom_convention_matches_esp_idf() {
        // binascii.crc32(struct.pack('I', seq), 0xFFFFFFFF), i.e. what
        // ESP-IDF and otatool.py compute.
        assert_eq!(ota_seq_crc(0), 0xFFFF_FFFF);
        assert_eq!(ota_seq_crc(1), 0x4743_989A);
        assert_eq!(ota_seq_crc(2), 0x55F6_3774);
        assert_eq!(ota_seq_crc(3), 0xED4A_5011);
        assert_eq!(ota_seq_crc(16), 0xAFE6_A860);
    }

    #[test]
    fn crc_is_sensitive_to_byte_order() {
        assert_ne!(
            crc32_le(0, &0x0102_0304u32.to_le_bytes()),
            crc32_le(0, &0x0102_0304u32.to_be_bytes())
        );
    }
}
