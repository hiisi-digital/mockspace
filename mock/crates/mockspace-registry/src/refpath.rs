//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one reader of a hierarchical reference, `root::step::step`.
//!
//! The syntax is the `letkajenkka` standard and `letkajenkka` is its parser: nothing in this crate
//! splits a reference on its separator. The host's half of the standard, the fold of a name and
//! the reading of a path as plain names, is `mockspace_refnames`. What this module adds is the
//! resolution that follows from it: `mock::design` reaches `DESIGN.md.tmpl`, and
//! `mock::design::the_four_lanes` reaches the heading a forge anchors as `the-four-lanes`, by the
//! walk down a root.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use letkajenkka::{Query, Selector};
use mockspace_refnames::{Folded, fold};
use regex::Regex;

/// The migration that rewrites a repository's references from the spelling before the standard.
pub const MIGRATION: &str = "references-to-letkajenkka";

/// Why a text is not a reference this crate reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The standard refuses the text, for the reason the error names.
    Syntax(letkajenkka::Error),
    /// A well formed query that is not a plain path of names where one is required: a set, a
    /// wildcard, a constraint, an operation, or a line selector where a name belongs.
    NotPlain,
}

impl Refusal {
    /// The refusal as a finding says it: the rule broken, and where the old spelling is mended.
    #[must_use]
    pub fn say(self, text: &str) -> String {
        match self {
            Self::Syntax(e) => {
                format!(
                    "`{text}` is not a reference under the letkajenkka standard ({e}). A repository written in the \
                     spelling before it is rewritten by `cargo mock migrate {MIGRATION}`"
                )
            },
            Self::NotPlain => {
                format!("`{text}` names no single thing here: a reference of this kind is a plain path of names")
            },
        }
    }
}

/// The names of a text that is a plain path: every step a name, no constraint, nothing widening.
///
/// # Errors
///
/// [`Refusal::Syntax`] when the standard refuses the text, [`Refusal::NotPlain`] when it parses and
/// is not a path of names.
pub fn names(text: &str) -> Result<Vec<&str>, Refusal> {
    let query = Query::parse(text).map_err(Refusal::Syntax)?;
    let Some(path) = query.path() else { return Err(Refusal::NotPlain) };
    mockspace_refnames::plain(path).map(Iterator::collect).ok_or(Refusal::NotPlain)
}

/// An operation `name(argument)` of exactly one argument, as the argument's text.
///
/// `None` when the text is not an operation of that name, or has another number of arguments.
#[must_use]
pub fn operation<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let Ok(Query::Operation {
        name: got,
        arguments,
    }) = Query::parse(text)
    else {
        return None;
    };
    let mut args = letkajenkka::Arguments::of(arguments);
    match (got == name, args.next(), args.next()) {
        (true, Some(one), None) => Some(one),
        _ => None,
    }
}

/// The name a text opens on, where it parses as a path whose first step is a name: the namespace or
/// the root of a reference. `None` for a text the standard refuses, and for one opening on anything
/// but a name, since neither says what it reaches.
#[must_use]
pub fn root(text: &str) -> Option<&str> {
    let path = Query::parse(text).ok()?.path()?;
    match path.steps().next()?.selector {
        Selector::Ident(name) => Some(name),
        _ => None,
    }
}

/// The items of a list of references as a row field writes it: split at the commas of its own level
/// by the syntax, each trimmed, and an empty item no item.
pub fn list(raw: &str) -> impl Iterator<Item = &str> {
    letkajenkka::Arguments::of(raw).map(str::trim).filter(|s| !s.is_empty())
}

/// What a file or a directory is called in a reference: the whole name of a directory, and the name
/// up to its first dot for a file, so the extensions are never written.
fn fold_entry(name: &str, is_dir: bool) -> Option<Folded<'_>> {
    if is_dir {
        fold(name)
    } else {
        fold(name.split('.').next().unwrap_or_default())
    }
}

/// A heading as GitHub anchors it, by the slugger it follows: lowercased, then every character
/// deleted but the space, the hyphen and what `slug_keeps` keeps, then each remaining space written as
/// one hyphen. Runs of spaces are not collapsed and nothing is trimmed.
#[must_use]
pub fn forge_slug(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            match c {
                ' ' => Some('-'),
                c if c == '-' || slug_keeps(c) => Some(c),
                _ => None,
            }
        })
        .collect()
}

