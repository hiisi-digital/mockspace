//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A pairing: the three keys that tie one commit to another repository's data.
//!
//! A landing is gated against a particular state of the mock data, and the record of which
//! state must survive the data moving: a repository rehoused, a history rewritten, a clone made
//! from a mirror that never carried the old objects. A commit hash alone does not survive that,
//! because the hash is gone with the object. So a pairing holds three keys.
//!
//! - the commit hash, which settles it at once when the commit is still there;
//! - the commit's time, which says where in the history to look when the hash is gone;
//! - a digest of the tree, computed here from paths and contents with no git object in it, which
//!   says which of the commits near that time is the one, and which stays the same under a
//!   rewrite that keeps the files.
//!
//! The record is written once, by [`Pairing::render`], and read once, by [`Pairing::parse`].
//! The trailer a commit carries (`Mock-Ref: <name> <hash> <time> <digest>`) and a pin file are
//! both this line, so neither can drift from the other.
//!
//! Nothing here runs git. The caller lists the candidate commits and gives a function that
//! reads a commit's files; [`resolve`] decides.

use sha2::{Digest, Sha256};

/// The trailer key a commit carries a pairing under.
pub const TRAILER_KEY: &str = "Mock-Ref";

/// How far from the recorded time a commit may be and still be looked at when the hash is gone.
/// A day: a pairing is written at the commit, so a rehouse or a rewrite moves the time by
/// hours at most, and the digest is what decides among the commits inside it.
pub const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;

/// One commit tied to the data it was gated against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    /// The commit hash, 40 or 64 lowercase hex digits.
    pub hash:   String,
    /// The commit's time, in seconds since the epoch (UTC).
    pub time:   i64,
    /// [`tree_digest`] of the commit's files, 64 lowercase hex digits.
    pub digest: String,
}

/// A pairing under the name of the data ref it is for, as a commit carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    /// The data ref's name: lowercase letters, digits, `_` and `-`, led by a letter or a digit.
    pub name:    String,
    pub pairing: Pairing,
}

impl Pairing {
    /// The pairing as a line: `<hash> <time> <digest>`, the time as `YYYY-MM-DDTHH:MM:SSZ`.
    pub fn render(&self) -> String {
        format!("{} {} {}", self.hash, iso(self.time), self.digest)
    }

    /// Reads what [`render`](Self::render) wrote, and nothing else.
    ///
    /// # Errors
    ///
    /// When the line is not three single-space separated fields, the hash is not 40 or 64
    /// lowercase hex digits, the time is not a real UTC time in the one format, or the digest
    /// is not 64 lowercase hex digits.
    pub fn parse(line: &str) -> Result<Self, String> {
        let fields: Vec<&str> = line.split(' ').collect();
        let [hash, time, digest] = fields[..] else {
            return Err(format!(
                "a pairing is `<hash> <time> <digest>`, three fields, and `{line}` has {}",
                fields.len()
            ));
        };
        if !is_hex(hash, &[40, 64]) {
            return Err(format!(
                "the hash `{hash}` is not 40 or 64 lowercase hex digits"
            ));
        }
        if !is_hex(digest, &[64]) {
            return Err(format!(
                "the digest `{digest}` is not 64 lowercase hex digits"
            ));
        }
        Ok(Self {
            hash:   hash.to_owned(),
            time:   parse_iso(time)?,
            digest: digest.to_owned(),
        })
    }
}

impl Trailer {
    /// The trailer line a commit message carries.
    pub fn render(&self) -> String {
        format!("{TRAILER_KEY}: {} {}", self.name, self.pairing.render())
    }

