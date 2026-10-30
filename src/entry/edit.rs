//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `check-edit` entry: ask one crate lint whether an edit that has not happened
//! yet would leave its file clean.
//!
//! This is how an agent hook named after a lint reaches the lint. The hook used to
//! carry its own substring test for what `forbidden-imports` forbids, which refused
//! a comment that mentioned a forbidden type and passed nothing the lint would not
//! have, so the same text got two verdicts depending on whether it was typed into an
//! editor or checked at commit. A hook named after a lint now hands the text to the
//! lint, through the pass the gate gives every crate, and the verdict is the lint's.
//!
//! Whatever a hook judges beyond what a lint judges is not this command's business:
//! it is a hook of its own, under its own name.

use std::path::Path;
use std::process::ExitCode;

use mockspace_lint_rules::{Level, LintMode, LintPack};

use crate::config::Config;

/// What to ask, of which edit.
pub(crate) struct Request<'a> {
    /// The per-crate lint to ask.
    pub lint:    &'a str,
    /// The file the edit lands in, absolute.
    pub file:    &'a Path,
    /// Text that replaces `replacing` in the file, or the file's whole new text
    /// when `replacing` is `None`.
    pub text:    String,
    /// The text an edit takes out, for the editing tools; absent for a write.
    pub replacing: Option<String>,
    /// Whether every occurrence of `replacing` goes, not only the first.
    pub all:     bool,
    /// A sequence of replacements (old, new, every occurrence) for a multi-edit tool, applied
    /// in order; when non-empty it is the edit and `replacing`, `text` and `all` are unused.
    pub edits:   Vec<(String, String, bool)>,
    /// Which gate's severities decide what blocks.
    pub mode:    LintMode,
}

/// `old` replaced by `new` in `now`, or `None` when the tool would refuse the edit: `old`
/// is empty, is not there, or is there more than once without `all`.
fn replaced(now: &str, old: &str, new: &str, all: bool) -> Option<String> {
    if old.is_empty() {
        return None;
    }
    match now.matches(old).count() {
        0 => None,
        1 => Some(now.replacen(old, new, 1)),
        _ if all => Some(now.replace(old, new)),
        _ => None,
    }
}

/// The file as it would read once the edit is made, or `None` when the edit names
/// text the file does not hold or holds ambiguously: the tool refuses that edit
/// itself, and a verdict on a file that will not exist would be a claim about nothing.
fn after(req: &Request, on_disk: impl FnOnce() -> Option<String>) -> Option<String> {
    if !req.edits.is_empty() {
        // A sequence of edits, each laid over what the one before left.
        // A first edit with an empty old text is how the tool creates a file: its new text
        // is the whole file, and the tool refuses it when the file is already there.
        let (mut now, rest) = match req.edits.split_first() {
            Some(((old, new, _), rest)) if old.is_empty() => {
                if on_disk().is_some() {
                    return None;
                }
                (new.clone(), rest)
            },
            _ => (on_disk()?, req.edits.as_slice()),
        };
        for (old, new, all) in rest {
            now = replaced(&now, old, new, *all)?;
        }
        return Some(now);
    }
    let Some(old) = &req.replacing else {
        return Some(req.text.clone());
    };
    replaced(&on_disk()?, old, &req.text, req.all)
}

/// `path` with its `.` and `..` components resolved lexically, or `None` when it is not
/// absolute. Symlinks are not looked at here; [`through_directory_links`] follows them.
fn normalised(path: &Path) -> Option<std::path::PathBuf> {
    use std::path::Component;
    if !path.is_absolute() {
        return None;
    }
    let mut out = std::path::PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {},
            Component::ParentDir => {
                // Against where the prefix really is, as the filesystem reads `..`.
                if let Ok(real) = out.canonicalize() {
                    out = real;
                }
                out.pop();
            },
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// A directory as the gate knows it: what it really is, and where it sits.
type GateDir = (std::path::PathBuf, std::path::PathBuf);

/// `dir` and every directory under it, followed through links the way the gate's own walk
/// follows them. A directory reached by several places is recorded at each, since the gate
/// lints its files in every crate that reaches them; only a directory that is its own
/// ancestor, a cycle, is not entered again.
fn walk_dirs(dir: &Path, out: &mut Vec<GateDir>, ancestors: &mut Vec<std::path::PathBuf>) {
    let Ok(real) = dir.canonicalize() else { return };
    if ancestors.contains(&real) {
        return;
    }
    out.push((real.clone(), dir.to_path_buf()));
    ancestors.push(real);
    if let Ok(read) = std::fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_dirs(&path, out, ancestors);
            } else if entry.file_type().is_ok_and(|t| t.is_symlink()) {
                // A file linked into a crate is read where it is linked, so it is a place for
                // its target too.
                if let Ok(target) = path.canonicalize() {
                    out.push((target, path));
                }
            }
        }
    }
    ancestors.pop();
}

