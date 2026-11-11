//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The one reader of a hierarchical reference, `root::step::step`.
//!
//! The syntax is the `letkajenkka` standard and `letkajenkka` is its parser: nothing in this crate
//! splits a reference on its separator. What this module adds is the host's half of the standard:
//! the folding a host does at resolution (`mock::design` reaches `DESIGN.md.tmpl`, and
//! `mock::design::the_four_lanes` reaches the heading a forge anchors as `the-four-lanes`), and
//! the walk down a root that follows from it.

use std::fs;
use std::path::{Path, PathBuf};

use letkajenkka::{Query, Selector};

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
    path.plain().map(Iterator::collect).ok_or(Refusal::NotPlain)
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

/// A name as the standard spells it, from what a filesystem or a document calls itself: case and
/// hyphens folded. `None` where no folding makes an identifier of it (it opens on a digit, holds a
/// dot, or holds a character outside the set).
#[must_use]
pub fn fold(name: &str) -> Option<String> {
    letkajenkka::ident::fold(name).map(|f| f.to_string())
}

/// What a file or a directory is called in a reference: the whole name of a directory, and the name
/// up to its first dot for a file, so the extensions are never written.
fn fold_entry(name: &str, is_dir: bool) -> Option<String> {
    if is_dir {
        fold(name)
    } else {
        fold(name.split('.').next().unwrap_or_default())
    }
}

/// A heading as GitHub anchors it: lowercase, everything deleted but letters, numbers, `_`, `-` and
/// the space, and each remaining space written as one hyphen. Runs of spaces are not collapsed and
/// nothing is trimmed.
#[must_use]
pub fn forge_slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    for c in title.chars() {
        if c == ' ' {
            out.push('-');
        } else if c.is_alphanumeric() || c == '-' || c == '_' {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// A heading as a reference names it: its forge slug, folded.
#[must_use]
pub fn heading_name(title: &str) -> Option<String> {
    fold(&forge_slug(title))
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

/// Every file under `dir` that `names` reach, the last name being the file's own.
fn walk(dir: &Path, names: &[String], out: &mut Vec<PathBuf>) {
    let Some((name, rest)) = names.split_first() else { return };
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_dir = path.is_dir();
        let is_file = path.is_file();
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if fold_entry(&file_name, is_dir).as_deref() != Some(name.as_str()) {
            continue;
        }
        if is_dir {
            walk(&path, rest, out);
        } else if is_file && rest.is_empty() {
            out.push(path);
        }
    }
}

/// The line a heading anchor stands at in `file`, and the forge slug a link to it carries.
#[must_use]
pub fn heading_in(file: &Path, name: &str) -> Option<(usize, String)> {
    let text = fs::read_to_string(file).ok()?;
    let mut fenced = false;
    text.lines().enumerate().find_map(|(i, l)| {
        let trimmed = l.trim_start();
        if trimmed.starts_with("```") {
            fenced = !fenced;
        }
        if fenced || !trimmed.starts_with('#') {
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
