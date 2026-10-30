//! The merge drivers are registered in the user's git config as well as the clone's, and none of their
//! commands names a path of one clone.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

const ALL: [&str; 5] = [
    "mockspace-generated",
    "mockspace-registry",
    "mockspace-round",
    "mockspace-test-history",
    "mockspace-append",
];

/// `git` in `dir` with the user config at `config` and no system config, so nothing of the machine's own
/// is read or written. Returns whether it succeeded and its output.
fn git_any(dir: &Path, config: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", config)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .expect("git runs");
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

/// The same, for a step that has to work.
fn git(dir: &Path, config: &Path, args: &[&str]) -> String {
    let (ok, out) = git_any(dir, config, args);
    assert!(ok, "git {args:?} in {}: {out}", dir.display());
    out
}

fn empty_config(root: &Path) -> PathBuf {
    let config = root.join("empty-gitconfig");
    fs::write(&config, "").unwrap();
    config
}

fn local_value(repo: &Path, config: &Path, key: &str) -> Option<String> {
    let (ok, out) = git_any(repo, config, &["config", "--local", key]);
    ok.then(|| out.trim().to_string())
}

#[test]
fn registering_in_the_user_config_registers_every_driver_and_a_second_run_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("gitconfig");
    let mut first = Vec::new();
    register_all_drivers(&ConfigTarget::User(Some(&file)), &mut first);
    for d in ALL {
        let (ok, _) = git_any(tmp.path(), &file, &["config", "--file", file.to_str().unwrap(), &format!("merge.{d}.driver")]);
        assert!(ok, "{d} is not registered; actions: {first:?}");
    }
    assert_eq!(first.len(), ALL.len(), "{first:?}");
    let mut second = Vec::new();
    register_all_drivers(&ConfigTarget::User(Some(&file)), &mut second);
    assert!(second.is_empty(), "{second:?}");
}

#[test]
fn no_registered_command_names_a_path_of_the_clone_it_was_written_from() {
    let tmp = tempfile::tempdir().unwrap();
    let config = empty_config(tmp.path());
    let repo = tmp.path().join("a-clone-with-a-distinctive-name");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &config, &["init", "-q", "-b", "main"]);
    let mut actions = Vec::new();
    register_all_drivers(&ConfigTarget::Clone(&repo), &mut actions);
    for d in ALL {
        let command = local_value(&repo, &config, &format!("merge.{d}.driver"))
            .unwrap_or_else(|| panic!("{d} is not registered: {actions:?}"));
        assert!(
            !command.contains("a-clone-with-a-distinctive-name"),
            "{d} names its clone: {command}"
        );
    }
}

/// A repository on `main` with `docs/x.md` changed on both sides (and `docs/y.md` on `side` alone), its
/// `.gitattributes` sending `docs/*` to the generated-file driver.
fn a_conflict_under_the_generated_driver(repo: &Path, config: &Path) {
    fs::create_dir_all(repo.join("docs")).unwrap();
    git(repo, config, &["init", "-q", "-b", "main"]);
    fs::write(repo.join(".gitattributes"), "/docs/* merge=mockspace-generated\n").unwrap();
    fs::write(repo.join("docs/x.md"), "a\n").unwrap();
    git(repo, config, &["add", "-A"]);
    git(repo, config, &["commit", "-qm", "base"]);
    git(repo, config, &["checkout", "-qb", "side"]);
    fs::write(repo.join("docs/x.md"), "side\n").unwrap();
    fs::write(repo.join("docs/y.md"), "only on side\n").unwrap();
    git(repo, config, &["add", "-A"]);
    git(repo, config, &["commit", "-qm", "side"]);
    git(repo, config, &["checkout", "-q", "main"]);
    fs::write(repo.join("docs/x.md"), "main\n").unwrap();
    git(repo, config, &["commit", "-qam", "main"]);
}

/// Put a script where the registered command looks for it, writing `label` into the merged file.
fn stub_script_in(git_common_dir: &Path, label: &str) {
    let script = git_common_dir.join("mockspace/merge-generated");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, format!("#!/bin/sh\necho {label} > \"$2\"\n")).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
}

fn registered_for_the_user(root: &Path) -> PathBuf {
    let config = root.join("gitconfig");
    register_all_drivers(&ConfigTarget::User(Some(&config)), &mut Vec::new());
    config
}

