#![no_std]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

//! Network service.
//!
//! The kernel does not parse packets; this crate owns the *service* view of the
//! network: is the link up, do we have a lease, when does it expire, do we need
//! to re-request. The packet path (smoltcp, `esp-radio`, MQTT) plugs in behind
//! `NetDevice` later; keeping the state machine separate means link flaps and
//! lease expiry are tested on the host, not discovered on a roof.
//!
//! Linux analogue: the `net` subsystem's device state machine plus the DHCP
//! client's lease timer, reduced to one interface. The device side is
//! `esp_sdk_hal::NetDevice`; the packet path is Phase 4.

#[cfg(test)]
extern crate std;

pub mod addr;
pub mod service;

pub use addr::{Ipv4Addr, ParseError};
pub use service::{NetworkService, State};