/// Every directory the gate reads Rust from: the source directories, each crate directory
/// beneath them that is a real directory (discovery does not follow a link there), and
/// everything under a crate's `src`.
fn gate_directories(src_dirs: &[std::path::PathBuf]) -> Vec<GateDir> {
    let mut out = Vec::new();
    for root in src_dirs {
        if let Ok(real) = root.canonicalize() {
            out.push((real, root.to_path_buf()));
        }
        let Ok(read) = std::fs::read_dir(root) else { continue };
        for entry in read.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())) {
            if let Ok(real) = entry.path().canonicalize() {
                out.push((real, entry.path()));
            }
            walk_dirs(&entry.path().join("src"), &mut out, &mut Vec::new());
        }
    }
    out
}

/// `path` spelled every way the gate reaches the same file: its directory taken to each
/// place in a crate that really is that directory, and its last component kept.
///
/// An edit written through a link, to a source directory or into a crate's linked `src` or
/// module directory, names no crate when read lexically and would pass unjudged, while the
/// gate lints the file at its place in every crate that reaches it. The deepest directory of
/// the path that exists is resolved, the rest is appended as spelled, since the file or its
/// directories may be what the edit creates. A path that reaches none of the gate's
/// directories comes back as it was.
fn through_directory_links(path: &Path, src_dirs: &[std::path::PathBuf]) -> Vec<std::path::PathBuf> {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return vec![path.to_path_buf()];
    };
    let mut rest = Vec::new();
    let mut probe = parent;
    let real = loop {
        match probe.canonicalize() {
            Ok(real) => break real,
            Err(_) => match (probe.parent(), probe.file_name()) {
                (Some(up), Some(last)) => {
                    rest.push(last.to_os_string());
                    probe = up;
                },
                _ => return vec![path.to_path_buf()],
            },
        }
    };
    let gate = gate_directories(src_dirs);
    let mut places: Vec<_> = gate
        .iter()
        .filter(|(r, _)| *r == real)
        .map(|(_, place)| {
            let mut out = place.clone();
            out.extend(rest.iter().rev());
            out.join(name)
        })
        .collect();
    // The file itself may be what a crate links to: an edit at its real path is the crate's
    // file at every place it is linked.
    if rest.is_empty() {
        if let Ok(real_file) = path.canonicalize() {
            places.extend(gate.iter().filter(|(r, p)| *r == real_file && p.is_file()).map(|(_, p)| p.clone()));
        }
    }
    if places.is_empty() { vec![path.to_path_buf()] } else { places }
}

/// Ask. Exit success when the lint blocks nothing in `req.file`, failure with the
/// findings printed when it does, and 2 when the lint named is not one that runs
/// here, so a hook naming a lint that is gone says so instead of passing everything.
pub(crate) fn run(cfg: &Config, pack: &LintPack, req: &Request) -> ExitCode {
    ExitCode::from(exit_of(cfg, pack, req))
}

