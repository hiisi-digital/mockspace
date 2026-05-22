//! ISO-8601 UTC timestamps (spec §16 task closure, §23 anchor capture).
//!
//! [`Iso8601Utc`] plays the **human-readable name** role for the
//! [`Instant`] entity: a validated string of the form
//! `YYYY-MM-DDTHH:MM:SSZ`, second resolution, no fractional seconds.
//!
//! Mockspace ships its own tiny civil-calendar conversion (Howard
//! Hinnant's algorithm) instead of pulling in `chrono` or `time` for
//! what is essentially a debug field + provenance record. The
//! 1970-9999 range is the supported domain.

use core::fmt;

use crate::entity::Instant;
use crate::identity::{NamedRefTo, RefTo};

/// A validated ISO-8601 UTC timestamp.
///
/// Construct via [`Iso8601Utc::now`] (current system clock) or
/// [`Iso8601Utc::from_unix_secs`] (typed construction from a
/// known epoch-seconds value), or `<Iso8601Utc as NamedRefTo<Instant>>::parse`
/// (parse + validate a string). Implements [`NamedRefTo<Instant>`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Iso8601Utc(String);

/// Why an [`Iso8601Utc`] string rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Iso8601UtcError {
    /// Empty input.
    Empty,
    /// Wrong length. Expected 20 characters: `YYYY-MM-DDTHH:MM:SSZ`.
    BadLength { len: usize },
    /// Wrong separator at a fixed position in the format string.
    BadSeparator { position: usize, expected: char, found: char },
    /// A digit position contained a non-digit.
    BadDigit { position: usize, found: char },
    /// Trailing character was not `Z`.
    MissingTrailingZ { found: char },
}

impl fmt::Display for Iso8601UtcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("timestamp is empty"),
            Self::BadLength { len } => write!(
                f,
                "timestamp length {len} does not match the expected 20-character `YYYY-MM-DDTHH:MM:SSZ` form"
            ),
            Self::BadSeparator { position, expected, found } => write!(
                f,
                "expected {expected:?} at byte position {position}, found {found:?}"
            ),
            Self::BadDigit { position, found } => write!(
                f,
                "expected digit at byte position {position}, found {found:?}"
            ),
            Self::MissingTrailingZ { found } => write!(
                f,
                "timestamp must end with `Z` (UTC), found {found:?}"
            ),
        }
    }
}

impl std::error::Error for Iso8601UtcError {}

impl Iso8601Utc {
    /// Synthesise the current wall-clock ISO-8601 UTC timestamp.
    /// Second resolution, no fractional seconds.
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self::from_unix_secs(secs)
    }

    /// Construct from a known epoch-seconds value. Useful for tests
    /// and for serialising a deterministic timestamp.
    pub fn from_unix_secs(secs: u64) -> Self {
        let days = secs / 86_400;
        let time_of_day = secs % 86_400;
        let hour = time_of_day / 3600;
        let minute = (time_of_day % 3600) / 60;
        let second = time_of_day % 60;
        let (year, month, day) = days_to_ymd(days);
        Self(format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        ))
    }

    /// The validated timestamp as a string slice. Convenience over
    /// the `AsRef<str>` supertrait method.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Iso8601Utc {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Iso8601Utc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl RefTo<Instant> for Iso8601Utc {}

impl NamedRefTo<Instant> for Iso8601Utc {
    type Error = Iso8601UtcError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(Iso8601UtcError::Empty);
        }
        if s.len() != 20 {
            return Err(Iso8601UtcError::BadLength { len: s.len() });
        }
        let bytes = s.as_bytes();
        let digit_positions = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
        for &pos in &digit_positions {
            let ch = bytes[pos] as char;
            if !ch.is_ascii_digit() {
                return Err(Iso8601UtcError::BadDigit { position: pos, found: ch });
            }
        }
        let separators = [(4, '-'), (7, '-'), (10, 'T'), (13, ':'), (16, ':')];
        for (pos, expected) in separators {
            let found = bytes[pos] as char;
            if found != expected {
                return Err(Iso8601UtcError::BadSeparator { position: pos, expected, found });
            }
        }
        let trailing = bytes[19] as char;
        if trailing != 'Z' {
            return Err(Iso8601UtcError::MissingTrailingZ { found: trailing });
        }
        Ok(Self(s.to_owned()))
    }
}

/// Convert days-since-1970 to (year, month, day). Civil calendar
/// algorithm; correct for the full range we care about (1970 to
/// 9999). Adapted from Howard Hinnant's date library reference.
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let days = days as i64 + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = (days - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + (era as u64) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_format_matches_iso8601() {
        let ts = Iso8601Utc::now();
        let s = ts.as_str();
        assert_eq!(s.len(), 20, "got {s:?}");
        assert!(s.ends_with('Z'));
    }

    #[test]
    fn from_unix_secs_deterministic() {
        // 1_779_494_400 is whatever date the civil-from-days algorithm
        // computes; pin the expected value to the algorithm's output
        // so the test guards against accidental algorithm drift.
        let ts = Iso8601Utc::from_unix_secs(1_779_494_400);
        assert_eq!(ts.as_str(), "2026-05-23T00:00:00Z");
    }

    #[test]
    fn from_unix_secs_zero_epoch() {
        let ts = Iso8601Utc::from_unix_secs(0);
        assert_eq!(ts.as_str(), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn parse_round_trip() {
        let original = Iso8601Utc::from_unix_secs(1_779_494_400);
        let parsed =
            <Iso8601Utc as NamedRefTo<Instant>>::parse(original.as_str()).expect("parse");
        assert_eq!(original, parsed);
    }

    #[test]
    fn parse_rejects_empty() {
        let err = <Iso8601Utc as NamedRefTo<Instant>>::parse("").expect_err("reject");
        assert_eq!(err, Iso8601UtcError::Empty);
    }

    #[test]
    fn parse_rejects_wrong_length() {
        let err = <Iso8601Utc as NamedRefTo<Instant>>::parse("2026-05-22").expect_err("reject");
        assert!(matches!(err, Iso8601UtcError::BadLength { .. }));
    }

    #[test]
    fn parse_rejects_missing_z() {
        let err = <Iso8601Utc as NamedRefTo<Instant>>::parse("2026-05-22T00:00:00X")
            .expect_err("reject");
        assert!(matches!(err, Iso8601UtcError::MissingTrailingZ { found: 'X' }));
    }

    #[test]
    fn parse_rejects_bad_separator() {
        let err = <Iso8601Utc as NamedRefTo<Instant>>::parse("2026/05-22T00:00:00Z")
            .expect_err("reject");
        assert!(matches!(
            err,
            Iso8601UtcError::BadSeparator { position: 4, expected: '-', found: '/' }
        ));
    }

    #[test]
    fn parse_rejects_non_digit() {
        let err = <Iso8601Utc as NamedRefTo<Instant>>::parse("202X-05-22T00:00:00Z")
            .expect_err("reject");
        assert!(matches!(err, Iso8601UtcError::BadDigit { position: 3, .. }));
    }

    #[test]
    fn satisfies_named_ref_to_instant_bound() {
        fn take<N: NamedRefTo<Instant>>(n: N) -> String {
            n.to_string()
        }
        let ts = Iso8601Utc::from_unix_secs(0);
        assert_eq!(take(ts), "1970-01-01T00:00:00Z");
    }
}