fn file(repo: &Path, name: &str) -> String {
    fs::read_to_string(repo.join(name)).unwrap().trim().to_string()
}

#[test]
fn the_generated_driver_still_runs_in_a_copy_of_the_clone_it_was_registered_in() {
    let tmp = tempfile::tempdir().unwrap();
    let config = empty_config(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    register_driver(&ConfigTarget::Clone(&repo), &mut Vec::new());
    stub_script_in(&repo.join(".git"), "ORIGINAL");
    let copy = tmp.path().join("two");
    assert!(Command::new("cp").arg("-r").arg(&repo).arg(&copy).status().unwrap().success());
    stub_script_in(&copy.join(".git"), "COPY");
    git_any(&copy, &config, &["merge", "side"]);
    assert_eq!(file(&copy, "docs/x.md"), "COPY", "the registered command did not reach the script of the copy");
}

#[test]
fn the_generated_driver_registered_for_the_user_runs_in_a_clone_that_never_registered_it() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    stub_script_in(&repo.join(".git"), "STUB");
    git_any(&repo, &config, &["merge", "side"]);
    assert_eq!(file(&repo, "docs/x.md"), "STUB");
}

#[test]
fn with_no_script_the_generated_driver_leaves_a_conflict_a_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    let (ok, _) = git_any(&repo, &config, &["merge", "side"]);
    assert!(!ok, "a conflicting merge exited zero");
    let text = file(&repo, "docs/x.md");
    assert!(text.contains("<<<<<<<") && text.contains("side") && text.contains("main"), "{text}");
    let (_, unmerged) = git_any(&repo, &config, &["ls-files", "-u"]);
    assert!(!unmerged.trim().is_empty(), "nothing is left unmerged");
}

#[test]
fn with_no_script_a_merge_that_does_not_conflict_is_clean() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    git(&repo, &config, &["checkout", "-q", "-B", "other", "side~1"]);
    fs::write(repo.join("docs/y.md"), "other\n").unwrap();
    git(&repo, &config, &["add", "-A"]);
    git(&repo, &config, &["commit", "-qm", "other"]);
    git(&repo, &config, &["checkout", "-q", "main"]);
    // main and side both changed docs/x.md; merging `other` (which changed only docs/y.md) has nothing to conflict on
    let (ok, out) = git_any(&repo, &config, &["merge", "--no-edit", "other"]);
    assert!(ok, "{out}");
    assert_eq!(file(&repo, "docs/y.md"), "other");
}

#[test]
fn a_linked_worktree_finds_the_script_of_the_clone_it_hangs_off() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    stub_script_in(&repo.join(".git"), "SHARED");
    let wt = tmp.path().join("wt");
    git(&repo, &config, &["worktree", "add", "-q", "-b", "wt-branch", wt.to_str().unwrap(), "main"]);
    git_any(&wt, &config, &["merge", "side"]);
    assert_eq!(file(&wt, "docs/x.md"), "SHARED");
}

#[test]
fn a_submodule_finds_the_script_in_its_own_git_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let inner = tmp.path().join("inner");
    a_conflict_under_the_generated_driver(&inner, &config);
    let outer = tmp.path().join("outer");
    fs::create_dir_all(&outer).unwrap();
    git(&outer, &config, &["init", "-q", "-b", "main"]);
    git(&outer, &config, &["submodule", "add", "-q", inner.to_str().unwrap(), "sub"]);
    let sub = outer.join("sub");
    git(&sub, &config, &["checkout", "-q", "main"]);
    let (_, gitdir) = git_any(&sub, &config, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    stub_script_in(Path::new(gitdir.trim()), "SUBMODULE");
    git_any(&sub, &config, &["merge", "origin/side"]);
    assert_eq!(file(&sub, "docs/x.md"), "SUBMODULE");
}

#[test]
fn a_bare_repository_runs_the_driver_for_merge_tree() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    let bare = tmp.path().join("bare.git");
    git(tmp.path(), &config, &["clone", "-q", "--bare", repo.to_str().unwrap(), bare.to_str().unwrap()]);
    stub_script_in(&bare, "BARE");
    let (_, out) = git_any(&bare, &config, &["merge-tree", "--write-tree", "main", "side"]);
    let tree = out.lines().next().unwrap().to_string();
    let x = git(&bare, &config, &["cat-file", "-p", &format!("{tree}:docs/x.md")]);
    assert_eq!(x.trim(), "BARE");
}

