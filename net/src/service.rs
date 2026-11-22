//! Link and DHCP-lease state machine.
//!
//! The service is deliberately synchronous: the caller feeds it events (link
//! up/down, lease acquired/lost/failed) and a monotonic clock, and asks
//! `next_deadline` when to be called again. That makes it trivially testable and
//! keeps the timer/executor coupling in one place.

use crate::addr::Ipv4Addr;

/// Default delay before retrying a failed DHCP exchange.
pub const DEFAULT_RETRY_US: u64 = 5_000_000;

/// Where the service is in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// No link.
    Down,
    /// Link is up, waiting for a DHCP lease.
    RequestingLease,
    /// Have an address until `expires_us`.
    Ready {
        /// Assigned address.
        address: Ipv4Addr,
        /// Default gateway.
        gateway: Ipv4Addr,
        /// Lease expiry (monotonic microseconds).
        expires_us: u64,
    },
    /// DHCP failed; retry after `retry_at_us`.
    Degraded {
        /// When to retry.
        retry_at_us: u64,
    },
}

impl State {
    /// Whether a link is present (even without a lease).
    #[must_use]
    pub const fn is_up(self) -> bool {
        !matches!(self, Self::Down)
    }

    /// Whether the node has a usable address.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// Network service: link, lease and retry state for one interface.
#[derive(Clone, Debug)]
pub struct NetworkService {
    state: State,
    retry_us: u64,
    transitions: u32,
}

impl Default for NetworkService {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkService {
    /// Create a service with no link.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: State::Down,
            retry_us: DEFAULT_RETRY_US,
            transitions: 0,
        }
    }

    /// Set the DHCP retry delay.
    #[must_use]
    pub const fn with_retry(mut self, retry_us: u64) -> Self {
        self.retry_us = retry_us;
        self
    }

    /// Current state.
    #[must_use]
    pub const fn state(&self) -> State {
        self.state
    }

    /// Whether the link is up.
    #[must_use]
    pub const fn is_up(&self) -> bool {
        self.state.is_up()
    }

    /// Whether an address is assigned.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.state.is_ready()
    }

    /// Assigned address, if ready.
    #[must_use]
    pub const fn address(&self) -> Option<Ipv4Addr> {
        match self.state {
            State::Ready { address, .. } => Some(address),
            _ => None,
        }
    }

    /// Link came up.
    pub fn link_up(&mut self) -> bool {
        match self.state {
            State::Down | State::Degraded { .. } => {
                self.set(State::RequestingLease);
                true
            }
            State::RequestingLease | State::Ready { .. } => false,
        }
    }

    /// Link went down.
    pub fn link_down(&mut self) -> bool {
        if self.state == State::Down {
            return false;
        }
        self.set(State::Down);
        true
    }

    /// A lease was acquired or renewed.
    pub fn lease_acquired(
        &mut self,
        now_us: u64,
        address: Ipv4Addr,
        gateway: Ipv4Addr,
        lease_us: u64,
    ) -> bool {
        if !matches!(self.state, State::RequestingLease | State::Ready { .. }) {
            return false;
        }
        self.set(State::Ready {
            address,
            gateway,
            expires_us: now_us.saturating_add(lease_us),
        });
        true
    }

    /// The DHCP exchange failed or the lease was refused.
    pub fn lease_failed(&mut self, now_us: u64) -> bool {
        if matches!(self.state, State::Down) {
            return false;
        }
        self.set(State::Degraded {
            retry_at_us: now_us.saturating_add(self.retry_us),
        });
        true
    }

    /// Advance the timer: lease expiry or retry deadline.
    ///
    /// Returns whether the state changed.
    pub fn tick(&mut self, now_us: u64) -> bool {
        match self.state {
            State::Ready { expires_us, .. } if now_us >= expires_us => {
                self.set(State::RequestingLease);
                true
            }
            State::Degraded { retry_at_us } if now_us >= retry_at_us => {
                self.set(State::RequestingLease);
                true
            }
            _ => false,
        }
    }

    /// Next moment the service needs a `tick`.
    #[must_use]
    pub const fn next_deadline(&self) -> Option<u64> {
        match self.state {
            State::Ready { expires_us, .. } => Some(expires_us),
            State::Degraded { retry_at_us } => Some(retry_at_us),
            State::Down | State::RequestingLease => None,
        }
    }

    /// Number of state changes since creation.
    #[must_use]
    pub const fn transitions(&self) -> u32 {
        self.transitions
    }

    fn set(&mut self, state: State) {
        self.state = state;
        self.transitions = self.transitions.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr::new(a, b, c, d)
    }

    #[test]
    fn full_bringup_sequence() {
        let mut service = NetworkService::new();
        assert_eq!(service.state(), State::Down);
        assert!(service.link_up());
        assert_eq!(service.state(), State::RequestingLease);
        assert!(!service.is_ready());

        assert!(service.lease_acquired(
            1_000,
            addr(192, 168, 1, 42),
            addr(192, 168, 1, 1),
            3_600_000_000
        ));
        assert!(service.is_ready());
        assert_eq!(service.address(), Some(addr(192, 168, 1, 42)));
        assert_eq!(service.next_deadline(), Some(1_000 + 3_600_000_000));
    }

    #[test]
    fn lease_expiry_returns_to_lease_request() {
        let mut service = NetworkService::new();
        service.link_up();
        service.lease_acquired(0, addr(10, 0, 0, 2), addr(10, 0, 0, 1), 1_000);

        assert!(!service.tick(999));
        assert!(service.tick(1_000));
        assert_eq!(service.state(), State::RequestingLease);
        assert!(!service.is_ready());
    }

    #[test]
    fn renewal_extends_the_lease() {
        let mut service = NetworkService::new();
        service.link_up();
        service.lease_acquired(0, addr(10, 0, 0, 2), addr(10, 0, 0, 1), 1_000);
        service.lease_acquired(500, addr(10, 0, 0, 2), addr(10, 0, 0, 1), 1_000);
        assert!(!service.tick(1_200));
        assert!(service.is_ready());
        assert_eq!(service.next_deadline(), Some(1_500));
    }

    #[test]
    fn failed_lease_degrades_then_retries() {
        let mut service = NetworkService::new().with_retry(5_000);
        service.link_up();
        assert!(service.lease_failed(1_000));
        assert_eq!(service.state(), State::Degraded { retry_at_us: 6_000 });
        assert!(!service.tick(5_999));
        assert!(service.tick(6_000));
        assert_eq!(service.state(), State::RequestingLease);
    }

    #[test]
    fn link_down_resets_everything() {
        let mut service = NetworkService::new();
        service.link_up();
        service.lease_acquired(0, addr(10, 0, 0, 2), addr(10, 0, 0, 1), 10_000);
        assert!(service.link_down());
        assert_eq!(service.state(), State::Down);
        assert_eq!(service.next_deadline(), None);
        assert!(!service.link_down());
    }

    #[test]
    fn lease_is_refused_when_the_link_is_down() {
        let mut service = NetworkService::new();
        assert!(!service.lease_acquired(0, addr(1, 2, 3, 4), addr(1, 2, 3, 1), 100));
        assert_eq!(service.state(), State::Down);
    }

    #[test]
    fn repeated_link_up_is_a_no_op() {
        let mut service = NetworkService::new();
        assert!(service.link_up());
        assert!(!service.link_up());
        assert_eq!(service.transitions(), 1);
    }
}