/// Whether the slugger keeps `c`: anything alphabetic, and the marks, the decimal digits and the
/// connector punctuation, which are not. It deletes the rest, other numbers and alphabetic-less
/// symbols among them (`mock/research/202611111500_github-heading-anchors.md`).
fn slug_keeps(c: char) -> bool {
    static NOT_ALPHABETIC: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\p{M}\p{Nd}\p{Pc}]$").expect("the classes are the regex crate's own"));
    c.is_alphabetic() || NOT_ALPHABETIC.is_match(c.encode_utf8(&mut [0; 4]))
}

/// A heading as a reference names it: its forge slug, folded.
#[must_use]
pub fn heading_name(title: &str) -> Option<String> {
    fold(&forge_slug(title)).map(|f| f.to_string())
}

/// What a citation points at within a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Anchor {
    /// A heading, by the name `heading_name` gives its title. Survives every edit that does not
    /// rename the heading, and announces itself when one does.
    Heading(String),
    /// A line, or a span of lines with both bounds included. Honest only where the file does not
    /// change.
    Lines(u64, u64),
}

/// A file citation: `root::step::...::anchor`, read but not yet resolved against a tree.
///
/// Which of the steps after the root are directories and files and which is a heading is decided
/// by the tree, since the grammar writes them alike. `names` holds every step after the root up to
/// the line, and `lines` the line selector when the citation ends in one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub root:  String,
    pub names: Vec<String>,
    pub lines: Option<(u64, u64)>,
}

impl FileRef {
    /// Reads a citation, which names a root, at least one step into it, and an anchor.
    ///
    /// # Errors
    ///
    /// The standard's refusal, or [`Refusal::NotPlain`] for a text that parses and is no citation.
    pub fn read(text: &str) -> Result<Self, Refusal> {
        let query = Query::parse(text).map_err(Refusal::Syntax)?;
        let Some(path) = query.path() else { return Err(Refusal::NotPlain) };
        let steps: Vec<_> = path.steps().collect();
        let Some((last, init)) = steps.split_last() else { return Err(Refusal::NotPlain) };
        let mut names = Vec::new();
        for s in init {
            match (s.selector, s.constraints.is_empty()) {
                (Selector::Ident(n), true) => names.push(n.to_string()),
                _ => return Err(Refusal::NotPlain),
            }
        }
        let lines = match (last.selector, last.constraints.is_empty()) {
            (Selector::Ident(n), true) => {
                names.push(n.to_string());
                None
            },
            // a document has no line zero
            (Selector::Ordinal(n), true) if n > 0 => Some((n, n)),
            (Selector::Range(lo, hi), true) if lo > 0 => Some((lo, hi)),
            _ => return Err(Refusal::NotPlain),
        };
        if names.len() < 3 - usize::from(lines.is_some()) {
            return Err(Refusal::NotPlain);
        }
        let root = names.remove(0);
        Ok(Self {
            root,
            names,
            lines,
        })
    }

    /// [`Self::read`], for a caller that does not say why.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::read(text).ok()
    }

    /// The citation written back.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = self.root.clone();
        for n in &self.names {
            out.push_str("::");
            out.push_str(n);
        }
        match self.lines {
            Some((lo, hi)) if lo == hi => out.push_str(&format!("::{lo}")),
            Some((lo, hi)) => out.push_str(&format!("::{lo}..{hi}")),
            None => {},
        }
        out
    }

    /// The path the names give, `/` joined, as a document names it.
    #[must_use]
    pub fn path_text(&self) -> String {
        let n = self.names.len() - usize::from(self.lines.is_none());
        self.names[.. n].join("/")
    }

    /// The anchor: the line selector, or the last name read as a heading.
    #[must_use]
    pub fn anchor(&self) -> Anchor {
        match (self.lines, self.names.last()) {
            (Some((lo, hi)), _) => Anchor::Lines(lo, hi),
            (None, Some(h)) => Anchor::Heading(h.clone()),
            (None, None) => Anchor::Lines(0, 0),
        }
    }

    /// The file this citation names under `root_dir`, by walking the names and folding each
    /// entry's own name as the standard has a host do.
    #[must_use]
    pub fn locate(&self, root_dir: &Path) -> Location {
        let file_names = &self.names[.. self.names.len() - usize::from(self.lines.is_none())];
        let mut found = Vec::new();
        walk(root_dir, file_names, &mut found);
        found.sort();
        found.dedup();
        match found.len() {
            0 => Location::Missing,
            1 => Location::Found(found.remove(0)),
            _ => {
                Location::Ambiguous(
                    found
                        .iter()
                        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .collect(),
                )
            },
        }
    }
}

