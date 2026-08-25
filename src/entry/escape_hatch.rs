//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Verifying the flags that narrow what gets linted.
//!
//! `--doc-only` and `--scope` exist so a hook can skip work that cannot be
//! affected by what is staged: no `.rs` files staged means source lints have
//! nothing to say, and only one crate touched means the others need not be
//! re-checked. Both are real optimisations and both are worth keeping.
//!
//! Both were also **trusted rather than verified**, which makes each an
//! unintended bypass. Passing `--doc-only` with source files staged skips every
//! source lint on them; passing `--scope other-crate` while the violating crate
//! is staged skips that crate entirely. Neither required deceit to exploit: the
//! obvious guess about what the flag does is the exploit.
//!
//! So the flags stay, and the claim each one makes is checked against what is
//! actually staged. A flag whose claim is false is an error that says what was
//! staged and what the flag asserted, rather than a quietly narrower run. That is
//! the same treatment every other anomalous state gets.
//!
//! Nothing here can be bypassed by a lint's own configuration, deliberately: a
//! project may tune severities, but it cannot make the gate lie about its scope.

use std::path::Path;
use std::process::Command;

/// The staged paths under `mock_rel`, relative to the repo root.
///
/// Empty when git is unavailable or nothing is staged. An error reading git is
/// treated as "nothing staged", because a verification step must not itself
/// become a reason a commit fails.
fn staged_paths(repo_root: &Path, mock_rel: &str) -> Vec<String> {
    let out = Command::new("git")
        .args(["diff", "--cached", "--name-only", "--", mock_rel])
        .current_dir(repo_root)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        },
        _ => Vec::new(),
    }
}

/// Staged Rust source files under the mock workspace's crates.
#[must_use]
pub fn staged_source_files(repo_root: &Path, mock_rel: &str) -> Vec<String> {
    staged_paths(repo_root, mock_rel)
        .into_iter()
        .filter(|p| p.ends_with(".rs"))
        .collect()
}

