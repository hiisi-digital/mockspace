//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The names of a mockspace reference, on the host's side of the `letkajenkka` standard.
//!
//! The standard has one character set and leaves two things to its host: folding the host's own
//! naming into that set, so `mock::design` reaches a file called `DESIGN.md` and
//! `stack_lints::no_heap` names the preset `no-heap`, and the policy over what a parsed path may
//! hold. Mockspace folds the same way and reads a path of names the same way wherever it meets a
//! reference, so both are held here once, and every crate of either workspace reaches them here.
//! The character set itself is the standard's and is never restated.
//!
//! Where a reference is read is held here too, for the same reason: the code fences of a document,
//! inside which nothing is a reference, and the config files a preset shorthand is written in.

use core::fmt;

mod fence;
pub use fence::Fences;

/// The files a config of lints is read from, from the repository root, in the order the loader
/// tries them: the preset shorthands of `[lints.<name>]` are written in these, so the loader and
/// the migration that rewrites the shorthands read the one list.
pub const CONFIG_FILES: &[&str] = &["lints.toml", "mock/lints.toml", "mockspace.toml"];

use letkajenkka::ident::{is_body, is_start};
use letkajenkka::{Path, Selector};

/// A name as a reference writes it, folded from what a filesystem, a document or a config calls
/// it: case lowered, each hyphen an underscore, and [`LEAD`] set before a name opening on a digit.
///
/// Only [`fold`] makes one, so every value folds into an identifier of the standard. It holds the
/// name as given and is never allocated: it writes itself folded through `Display` and compares
/// with a written name through `PartialEq<str>`, and those two are all it shows.
#[derive(Clone, Copy, Debug)]
pub struct Folded<'a>(&'a str);

/// `name` folded into the standard's character set, or `None` where no folding makes an identifier
/// of it: it is empty, opens on an underscore, or holds a character the fold does not map into the
/// set (a dot, a space, a separator, anything outside ASCII).
///
/// The standard gives the fold of a host's naming to the host (letkajenkka `SPEC.md` 3.2) and lets
/// no name open on a digit, so a name that does, a research file's stamp, is led by [`LEAD`]:
/// `202611011713_renderer-roadmap.md` is cited as `t202611011713_renderer_roadmap`. Never by the
/// slug after the stamp, which a second file of the same slug would make ambiguous. A name of the
/// host's own that reads as a led stamp is refused by the lint `a-name-takes-no-led-stamp`,
/// through [`Folded::takes_a_led_stamp`], so a citation reaches one of the two.
#[must_use]
pub const fn fold(name: &str) -> Option<Folded<'_>> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || !(bytes[0].is_ascii_digit() || is_start(fold_byte(bytes[0]))) {
        return None;
    }
    let mut i = 1;
    while i < bytes.len() {
        if !is_body(fold_byte(bytes[i])) {
            return None;
        }
        i += 1;
    }
    Some(Folded(name))
}

/// The letter [`fold`] sets before a name opening on a digit.
pub const LEAD: char = 't';

/// The digits of a stamp, `YYYYMMDDHHMM`, that a research file or a round opens on.
pub const STAMP_DIGITS: usize = 12;

/// What a directory entry is called in a reference: a directory by its whole name, a file by its
/// name up to its first dot, so the extensions are never written.
#[must_use]
pub fn entry(name: &str, is_dir: bool) -> Option<Folded<'_>> {
    if is_dir { fold(name) } else { fold(name.split('.').next().unwrap_or_default()) }
}

