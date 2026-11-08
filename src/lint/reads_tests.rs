//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A commit or push gate runs a lint declaring the paths it reads only when it meets them.
//!
//! One real git repository with a bare remote. `ReadsContent` reads `content/**` and blocks when
//! `content/a.toml` says `broken`; `DeclaresNothing` declares nothing and blocks when `notes/b.txt`
//! says `broken`. A run that settles the lint set key comes first (`warm`), since a repository that
//! never had a clean full run is judged whole. The cases that must fail are the ones where the
//! lint reads what the change touches, and the ones where the lint set moved with no file touched.

use std::fs;
use std::path::Path;
use std::process::Command;

use mockspace_lint_rules::{Lint, LintError, LintMode, LintPack, Reach, RegistryView, RepoContext, RepoLint};

use crate::config::Config;

fn says_broken(ctx: &RepoContext, rel: &str) -> bool {
    fs::read_to_string(ctx.repo_root.join(rel)).is_ok_and(|t| t.contains("broken"))
}

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
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        if !says_broken(ctx, "content/a.toml") {
            return Vec::new();
        }
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
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        if !says_broken(ctx, "notes/b.txt") {
            return Vec::new();
        }
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

/// A repository whose mock directory is `mock_rel`, its clean base committed and pushed to a bare
/// remote beside it.
fn repo(top: &Path, mock_rel: &str) -> Config {
    let root = top.join("repo");
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "dev"]);
    let remote = top.join("remote.git");
    git(top, &["init", "-q", "--bare", remote.to_str().unwrap()]);
    git(&root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    fs::create_dir_all(root.join(mock_rel).join("crates")).unwrap();
    for f in ["content/a.toml", "notes/b.txt", "mock/lints/x.rs"] {
        write(&root, f, "fine\n");
    }
    write(&root, &format!("{mock_rel}/mockspace.toml"), "project_name = \"fixture\"\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "chore: base"]);
    git(&root, &["push", "-q", "origin", "dev"]);
    let mock = root.join(mock_rel);
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = root;
    cfg.mock_dir = mock.clone();
    cfg.src_dirs = vec![mock.join("crates")];
    cfg.crate_prefix = "sim".to_string();
    cfg
}

/// The blocking count of a run in `mode` with both lints in a pack named `identity`.
fn run(cfg: &Config, mode: LintMode, identity: &str) -> usize {
    run_with(cfg, mode, identity, vec![Box::new(ReadsContent), Box::new(DeclaresNothing)])
}

/// [`run`] with the lints given, over a verdict store of the test's own, beside its repository.
fn run_with(cfg: &Config, mode: LintMode, identity: &str, lints: Vec<Box<dyn RepoLint>>) -> usize {
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    let mut pack = LintPack { repo_lints: lints, ..LintPack::default() };
    pack.identity = identity.to_string();
    let store = cfg.repo_root.parent().unwrap().join("store");
    crate::lint::verdicts::test_base::with(store, || lint_run(cfg, mode, &crates, &pack))
}

fn lint_run(cfg: &Config, mode: LintMode, crates: &crate::model::CrateMap, pack: &LintPack) -> usize {
    crate::lint::run_lints(
        crates,
        &cfg.src_dirs,
        &cfg.mock_dir,
        &cfg.repo_root,
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
        pack,
    )
}

/// The clean full run that keeps the lint set key, as the first commit of a repository is.
fn warm(cfg: &Config) {
    assert_eq!(run(cfg, LintMode::Commit, "v1"), 0, "the base is clean");
}

fn stage(cfg: &Config, rel: &str, text: &str) {
    write(&cfg.repo_root, rel, text);
    git(&cfg.repo_root, &["add", rel]);
}

fn commit(cfg: &Config, rel: &str, text: &str) {
    stage(cfg, rel, text);
    git(&cfg.repo_root, &["commit", "-q", "-m", "chore: change"]);
}

#[test]
fn a_commit_staging_a_file_the_lint_reads_still_runs_and_blocks_on_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    warm(&cfg);
    stage(&cfg, "content/a.toml", "broken\n");
    stage(&cfg, "notes/b.txt", "broken\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 2, "both lints must be heard from");
}

#[test]
fn a_commit_staging_nothing_the_lint_reads_does_not_run_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    warm(&cfg);
    // A standing finding in what the lint reads, in the worktree and in no commit.
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    stage(&cfg, "notes/b.txt", "broken\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 1, "only the lint declaring nothing runs");
}

#[test]
fn a_commit_changing_the_lint_set_runs_every_lint() {
    for set in ["mock/lints/x.rs", "mock/mockspace.toml"] {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = repo(tmp.path(), "mock");
        warm(&cfg);
        write(&cfg.repo_root, "content/a.toml", "broken\n");
        stage(&cfg, set, "project_name = \"changed\"\n");
        assert_eq!(run(&cfg, LintMode::Commit, "v1"), 1, "{set} changes the lint set");
    }
}

#[test]
fn a_pack_that_moved_with_no_file_touched_runs_every_lint() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    warm(&cfg);
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    stage(&cfg, "notes/b.txt", "fine too\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 0, "the control: the same pack skips the lint");
    assert_eq!(run(&cfg, LintMode::Commit, "v2"), 1, "a branch pack re-resolved is a new lint set");
}

#[test]
fn a_blocked_full_run_does_not_keep_the_key_so_the_retry_is_full_too() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    stage(&cfg, "notes/b.txt", "fine too\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 1, "no clean full run yet, so the lint runs");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 1, "and the retry runs it again");
}

#[test]
fn a_push_skips_the_lint_until_a_commit_it_carries_touches_what_it_reads() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    // A key kept by a commit run is not the push gate's, so the push gate has its own clean run.
    assert_eq!(run(&cfg, LintMode::Push, "v1"), 0, "the base is clean");
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    commit(&cfg, "notes/b.txt", "fine too\n");
    assert_eq!(run(&cfg, LintMode::Push, "v1"), 0, "the only unpushed commit reads nothing of content");
    commit(&cfg, "content/a.toml", "broken\n");
    assert_eq!(run(&cfg, LintMode::Push, "v1"), 1, "a commit of content makes the push run the lint and block");
}

