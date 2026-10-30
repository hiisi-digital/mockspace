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
        let mut now = on_disk()?;
        for (old, new, all) in &req.edits {
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
/// absolute. A symlink is not followed: a source directory reached through one is the
/// caller's spelling to fix.
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
                out.pop();
            },
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
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

    let Some(text) = after(req, || std::fs::read_to_string(&file).ok()) else {
        return 0;
    };

    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    let Some(found) = crate::lint::lint_edit(
        &crates,
        &cfg.src_dirs,
        &cfg.mock_dir,
        &cfg.proc_macro_crates,
        cfg.lint_proc_macro_source,
        &cfg.crate_prefix,
        &cfg.lint_overrides,
        &cfg.primitive_introductions,
        pack,
        req.lint,
        &file,
        &text,
    ) else {
        // In no crate of this repository, so no lint here is about it.
        return 0;
    };

    let blocking: Vec<_> = found
        .iter()
        .filter(|e| e.severity.effective(req.mode) == Level::Error)
        .collect();
    for e in &blocking {
        eprintln!("{e}");
    }
    if blocking.is_empty() { 0 } else { 1 }
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