impl Folded<'_> {
    /// Whether the fold sets [`LEAD`] before the name.
    const fn is_led(&self) -> bool {
        self.0.as_bytes()[0].is_ascii_digit()
    }

    /// Whether the name folds to [`LEAD`] and a stamp of [`STAMP_DIGITS`] digits: the
    /// spelling the fold gives a stamped name, so one citation would reach both.
    ///
    /// Only a stamp is asked of. A shorter run of digits after the lead names things of their own
    /// (`t3_timing_probe`, a probe `t4_update`), and where one of them meets a name led by the fold,
    /// the reference reads as ambiguous and says so.
    #[must_use]
    pub const fn takes_a_led_stamp(&self) -> bool {
        let b = self.0.as_bytes();
        if b.len() <= STAMP_DIGITS || fold_byte(b[0]) != LEAD as u8 {
            return false;
        }
        let mut i = 1;
        while i <= STAMP_DIGITS {
            if !b[i].is_ascii_digit() {
                return false;
            }
            i += 1;
        }
        b.len() == STAMP_DIGITS + 1 || !b[STAMP_DIGITS + 1].is_ascii_digit()
    }
}

const fn fold_byte(b: u8) -> u8 {
    if b == HOST_SEPARATOR { b'_' } else { b.to_ascii_lowercase() }
}

/// The separator of a host's own naming that [`fold`] makes an underscore.
const HOST_SEPARATOR: u8 = b'-';

/// `text` with each hyphen the underscore [`fold`] makes of it and nothing else changed, byte for
/// byte, so a name read from the result sits at the same place in `text`; and whether there was
/// any. The case is kept: a reference whose root is a host's name, a namespace named after its
/// directory, is read with this and its names taken from `text` where they stand.
#[must_use]
pub fn fold_separators(text: &str) -> Option<String> {
    let sep = char::from(HOST_SEPARATOR);
    text.contains(sep).then(|| text.replace(sep, "_"))
}

impl fmt::Display for Folded<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_led() {
            fmt::Write::write_char(f, LEAD)?;
        }
        for b in self.0.bytes() {
            fmt::Write::write_char(f, char::from(fold_byte(b)))?;
        }
        Ok(())
    }
}

impl PartialEq<str> for Folded<'_> {
    fn eq(&self, other: &str) -> bool {
        let other = if self.is_led() {
            match other.strip_prefix(LEAD) {
                Some(rest) => rest,
                None => return false,
            }
        } else {
            other
        };
        self.0.len() == other.len()
            && self
                .0
                .bytes()
                .zip(other.bytes())
                .all(|(a, b)| fold_byte(a) == b)
    }
}

impl PartialEq<&str> for Folded<'_> {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

/// The names of `path` when every step is a plain name, with no constraint and nothing widening or
/// selecting; `None` when any step is anything else.
///
/// Every step is checked before the first name is yielded, so a reader of a path of names never
/// meets a half valid one.
#[must_use]
pub fn plain(path: Path<'_>) -> Option<impl Iterator<Item = &str>> {
    let all_names = path
        .steps()
        .all(|s| matches!(s.selector, Selector::Ident(_)) && s.constraints.is_empty());
    all_names.then(|| {
        path.steps().filter_map(|s| {
            match s.selector {
                Selector::Ident(n) => Some(n),
                _ => None,
            }
        })
    })
}

/// The names of `text` read under the standard alone, `None` when the standard refuses it or it is
/// not a path of plain names.
#[must_use]
pub fn standard_names(text: &str) -> Option<Vec<&str>> {
    plain(letkajenkka::Query::parse(text).ok()?.path()?).map(Iterator::collect)
}

/// The names of `text` where its root alone carries a hyphen, each as written in `text`: a namespace
/// named after its directory (`ikiuni-renderer::a_row`). `None` unless the path has two names or
/// more and no name past the root needs the fold, so a hyphen past the root stays refused. The fold
/// is byte for byte, so each name read from the folded text stands at the same place in `text`.
#[must_use]
pub fn root_in_host_naming(text: &str) -> Option<Vec<&str>> {
    let folded = fold_separators(text)?;
    let base = folded.as_ptr().addr();
    let read: Vec<&str> = standard_names(&folded)?
        .into_iter()
        .map(|n| {
            let at = n.as_ptr().addr() - base;
            &text[at..at + n.len()]
        })
        .collect();
    (read.len() >= 2 && read[1..].iter().all(|n| fold_separators(n).is_none())).then_some(read)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
