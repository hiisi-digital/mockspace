//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The only place the spelling of a reference before the `letkajenkka` standard is still read.
//!
//! The spelling allowed uppercase and file extensions in a segment, wrote a heading as
//! `::#heading`, tolerated whitespace beside a separator, and named a heading by a slug that
//! hyphenated every run of punctuation. Nothing resolves it any more; this module reads it so that
//! `cargo mock migrate references-to-letkajenkka` can write the new spelling.
//!
//! A rewrite is made only where the new spelling reaches the file, and the line, that the old one
//! did. What cannot be shown to is left as written and named.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::refpath::{self, Anchor, FileRef, Location};
use crate::{CRATE_ROOT, REGISTRY_ROOT};

/// What a rewrite is read against.
pub struct Ctx<'a> {
    pub repo_root:  &'a Path,
    /// Every root, builtin ones included, as its directory from the repository root.
    pub roots:      &'a BTreeMap<String, String>,
    pub namespaces: &'a BTreeSet<&'a str>,
}

/// What became of one reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Already in the new spelling, or no reference at all.
    Same,
    Rewritten(String),
    /// Not rewritten, and why.
    Left(String),
}

/// One reference of a text, where it stands, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The bytes of the text it was read from that the reference takes: the whole placeholder,
    /// braces included, the item of a list, or the string of a config that holds it.
    pub span:    Range<usize>,
    /// The reference as written: the whole placeholder, braces included, or the item.
    pub token:   String,
    /// The expression inside it.
    pub old:     String,
    pub outcome: Outcome,
}

impl Found {
    /// The token as the rewrite writes it, keeping the spacing inside its braces; `None` unless
    /// it is rewritten.
    #[must_use]
    pub fn rewritten(&self) -> Option<String> {
        match &self.outcome {
            Outcome::Rewritten(new) => Some(self.token.replacen(self.old.trim(), new, 1)),
            _ => None,
        }
    }
}

/// Every placeholder of `text` outside a code fence that is not in the new spelling already.
#[must_use]
pub fn in_text(text: &str, ctx: &Ctx<'_>) -> Vec<Found> {
    let mut out = Vec::new();
    let mut fenced = false;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        let start = at;
        at += line.len();
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for (span, old) in crate::placeholder_spans(line) {
            let outcome = expr(old, ctx);
            if outcome != Outcome::Same {
                out.push(Found {
                    span: start + span.start .. start + span.end,
                    token: line[span].to_string(),
                    old: old.to_string(),
                    outcome,
                });
            }
        }
    }
    out
}

/// Every item of a list of references, a field declared to hold them, that is not in the new
/// spelling already. An item holding a placeholder is read as the placeholder, by [`in_text`].
#[must_use]
pub fn in_list(value: &str, ctx: &Ctx<'_>) -> Vec<Found> {
    refpath::list(value)
        .filter(|item| !item.contains("{{") && !item.contains("}}"))
        .filter_map(|item| {
            let outcome = expr(item, ctx);
            // an item is a slice of the value, so its offset is where it stands in it
            let start = item.as_ptr() as usize - value.as_ptr() as usize;
            (outcome != Outcome::Same).then(|| Found {
                span: start .. start + item.len(),
                token: item.to_string(),
                old: item.to_string(),
                outcome,
            })
        })
        .collect()
}

/// The text with each rewritten reference written in the new spelling at the span it was found
/// at, and no other byte of the text touched: a reference spelled alike inside a code fence, or as
/// the start of a longer one, stays as written.
#[must_use]
pub fn apply(text: &str, found: &[Found]) -> String {
    let mut edits: Vec<(Range<usize>, String)> =
        found.iter().filter_map(|f| f.rewritten().map(|new| (f.span.clone(), new))).collect();
    edits.sort_by_key(|(span, _)| span.start);
    assert!(edits.windows(2).all(|w| w[0].0.end <= w[1].0.start), "two rewrites of one text overlap: {edits:?}");
    let mut out = text.to_string();
    for (span, new) in edits.into_iter().rev() {
        out.replace_range(span, &new);
    }
    out
}