/// The crate directory names with staged changes under `<mock>/crates/`.
#[must_use]
pub fn staged_crates(repo_root: &Path, mock_rel: &str) -> Vec<String> {
    let prefix = format!("{mock_rel}/crates/");
    let mut v: Vec<String> = staged_paths(repo_root, mock_rel)
        .into_iter()
        .filter_map(|p| {
            p.strip_prefix(&prefix)
                .and_then(|rest| rest.split('/').next())
                .map(str::to_string)
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Why a narrowing flag was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// `--doc-only` was passed while Rust sources are staged.
    DocOnlyWithStagedSource {
        staged: Vec<String>,
    },
    /// `--scope` omitted crates that have staged changes.
    ScopeOmitsStagedCrates {
        omitted: Vec<String>,
        scoped:  Vec<String>,
    },
}

impl Refusal {
    /// The message shown when the run is refused.
    ///
    /// Names both what was claimed and what is actually staged, because a reader
    /// who cannot see the discrepancy will assume the gate is broken and reach
    /// for a way around it.
    #[must_use]
    pub fn explain(&self) -> String {
        match self {
            Self::DocOnlyWithStagedSource {
                staged,
            } => {
                format!(
                    "--doc-only claims no source is staged, but {} Rust file(s) are:\n  {}\n\
                     \n  Source lints would be skipped on staged source, which is the one thing\n  \
                     this flag must never do. Drop --doc-only, or unstage the source.",
                    staged.len(),
                    staged.join("\n  ")
                )
            },
            Self::ScopeOmitsStagedCrates {
                omitted,
                scoped,
            } => {
                format!(
                    "--scope {} omits crate(s) with staged changes:\n  {}\n\
                     \n  Those crates would not be linted at all. Widen the scope to include\n  \
                     them, or unstage them.",
                    scoped.join(","),
                    omitted.join("\n  ")
                )
            },
        }
    }
}

/// Verify `--doc-only`'s claim that no source is staged.
#[must_use]
pub fn verify_doc_only(repo_root: &Path, mock_rel: &str) -> Option<Refusal> {
    let staged = staged_source_files(repo_root, mock_rel);
    if staged.is_empty() {
        return None;
    }
    Some(Refusal::DocOnlyWithStagedSource {
        staged,
    })
}

/// Verify that `--scope` covers every crate with staged changes.
///
/// `infra` is not a crate list and means "no crate files staged", so it is
/// verified by the same staged-crates check rather than by name comparison.
#[must_use]
pub fn verify_scope(repo_root: &Path, mock_rel: &str, scope: &str) -> Option<Refusal> {
    let staged = staged_crates(repo_root, mock_rel);
    if staged.is_empty() {
        return None;
    }
    let scoped: Vec<String> = if scope == "infra" {
        Vec::new()
    } else {
        scope
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let omitted: Vec<String> = staged.into_iter().filter(|c| !scoped.contains(c)).collect();
    if omitted.is_empty() {
        return None;
    }
    Some(Refusal::ScopeOmitsStagedCrates {
        omitted,
        scoped,
    })
}

#[cfg(test)]
mod tests {
    use std::process::Command as C;

    use super::*;

    /// A git repo with the given paths staged.
    fn repo_with_staged(paths: &[(&str, &str)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ms_hatch_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            C::new("git")
                .current_dir(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap()
        };
        git(&["init", "-q", "-b", "main"]);
        for (path, contents) in paths {
            let p = dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, contents).unwrap();
        }
        git(&["add", "-A"]);
        dir
    }

    #[test]
    fn doc_only_passes_when_only_docs_are_staged() {
        // The legitimate case the flag exists for, so verification must not
        // break it.
        let repo = repo_with_staged(&[("mock/crates/foo/DESIGN.md.tmpl", "x")]);
        let r = verify_doc_only(&repo, "mock");
        let _ = std::fs::remove_dir_all(&repo);
        assert_eq!(r, None);
    }

    #[test]
    fn doc_only_is_refused_when_source_is_staged() {
        // The bypass: source lints would be skipped on staged source.
        let repo = repo_with_staged(&[("mock/crates/foo/src/lib.rs", "fn x() {}")]);
        let r = verify_doc_only(&repo, "mock");
        let _ = std::fs::remove_dir_all(&repo);
        match r {
            Some(Refusal::DocOnlyWithStagedSource {
                staged,
            }) => {
                assert_eq!(staged, vec!["mock/crates/foo/src/lib.rs".to_string()]);
            },
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_refusal_names_both_the_claim_and_the_reality() {
        // A reader who cannot see the discrepancy assumes the gate is broken and
        // looks for a way around it.
        let r = Refusal::DocOnlyWithStagedSource {
            staged: vec!["mock/crates/foo/src/lib.rs".into()],
        };
        let m = r.explain();
        assert!(m.contains("--doc-only"));
        assert!(m.contains("mock/crates/foo/src/lib.rs"));
        assert!(m.contains("Drop --doc-only"));
    }

    #[test]
    fn scope_passes_when_it_covers_every_staged_crate() {
        let repo = repo_with_staged(&[
            ("mock/crates/foo/src/lib.rs", "fn a() {}"),
            ("mock/crates/bar/src/lib.rs", "fn b() {}"),
        ]);
        let r = verify_scope(&repo, "mock", "foo,bar");
        let _ = std::fs::remove_dir_all(&repo);
        assert_eq!(r, None);
    }

    #[test]
    fn scope_is_refused_when_it_omits_a_staged_crate() {
        // The bypass: scope away the crate that has the violation.
        let repo = repo_with_staged(&[
            ("mock/crates/foo/src/lib.rs", "fn a() {}"),
            ("mock/crates/bar/src/lib.rs", "fn b() {}"),
        ]);
        let r = verify_scope(&repo, "mock", "foo");
        let _ = std::fs::remove_dir_all(&repo);
        match r {
            Some(Refusal::ScopeOmitsStagedCrates {
                omitted,
                ..
            }) => {
                assert_eq!(omitted, vec!["bar".to_string()]);
            },
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    #[test]
    fn infra_scope_is_refused_when_crate_files_are_staged() {
        // `--scope infra` asserts no crate files are staged, so it is the same
        // claim as --doc-only and gets the same verification.
        let repo = repo_with_staged(&[("mock/crates/foo/src/lib.rs", "fn a() {}")]);
        let r = verify_scope(&repo, "mock", "infra");
        let _ = std::fs::remove_dir_all(&repo);
        assert!(matches!(r, Some(Refusal::ScopeOmitsStagedCrates { .. })));
    }

    #[test]
    fn infra_scope_passes_when_no_crate_files_are_staged() {
        let repo = repo_with_staged(&[("mock/mockspace.toml", "x = 1")]);
        let r = verify_scope(&repo, "mock", "infra");
        let _ = std::fs::remove_dir_all(&repo);
        assert_eq!(r, None);
    }

    #[test]
    fn a_missing_git_is_not_itself_a_reason_to_fail() {
        // Verification must not become a new way for a commit to break. With no
        // repo there is nothing staged, so nothing is refused.
        let nowhere = std::env::temp_dir().join("ms_hatch_definitely_not_a_repo");
        let _ = std::fs::create_dir_all(&nowhere);
        assert_eq!(verify_doc_only(&nowhere, "mock"), None);
        assert_eq!(verify_scope(&nowhere, "mock", "foo"), None);
        let _ = std::fs::remove_dir_all(&nowhere);
    }
}
