//! Network device types and trait.

use crate::Result;

/// A 48-bit MAC address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct MacAddr([u8; 6]);

impl MacAddr {
    /// Construct from bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// The raw bytes.
    #[must_use]
    pub const fn as_bytes(self) -> [u8; 6] {
        self.0
    }

    /// Whether the group bit is set.
    #[must_use]
    pub const fn is_multicast(self) -> bool {
        self.0[0] & 1 == 1
    }
}

/// Link state of a network device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    /// No carrier.
    Down,
    /// Carrier present.
    Up,
}

/// Packet and error counters for a network device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetStats {
    /// Frames received.
    pub rx_packets: u32,
    /// Frames transmitted.
    pub tx_packets: u32,
    /// Bytes received.
    pub rx_bytes: u32,
    /// Bytes transmitted.
    pub tx_bytes: u32,
    /// Frames dropped on receive.
    pub rx_dropped: u32,
    /// Transmit errors.
    pub tx_errors: u32,
}

/// A network device (netdev).
///
/// The packet path (smoltcp, DHCP, MQTT) uses this trait; the network *service*
/// state machine lives in `esp-sdk-net` and is tested independently.
pub trait NetDevice {
    /// Interface name, e.g. `wlan0`.
    fn name(&self) -> &'static str;

    /// Hardware address.
    fn mac(&self) -> MacAddr;

    /// Maximum transmission unit.
    fn mtu(&self) -> usize;

    /// Bring the interface up.
    async fn up(&mut self) -> Result<()>;

    /// Bring the interface down.
    async fn down(&mut self);

    /// Current link state.
    fn link(&self) -> LinkState;

    /// Counters snapshot.
    fn stats(&self) -> NetStats;

    /// Transmit one Ethernet frame.
    async fn transmit(&mut self, frame: &[u8]) -> Result<()>;

    /// Receive one frame, returning its length.
    async fn receive(&mut self, buf: &mut [u8]) -> Result<usize>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_multicast_bit() {
        assert!(MacAddr::new([0x01, 0, 0, 0, 0, 0]).is_multicast());
        assert!(!MacAddr::new([0x02, 0, 0, 0, 0, 0]).is_multicast());
        assert_eq!(
            MacAddr::new([1, 2, 3, 4, 5, 6]).as_bytes(),
            [1, 2, 3, 4, 5, 6]
        );
    }
}
