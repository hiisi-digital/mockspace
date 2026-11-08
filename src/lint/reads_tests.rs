//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit gate runs a lint declaring the paths it reads only for a commit that meets them.
//!
//! One real git repository, a lint reading `content/**` that always has a finding, and one that
//! declares nothing and always has one too. What is staged decides which of them the gate hears
//! from. The case that must fail is the first: staging a file the lint reads still blocks.

use std::fs;
use std::path::Path;
use std::process::Command;

use mockspace_lint_rules::{Lint, LintError, LintMode, LintPack, Reach, RegistryView, RepoContext, RepoLint};

use crate::config::Config;

struct ReadsContent;

impl Lint for ReadsContent {
    fn name(&self) -> &'static str {
        "reads-content"
    }

    fn reach(&self) -> Reach {
        Reach::Paths(&["content/**"])
    }
}

impl RepoLint for ReadsContent {
    fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
        vec![LintError::error("alpha".to_string(), 0, "reads-content", "broken".to_string())]
    }
}

struct DeclaresNothing;

impl Lint for DeclaresNothing {
    fn name(&self) -> &'static str {
        "declares-nothing"
    }
}

impl RepoLint for DeclaresNothing {
    fn check_repo(&self, _ctx: &RepoContext) -> Vec<LintError> {
        vec![LintError::error("alpha".to_string(), 0, "declares-nothing", "broken".to_string())]
    }
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A committed repository, with `files` changed and staged.
fn repo_staging(root: &Path, files: &[&str]) -> Config {
    git(root, &["init", "-q", "-b", "dev"]);
    fs::create_dir_all(root.join("mock/crates")).unwrap();
    for f in ["content/a.toml", "notes/b.txt", "mock/lints/x.rs"] {
        write(root, f, "base\n");
    }
    write(root, "mock/mockspace.toml", "project_name = \"fixture\"\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "chore: base"]);
    for f in files {
        let text = if f.ends_with("mockspace.toml") { "project_name = \"changed\"\n" } else { "changed\n" };
        write(root, f, text);
        git(root, &["add", f]);
    }
    let mock = root.join("mock");
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = root.to_path_buf();
    cfg.mock_dir = mock.clone();
    cfg.src_dirs = vec![mock.join("crates")];
    cfg.crate_prefix = "sim".to_string();
    cfg
}

/// The blocking count of the commit gate, and of a push, with both lints in the pack.
fn counts(cfg: &Config) -> (usize, usize) {
    let run = |mode: LintMode| {
        let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
        let pack = LintPack {
            repo_lints: vec![Box::new(ReadsContent), Box::new(DeclaresNothing)],
            ..LintPack::default()
        };
        crate::lint::run_lints(
            &crates,
            &cfg.src_dirs,
            &cfg.mock_dir,
            mode,
            None,
            false,
            &cfg.proc_macro_crates,
            cfg.lint_proc_macro_source,
            &cfg.crate_prefix,
            &cfg.lint_overrides,
            &RegistryView::default(),
            &[],
            &[],
            &cfg.primitive_introductions,
            &pack,
        )
    };
    (run(LintMode::Commit), run(LintMode::Push))
}

#[test]
fn a_commit_staging_a_file_the_lint_reads_still_runs_and_blocks_on_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_staging(tmp.path(), &["content/a.toml"]);
    assert_eq!(counts(&cfg).0, 2, "both lints must be heard from");
}

#[test]
fn a_commit_staging_nothing_the_lint_reads_does_not_run_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_staging(tmp.path(), &["notes/b.txt"]);
    assert_eq!(counts(&cfg).0, 1, "only the lint declaring nothing runs");
}

#[test]
fn a_commit_changing_the_lint_set_runs_every_lint() {
    for set in ["mock/lints/x.rs", "mock/mockspace.toml"] {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = repo_staging(tmp.path(), &["notes/b.txt", set]);
        assert_eq!(counts(&cfg).0, 2, "{set} changes the lint set");
    }
}

#[test]
fn a_push_runs_every_lint_whatever_it_carries() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_staging(tmp.path(), &["notes/b.txt"]);
    assert_eq!(counts(&cfg).1, 2);
}