/// A preset shorthand of `extends` in `mockspace.toml`: `<host>::<name>`, each folded into the
/// standard's character set, so the hyphen of `stack-lints::no-heap` becomes the underscore.
#[must_use]
pub fn preset_shorthand(old: &str) -> Outcome {
    let s = old.trim();
    let parts: Vec<&str> = s.split("::").map(str::trim).collect();
    if parts.len() != 2 {
        return Outcome::Left("a preset is a host and a name, `<host>::<name>`".to_string());
    }
    let mut folded = Vec::new();
    for p in &parts {
        match mockspace_refnames::fold(p) {
            Some(n) => folded.push(n.to_string()),
            None => {
                return Outcome::Left(format!(
                    "`{p}` has no name under the standard (it opens on a digit or holds a character outside a-z, 0-9, _ and -)"
                ));
            },
        }
    }
    same_or(old, folded.join("::"))
}

fn expr(old: &str, ctx: &Ctx<'_>) -> Outcome {
    let s = old.trim();
    if s.is_empty() {
        return Outcome::Same;
    }
    for op in ["pathof", "refsto", "sourcesof"] {
        let Some(rest) = s.strip_prefix(op).and_then(|r| r.strip_prefix('(')) else { continue };
        let Some(close) = closing(rest) else {
            return Outcome::Left(format!("`{op}(` is never closed"));
        };
        let (inner, chain) = (&rest[.. close], &rest[close + 1 ..]);
        return match expr(inner, ctx) {
            Outcome::Left(why) => Outcome::Left(why),
            Outcome::Same | Outcome::Rewritten(_) if inner.trim().is_empty() => Outcome::Left(format!("`{op}` has no argument")),
            Outcome::Same => same_or(s, format!("{op}({}){chain}", inner.trim())),
            Outcome::Rewritten(new) => Outcome::Rewritten(format!("{op}({new}){chain}")),
        };
    }
    let parts: Vec<&str> = s.split("::").map(str::trim).collect();
    if parts.len() == 1 {
        return Outcome::Same;
    }
    let joined = parts.join("::");
    if refpath::names(&joined).is_ok() || FileRef::read(&joined).is_ok() {
        return same_or(s, joined);
    }
    if parts[0] == CRATE_ROOT && parts.len() == 2 {
        return match mockspace_refnames::fold(parts[1]) {
            Some(name) => same_or(s, format!("{CRATE_ROOT}::{name}")),
            None => Outcome::Left(format!("crate `{}` has no name under the standard", parts[1])),
        };
    }
    if parts[0] == REGISTRY_ROOT || ctx.namespaces.contains(parts[0]) {
        return match refpath::names(&joined) {
            Err(why) => Outcome::Left(why.say(&joined)),
            Ok(_) => Outcome::Same,
        };
    }
    citation(&parts, ctx)
}

/// The index of the parenthesis closing the one `rest` follows.
fn closing(rest: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            },
            _ => {},
        }
    }
    None
}

fn same_or(old: &str, new: String) -> Outcome {
    if old == new { Outcome::Same } else { Outcome::Rewritten(new) }
}

