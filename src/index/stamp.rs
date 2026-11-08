//! The date a document carries in its own name, as days since 1970-01-01.
//!
//! Rounds, research directories and captures are all named `YYYYMMDDHHMM_title`, so the date is
//! read off the path with no file read and no git call. The deepest component that carries a stamp
//! wins: a changelist inside a round dated an hour earlier than the round's directory is the
//! changelist's own date.

/// Days from 1970-01-01 to a civil date, proleptic Gregorian.
///
/// Howard Hinnant's `days_from_civil`, exact over the whole range a stamp can hold.
#[must_use]
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The day a component's leading `YYYYMMDDHHMM` names, if it starts with a valid one.
fn day_of_component(name: &str) -> Option<i64> {
    let digits: &[u8] = name.as_bytes().get(..12)?;
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    // A thirteenth digit makes it a number, not a stamp.
    if name.as_bytes().get(12).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| name[r].parse::<i64>().ok();
    let (y, mo, d, h, mi) = (n(0 .. 4)?, n(4 .. 6)?, n(6 .. 8)?, n(8 .. 10)?, n(10 .. 12)?);
    let valid = (1970 ..= 2999).contains(&y)
        && (1 ..= 12).contains(&mo)
        && (1 ..= 31).contains(&d)
        && h < 24
        && mi < 60;
    valid.then(|| days_from_civil(y, mo, d))
}

/// The day a relative path's name carries, taking the deepest component that has a stamp.
#[must_use]
pub fn day_from_path(path: &str) -> Option<i64> {
    path.rsplit('/').find_map(day_of_component)
}

#[cfg(test)]
mod tests;
