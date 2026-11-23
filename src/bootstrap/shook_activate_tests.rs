//! Activation where `git shook` runs: the gate is registered through it, and git-shook takes
//! `core.hooksPath` whatever the key held. A clone the gate held the key of is moved over, and every
//! event that ran before still runs after it, once. Where `git shook` does not run, the gate takes
//! the key itself and says so in one line.
//!
//! The tests need the real `git shook`, and say so when they cannot run rather than passing.

use std::path::PathBuf;

use super::tests::{git, have_shook, repo};
use super::*;

/// A directory whose `git-shook` refuses to run, put in front of the `PATH`: `git shook` does not run
/// there, whatever else the machine has.
fn no_shook(root: &Path) -> PathBuf {
    let bin = root.join("no-shook-bin");
    fs::create_dir_all(&bin).unwrap();
    crate::test_exec::write_executable(&bin.join("git-shook"), "#!/bin/sh\nexit 1\n");
    bin
}

/// A clone with a generated hooks directory, so `activate` gets to the key, and one hook of the
/// person's own at `<dir>/<place>/pre-commit` that appends `WHO` to the returned log.
fn clone_with_hook(place: &str, who: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
    let guard = repo("activate");
    let dir = guard.path().to_path_buf();
    let mock = dir.join("mock");
    // `hooks_path_target` answers the generated directory in a test, where each call to
    // `durable_hooks_dir` is a new scratch directory, so the gate's own hooks go there
    let gate = mock.join("target").join("hooks");
    let _ = mockspace_manifest::gate::install_durable_hooks(&gate, HOOK_VERSION);
    let log = dir.join("user.log");
    crate::test_exec::write_executable(
        &dir.join(place).join("pre-commit"),
        format!("#!/bin/sh\necho {who} >> {}\n", log.display()),
    );
    (guard, dir, mock, log)
}

fn commit(dir: &Path, name: &str) {
    fs::write(dir.join(name), name).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-q", "-m", name]);
}

fn runs_of(log: &Path, who: &str) -> usize {
    fs::read_to_string(log).unwrap_or_default().lines().filter(|l| *l == who).count()
}

fn key(dir: &Path) -> String {
    git(dir, &["config", "--local", "core.hooksPath"])
}

fn activate_say(dir: &Path, mock: &Path, on_path: Option<&Path>) -> Vec<String> {
    let mut said = Vec::new();
    activate_with(dir, mock, on_path, &mut |l| said.push(l.to_string())).unwrap();
    said
}

#[test]
fn a_clone_with_no_key_is_registered_through_git_shook_and_keeps_its_hook() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not found");
        return;
    }
    let (_g, dir, mock, log) = clone_with_hook(".git/hooks", "USER");
    let shook = test_shook_dir();

    activate_say(&dir, &mock, shook.as_deref());

    assert!(is_shook_path(&dir, &key(&dir)), "git-shook took the key: {}", key(&dir));
    assert!(is_active(&dir));
    assert!(has_manifest(&dir));
    commit(&dir, "a.txt");
    assert_eq!(runs_of(&log, "USER"), 1, "the hook that was in `.git/hooks` runs, once");
}

#[test]
fn a_clone_whose_key_the_gate_held_is_moved_over_and_every_event_still_runs_once() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not found");
        return;
    }
    let (_g, dir, mock, log) = clone_with_hook(".git/hooks", "USER");
    let shook = test_shook_dir();
    // the way it was: the gate took the key itself
    let old = no_shook(&dir);
    activate_say(&dir, &mock, Some(&old));
    assert!(mockspace_manifest::gate::is_ours(&key(&dir)), "control: the gate holds the key: {}", key(&dir));
    commit(&dir, "before.txt");
    assert_eq!(runs_of(&log, "USER"), 1, "control: the person's hook ran through the gate's chain");

    activate_say(&dir, &mock, shook.as_deref());

    assert!(is_shook_path(&dir, &key(&dir)), "git-shook holds it now: {}", key(&dir));
    assert!(is_active(&dir));
    // the gate's own hooks are an entry, not also a kept hook beside it
    let kept = shook_dir(&dir).join("entries/pre-commit/00-existing");
    assert!(!kept.exists(), "the gate's own hook is not kept as an existing one");
    commit(&dir, "after.txt");
    assert_eq!(runs_of(&log, "USER"), 2, "the same hook ran once more, not twice");
}

#[test]
fn a_clone_on_another_tools_hooks_path_is_moved_over_and_its_hook_still_runs_once() {
    if !have_shook() {
        eprintln!("SKIPPED: git shook is not found");
        return;
    }
    let (_g, dir, mock, log) = clone_with_hook(".husky", "HUSKY");
    git(&dir, &["config", "--local", "core.hooksPath", ".husky"]);
    commit(&dir, "before.txt");
    assert_eq!(runs_of(&log, "HUSKY"), 1, "control: the other tool's hook runs on its own key");

    activate_say(&dir, &mock, test_shook_dir().as_deref());

    assert!(is_shook_path(&dir, &key(&dir)), "git-shook holds it now: {}", key(&dir));
    commit(&dir, "after.txt");
    assert_eq!(runs_of(&log, "HUSKY"), 2, "the adopted hook runs once more, not twice and not never");
}

#[test]
fn where_git_shook_does_not_run_the_gate_takes_the_key_and_says_so_in_one_line() {
    let (_g, dir, mock, _log) = clone_with_hook(".git/hooks", "USER");
    let none = no_shook(&dir);

    let said = activate_say(&dir, &mock, Some(&none));

    assert!(mockspace_manifest::gate::is_ours(&key(&dir)), "the gate took the key: {}", key(&dir));
    assert!(!has_manifest(&dir), "nothing was registered with git-shook");
    let line: Vec<_> = said.iter().filter(|l| l.contains("git shook")).collect();
    assert_eq!(line.len(), 1, "one line says why: {said:?}");
    assert!(!line[0].contains('\n'), "{line:?}");
    assert!(line[0].contains("core.hooksPath"), "{line:?}");
}
