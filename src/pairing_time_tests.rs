//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// 2026-11-03T10:15:23Z
const T: i64 = 1_793_700_923;

#[test]
fn a_time_goes_to_text_and_back_for_the_dates_that_trip_a_calendar() {
    for (secs, text) in [
        (0, "1970-01-01T00:00:00Z"),
        (T, "2026-11-03T10:15:23Z"),
        (951_782_400, "2000-02-29T00:00:00Z"),
        (1_709_164_799, "2024-02-28T23:59:59Z"),
        (1_709_164_800, "2024-02-29T00:00:00Z"),
        (1_709_251_200, "2024-03-01T00:00:00Z"),
        (4_102_444_799, "2099-12-31T23:59:59Z"),
        (-1, "1969-12-31T23:59:59Z"),
    ] {
        assert_eq!(iso(secs), text);
        assert_eq!(parse_iso(text).unwrap(), secs, "{text}");
    }
}

#[test]
fn a_time_that_is_not_one_real_utc_time_in_the_one_format_is_refused() {
    for bad in [
        "2026-02-30T00:00:00Z",
        "2025-02-29T00:00:00Z",
        "1900-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-00-10T00:00:00Z",
        "2026-11-03T24:00:00Z",
        "2026-11-03T10:60:00Z",
        "2026-11-03T10:15:60Z",
        "2026-11-03 10:15:23Z",
        "2026-11-03T10:15:23",
        "2026-11-03T10:15:23+00:00",
        "2026-11-03T10:15:23.5Z",
        "26-11-03T10:15:23Z",
        "2026-1-03T10:15:23Z",
        "+026-11-03T10:15:23Z",
        "2026-11-03T10:15:2xZ",
        "",
    ] {
        assert!(parse_iso(bad).is_err(), "{bad}");
    }
}

#[test]
fn every_day_from_year_0_to_year_9999_goes_to_text_and_back() {
    let first = days_from_civil(0, 1, 1);
    let last = days_from_civil(9999, 12, 31);
    assert_eq!(
        last - first + 1,
        3_652_425,
        "the days of 10,000 Gregorian years"
    );
    let mut day = first;
    while day <= last {
        let secs = day * 86_400 + 86_399;
        let text = iso(secs);
        assert_eq!(parse_iso(&text), Ok(secs), "{text}");
        day += 1;
    }
}