    /// Reads one trailer line.
    ///
    /// # Errors
    ///
    /// When the line does not start with the trailer key, the name is not a ref name, or the
    /// pairing after it does not parse.
    pub fn parse(line: &str) -> Result<Self, String> {
        let Some(rest) = line
            .strip_prefix(TRAILER_KEY)
            .and_then(|r| r.strip_prefix(": "))
        else {
            return Err(format!("`{line}` is not a `{TRAILER_KEY}: ` line"));
        };
        let (name, pairing) = rest
            .split_once(' ')
            .ok_or_else(|| format!("`{line}` names no pairing after the ref name"))?;
        if !is_ref_name(name) {
            return Err(format!(
                "the ref name `{name}` is not lowercase letters, digits, `_` and `-`, led by a \
                 letter or a digit"
            ));
        }
        Ok(Self {
            name:    name.to_owned(),
            pairing: Pairing::parse(pairing)?,
        })
    }
}

/// Every trailer in a commit message, in order. Lines that are not trailers are left alone.
///
/// # Errors
///
/// When a line starts with the trailer key and does not parse, or two trailers name the same
/// ref: a pairing that is read wrongly is an error here and never skipped, because skipping it
/// would pair the commit with nothing and say so to nobody.
pub fn trailers(message: &str) -> Result<Vec<Trailer>, String> {
    let mut out: Vec<Trailer> = Vec::new();
    for line in message.lines() {
        if !line.starts_with(TRAILER_KEY) {
            continue;
        }
        let trailer = Trailer::parse(line)?;
        if out.iter().any(|t| t.name == trailer.name) {
            return Err(format!(
                "two `{TRAILER_KEY}` lines name the ref `{}`",
                trailer.name
            ));
        }
        out.push(trailer);
    }
    Ok(out)
}

/// The digest of a set of files: SHA-256 over each file's path and contents, in path order,
/// each length-prefixed so that no two different sets run together into the same bytes. No git
/// object is read, so the same files give the same digest in any repository they are in.
///
/// Paths are relative and `/` separated, as git lists them. File modes are not part of it.
///
/// # Errors
///
/// When a path is empty, absolute, holds a backslash or a `.` or `..` segment, or is given twice.
pub fn tree_digest<'a, I>(files: I) -> Result<String, String>
where
    I: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    let mut entries: Vec<(&str, &[u8])> = files.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    let mut hasher = TreeHasher::new();
    for (path, content) in entries {
        hasher.file(path, content)?;
    }
    Ok(hasher.finish())
}

/// [`tree_digest`] a file at a time, for a tree too large to hold: the files are given in
/// ascending path order, one after another, and only the one in hand is in memory.
#[derive(Debug, Default)]
pub struct TreeHasher {
    hasher: Sha256,
    last:   Option<String>,
}

impl TreeHasher {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next file. Its path has to be above the one before it, which also refuses a path
    /// given twice.
    ///
    /// # Errors
    ///
    /// When the path is not a relative path of `/` separated names without `.` or `..`, is
    /// given twice, or is not above the path before it.
    pub fn file(&mut self, path: &str, content: &[u8]) -> Result<(), String> {
        check_path(path)?;
        if let Some(last) = &self.last {
            if path == last {
                return Err(format!("the path `{path}` is given twice"));
            }
            if path < last.as_str() {
                return Err(format!(
                    "the path `{path}` comes after `{last}`, and the files are given in ascending \
                     order"
                ));
            }
        }
        self.hasher.update((path.len() as u64).to_le_bytes());
        self.hasher.update(path.as_bytes());
        self.hasher.update((content.len() as u64).to_le_bytes());
        self.hasher.update(content);
        self.last = Some(path.to_owned());
        Ok(())
    }

    /// The digest of the files given, 64 lowercase hex digits.
    pub fn finish(self) -> String {
        hex(&self.hasher.finalize())
    }
}

fn check_path(path: &str) -> Result<(), String> {
    let bad = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..");
    if bad {
        return Err(format!(
            "`{path}` is not a relative path of `/` separated names without `.` or `..`"
        ));
    }
    Ok(())
}

/// A commit that might be the one a pairing names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate<'a> {
    pub hash: &'a str,
    /// The commit's time, in seconds since the epoch (UTC).
    pub time: i64,
}

