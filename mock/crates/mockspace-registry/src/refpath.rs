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
use mockspace_refnames::fold;
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
    /// A citation carrying a shape anywhere but on the step naming its file, or more than one
    /// there. The one shape a citation takes is the file's extension (letkajenkka `SPEC.md` 9.1).
    MisplacedShape,
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
            Self::MisplacedShape => {
                format!(
                    "`{text}` carries a shape where a citation takes none: the one shape it takes is the extension of the \
                     file it names, alone and on the step naming the file, as in `panel::probes::run<sh>::1`"
                )
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
///
/// The root of a path of two names or more may be written in the host's own naming, as a namespace
/// named after its directory is (`ikiuni-renderer::a_row`): the root is given as written, so it
/// meets the key it was declared under. Every name after it is the standard's spelling alone, and
/// so is a path of one name, which is a slug.
pub fn names(text: &str) -> Result<Vec<&str>, Refusal> {
    match standard_names(text) {
        Err(refused) => mockspace_refnames::root_in_host_naming(text).ok_or(refused),
        read => read,
    }
}

/// The names of `text` read under the standard alone.
fn standard_names(text: &str) -> Result<Vec<&str>, Refusal> {
    let query = Query::parse(text).map_err(Refusal::Syntax)?;
    let Some(path) = query.path() else { return Err(Refusal::NotPlain) };
    mockspace_refnames::plain(path).map(Iterator::collect).ok_or(Refusal::NotPlain)
}

/// An operation `name(argument)` of exactly one argument, as the argument's text.
///
/// `None` when the text is not an operation of that name, or has another number of arguments. An
/// argument whose root is written in the host's naming (`pathof(ikiuni-renderer::a_row)`) is read
/// as [`names`] reads it: the argument is given as written, and the query inside it is the caller's
/// to read, which still refuses a hyphen past the root.
#[must_use]
pub fn operation<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    match Query::parse(text) {
        Ok(Query::Operation { name: got, arguments }) => one_argument(got, arguments, name),
        Ok(_) => None,
        Err(_) => {
            let folded = mockspace_refnames::fold_separators(text)?;
            let base = folded.as_ptr().addr();
            let Ok(Query::Operation { name: got, arguments }) = Query::parse(&folded) else {
                return None;
            };
            let one = one_argument(got, arguments, name)?;
            let at = one.as_ptr().addr() - base;
            Some(&text[at..at + one.len()])
        },
    }
}

/// The one argument of an operation called `got`, when it is called `name`.
fn one_argument<'a>(got: &str, arguments: &'a str, name: &str) -> Option<&'a str> {
    let mut args = letkajenkka::Arguments::of(arguments);
    match (got == name, args.next(), args.next()) {
        (true, Some(one), None) => Some(one),
        _ => None,
    }
}

/// The name a text opens on, where it parses as a path whose first step is a name: the namespace or
/// the root of a reference. `None` for a text the standard refuses, and for one opening on anything
/// but a name, since neither says what it reaches. A root written in the host's naming
/// (`ikiuni-renderer::a_row`) is read as [`names`] reads it.
#[must_use]
pub fn root(text: &str) -> Option<&str> {
    let Some(path) = Query::parse(text).ok().and_then(Query::path) else {
        return mockspace_refnames::root_in_host_naming(text)?.first().copied();
    };
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
/// the line, and `lines` the line selector when the citation ends in one. `extension` is the shape
/// on the step naming the file, where two files differ by nothing else (letkajenkka `SPEC.md` 9.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    pub root:      String,
    pub names:     Vec<String>,
    pub lines:     Option<(u64, u64)>,
    pub extension: Option<String>,
}

impl FileRef {
    /// Reads a citation, which names a root, at least one step into it, and an anchor.
    ///
    /// # Errors
    ///
    /// The standard's refusal, [`Refusal::NotPlain`] for a text that parses and is no citation, and
    /// [`Refusal::MisplacedShape`] for one carrying a shape other than its file's extension.
    pub fn read(text: &str) -> Result<Self, Refusal> {
        let query = Query::parse(text).map_err(Refusal::Syntax)?;
        let Some(path) = query.path() else { return Err(Refusal::NotPlain) };
        let steps: Vec<_> = path.steps().collect();
        let Some((last, init)) = steps.split_last() else { return Err(Refusal::NotPlain) };
        let mut names = Vec::new();
        let mut shapes = Vec::new();
        for s in init {
            match s.selector {
                Selector::Ident(n) => {
                    names.push(n.to_string());
                    shapes.push(s.constraints);
                },
                _ => return Err(Refusal::NotPlain),
            }
        }
        let lines = match (last.selector, last.constraints.is_empty()) {
            (Selector::Ident(n), _) => {
                names.push(n.to_string());
                shapes.push(last.constraints);
                None
            },
            // a document has no line zero
            (Selector::Ordinal(n), true) if n > 0 => Some((n, n)),
            (Selector::Range(lo, hi), true) if lo > 0 => Some((lo, hi)),
            (Selector::Ordinal(_) | Selector::Range(..), false) => return Err(Refusal::MisplacedShape),
            _ => return Err(Refusal::NotPlain),
        };
        if names.len() < 3 - usize::from(lines.is_some()) {
            return Err(Refusal::NotPlain);
        }
        // the step naming the file: the last name before a line, or the one before a heading
        let file_at = names.len() - 1 - usize::from(lines.is_none());
        let mut extension = None;
        for (at, shape) in shapes.iter().enumerate().filter(|(_, c)| !c.is_empty()) {
            let mut each = shape.names();
            match (at == file_at, each.next(), each.next()) {
                (true, Some(one), None) => extension = Some(one.to_string()),
                _ => return Err(Refusal::MisplacedShape),
            }
        }
        let root = names.remove(0);
        Ok(Self {
            root,
            names,
            lines,
            extension,
        })
    }

