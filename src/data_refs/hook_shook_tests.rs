//! The engine's hooks beside a hook of the person's own that git-shook adopted: each runs once per
//! git event, and the engine's copy of the files of an instance is never the one dropped to get
//! there.
//!
//! The fixture is the order a clone meets: the person's `post-checkout` (git-lfs's, a regenerating
//! shim) is in `.git/hooks`, git-shook is installed and keeps it as `00-existing`, the gate is
//! registered, and then the data refs chain it (`plan` and `write` of this module, the real ones)
//! and the engine's hook stands in its place. The tests need the real `git shook`, found by
//! `bootstrap::test_shook_dir`, and say so when they cannot run rather than passing.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::bootstrap::{register_gate_with_shook_on, test_shook_dir};

fn git(dir: &Path, args: &[&str]) -> String {
    let mut git = Command::new("git");
    if let Some(bin) = test_shook_dir() {
        let mut joined = std::ffi::OsString::from(bin);
        joined.push(":");
        joined.push(std::env::var_os("PATH").unwrap_or_default());
        git.env("PATH", joined);
    }
    let out = git.args(args).current_dir(dir).output().expect("git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn script(path: &Path, body: String) {
    mockspace_manifest::write_executable(path, body).unwrap();
}

/// A clone with two commits, the person's own `post-checkout` in `.git/hooks` appending `PERSON`
/// to the returned log, and the engine standing in as a program that appends `ENGINE`. The tracked
/// `shook.toml` is what lets git-shook take the clone.
fn clone(root: &Path) -> (PathBuf, PathBuf) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "t@t"]);
    git(root, &["config", "user.name", "t"]);
    std::fs::write(root.join("shook.toml"), "[hooks.pre-commit]\nrun = [\"true\"]\n").unwrap();
    git(root, &["add", "shook.toml"]);
    git(root, &["commit", "-q", "-m", "one"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let log = root.join("events.log");
    script(&root.join(".git/hooks/post-checkout"), format!("#!/bin/sh\necho PERSON >> '{}'\n", log.display()));
    let stub = root.join("engine-stub");
    script(&stub, format!("#!/bin/sh\necho ENGINE >> '{}'\nexit 0\n", log.display()));
    (log, stub)
}

/// The data refs' hooks written by the real `plan` and `write`, then the engine of each swapped for
/// `stub`, so a run of the copy can be counted.
fn write_data_refs_hooks(root: &Path, stub: &Path) {
    let mut cfg = Config::from_dir(Path::new("/nonexistent-mock-dir"));
    cfg.repo_root = root.to_path_buf();
    cfg.mock_dir = root.join("mock");
    write(plan(&cfg).unwrap()).unwrap();
    for event in ["post-checkout", "reference-transaction"] {
        let path = root.join(".git/hooks").join(event);
        let text = std::fs::read_to_string(&path).unwrap();
        let swapped: Vec<String> = text
            .lines()
            .map(|l| if l.starts_with("engine=") { format!("engine={}", sh_quote(&stub.to_string_lossy())) } else { l.to_string() })
            .collect();
        script(&path, format!("{}\n", swapped.join("\n")));
    }
}

/// git-shook takes the clone and the gate is registered with it, in the order monitor's `write`
/// does before this module's.
fn adopt_and_register(root: &Path) {
    let hooks = root.join("durable");
    let _ = mockspace_manifest::gate::install_durable_hooks(&hooks, mockspace_manifest::gate::HOOK_VERSION);
    git(root, &["shook", "install", "--yes"]);
    register_gate_with_shook_on(root, &hooks, test_shook_dir().as_deref()).unwrap();
}

fn count(log: &Path, who: &str) -> usize {
    std::fs::read_to_string(log).unwrap_or_default().lines().filter(|l| *l == who).count()
}

// Mutation: leave `ms_shook_keeps` comparing the kept copy with `hooks/<event>` alone. The engine's
// hook has replaced the person's there, the two differ, the gate hands over to it, and it runs the
// chained hook the entry `00-existing` already ran: git-lfs and the regeneration shim, twice per
// checkout.
#[test]
fn a_hook_git_shook_adopted_before_the_data_refs_chained_it_runs_once_per_checkout() {
    let Some(_) = test_shook_dir() else {
        eprintln!("SKIPPED: the git-shook script is not found (GIT_SHOOK_DIR, the PATH, /home/user/git-shook)");
        return;
    };
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let root = tmp.path();
    let (log, stub) = clone(root);
    adopt_and_register(root);
    write_data_refs_hooks(root, &stub);
    let sd = root.join(".git/shook");
    assert!(sd.join("entries/post-checkout/00-existing").is_file(), "control: git-shook kept the person's hook as an entry");
    assert!(root.join(".git/hooks/post-checkout.chained").is_file(), "control: the data refs chained it");
    assert!(std::fs::read_to_string(root.join(".git/hooks/post-checkout")).unwrap().contains("# chains: post-checkout.chained"));

    git(root, &["checkout", "-q", "--detach", "HEAD~1"]);

    assert_eq!(count(&log, "PERSON"), 1, "the person's hook, once: {}", std::fs::read_to_string(&log).unwrap_or_default());
    assert_eq!(count(&log, "ENGINE"), 1, "and the engine's own copy still runs, once");
}

// The control: a hook that arrived after git-shook took the clone is no entry, and the engine's
// hook chains it, so the chain is the only way it runs. Fails if the fix drops the chained run
// everywhere.
#[test]
fn a_hook_that_arrived_after_the_adoption_still_runs_once_through_the_engines_hook() {
    let Some(_) = test_shook_dir() else {
        eprintln!("SKIPPED: the git-shook script is not found (GIT_SHOOK_DIR, the PATH, /home/user/git-shook)");
        return;
    };
    let tmp = mockspace_manifest::scratch::tempdir().unwrap();
    let root = tmp.path();
    let (log, stub) = clone(root);
    // the hook is taken away until the adoption is done, as if it came later
    let own = root.join(".git/hooks/post-checkout");
    let held = std::fs::read(&own).unwrap();
    std::fs::remove_file(&own).unwrap();
    adopt_and_register(root);
    assert!(!root.join(".git/shook/entries/post-checkout/00-existing").exists(), "control: nothing was kept");
    // the person's hook arrives, where the engine's would go, and is chained
    mockspace_manifest::write_executable(&own, String::from_utf8(held).unwrap()).unwrap();
    write_data_refs_hooks(root, &stub);
    assert!(root.join(".git/hooks/post-checkout.chained").is_file(), "control: chained");

    git(root, &["checkout", "-q", "--detach", "HEAD~1"]);

    assert_eq!(count(&log, "PERSON"), 1, "{}", std::fs::read_to_string(&log).unwrap_or_default());
    assert_eq!(count(&log, "ENGINE"), 1);
}