#[test]
fn a_plain_run_runs_every_lint() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    warm(&cfg);
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    assert_eq!(run(&cfg, LintMode::Build, "v1"), 1);
}

#[test]
fn a_mock_directory_nested_below_the_root_still_meets_the_files_of_the_whole_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "tools/mock");
    warm(&cfg);
    stage(&cfg, "content/a.toml", "broken\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 1, "the root is the project's, not the mock directory's parent");
}

/// Reads `content/**` and blocks only at the push gate.
struct PushOnly;

impl Lint for PushOnly {
    fn name(&self) -> &'static str {
        "push-only"
    }

    fn reach(&self) -> Reach {
        Reach::Paths(&["content/**"])
    }
}

impl RepoLint for PushOnly {
    fn check_repo(&self, ctx: &RepoContext) -> Vec<LintError> {
        if !says_broken(ctx, "content/a.toml") {
            return Vec::new();
        }
        let sev = mockspace_lint_rules::Severity::PUSH_GATE;
        vec![LintError::with_severity("alpha".to_string(), 0, "push-only", "broken".to_string(), sev)]
    }
}

fn run_push_only(cfg: &Config, mode: LintMode) -> usize {
    run_with(cfg, mode, "p1", vec![Box::new(PushOnly)])
}

#[test]
fn a_commit_runs_clean_key_is_not_trusted_by_the_push_gate() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    // Content already broken on the remote, from before the lint existed.
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    git(&cfg.repo_root, &["commit", "-qam", "chore: content"]);
    git(&cfg.repo_root, &["push", "-q", "origin", "dev"]);
    stage(&cfg, "notes/b.txt", "fine too\n");
    assert_eq!(run_push_only(&cfg, LintMode::Commit), 0, "the commit gate only warns, and keeps its key");
    git(&cfg.repo_root, &["commit", "-q", "-m", "chore: change"]);
    assert_eq!(run_push_only(&cfg, LintMode::Push), 1, "the push gate must still block");
}

#[test]
fn a_push_with_no_key_kept_blocks() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    write(&cfg.repo_root, "content/a.toml", "broken\n");
    git(&cfg.repo_root, &["commit", "-qam", "chore: content"]);
    git(&cfg.repo_root, &["push", "-q", "origin", "dev"]);
    commit(&cfg, "notes/b.txt", "fine too\n");
    assert_eq!(run_push_only(&cfg, LintMode::Push), 1, "the control");
}

#[test]
fn a_sibling_worktrees_clean_run_does_not_settle_the_key_for_a_tree_it_never_saw() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    warm(&cfg);
    let b_root = tmp.path().join("b");
    git(&cfg.repo_root, &["worktree", "add", "-q", "-b", "feat", b_root.to_str().unwrap()]);
    let mut b = cfg.clone();
    b.mock_dir = b_root.join("mock");
    b.src_dirs = vec![b.mock_dir.join("crates")];
    b.repo_root = b_root.clone();
    write(&b_root, "content/a.toml", "broken\n");
    git(&b_root, &["commit", "-qam", "chore: content"]);
    // The pack moves, and the first worktree settles the new set over its own clean tree.
    stage(&cfg, "notes/b.txt", "fine too\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v2"), 0);
    // The second worktree has never been judged under it.
    stage(&b, "notes/b.txt", "other\n");
    assert_eq!(run_with_store(&cfg, &b, "v2"), 1, "the new lint set was never run over this tree");
}

/// A run over `b` whose store is the first worktree's, as the machine's one store is shared.
fn run_with_store(shared: &Config, b: &Config, identity: &str) -> usize {
    let crates = crate::parse::discover_crates_in(&b.src_dirs, &b.crate_prefix);
    let mut pack = LintPack {
        repo_lints: vec![Box::new(ReadsContent), Box::new(DeclaresNothing)],
        ..LintPack::default()
    };
    pack.identity = identity.to_string();
    let store = shared.repo_root.parent().unwrap().join("store");
    crate::lint::verdicts::test_base::with(store, || lint_run(b, LintMode::Commit, &crates, &pack))
}

#[test]
fn a_branch_switch_to_a_tree_that_does_not_descend_from_the_covered_one_is_full() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo(tmp.path(), "mock");
    git(&cfg.repo_root, &["checkout", "-q", "-b", "b"]);
    commit(&cfg, "content/a.toml", "broken\n");
    git(&cfg.repo_root, &["checkout", "-q", "-b", "a", "dev"]);
    commit(&cfg, "notes/b.txt", "a\n");
    warm(&cfg);
    stage(&cfg, "notes/b.txt", "a again\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 0, "the control: on the branch the key covered");
    git(&cfg.repo_root, &["commit", "-q", "-m", "chore: a again"]);
    git(&cfg.repo_root, &["checkout", "-q", "b"]);
    stage(&cfg, "notes/b.txt", "b\n");
    assert_eq!(run(&cfg, LintMode::Commit, "v1"), 1, "`b` does not descend from the commit the key covered");
}
