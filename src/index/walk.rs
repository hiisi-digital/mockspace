//! Finding the files the index reads.
//!
//! The roots are named by the caller, never discovered: a rule walking everything under the
//! mock directory would index the registry and the crates' designs and hand them back as research.
//! A root that does not exist is reported, not skipped in silence.

use std::collections::BTreeMap;
use std::path::PathBuf;

use mockspace_lint_rules::DocClass;

/// Directories never descended into, under any root.
const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules"];

/// Extensions worth reading. Everything else is counted and left out: images, tables of numbers
/// and rendered pages are most of a research tree by file count and carry no sentence a question
/// could match.
const KEEP_EXT: &[&str] = &["md", "txt", "toml", "rs", "cjs", "js", "sh", "wgsl", "slang"];

/// The extensions that make a file under a research root a sketch rather than a note.
const SKETCH_EXT: &[&str] = &["rs", "cjs", "js", "sh", "wgsl", "slang"];

/// One root to read.
#[derive(Clone, Debug)]
pub struct Source {
    /// Where it is on disk.
    pub dir:   PathBuf,
    /// What a path below it is shown under, such as `research` or `design_rounds`.
    pub shown: String,
    /// Who wrote what is under it.
    pub class: DocClass,
    /// Where set, a file whose text never names this is held out as about something else. A
    /// human's captures cover every project at once, and a capture about another one would
    /// otherwise outrank this project's own notes at a human's weight.
    pub about: Option<String>,
}

/// One file found.
pub struct Found {
    pub shown: String,
    pub file:  PathBuf,
    pub class: DocClass,
    pub about: Option<String>,
}

/// What a walk saw.
#[derive(Default)]
pub struct Walked {
    pub files:      Vec<Found>,
    /// Roots that are not on disk.
    pub missing:    Vec<String>,
    /// Files left out for their extension, by extension.
    pub skipped:    BTreeMap<String, usize>,
    /// Directories that could not be listed.
    pub unreadable: usize,
}

/// Every readable file under every source.
#[must_use]
pub fn walk(sources: &[Source]) -> Walked {
    let mut out = Walked::default();
    for s in sources {
        if !s.dir.is_dir() {
            out.missing.push(s.shown.clone());
            continue;
        }
        let mut stack = vec![s.dir.clone()];
        while let Some(d) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&d) else {
                out.unreadable += 1;
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() {
                    if !SKIP_DIRS.contains(&name.as_str()) {
                        stack.push(p);
                    }
                    continue;
                }
                let ext = p
                    .extension()
                    .and_then(|x| x.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if !KEEP_EXT.contains(&ext.as_str()) {
                    let key = if ext.is_empty() { "(none)".to_string() } else { ext };
                    *out.skipped.entry(key).or_default() += 1;
                    continue;
                }
                let rel = p.strip_prefix(&s.dir).unwrap_or(&p);
                let class = if s.class == DocClass::Research && SKETCH_EXT.contains(&ext.as_str())
                {
                    DocClass::Sketch
                } else {
                    s.class
                };
                out.files.push(Found {
                    shown: format!("{}/{}", s.shown, rel.to_string_lossy().replace('\\', "/")),
                    file: p,
                    class,
                    about: s.about.clone(),
                });
            }
        }
    }
    out.files.sort_by(|a, b| a.shown.cmp(&b.shown));
    out
}
