//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The id of a design round, and the only place its shape is decided.
//!
//! A round is named by the stamp of the minute its first file was written,
//! `YYYYMMDDHHMM`, and its files start with an id: one id throughout for a round
//! opened with `cargo mock round-id`, while an older round stamped its doc and its
//! src changelist apart, so the round is named by its earliest changelist and a file
//! written beside either carries that changelist's id. A stamp alone is not
//! unique: two sessions in separate containers, on separate branches, open a
//! round in the same minute, both close it, and the second to land meets the
//! first in an add/add on the same directory.
//!
//! The shape is the stamp with an optional tag, `YYYYMMDDHHMM-tttt`, `t` a
//! lowercase letter or digit and the first of them a letter.
//!
//! - The stamp stays first and stays twelve digits, so every id already in a
//!   history is still an id, and a plain string sort of file names orders old
//!   and new by minute. Within one minute the order is arbitrary but fixed,
//!   which is all that two rounds sharing a minute ever had.
//! - The tag is drawn from where the round is opened (branch, host, process,
//!   nanoseconds), not from a counter, because a counter needs the other branch
//!   to be visible and it never is. Twenty-six times thirty-six cubed ids exist
//!   per minute, and [`fresh`] walks past any that the repository, its tags or
//!   its trunk already hold.
//! - The first character is a letter so a tag is never read as the `-N` that a
//!   close appends to a directory when two rounds still share a name (digits
//!   only, see [`is_round_dir`]).
//! - `-abandoned` is not a tag although it opens with four tag characters: a tag
//!   ends the id only at the end of the name, at `_` or at a further `-`.
//!
//! What reads the id: the changelist parser and the round name taken from the
//! earliest changelist ([`split_prefix`]), the `changelist-seal` lint on closed
//! directories ([`is_round_dir`]), the `.meta` `round:` field and the
//! `round/<dir>/<end|abandoned>` tag, both written from the directory name.
//! [`rename_prefix`] is the one rewrite of an id inside a file name.

/// Characters of a tag.
pub const TAG_LEN: usize = 4;

/// Whether `s` is a tag: [`TAG_LEN`] lowercase letters or digits, a letter first.
pub fn is_tag(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == TAG_LEN
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_stamp(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_digit())
}

/// The id at the start of `name` and what follows it.
///
/// Reads the twelve-digit stamp and, directly after it, a `-tag` if there is
/// one. Anything after that is the rest, which for a round file starts with `_`.
pub fn split_prefix(name: &str) -> Option<(&str, &str)> {
    let stamp = name.get(.. 12)?;
    if !is_stamp(stamp) {
        return None;
    }
    if let Some(tagged) = name.get(.. 12 + 1 + TAG_LEN) {
        let closes = matches!(name.as_bytes().get(tagged.len()), None | Some(b'_' | b'-'));
        if tagged.as_bytes()[12] == b'-' && is_tag(&tagged[13 ..]) && closes {
            return Some((tagged, &name[tagged.len() ..]));
        }
    }
    Some((stamp, &name[12 ..]))
}

/// The id a round directory's name starts with, `-2` and `-abandoned` dropped.
pub fn base_id(dir_name: &str) -> Option<&str> {
    split_prefix(dir_name).map(|(id, _)| id)
}

/// What follows the id in a round directory name that must survive a rename:
/// `-abandoned`, with its `-N` collision count dropped, or nothing. A bare `-N`
/// is the collision walk of a close and goes too. Dropping `-abandoned-N` to
/// nothing would turn an abandoned round into one [`is_round_dir`] reads as
/// closed, which `changelist-seal` then holds to the rules of a closed round.
pub fn kept_tail(dir_name: &str) -> &str {
    match split_prefix(dir_name) {
        Some((_, rest)) if is_abandoned_tail(rest) => "-abandoned",
        _ => "",
    }
}

/// `-abandoned`, or `-abandoned-N` for the digits `N` a close appends when two
/// abandoned rounds still shared a name.
fn is_abandoned_tail(rest: &str) -> bool {
    match rest.strip_prefix("-abandoned") {
        Some("") => true,
        Some(count) => {
            count
                .strip_prefix('-')
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        },
        None => false,
    }
}

/// A closed round's directory: an id, or an id with the `-N` a close appends
/// when two rounds still shared its name. Not `-abandoned`, nor anything else.
pub fn is_round_dir(name: &str) -> bool {
    let Some((_, rest)) = split_prefix(name) else {
        return false;
    };
    match rest.strip_prefix('-') {
        None => rest.is_empty(),
        Some(n) => !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()),
    }
}

/// `name` with the id `old` at its start replaced by `new`; `None` where it does
/// not start with `old` followed by `_` (so `2026..-kx3f` is not a prefix of
/// `2026..-kx3fz`, and a stamp is not a prefix of a tagged id).
pub fn rename_prefix(name: &str, old: &str, new: &str) -> Option<String> {
    let (id, rest) = split_prefix(name)?;
    (id == old && rest.starts_with('_')).then(|| format!("{new}{rest}"))
}

/// The tag for `seed` and `attempt`: FNV-1a over both, in base 36, first digit
/// forced to a letter.
pub fn tag_for(seed: &str, attempt: u32) -> String {
    const ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes().chain(attempt.to_le_bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut tag = [0u8; TAG_LEN];
    tag[0] = ALPHABET[(h % 26) as usize];
    h /= 26;
    for slot in &mut tag[1 ..] {
        *slot = ALPHABET[(h % 36) as usize];
        h /= 36;
    }
    String::from_utf8_lossy(&tag).into_owned()
}

/// A new id for `stamp`: the first `stamp-tag` that `taken` does not claim.
///
/// Pure over `taken`, so the walk past a collision is tested without a
/// repository. `stamp` is the twelve digits of the minute.
pub fn fresh(stamp: &str, seed: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut attempt = 0u32;
    loop {
        let candidate = format!("{stamp}-{}", tag_for(seed, attempt));
        if !taken(&candidate) {
            return candidate;
        }
        attempt += 1;
    }
}

#[cfg(test)]
#[path = "round_id_tests.rs"]
mod tests;
