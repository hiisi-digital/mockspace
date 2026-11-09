//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Makers whose products are ignored by git run ahead of the checks that read them.
//!
//! A tool declaring [`Purpose::Make`] normally runs only when invoked. A product the repository does not commit has
//! no other way to exist: a fresh checkout lacks it, and every lint, test and tool that reads it by path would
//! read nothing. So before `mock test` and before the lints, each maker whose every declared write is a fixed path
//! git ignores is run, when a product is missing or an input it declares is newer than the oldest product.
//!
//! A maker with a product git tracks is never run here: its product is committed, and running it is the author's
//! act. A write that is a pattern or names an argument cannot be told ignored from the declaration, so it also
//! keeps its maker out.

use std::time::SystemTime;

/// Whether a maker declaring `writes` is run ahead of the checks: it declares some, each is a fixed path, and git
/// ignores every one of them.
pub(crate) fn runs_ahead(writes: &[&str], ignored: &dyn Fn(&str) -> bool) -> bool {
    !writes.is_empty()
        && writes
            .iter()
            .all(|w| !w.contains(['*', '?', '[', '{']) && ignored(w))
}

/// Whether the products are stale: one is missing (`None`), or an input is newer than the oldest product.
pub(crate) fn stale(products: &[Option<SystemTime>], inputs: &[SystemTime]) -> bool {
    let Some(oldest) = products
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()
        .and_then(|p| p.into_iter().min())
    else {
        return true;
    };
    inputs.iter().any(|i| *i > oldest)
}

use std::path::Path;
use std::process::{Command, ExitCode};

use mockspace_lint_rules::LintPack;
use mockspace_lint_rules::path_filter::glob_match_anchored;
use mockspace_lint_rules::tool::Purpose;

use crate::config::Config;

/// Whether git ignores `path` in the tree at `root`. A failure to ask is "not ignored", which keeps the maker out.
fn git_ignores(root: &Path, path: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-q", "--", path])
        .status()
        .is_ok_and(|s| s.success())
}

fn modified(root: &Path, path: &str) -> Option<SystemTime> {
    std::fs::metadata(root.join(path))
        .and_then(|m| m.modified())
        .ok()
}

/// The modification times of the files in the tree matching any of `patterns`, tracked or not and ignored or not.
fn input_times(root: &Path, patterns: &[&str]) -> Vec<SystemTime> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|f| patterns.iter().any(|p| glob_match_anchored(p, f)))
        .filter_map(|f| modified(root, f))
        .collect()
}

/// Runs each maker of the pack that runs ahead and is stale, before the checks. Returns the failing exit code of the
/// first that fails, since a check reading what it did not make would report on nothing.
pub(crate) fn run(cfg: &Config, pack: &LintPack) -> Result<(), ExitCode> {
    for tool in &pack.tools {
        let Purpose::Make {
            writes,
            ..
        } = tool.purpose()
        else {
            continue;
        };
        let root = &cfg.repo_root;
        if !runs_ahead(writes, &|w| git_ignores(root, w)) {
            continue;
        }
        let products: Vec<_> = writes.iter().map(|w| modified(root, w)).collect();
        if !stale(&products, &input_times(root, tool.inputs())) {
            continue;
        }
        eprintln!(
            "--- make: `{}` writes what is not committed, running it before the checks ---",
            tool.name()
        );
        let code = super::tool::run(cfg, pack, tool.name(), &[], false);
        if code != ExitCode::SUCCESS {
            return Err(code);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::{runs_ahead, stale};

    fn at(s: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(s)
    }

    #[test]
    fn a_maker_of_ignored_fixed_paths_runs_ahead() {
        assert!(runs_ahead(&["gen/a.slang", "gen/b.slang"], &|_| true));
    }

    /// The case that must fail: one product git tracks keeps the maker out, since its product is committed.
    #[test]
    fn a_maker_with_a_tracked_product_stays_out() {
        assert!(!runs_ahead(&["gen/a.slang", "CHANGELOG.md"], &|p| {
            p.starts_with("gen/")
        }));
    }

    #[test]
    fn a_pattern_or_an_argument_keeps_its_maker_out() {
        assert!(!runs_ahead(&["gen/**"], &|_| true));
        assert!(!runs_ahead(&["gen/*.slang"], &|_| true));
        assert!(!runs_ahead(&["{output}.rs"], &|_| true));
    }

    #[test]
    fn a_maker_declaring_nothing_stays_out() {
        assert!(!runs_ahead(&[], &|_| true));
    }

    #[test]
    fn a_missing_product_is_stale() {
        assert!(stale(&[Some(at(5)), None], &[]));
    }

    #[test]
    fn a_newer_input_is_stale_and_an_older_one_is_not() {
        assert!(stale(&[Some(at(5)), Some(at(9))], &[at(7)]));
        assert!(!stale(&[Some(at(5)), Some(at(9))], &[at(4)]));
    }

    #[test]
    fn products_with_no_inputs_declared_are_current_while_present() {
        assert!(!stale(&[Some(at(5))], &[]));
    }
}