/// What [`resolve`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The hash is there and its files give the recorded digest.
    ByHash(String),
    /// The hash is gone, and one commit near the recorded time has the recorded digest.
    ByTimeAndDigest(String),
    /// The hash is gone, and several commits near the recorded time have the recorded digest,
    /// nearest in time first. They hold the same files, so any of them is the data; the caller
    /// decides which to take and says that it chose.
    Ambiguous(Vec<String>),
    /// The hash is there and its files do not give the recorded digest, or cannot be read: the
    /// commit is not the one that was paired, whatever its hash says.
    Mismatch {
        hash:  String,
        /// The digest the commit's files give, or nothing where they could not be read.
        found: Option<String>,
    },
    /// The hash is gone and no commit near the recorded time has the recorded digest.
    Missing,
}

/// Which commit a pairing names, among `candidates`.
///
/// The hash is tried first. When a candidate carries it, the answer is that commit if its
/// files give the recorded digest and a [`Resolution::Mismatch`] if they do not, and the other
/// keys are not used to look further, since a hash that names the wrong tree is a fault to
/// report rather than a reason to search. When no candidate carries it, the candidates within
/// `window_secs` of the recorded time are read through `digest_of` and those that give the
/// recorded digest are the answer.
///
/// `digest_of` gives the [`tree_digest`] of a commit's files, or nothing when they cannot be
/// read. It is asked for the matched hash once, and otherwise for the commits in the window,
/// nearest first, so a window with a thousand commits in it costs a thousand reads.
pub fn resolve<F>(
    recorded: &Pairing,
    candidates: &[Candidate<'_>],
    window_secs: i64,
    mut digest_of: F,
) -> Resolution
where
    F: FnMut(&str) -> Option<String>,
{
    if candidates.iter().any(|c| c.hash == recorded.hash) {
        return match digest_of(&recorded.hash) {
            Some(found) if found == recorded.digest => Resolution::ByHash(recorded.hash.clone()),
            found => {
                Resolution::Mismatch {
                    hash: recorded.hash.clone(),
                    found,
                }
            },
        };
    }
    let mut near: Vec<&Candidate<'_>> = candidates
        .iter()
        .filter(|c| (c.time - recorded.time).abs() <= window_secs)
        .collect();
    near.sort_by_key(|c| ((c.time - recorded.time).abs(), c.hash));
    let mut hits: Vec<String> = near
        .into_iter()
        .filter(|c| digest_of(c.hash).as_deref() == Some(recorded.digest.as_str()))
        .map(|c| c.hash.to_owned())
        .collect();
    match hits.len() {
        0 => Resolution::Missing,
        1 => Resolution::ByTimeAndDigest(hits.remove(0)),
        _ => Resolution::Ambiguous(hits),
    }
}

/// Whether `s` is lowercase hex of one of the lengths.
const fn is_hex(s: &str, lengths: &[usize]) -> bool {
    let bytes = s.as_bytes();
    let mut ok_len = false;
    let mut i = 0;
    while i < lengths.len() {
        if lengths[i] == bytes.len() {
            ok_len = true;
        }
        i += 1;
    }
    let mut j = 0;
    while j < bytes.len() {
        if !matches!(bytes[j], b'0'..=b'9' | b'a'..=b'f') {
            return false;
        }
        j += 1;
    }
    ok_len
}

/// Whether `s` is a data ref name, which is also what a trailer calls the ref: lowercase letters,
/// digits, `_` and `-`, led by a letter or a digit.
pub fn is_ref_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from(DIGITS[usize::from(b >> 4)]));
        out.push(char::from(DIGITS[usize::from(b & 15)]));
    }
    out
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
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
fn iso(secs: i64) -> String {
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
fn parse_iso(s: &str) -> Result<i64, String> {
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
#[path = "pairing_tests.rs"]
mod tests;
