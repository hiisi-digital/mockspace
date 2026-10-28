//! What the gate's chain reaches once git-shook has adopted a repository: the
//! repository's own hook, once and never twice, whether it arrived before the
//! adoption or after, and the order of the entries git-shook runs.
//!
//! The fixtures are those of `shook_tests`, and the tests need the real
//! `git shook`, and say so when they cannot run rather than passing.

use std::path::PathBuf;

use super::tests::{git, have_shook, repo};
use super::*;

#[test]
fn adopting_shook_in_a_gated_repo_runs_the_gate_once() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("once");
    let log = dir.join("user.log");
    // the real durable hooks, which in a repository that is not a mockspace
    // project pass and chain to the repository's own hook: the chain is what a
    // second copy of the gate would run twice
    let hooks = dir.join("durable");
    let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, HOOK_VERSION);
    let own = dir.join(".git/hooks/pre-commit");
    crate::test_exec::write_executable(
        &own,
        format!("#!/bin/sh\necho USER >> {}\n", log.display()),
    );
    // the repository is gated the old way, by core.hooksPath on the durable dir
    git(&dir, &["config", "--local", "core.hooksPath", hooks.to_str().unwrap()]);
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    assert!(
        shook_dir(&dir).join("entries/pre-commit/00-existing").is_file(),
        "control: git-shook did keep the gate as an existing hook"
    );

    register(&dir, &hooks).unwrap();

    fs::write(dir.join("f.txt"), "x").unwrap();
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    // once: the gate's chain reaches the repository's own hook, and the kept
    // copy of the gate is gone so it does not reach it a second time. Without
    // `drop_own_kept_hooks` this reads twice.
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "USER").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}

/// A repository with the real durable hooks, git-shook adopted and the gate
/// registered, and the repository's own `pre-commit` written by `own_hook`
/// before or after the adoption. Returns the log the hook appends to.
fn adopted_with_own_hook(tag: &str, own_before_adoption: bool) -> (PathBuf, PathBuf) {
    let dir = repo(tag);
    let log = dir.join("user.log");
    let hooks = dir.join("durable");
    let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, HOOK_VERSION);
    let own = dir.join(".git/hooks/pre-commit");
    let write_own = || {
        crate::test_exec::write_executable(
            &own,
            format!("#!/bin/sh\necho USER >> {}\n", log.display()),
        );
    };
    if own_before_adoption {
        write_own();
    }
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    register(&dir, &hooks).unwrap();
    if !own_before_adoption {
        write_own();
    }
    fs::write(dir.join("f.txt"), "x").unwrap();
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    (dir, log)
}