/// The old citation, `root::seg::...::anchor`, written the new way where that reaches the same
/// file and line.
fn citation(parts: &[&str], ctx: &Ctx<'_>) -> Outcome {
    let (root, anchor) = (parts[0], parts[parts.len() - 1]);
    let Some(rel) = ctx.roots.get(root) else {
        return Outcome::Left(format!("`{root}` is no declared root, so nothing says what this reaches"));
    };
    if parts.len() < 3 {
        return Outcome::Left("a citation names a root, a path and an anchor".to_string());
    }
    let root_dir = ctx.repo_root.join(rel);
    let path = parts[1 .. parts.len() - 1].join("/");
    let file = match legacy_locate(&root_dir, &path) {
        Ok(f) => f,
        Err(why) => return Outcome::Left(why),
    };
    // the names the file has under the standard, from the root down
    let Ok(under) = file.strip_prefix(&root_dir) else {
        return Outcome::Left(format!("`{}` is not under root `{root}`", file.display()));
    };
    let comps: Vec<String> = under.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let mut names = Vec::new();
    for (i, c) in comps.iter().enumerate() {
        let last = i + 1 == comps.len();
        let folded = if last { mockspace_refnames::fold(c.split('.').next().unwrap_or_default()) } else { mockspace_refnames::fold(c) };
        match folded {
            Some(n) => names.push(n.to_string()),
            None => {
                return Outcome::Left(format!(
                    "`{c}` has no name under the standard (it opens on a digit, or holds a dot or a character \
                     outside a-z, 0-9 and _), so the file cannot be cited"
                ));
            },
        }
    }
    let (tail, line) = match anchor.strip_prefix('#') {
        Some(h) if !h.is_empty() => match heading_title(&file, h) {
            Some((title, at)) => match refpath::heading_name(&title) {
                Some(name) => (Some(name), Some(at)),
                None => {
                    return Outcome::Left(format!(
                        "heading `{title}` has no name under the standard (it opens on a digit, or holds a \
                         character outside a-z, 0-9 and _)"
                    ));
                },
            },
            None => return Outcome::Left(format!("heading `{h}` is in no heading of {}, so it resolves to nothing today", file.display())),
        },
        Some(_) => return Outcome::Left("an empty heading anchor".to_string()),
        None => match anchor.parse::<u64>() {
            Ok(n) if n > 0 && !anchor.starts_with('0') => {
                names.push(n.to_string());
                (None, None)
            },
            _ => return Outcome::Left(format!("`{anchor}` is neither a heading nor a line")),
        },
    };
    if let Some(name) = &tail {
        names.push(name.clone());
    }
    let new = format!("{root}::{}", names.join("::"));
    let Ok(read) = FileRef::read(&new) else {
        return Outcome::Left(format!("`{new}` is not a reference under the standard"));
    };
    match read.locate(&root_dir) {
        Location::Found(f) if f == file => {},
        Location::Found(f) => return Outcome::Left(format!("`{new}` would reach {}, not {}", f.display(), file.display())),
        Location::Missing => return Outcome::Left(format!("`{new}` would reach no file")),
        Location::Ambiguous(all) => {
            return Outcome::Left(format!("`{new}` would reach several files ({}), and no spelling tells them apart", all.join(", ")));
        },
    }
    if let (Some(at), Anchor::Heading(h)) = (line, read.anchor()) {
        if refpath::heading_in(&file, &h).map(|(l, _)| l) != Some(at) {
            return Outcome::Left(format!("`{new}` would reach another heading than line {at} of {}", file.display()));
        }
    }
    Outcome::Rewritten(new)
}

/// The file the old reader reached for `path` under `root_dir`: the file itself, or the one file
/// whose name is `path` with an extension after it.
fn legacy_locate(root_dir: &Path, path: &str) -> Result<PathBuf, String> {
    let exact = root_dir.join(path);
    if exact.is_file() {
        return Ok(exact);
    }
    let target = Path::new(path);
    let dir = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => root_dir.join(p),
        _ => root_dir.to_path_buf(),
    };
    let stem = target.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Err(format!("`{path}` matches no file under {}", root_dir.display()));
    };
    let mut hits: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n == stem || n.starts_with(&format!("{stem}."))
                })
        })
        .collect();
    hits.sort();
    match hits.len() {
        0 => Err(format!("`{path}` matches no file under {}", root_dir.display())),
        1 => Ok(hits.remove(0)),
        _ => Err(format!("`{path}` matches several files, so it was ambiguous before and stays so")),
    }
}

/// The heading `slug` names in `file`, as the old reader matched it by either slug it accepted, and
/// the line it stands at.
fn heading_title(file: &Path, slug: &str) -> Option<(String, usize)> {
    let text = fs::read_to_string(file).ok()?;
    text.lines().enumerate().find_map(|(i, l)| {
        let t = l.trim_start();
        if !t.starts_with('#') {
            return None;
        }
        let title = t.trim_start_matches('#').trim();
        (project_slug(title) == slug || refpath::forge_slug(title) == slug).then(|| (title.to_string(), i + 1))
    })
}

/// The slug the old reader generated: every run of non-alphanumerics a hyphen.
fn project_slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut last_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
#[path = "legacy_refs_tests.rs"]
mod tests;
