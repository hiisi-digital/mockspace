//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A suite started from inside a repository leaves that repository as it found it.
//!
//! A git hook exports `GIT_DIR`, `GIT_WORK_TREE` and `GIT_INDEX_FILE` into what
//! it runs, and `GIT_DIR=<repo>/.git cargo test` does the same by hand. Those
//! variables outrank a child's working directory, so a test that ran `git` in
//! its own temporary repository ran it in the one the suite was started from:
//! HEAD moved onto test commits, fixture files were staged into the index, a
//! branch appeared and `user.name` landed in the config.
//!
//! This builds a throwaway clone, runs every test that spawns git (the lib
//! tests, every integration target that names git, and the lint pack's lib
//! tests) as a child `cargo test` with the hook's variables pointing into the
//! clone, and holds every byte of the clone, `.git` included, against a
//! snapshot taken before.
//!
//! The control is that the child ran tests at all: a child that failed to start
//! leaves the clone untouched too.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "common/ambient.rs"]
mod ambient;

/// Set in the child so it does not run this test again, which would recurse.
const CHILD: &str = "MOCKSPACE_SUITE_GUARD_CHILD";

const SELF_TARGET: &str = "the_suite_leaves_the_repository_it_started_in_alone";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut c = Command::new("git");
    ambient::clean_of_the_machine(&mut c);
    let out = c
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=Guard", "-c", "user.email=guard@example.com"])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A clone with a commit, a second branch, a config key of its own and a clean
/// index, made from a repository of its own so it has a remote like a real one.
fn clone_in(base: &Path) -> PathBuf {
    let origin = base.join("origin");
    fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    fs::write(
        origin.join("README"),
        "the repository the suite was started from\n",
    )
    .unwrap();
    git(&origin, &["add", "README"]);
    git(&origin, &["commit", "-q", "-m", "chore: start"]);

    let clone = base.join("clone");
    git(base, &[
        "clone",
        "-q",
        origin.to_str().unwrap(),
        clone.to_str().unwrap(),
    ]);
    git(&clone, &["branch", "topic"]);
    git(&clone, &["config", "guard.sentinel", "untouched"]);
    // a refresh of the index belongs before the snapshot, not in it
    git(&clone, &["status", "--porcelain"]);
    assert_eq!(
        git(&clone, &["status", "--porcelain"]),
        "",
        "the clone starts clean"
    );
    clone
}

/// Every file under the clone, `.git` included, by path.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                walk(base, &p, into);
            } else {
                into.insert(rel, fs::read(&p).unwrap_or_default());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// What differs, named, so a failure says which part of the clone was written.
fn differences(
    before: &BTreeMap<String, Vec<u8>>,
    after: &BTreeMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (k, v) in before {
        match after.get(k) {
            None => out.push(format!("removed  {k}")),
            Some(a) if a != v => out.push(format!("changed  {k}")),
            Some(_) => {},
        }
    }
    for k in after.keys().filter(|k| !before.contains_key(*k)) {
        out.push(format!("added    {k}"));
    }
    out
}

/// The integration targets that spawn git: those whose source names it.
fn targets_that_spawn_git() -> Vec<String> {
    let mut out = Vec::new();
    for e in fs::read_dir(root().join("tests")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        if name == SELF_TARGET {
            continue;
        }
        let text = fs::read_to_string(&p).unwrap();
        if text.contains("\"git\"") || text.contains("git_in(") || text.contains("git_init") {
            out.push(name);
        }
    }
    out.sort();
    out
}

fn run_suite(clone: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO"))
        .current_dir(root())
        .args(["test", "--no-fail-fast", "--color", "never"])
        .args(args)
        .args(["--", "--skip", SELF_TARGET])
        .env(CHILD, "1")
        .env("GIT_DIR", clone.join(".git"))
        .env("GIT_WORK_TREE", clone)
        .env("GIT_INDEX_FILE", clone.join(".git/index"))
        .output()
        .expect("cargo runs")
}

fn ran_tests(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stdout).contains("test result:")
}

#[test]
fn the_suite_leaves_the_repository_it_started_in_alone() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let clone = clone_in(base.path());
    let before = snapshot(&clone);

    let targets = targets_that_spawn_git();
    assert!(
        targets.len() > 5,
        "the scan found {} integration targets that name git; it is looking in the wrong place",
        targets.len()
    );
    let mut root_args = vec!["--lib".to_string()];
    for t in &targets {
        root_args.push("--test".into());
        root_args.push(t.clone());
    }
    let root_args: Vec<&str> = root_args.iter().map(String::as_str).collect();

    let root_run = run_suite(&clone, &root_args);
    let pack_run = run_suite(&clone, &["--package", "mockspace-lint-rules", "--lib"]);

    for (what, out) in [("the root package", &root_run), ("the lint pack", &pack_run)] {
        assert!(
            ran_tests(out),
            "{what}: the child ran no tests, so the clone being untouched proves nothing:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let after = snapshot(&clone);
    let diff = differences(&before, &after);
    assert!(
        diff.is_empty(),
        "the suite wrote into the repository it was started from ({} paths):\n{}\n\
         HEAD is now {}",
        diff.len(),
        diff.iter().take(30).cloned().collect::<Vec<_>>().join("\n"),
        git(&clone, &["log", "-1", "--format=%h %s"]).trim()
    );
}
