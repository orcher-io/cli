//! A UTC timestamp for the CLI, a thin wrapper over `jiff::Timestamp` with
//! the conversions and arithmetic the commands use.

use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamptz(Timestamp);

impl Timestamptz {
    pub fn now() -> Self {
        Self(Timestamp::now())
    }

    /// A Unix time in seconds, or `None` when it is out of range.
    pub fn from_second(secs: i64) -> Option<Self> {
        Timestamp::from_second(secs).ok().map(Self)
    }

    /// RFC 3339, as in `2024-01-15T12:30:00Z`.
    pub fn to_rfc3339(self) -> String {
        self.0.to_string()
    }

    pub fn parse_rfc3339(s: &str) -> Result<Self, jiff::Error> {
        s.parse::<Timestamp>().map(Self)
    }

    /// Formats with `strftime` directives, in UTC.
    pub fn format(self, fmt: &str) -> String {
        self.0.strftime(fmt).to_string()
    }
}

impl fmt::Display for Timestamptz {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add<SignedDuration> for Timestamptz {
    type Output = Self;

    fn add(self, rhs: SignedDuration) -> Self {
        Self(self.0 + rhs)
    }
}

impl Sub<SignedDuration> for Timestamptz {
    type Output = Self;

    fn sub(self, rhs: SignedDuration) -> Self {
        Self(self.0 - rhs)
    }
}

impl Sub<Timestamptz> for Timestamptz {
    type Output = SignedDuration;

    fn sub(self, rhs: Timestamptz) -> SignedDuration {
        self.0.duration_since(rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_rfc3339_and_formats_in_utc() {
        let t = Timestamptz::parse_rfc3339("2023-11-14T22:13:20Z").unwrap();
        assert_eq!(Timestamptz::from_second(1_700_000_000), Some(t));
        assert_eq!(t.to_rfc3339(), "2023-11-14T22:13:20Z");
        assert_eq!(t.format("%Y-%m-%d %H:%M:%S"), "2023-11-14 22:13:20");
        let earlier = t - SignedDuration::from_secs(90);
        assert_eq!((t - earlier).as_secs(), 90);
    }
}
