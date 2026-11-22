#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! The app-side boot contract (ADR-009).
//!
//! The 2nd-stage bootloader stays ESP-IDF's; what lives here is everything the
//! application must understand about it:
//!
//! - [`partition`] — the partition table, validated (alignment, overlap, bounds).
//! - [`otadata`] — the OTA selection structure: CRC, validity and slot choice,
//!   bit-compatible with ESP-IDF's `esp_ota_select_entry_t`.
//! - [`select`] — the decision of what to boot, combining the two.
//! - [`appdesc`] — the `esp_app_desc_t` at offset 0x20 of an application image.
//! - [`crc32`] — the CRC the otadata format uses.
//!
//! Everything is pure logic over byte slices, so it is unit-tested on the host
//! against synthetic partitions/uploads and against ESP-IDF's own rules.

#[cfg(test)]
extern crate std;

pub mod appdesc;
pub mod crc32;
pub mod otadata;
pub mod partition;
pub mod select;