/// Where a citation's file is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Found(PathBuf),
    Missing,
    /// Several files fold to the names given. A host does not pick one.
    Ambiguous(Vec<String>),
}

/// What one entry of a directory is, as a citation's walk needs it.
#[derive(Debug, Clone)]
struct Entry {
    name:    String,
    path:    PathBuf,
    is_dir:  bool,
    is_file: bool,
}

thread_local! {
    /// The directory listings held while a [`CacheScope`] is open, and how many scopes are.
    static LISTINGS: std::cell::RefCell<(usize, std::collections::HashMap<PathBuf, std::rc::Rc<[Entry]>>)> =
        std::cell::RefCell::new((0, std::collections::HashMap::new()));
    static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// While this is alive, a directory a citation walks through is listed once and the listing held,
/// so a pass that resolves many citations reads each directory once and not once for each of them.
/// Nothing is held outside a scope, so a file made between two calls is found, and a scope is
/// opened around a pass that does not write.
#[must_use]
pub struct CacheScope(());

/// Opens a [`CacheScope`] on this thread. Scopes nest, and the listings go when the outermost ends.
pub fn cache_scope() -> CacheScope {
    LISTINGS.with(|l| l.borrow_mut().0 += 1);
    CacheScope(())
}

impl Drop for CacheScope {
    fn drop(&mut self) {
        LISTINGS.with(|l| {
            let mut l = l.borrow_mut();
            l.0 -= 1;
            if l.0 == 0 {
                l.1.clear();
            }
        });
    }
}

/// How many directories this thread has listed for citations, held ones not counted again.
#[must_use]
pub fn dir_reads() -> usize {
    READS.with(std::cell::Cell::get)
}

fn listing(dir: &Path) -> std::rc::Rc<[Entry]> {
    if let Some(held) = LISTINGS.with(|l| l.borrow().1.get(dir).cloned()) {
        return held;
    }
    READS.with(|r| r.set(r.get() + 1));
    let read: Vec<Entry> = fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| {
                    let path = e.path();
                    Entry {
                        name:    e.file_name().to_string_lossy().into_owned(),
                        is_dir:  path.is_dir(),
                        is_file: path.is_file(),
                        path,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let held: std::rc::Rc<[Entry]> = read.into();
    LISTINGS.with(|l| {
        let mut l = l.borrow_mut();
        if l.0 > 0 {
            l.1.insert(dir.to_path_buf(), held.clone());
        }
    });
    held
}

/// Every file under `dir` that `names` reach, the last name being the file's own.
fn walk(dir: &Path, names: &[String], out: &mut Vec<PathBuf>) {
    let Some((name, rest)) = names.split_first() else { return };
    for entry in listing(dir).iter() {
        if !fold_entry(&entry.name, entry.is_dir).is_some_and(|f| f == name.as_str()) {
            continue;
        }
        if entry.is_dir {
            walk(&entry.path, rest, out);
        } else if entry.is_file && rest.is_empty() {
            out.push(entry.path.clone());
        }
    }
}

/// The line a heading anchor stands at in `file`, and the forge slug a link to it carries.
#[must_use]
pub fn heading_in(file: &Path, name: &str) -> Option<(usize, String)> {
    let text = fs::read_to_string(file).ok()?;
    let mut fences = mockspace_refnames::Fences::new();
    text.lines().enumerate().find_map(|(i, l)| {
        let trimmed = l.trim_start();
        if !fences.prose(l) || !trimmed.starts_with('#') {
            return None;
        }
        let title = trimmed.trim_start_matches('#').trim();
        (heading_name(title).as_deref() == Some(name)).then(|| (i + 1, forge_slug(title)))
    })
}

/// The line an anchor stands at in `file`: a heading's line, or the first line of a span that is
/// inside the file.
#[must_use]
pub fn resolve_anchor(file: &Path, anchor: &Anchor) -> Option<usize> {
    match anchor {
        Anchor::Lines(lo, hi) => {
            let count = fs::read_to_string(file).ok()?.lines().count() as u64;
            (*hi <= count).then(|| usize::try_from(*lo).ok()).flatten()
        },
        Anchor::Heading(name) => heading_in(file, name).map(|(line, _)| line),
    }
}

#[cfg(test)]
#[path = "refpath_tests.rs"]
mod tests;