#[test]
fn the_script_is_written_where_the_driver_looks_from_a_worktree_run_below_its_root() {
    let tmp = tempfile::tempdir().unwrap();
    let config = empty_config(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    let wt = tmp.path().join("wt");
    git(&repo, &config, &["worktree", "add", "-q", "-b", "wt-branch", wt.to_str().unwrap(), "main"]);
    register_driver(&ConfigTarget::Clone(&wt), &mut Vec::new());
    let (_, common) = git_any(&wt, &config, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let script = Path::new(common.trim()).join("mockspace/merge-generated");
    assert!(script.is_file(), "no script at {}", script.display());
}

#[test]
fn a_script_that_cannot_be_written_is_reported_and_the_driver_is_not_registered() {
    let tmp = tempfile::tempdir().unwrap();
    let config = empty_config(tmp.path());
    let repo = tmp.path().join("one");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &config, &["init", "-q", "-b", "main"]);
    // a file where the script's directory has to be
    fs::write(repo.join(".git/mockspace"), "in the way").unwrap();
    let mut actions = Vec::new();
    register_driver(&ConfigTarget::Clone(&repo), &mut actions);
    assert!(actions.iter().any(|a| a.contains("could not write")), "{actions:?}");
    assert_eq!(local_value(&repo, &config, "merge.mockspace-generated.driver"), None);
}

/// The command as it was before the path left it: absolute, in one clone's git directory.
fn the_old_entry(clone: &Path) -> String {
    format!("'{}/.git/mockspace/merge-generated' %O %A %B %P %L", clone.display())
}

#[test]
fn a_clone_still_naming_its_own_path_has_that_entry_retired_and_the_user_one_takes_over() {
    let tmp = tempfile::tempdir().unwrap();
    let config = registered_for_the_user(tmp.path());
    let repo = tmp.path().join("one");
    a_conflict_under_the_generated_driver(&repo, &config);
    git(&repo, &config, &["config", "--local", "merge.mockspace-generated.driver", &the_old_entry(&repo)]);
    stub_script_in(&repo.join(".git"), "STUB");
    let mut actions = Vec::new();
    retire_stale_clone_entries(&repo, &mut actions);
    assert_eq!(local_value(&repo, &config, "merge.mockspace-generated.driver"), None, "{actions:?}");
    assert!(actions.iter().any(|a| a.contains("mockspace-generated")), "{actions:?}");
    git_any(&repo, &config, &["merge", "side"]);
    assert_eq!(file(&repo, "docs/x.md"), "STUB");
}

#[test]
fn a_clone_entry_that_names_no_path_of_ours_is_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let config = empty_config(tmp.path());
    let repo = tmp.path().join("one");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &config, &["init", "-q", "-b", "main"]);
    git(&repo, &config, &["config", "--local", "merge.mockspace-generated.driver", "my-own-driver %O %A %B"]);
    let mut actions = Vec::new();
    retire_stale_clone_entries(&repo, &mut actions);
    assert_eq!(
        local_value(&repo, &config, "merge.mockspace-generated.driver").as_deref(),
        Some("my-own-driver %O %A %B")
    );
    assert!(actions.is_empty(), "{actions:?}");
}

#[test]
fn registering_in_a_submodule_writes_the_script_where_its_command_looks() {
    let tmp = tempfile::tempdir().unwrap();
    let config = empty_config(tmp.path());
    let inner = tmp.path().join("inner");
    a_conflict_under_the_generated_driver(&inner, &config);
    let outer = tmp.path().join("outer");
    fs::create_dir_all(&outer).unwrap();
    git(&outer, &config, &["init", "-q", "-b", "main"]);
    git(&outer, &config, &["submodule", "add", "-q", inner.to_str().unwrap(), "sub"]);
    let sub = outer.join("sub");
    register_driver(&ConfigTarget::Clone(&sub), &mut Vec::new());
    let common = git(&sub, &config, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let script = Path::new(common.trim()).join("mockspace/merge-generated");
    assert!(script.is_file(), "no script at {}", script.display());
}