#[test]
fn a_hook_git_shook_kept_is_not_also_chained_to_by_the_gate() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let (dir, log) = adopted_with_own_hook("keptonce", true);
    assert!(
        shook_dir(&dir).join("entries/pre-commit/00-existing").is_file(),
        "control: git-shook kept the repository's own hook as an entry"
    );
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "USER").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_hook_that_arrived_after_the_adoption_is_still_reached_by_the_chain() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let (dir, log) = adopted_with_own_hook("lateonce", false);
    assert!(
        !shook_dir(&dir).join("entries/pre-commit/00-existing").exists(),
        "control: nothing was kept, so the chain is the only way to it"
    );
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "USER").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_kept_hook_that_is_not_ours_survives_registration() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("kept");
    let hooks = dir.join("durable");
    fs::create_dir_all(&hooks).unwrap();
    for event in events() {
        let p = hooks.join(event);
        crate::test_exec::write_executable(&p, "#!/bin/sh\ntrue\n");
    }
    let own = dir.join(".git/hooks/pre-commit");
    crate::test_exec::write_executable(&own, "#!/bin/sh\necho foreign\n");
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    let kept = shook_dir(&dir).join("entries/pre-commit/00-existing");
    assert!(kept.is_file(), "control: the repository's own hook was kept");
    register(&dir, &hooks).unwrap();
    assert!(kept.is_file(), "a kept hook without the managed marker is not dropped");
    assert!(shook_dir(&dir).join("kept/pre-commit").is_file());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn deactivating_after_shook_gave_up_the_key_takes_the_gate_off_it() {
    let dir = repo("stale");
    let sd = shook_dir(&dir);
    fs::create_dir_all(sd.join("manifests")).unwrap();
    fs::write(manifest_path(&dir), "name = \"mockspace\"\n").unwrap();
    git(&dir, &["config", "--local", "core.hooksPath", "mock/target/hooks"]);
    assert!(is_active(&dir), "control: the gate holds the key");

    deactivate(&dir).unwrap();

    assert!(!has_manifest(&dir), "the stale registration is gone");
    assert_eq!(git(&dir, &["config", "--local", "core.hooksPath"]), "", "and so is the key");
    assert!(!is_active(&dir));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_manifest_alone_is_not_an_active_gate() {
    let dir = repo("alone");
    let sd = shook_dir(&dir);
    fs::create_dir_all(sd.join("manifests")).unwrap();
    fs::write(manifest_path(&dir), "name = \"mockspace\"\n").unwrap();
    let hooks_path = sd.join("hooks").to_string_lossy().to_string();
    assert!(!is_live(&dir, Some(&hooks_path)), "no entries were registered");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_repository_without_shook_is_not_reported_active_by_the_shook_path() {
    let dir = repo("plain");
    assert!(!is_active(&dir));
    assert!(!has_manifest(&dir));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_hook_git_shook_declined_because_a_manifest_runs_it_is_not_chained_to_either() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("declined");
    let log = dir.join("tool.log");
    let tool = dir.join("tool");
    crate::test_exec::write_executable(
        &tool,
        format!("#!/bin/sh\necho TOOL >> {}\n", log.display()),
    );
    // the repository's own hook does nothing but run what the manifest declares
    let own = dir.join(".git/hooks/pre-commit");
    crate::test_exec::write_executable(
        &own,
        format!("#!/bin/sh\nexec {} \"$@\"\n", tool.display()),
    );
    fs::write(
        dir.join("shook.toml"),
        format!("[hooks.pre-commit]\nrun = [\"{}\"]\n", tool.display()),
    )
    .unwrap();
    git(&dir, &["add", "shook.toml"]);
    let hooks = dir.join("durable");
    let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, HOOK_VERSION);
    git(&dir, &["shook", "install", "--yes"]);
    register(&dir, &hooks).unwrap();
    assert!(
        !shook_dir(&dir).join("entries/pre-commit/00-existing").exists(),
        "control: git-shook declined to keep it, and left the file where it was"
    );
    assert!(own.is_file(), "control: the original is still in `.git/hooks`");
    fs::write(dir.join("f.txt"), "x").unwrap();
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "TOOL").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_hook_edited_after_git_shook_kept_a_copy_is_still_reached_by_the_chain() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let (dir, log) = adopted_with_own_hook("editedafter", true);
    assert!(
        shook_dir(&dir).join("entries/pre-commit/00-existing").is_file(),
        "control: git-shook kept a copy as an entry"
    );
    let own = dir.join(".git/hooks/pre-commit");
    fs::write(&own, format!("#!/bin/sh\necho EDITED >> {}\n", log.display())).unwrap();
    fs::write(dir.join("g.txt"), "y").unwrap();
    git(&dir, &["add", "g.txt"]);
    git(&dir, &["commit", "-q", "-m", "second"]);
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(
        ran.lines().filter(|l| *l == "EDITED").count(),
        1,
        "the current file runs through the chain, the stale copy being the entry's: {ran}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_hook_adopted_from_another_hooks_directory_leaves_git_hooks_to_the_chain() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    let dir = repo("otherdir");
    let log = dir.join("user.log");
    let hooks = dir.join("durable");
    let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, HOOK_VERSION);
    // the old hooks path holds one hook, and `.git/hooks` another
    fs::create_dir_all(dir.join(".githooks")).unwrap();
    let old = dir.join(".githooks/pre-commit");
    crate::test_exec::write_executable(
        &old,
        format!("#!/bin/sh\necho OLDDIR >> {}\n", log.display()),
    );
    git(&dir, &["config", "core.hooksPath", ".githooks"]);
    let own = dir.join(".git/hooks/pre-commit");
    crate::test_exec::write_executable(
        &own,
        format!("#!/bin/sh\necho GITDIR >> {}\n", log.display()),
    );
    fs::write(dir.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(&dir, &["add", "shook.toml"]);
    git(&dir, &["shook", "install", "--yes"]);
    register(&dir, &hooks).unwrap();
    fs::write(dir.join("f.txt"), "x").unwrap();
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "OLDDIR").count(), 1, "{ran}");
    assert_eq!(ran.lines().filter(|l| *l == "GITDIR").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_gate_entry_runs_before_an_entry_named_homma() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    // the repository's own hook arrives after the adoption, so it runs only as
    // the tail of the gate's entry, which makes it the mark of the gate's turn
    let (dir, log) = adopted_with_own_hook("order", false);
    let tool = dir.join("homma-stand-in");
    crate::test_exec::write_executable(
        &tool,
        format!("#!/bin/sh\necho HOMMA >> {}\n", log.display()),
    );
    let manifests = shook_dir(&dir).join("manifests");
    fs::write(
        manifests.join("homma.toml"),
        format!("name = \"homma\"\n\n[hooks.pre-commit]\nrun = [\"{}\"]\n", tool.display()),
    )
    .unwrap();
    git(&dir, &["shook", "install", "--yes", "--only", "homma"]);
    fs::write(&log, "").unwrap();
    fs::write(dir.join("g.txt"), "y").unwrap();
    git(&dir, &["add", "g.txt"]);
    git(&dir, &["commit", "-q", "-m", "second"]);
    let ran: Vec<String> = fs::read_to_string(&log).unwrap_or_default().lines().map(String::from).collect();
    assert_eq!(ran, ["USER", "HOMMA"], "the gate's entry, and the hook it ends in, go first");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_tool_run_by_a_manifest_and_by_the_hook_in_git_hooks_runs_once_after_the_gate_held_the_key() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not on the path");
        return;
    }
    // the order a gated repository meets: the gate holds `core.hooksPath`, the
    // tool's entrypoint sits in `.git/hooks`, a tracked manifest declares the
    // same tool, and then git-shook is installed and the gate registered
    let dir = repo("gateheld");
    let log = dir.join("tool.log");
    let tool = dir.join("tool");
    crate::test_exec::write_executable(
        &tool,
        format!("#!/bin/sh\necho TOOL >> {}\n", log.display()),
    );
    let own = dir.join(".git/hooks/pre-commit");
    crate::test_exec::write_executable(
        &own,
        format!("#!/bin/sh\nexec {} \"$@\"\n", tool.display()),
    );
    fs::write(
        dir.join("shook.toml"),
        format!("[hooks.pre-commit]\nrun = [\"{}\"]\n", tool.display()),
    )
    .unwrap();
    git(&dir, &["add", "shook.toml"]);
    let hooks = dir.join("durable");
    let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, HOOK_VERSION);
    git(&dir, &["config", "core.hooksPath", hooks.to_str().unwrap()]);
    git(&dir, &["shook", "install", "--yes"]);
    register(&dir, &hooks).unwrap();
    fs::write(dir.join("f.txt"), "x").unwrap();
    git(&dir, &["add", "f.txt"]);
    git(&dir, &["commit", "-q", "-m", "init"]);
    let ran = fs::read_to_string(&log).unwrap_or_default();
    assert_eq!(ran.lines().filter(|l| *l == "TOOL").count(), 1, "{ran}");
    let _ = fs::remove_dir_all(&dir);
}
