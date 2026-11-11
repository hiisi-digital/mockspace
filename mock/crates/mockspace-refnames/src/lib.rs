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

use core::fmt;

use letkajenkka::ident::{is_body, is_start};
use letkajenkka::{Path, Selector};

/// A name as a reference writes it, folded from what a filesystem, a document or a config calls
/// it: case lowered and each hyphen an underscore.
///
/// Only [`fold`] makes one, so every value folds into an identifier of the standard. It holds the
/// name as given and is never allocated: it writes itself folded through `Display` and compares
/// with a written name through `PartialEq<str>`, and those two are all it shows.
#[derive(Clone, Copy, Debug)]
pub struct Folded<'a>(&'a str);

/// `name` folded into the standard's character set, or `None` where no folding makes an identifier
/// of it: it is empty, opens on a digit or an underscore, or holds a character the fold does not
/// map into the set (a dot, a space, a separator, anything outside ASCII).
#[must_use]
pub const fn fold(name: &str) -> Option<Folded<'_>> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || !is_start(fold_byte(bytes[0])) {
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

const fn fold_byte(b: u8) -> u8 {
    if b == b'-' { b'_' } else { b.to_ascii_lowercase() }
}

impl fmt::Display for Folded<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0.bytes() {
            fmt::Write::write_char(f, char::from(fold_byte(b)))?;
        }
        Ok(())
    }
}

impl PartialEq<str> for Folded<'_> {
    fn eq(&self, other: &str) -> bool {
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
