//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one text form a pairing writes a time in, `YYYY-MM-DDTHH:MM:SSZ`, and the civil calendar
//! arithmetic under it. No time library is linked for it: the form is fixed, the range is the
//! years 0000 to 9999, and the round trip of every day in that range is a test.

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub(crate) const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count since 1970-01-01.
const fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// A time as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn iso(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Reads the one format [`iso`] writes: four-digit year, a real date, a real time, `Z`.
pub(crate) fn parse_iso(s: &str) -> Result<i64, String> {
    let bad = || format!("`{s}` is not a UTC time as `YYYY-MM-DDTHH:MM:SSZ`");
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return Err(bad());
    }
    let num = |from: usize, to: usize| -> Result<i64, String> {
        let part = &s[from .. to];
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse::<i64>().map_err(|_| bad())
        } else {
            Err(bad())
        }
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, se) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1 ..= 12).contains(&mo) || !(1 ..= 31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return Err(bad());
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + se;
    // a day past the end of its month does not round-trip, so Feb 30 is refused here
    if iso(secs) == s { Ok(secs) } else { Err(bad()) }
}

#[cfg(test)]
#[path = "pairing_time_tests.rs"]
mod tests;
