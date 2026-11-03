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
//! - the commit's author time, which says where in the history to look when the hash is gone,
//!   and which a rebase or a cherry-pick keeps where it resets the committer time;
//! - a digest of the tree, computed here from paths and contents with no git object in it, which
//!   says which of the commits near that time is the one, and which stays the same under a
//!   rewrite that keeps the files.
//!
//! The record is written once, by [`Pairing::render`], and read once, by [`Pairing::parse`].
//! The trailer a commit carries (`Mock-Ref: <name> <hash> <time> <digest>`) and a pin file are
//! both this line, so neither can drift from the other. A [`Pairing`] and a [`Trailer`] can only
//! be made through [`Pairing::new`] and [`Trailer::new`] or by parsing, and both check what
//! [`Pairing::parse`] checks, so what `render` writes is always what `parse` reads.
//!
//! Nothing here runs git. The caller lists the candidate commits and gives a function that
//! reads a commit's files; [`resolve`] decides.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use crate::pairing_time::{iso, parse_iso};

/// The trailer key a commit carries a pairing under.
pub const TRAILER_KEY: &str = "Mock-Ref";

/// How far from the recorded time a commit may be and still be read when the hash is gone. The
/// window only bounds how many commits are read, and the digest decides among them.
pub const DEFAULT_WINDOW_SECS: i64 = 24 * 60 * 60;

/// The bytes a [`tree_digest`] starts with, so that a change to the encoding is a change to
/// every digest rather than a silent disagreement between two versions.
const DIGEST_TAG: &[u8] = b"mock-tree-digest-v1\0";

/// One commit tied to the data it was gated against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    hash:   String,
    time:   i64,
    digest: String,
}

/// A pairing under the name of the data ref it is for, as a commit carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trailer {
    name:    String,
    pairing: Pairing,
}

impl Pairing {
    /// A pairing of `hash` (40 or 64 lowercase hex digits), the commit's author time in seconds
    /// since the epoch (UTC, between years 0000 and 9999), and `digest` ([`tree_digest`] of the
    /// commit's files, 64 lowercase hex digits).
    ///
    /// # Errors
    ///
    /// When any of the three is outside what is said above.
    pub fn new(
        hash: impl Into<String>,
        time: i64,
        digest: impl Into<String>,
    ) -> Result<Self, String> {
        let (hash, digest) = (hash.into(), digest.into());
        if !is_hex(&hash, &[40, 64]) {
            return Err(format!(
                "the hash `{hash}` is not 40 or 64 lowercase hex digits"
            ));
        }
        if !is_hex(&digest, &[64]) {
            return Err(format!(
                "the digest `{digest}` is not 64 lowercase hex digits"
            ));
        }
        if parse_iso(&iso(time)).is_err() {
            return Err(format!(
                "the time {time} is not between the years 0000 and 9999"
            ));
        }
        Ok(Self {
            hash,
            time,
            digest,
        })
    }

    /// The commit hash.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// The commit's author time, in seconds since the epoch (UTC).
    pub const fn time(&self) -> i64 {
        self.time
    }

    /// The [`tree_digest`] of the commit's files.
    pub fn digest(&self) -> &str {
        &self.digest
    }

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
        Self::new(hash, parse_iso(time)?, digest)
    }
}

impl Trailer {
    /// A trailer for the data ref `name`: lowercase letters, digits, `_` and `-`, led by a
    /// letter or a digit.
    ///
    /// # Errors
    ///
    /// When the name is not one.
    pub fn new(name: impl Into<String>, pairing: Pairing) -> Result<Self, String> {
        let name = name.into();
        if !is_ref_name(&name) {
            return Err(format!(
                "the ref name `{name}` is not lowercase letters, digits, `_` and `-`, led by a \
                 letter or a digit"
            ));
        }
        Ok(Self {
            name,
            pairing,
        })
    }

    /// The data ref's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The pairing the trailer carries.
    pub const fn pairing(&self) -> &Pairing {
        &self.pairing
    }

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
        Self::new(name, Pairing::parse(pairing)?)
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

/// The digest of a set of files, as bytes fed to SHA-256: the tag `mock-tree-digest-v1` and a NUL,
/// then for each file in ascending path order the path's length in bytes as 8 little-endian
/// bytes, the path, the content's length as 8 little-endian bytes, and the content. The lengths
/// make the stream read back one way only, so no two different sets give the same bytes. No git
/// object is read, so the same files give the same digest in any repository they are in.
///
/// Paths are relative and `/` separated, as git lists them. File modes are not part of it, and
/// the digest cannot tell a symlink from a file that holds the link's text: a caller whose tree
/// has symlinks must refuse them before it gets here.
///
/// # Errors
///
/// When a path is empty, absolute, holds a backslash or a `.` or `..` segment, is given twice, or
/// is a file and also a directory of another path.
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
/// ascending path order, one after another. The paths are kept, to refuse a file that is also
/// a directory, and the contents are not.
#[derive(Debug)]
pub struct TreeHasher {
    hasher: Sha256,
    given:  BTreeSet<String>,
}

impl Default for TreeHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeHasher {
    pub fn new() -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DIGEST_TAG);
        Self {
            hasher,
            given: BTreeSet::new(),
        }
    }

    /// The next file. Its path has to be above the one before it, which also refuses a path
    /// given twice.
    ///
    /// # Errors
    ///
    /// When the path is not a relative path of `/` separated names without `.` or `..`, is
    /// given twice, is not above the path before it, or has a directory that was given as a
    /// file.
    pub fn file(&mut self, path: &str, content: &[u8]) -> Result<(), String> {
        check_path(path)?;
        if let Some(last) = self.given.last() {
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
        for (at, _) in path.match_indices('/') {
            if self.given.contains(&path[.. at]) {
                return Err(format!(
                    "`{}` is a file, and `{path}` is under it",
                    &path[.. at]
                ));
            }
        }
        self.hasher.update((path.len() as u64).to_le_bytes());
        self.hasher.update(path.as_bytes());
        self.hasher.update((content.len() as u64).to_le_bytes());
        self.hasher.update(content);
        self.given.insert(path.to_owned());
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
    /// The commit's author time, in seconds since the epoch (UTC).
    pub time: i64,
}

/// What [`resolve`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The hash is there and its files give the recorded digest.
    ByHash(String),
    /// The hash is gone, and one commit near the recorded time among the candidates has the
    /// recorded digest. Among the candidates only: where the history was cut short, a commit
    /// that was not fetched may hold the same files, and what this names is still those files.
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
    /// As [`Resolution::Missing`], but the history the caller holds is cut short, so the commit
    /// may be one that was not fetched. [`resolve`] never gives this: it does not know how much
    /// history it was given, and the caller that does turns a `Missing` into this one.
    Truncated,
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
pub const fn is_ref_name(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let letter_or_digit = matches!(bytes[i], b'a'..=b'z' | b'0'..=b'9');
        let inner = bytes[i] == b'_' || bytes[i] == b'-';
        if !(letter_or_digit || (i > 0 && inner)) {
            return false;
        }
        i += 1;
    }
    !bytes.is_empty()
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

#[cfg(test)]
#[path = "pairing_tests.rs"]
mod tests;
