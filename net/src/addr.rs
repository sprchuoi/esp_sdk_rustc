//! IPv4 address type with parsing and formatting.

use core::fmt;

/// An IPv4 address.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ipv4Addr([u8; 4]);

impl Ipv4Addr {
    /// `0.0.0.0`.
    pub const UNSPECIFIED: Self = Self([0, 0, 0, 0]);

    /// Construct from octets.
    #[must_use]
    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Self([a, b, c, d])
    }

    /// The four octets.
    #[must_use]
    pub const fn octets(self) -> [u8; 4] {
        self.0
    }

    /// Whether this is `0.0.0.0`.
    #[must_use]
    pub const fn is_unspecified(self) -> bool {
        self.0[0] == 0 && self.0[1] == 0 && self.0[2] == 0 && self.0[3] == 0
    }

    /// Whether this is a private (RFC 1918) address.
    #[must_use]
    pub const fn is_private(self) -> bool {
        self.0[0] == 10
            || (self.0[0] == 172 && self.0[1] >= 16 && self.0[1] < 32)
            || (self.0[0] == 192 && self.0[1] == 168)
    }

    /// Parse dotted-quad notation.
    ///
    /// # Errors
    /// [`ParseError`] for wrong octet count or an octet outside `0..=255`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let mut octets = [0u8; 4];
        let mut count = 0;
        for part in text.split('.') {
            if count == 4 {
                return Err(ParseError::TooManyOctets);
            }
            if part.is_empty() {
                return Err(ParseError::EmptyOctet);
            }
            octets[count] = part.parse::<u8>().map_err(|_| ParseError::InvalidOctet)?;
            count += 1;
        }
        if count != 4 {
            return Err(ParseError::TooFewOctets);
        }
        Ok(Self(octets))
    }
}

impl fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}.{}", self.0[0], self.0[1], self.0[2], self.0[3])
    }
}

impl fmt::Debug for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ipv4Addr({self})")
    }
}

/// Why an address failed to parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Fewer than four octets.
    TooFewOctets,
    /// More than four octets.
    TooManyOctets,
    /// An octet was empty (`1..2.3.4`).
    EmptyOctet,
    /// An octet was not a number in `0..=255`.
    InvalidOctet,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        let address = Ipv4Addr::parse("192.168.1.42").unwrap();
        assert_eq!(address.octets(), [192, 168, 1, 42]);
        assert_eq!(std::format!("{address}"), "192.168.1.42");
        assert!(address.is_private());
        assert!(!address.is_unspecified());
    }

    #[test]
    fn rejects_malformed_addresses() {
        assert_eq!(Ipv4Addr::parse("1.2.3"), Err(ParseError::TooFewOctets));
        assert_eq!(Ipv4Addr::parse("1.2.3.4.5"), Err(ParseError::TooManyOctets));
        assert_eq!(Ipv4Addr::parse("1..3.4"), Err(ParseError::EmptyOctet));
        assert_eq!(Ipv4Addr::parse("1.2.3.256"), Err(ParseError::InvalidOctet));
        assert_eq!(Ipv4Addr::parse("a.b.c.d"), Err(ParseError::InvalidOctet));
    }

    #[test]
    fn classifies_ranges() {
        assert!(Ipv4Addr::new(10, 0, 0, 1).is_private());
        assert!(Ipv4Addr::new(172, 20, 0, 1).is_private());
        assert!(!Ipv4Addr::new(172, 32, 0, 1).is_private());
        assert!(Ipv4Addr::new(192, 168, 0, 1).is_private());
        assert!(!Ipv4Addr::new(8, 8, 8, 8).is_private());
        assert!(Ipv4Addr::UNSPECIFIED.is_unspecified());
    }
}