    /// The spellings naming an extension that reach one each of `files`, the names an ambiguous
    /// citation reaches, for each that has one; empty where the files differ by more than it.
    #[must_use]
    pub fn extension_spellings(&self, root_dir: &Path, files: &[String]) -> Vec<String> {
        files
            .iter()
            .filter_map(|f| self.with_extension_of(f))
            .filter(|r| matches!(r.locate(root_dir), Location::Found(_)))
            .map(|r| r.render())
            .collect()
    }

    /// The index in `names` of the step naming the file.
    fn file_at(&self) -> usize {
        self.names.len() - 1 - usize::from(self.lines.is_none())
    }

    /// This citation naming the extension `file_name` has, where it has one: the spelling a
    /// citation takes when its name alone reaches that file and another beside it.
    #[must_use]
    pub fn with_extension_of(&self, file_name: &str) -> Option<Self> {
        let extension = mockspace_refnames::extension(file_name)?.to_string();
        Some(Self {
            extension: Some(extension),
            ..self.clone()
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
        let file_at = self.file_at();
        for (at, n) in self.names.iter().enumerate() {
            out.push_str("::");
            out.push_str(n);
            if let (true, Some(ext)) = (at == file_at, &self.extension) {
                out.push_str(&format!("<{ext}>"));
            }
        }
        match self.lines {
            Some((lo, hi)) if lo == hi => out.push_str(&format!("::{lo}")),
            Some((lo, hi)) => out.push_str(&format!("::{lo}..{hi}")),
            None => {},
        }
        out
    }

    /// The path the names give, `/` joined, as a document names it, and the file's extension
    /// after it as the citation writes it, where it names one.
    #[must_use]
    pub fn path_text(&self) -> String {
        let mut path = self.names[..= self.file_at()].join("/");
        if let Some(ext) = &self.extension {
            path.push_str(&format!("<{ext}>"));
        }
        path
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
    /// entry's own name as the standard has a host do. An extension narrows the walk to the files
    /// carrying it, and is refused where the names alone reach one file.
    #[must_use]
    pub fn locate(&self, root_dir: &Path) -> Location {
        let file_names = &self.names[..= self.file_at()];
        let mut found = Vec::new();
        walk(root_dir, file_names, self.extension.as_deref(), &mut found);
        found.sort();
        found.dedup();
        match found.len() {
            0 => Location::Missing,
            1 if self.extension.is_some() => {
                let mut alone = Vec::new();
                walk(root_dir, file_names, None, &mut alone);
                if alone.len() == 1 {
                    Location::NeedlessExtension(found.remove(0))
                } else {
                    Location::Found(found.remove(0))
                }
            },
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
    /// The file found, by a citation naming an extension where its names alone reach that file and
    /// no other: a document has one spelling, so the extension is refused (letkajenkka `SPEC.md` 9.1).
    NeedlessExtension(PathBuf),
}

#[path = "refpath_listing.rs"]
mod listing;
use listing::listing;
pub use listing::{CacheScope, cache_scope, dir_reads};

/// Every file under `dir` that `names` reach, the last name being the file's own, and carrying
/// `extension` where one is given.
fn walk(dir: &Path, names: &[String], extension: Option<&str>, out: &mut Vec<PathBuf>) {
    let Some((name, rest)) = names.split_first() else { return };
    for entry in listing(dir).iter() {
        if !mockspace_refnames::entry(&entry.name, entry.is_dir).is_some_and(|f| f == name.as_str()) {
            continue;
        }
        if entry.is_dir {
            walk(&entry.path, rest, extension, out);
        } else if entry.is_file
            && rest.is_empty()
            && extension.is_none_or(|e| mockspace_refnames::extension(&entry.name).is_some_and(|x| x == e))
        {
            out.push(entry.path.clone());
        }
    }
}

/// The line a heading anchor stands at in `file`, and the forge slug a link to it carries. `None`
/// where no heading folds to `name`, and where two do: the citation would reach either, and is no
/// anchor until one is renamed.
#[must_use]
pub fn heading_in(file: &Path, name: &str) -> Option<(usize, String)> {
    let text = fs::read_to_string(file).ok()?;
    let mut fences = mockspace_refnames::Fences::new();
    let mut found = text.lines().enumerate().filter_map(|(i, l)| {
        let trimmed = l.trim_start();
        if !fences.prose(l) || !trimmed.starts_with('#') {
            return None;
        }
        let title = trimmed.trim_start_matches('#').trim();
        (heading_name(title).as_deref() == Some(name)).then(|| (i + 1, forge_slug(title)))
    });
    let one = found.next()?;
    found.next().is_none().then_some(one)
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

#[cfg(test)]
#[path = "refpath_extension_tests.rs"]
mod extension_tests;
