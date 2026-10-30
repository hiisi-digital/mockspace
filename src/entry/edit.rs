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
    /// Which gate's severities decide what blocks.
    pub mode:    LintMode,
}

/// The file as it would read once the edit is made, or `None` when the edit names
/// text the file does not hold: the tool refuses that edit itself, and a verdict on
/// a file that will not exist would be a claim about nothing.
fn after(req: &Request, on_disk: impl FnOnce() -> Option<String>) -> Option<String> {
    let Some(old) = &req.replacing else {
        return Some(req.text.clone());
    };
    let now = on_disk()?;
    if old.is_empty() || !now.contains(old.as_str()) {
        return None;
    }
    Some(if req.all {
        now.replace(old.as_str(), &req.text)
    } else {
        now.replacen(old.as_str(), &req.text, 1)
    })
}

/// Ask. Exit success when the lint blocks nothing in `req.file`, failure with the
/// findings printed when it does, and 2 when the lint named is not one that runs
/// here, so a hook naming a lint that is gone says so instead of passing everything.
pub(crate) fn run(cfg: &Config, pack: &LintPack, req: &Request) -> ExitCode {
    let known = mockspace_lint_rules::all_lints()
        .iter()
        .any(|l| l.name() == req.lint)
        || pack.crate_lints.iter().any(|l| l.name() == req.lint);
    if !known {
        eprintln!(
            "mock check-edit: no per-crate lint answers to `{}`. `mock lints` lists what does.",
            req.lint
        );
        return ExitCode::from(2);
    }

    let Some(text) = after(req, || std::fs::read_to_string(req.file).ok()) else {
        return ExitCode::SUCCESS;
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
        req.file,
        &text,
    ) else {
        // In no crate of this repository, so no lint here is about it.
        return ExitCode::SUCCESS;
    };

    let blocking: Vec<_> = found
        .iter()
        .filter(|e| e.severity.effective(req.mode) == Level::Error)
        .collect();
    for e in &blocking {
        eprintln!("{e}");
    }
    if blocking.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
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
        assert_eq!(after(&req("X", Some("a"), false), disk).as_deref(), Some("X b a"));
        assert_eq!(after(&req("X", Some("a"), true), disk).as_deref(), Some("X b X"));
    }

    #[test]
    fn an_edit_naming_text_the_file_lacks_has_no_after() {
        assert_eq!(after(&req("X", Some("zzz"), false), || Some("a".into())), None);
        assert_eq!(after(&req("X", Some("a"), false), || None), None);
        assert_eq!(after(&req("X", Some(""), false), || Some("a".into())), None);
    }
}