/// The status `run` exits with.
fn exit_of(cfg: &Config, pack: &LintPack, req: &Request) -> u8 {
    let builtins = mockspace_lint_rules::all_lints();
    let named = |n: &str| {
        builtins
            .iter()
            .find(|l| l.name() == n)
            .map(|l| l.per_file())
            .or_else(|| pack.crate_lints.iter().find(|l| l.name() == n).map(|l| l.per_file()))
    };
    match named(req.lint) {
        None => {
            eprintln!(
                "mock check-edit: no per-crate lint answers to `{}`. `mock lints` lists what does.",
                req.lint
            );
            return 2;
        },
        Some(false) => {
            eprintln!(
                "mock check-edit: `{}` judges the crate as a whole and not one file, so an edit has \
                 no verdict from it.",
                req.lint
            );
            return 2;
        },
        Some(true) => {},
    }

    // A lint reads a file by its place under a source directory, so a path that only
    // reaches it by `..` or a relative spelling would name no crate and pass. It is
    // normalised lexically here, and a relative one is refused.
    let Some(file) = normalised(req.file) else {
        eprintln!(
            "mock check-edit: `{}` is not an absolute path, so no crate of this repository can be told from it.",
            req.file.display()
        );
        return 2;
    };

    // The source directories as they really are, and the file reached through a link to one
    // taken to the real directory, so both sides of the comparison are real paths.
    let src_dirs: Vec<std::path::PathBuf> = cfg
        .src_dirs
        .iter()
        .map(|d| d.canonicalize().unwrap_or_else(|_| d.clone()))
        .collect();
    let places = through_directory_links(&file, &src_dirs);

    let crates = crate::parse::discover_crates_in(&src_dirs, &cfg.crate_prefix);
    // The gate lints a file once in every crate that reaches it, so the edit is refused when
    // any of those places would block.
    let mut blocked = false;
    for file in &places {
        let Some(text) = after(req, || std::fs::read_to_string(file).ok()) else {
            continue;
        };
        let Some(found) = crate::lint::lint_edit(
            &crates,
            &src_dirs,
            &cfg.mock_dir,
            &cfg.proc_macro_crates,
            cfg.lint_proc_macro_source,
            &cfg.crate_prefix,
            &cfg.lint_overrides,
            &cfg.primitive_introductions,
            pack,
            req.lint,
            file,
            &text,
        ) else {
            // In no crate of this repository, so no lint here is about it.
            continue;
        };
        for e in found.iter().filter(|e| e.severity.effective(req.mode) == Level::Error) {
            eprintln!("{e}");
            blocked = true;
        }
    }
    u8::from(blocked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str, replacing: Option<&str>, all: bool) -> Request<'static> {
        Request {
            lint: "forbidden-imports",
            file: Path::new("/x/y.rs"),
            text: text.to_string(),
            replacing: replacing.map(String::from),
            all,
            edits: Vec::new(),
            mode: LintMode::Commit,
        }
    }

    #[test]
    fn a_write_is_the_text_itself() {
        let r = req("new", None, false);
        assert_eq!(after(&r, || panic!("a write reads no file")).as_deref(), Some("new"));
    }

    #[test]
    fn an_edit_replaces_the_first_occurrence_unless_all() {
        let disk = || Some("a b a".to_string());
        assert_eq!(after(&req("X", Some("b"), false), disk).as_deref(), Some("a X a"));
        // Ambiguous without `all`, as the tool itself refuses it.
        assert_eq!(after(&req("X", Some("a"), false), disk), None);
        assert_eq!(after(&req("X", Some("a"), true), disk).as_deref(), Some("X b X"));
    }

    #[test]
    fn a_sequence_of_edits_is_laid_over_what_each_one_left() {
        let mut r = req("", None, false);
        r.edits = vec![("a".into(), "b".into(), false), ("b c".into(), "X".into(), false)];
        assert_eq!(after(&r, || Some("a c".into())).as_deref(), Some("X"));
        // One the tool would refuse refuses the sequence.
        r.edits.push(("zzz".into(), "y".into(), false));
        assert_eq!(after(&r, || Some("a c".into())), None);
    }

    #[test]
    fn a_multi_edit_that_creates_the_file_judges_its_new_text_whole() {
        let mut r = req("", None, false);
        r.edits = vec![("".into(), "use a;".into(), false), ("a".into(), "b".into(), false)];
        assert_eq!(after(&r, || None).as_deref(), Some("use b;"));
        // The tool refuses creating what is already there.
        assert_eq!(after(&r, || Some("x".into())), None);
    }

    #[test]
    fn an_edit_naming_text_the_file_lacks_has_no_after() {
        assert_eq!(after(&req("X", Some("zzz"), false), || Some("a".into())), None);
        assert_eq!(after(&req("X", Some("a"), false), || None), None);
        assert_eq!(after(&req("X", Some(""), false), || Some("a".into())), None);
    }
}

/// The exit status `run` gives an empty edit of `file` by `lint`, for the render tests,
/// which have no process to read it from.
#[cfg(test)]
pub(crate) fn run_for_test(cfg: &Config, lint: &str, file: &Path) -> u8 {
    run_for_test_at(cfg, lint, file, LintMode::Commit)
}

/// As [`run_for_test`], under the severities of `mode`.
#[cfg(test)]
pub(crate) fn run_for_test_at(cfg: &Config, lint: &str, file: &Path, mode: LintMode) -> u8 {
    exit_of(cfg, &LintPack::default(), &Request {
        lint,
        file,
        text: std::fs::read_to_string(file).unwrap_or_default(),
        replacing: None,
        all: false,
        edits: Vec::new(),
        mode,
    })
}
