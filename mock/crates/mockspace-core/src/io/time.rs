//! Tiny ISO-8601 timestamp helper shared by the IO layer.
//!
//! Both the flock guard (debug payload) and the anchor capture
//! (`Anchor::captured_at`) want a wall-clock ISO-8601 UTC stamp.
//! Avoid a chrono/time dep for what is essentially a debug field
//! and a provenance record; the civil-calendar conversion fits in
//! a few lines and is correct for the 1970-9999 range.

use std::time::{SystemTime, UNIX_EPOCH};

/// Synthesise a minimal ISO-8601 UTC timestamp. Format:
/// `YYYY-MM-DDTHH:MM:SSZ`. Second resolution; no fractional seconds.
pub(crate) fn current_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let hour = time_of_day / 3600;
    let minute = (time_of_day % 3600) / 60;
    let second = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z",
        year = year,
        month = month,
        day = day,
        hour = hour,
        minute = minute,
        second = second,
    )
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
    fn iso8601_format_has_canonical_separators() {
        let s = current_iso8601();
        assert_eq!(s.len(), 20, "got {s:?}");
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-", "got {s:?}");
        assert_eq!(&s[7..8], "-", "got {s:?}");
        assert_eq!(&s[10..11], "T", "got {s:?}");
        assert_eq!(&s[13..14], ":", "got {s:?}");
        assert_eq!(&s[16..17], ":", "got {s:?}");
    }
}
