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
//! This builds a throwaway clone, runs every test of every workspace member as a
//! child `cargo test` with the hook's variables pointing into the clone, the
//! ignored ones included, and holds every byte of the clone, `.git` included,
//! against a snapshot taken before. The ignored tests are the ones that run real
//! hooks and real `cargo build`s, which is where a write into the wrong
//! repository is likeliest, and about two thirds of them pass; the rest are
//! catalogued reds or need a tool or a variable the machine may lack, and a red
//! child is no concern of this test, which reads the clone and not the results.
//!
//! Which targets spawn git is not guessed from their text: a target that
//! drives git through a helper names it nowhere, and 28 of them were missed that
//! way.
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

/// Every top-level integration target of this package, bar the guard itself, by
/// name and path. Not picked by what the source says: a target that drives git
/// through a helper names it nowhere.
fn top_level_targets() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for e in fs::read_dir(root().join("tests")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") {
            continue;
        }
        let name = p.file_stem().unwrap().to_string_lossy().into_owned();
        if name != SELF_TARGET {
            out.push((name, p));
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
        .args(["--", "--skip", SELF_TARGET, "--include-ignored"])
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

    // every test of every workspace member, this guard aside and the ignored
    // ones with it: which targets spawn git is not something to guess at from
    // their text
    let run = run_suite(&clone, &["--workspace"]);
    assert!(
        ran_tests(&run),
        "the child ran no tests, so the clone being untouched proves nothing:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );

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

/// The files that make a test target say it is scrubbed: it includes the one file.
fn declares_the_scrub(text: &str) -> bool {
    text.contains("#[path = \"common/ambient.rs\"]")
}

/// The targets in `targets` whose source does not declare the scrub.
fn unscrubbed(targets: &[(String, PathBuf)]) -> Vec<String> {
    targets
        .iter()
        .filter(|(_, p)| !declares_the_scrub(&fs::read_to_string(p).unwrap()))
        .map(|(n, _)| n.clone())
        .collect()
}

#[test]
fn every_top_level_target_declares_the_scrub() {
    let targets = top_level_targets();
    assert!(
        targets.len() > 30,
        "the scan found {} targets; it is looking in the wrong place",
        targets.len()
    );
    assert_eq!(
        unscrubbed(&targets),
        Vec::<String>::new(),
        "these targets spawn processes without `#[path = \"common/ambient.rs\"] mod ambient;`"
    );
}

#[test]
fn a_target_without_the_scrub_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("bare.rs");
    fs::write(
        &bare,
        "#[test]\nfn t() { std::process::Command::new(\"git\"); }\n",
    )
    .unwrap();
    let kept = dir.path().join("kept.rs");
    fs::write(&kept, "#[path = \"common/ambient.rs\"]\nmod ambient;\n").unwrap();
    assert_eq!(
        unscrubbed(&[("bare".into(), bare), ("kept".into(), kept)]),
        vec!["bare".to_string()]
    );
}

/// The directories of the workspace members other than the root package, read
/// from the root manifest's `members` list.
fn other_members() -> Vec<PathBuf> {
    let text = fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let list = text
        .split("members = [")
        .nth(1)
        .and_then(|t| t.split(']').next())
        .expect("the root manifest lists its members");
    list.split(',')
        .map(|m| m.trim().trim_matches('"'))
        .filter(|m| !m.is_empty() && *m != ".")
        .map(|m| root().join(m))
        .collect()
}

/// Every `.rs` file under `dir`, `target` excluded.
fn rust_files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "target") {
                rust_files(&p, into);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            into.push(p);
        }
    }
}

/// What a member's `lib.rs` says when its tests are scrubbed: it includes the
/// shared file, or, in a package that publishes its tests and so cannot reach
/// above its root, it expands the scrub that package carries.
fn lib_declares_the_scrub(lib: &str) -> bool {
    lib.contains("tests/common/ambient.rs") || lib.contains("scrub_before_main!")
}

/// Members whose own sources spawn git and whose `lib.rs` does not scrub its tests.
fn members_that_spawn_git_unscrubbed(members: &[PathBuf]) -> Vec<String> {
    let mut out = Vec::new();
    for m in members {
        let mut files = Vec::new();
        rust_files(&m.join("src"), &mut files);
        let spawns = files.iter().any(|f| {
            fs::read_to_string(f)
                .unwrap()
                .contains("Command::new(\"git\")")
        });
        let scrubbed = fs::read_to_string(m.join("src/lib.rs"))
            .is_ok_and(|t| lib_declares_the_scrub(&t));
        if spawns && !scrubbed {
            out.push(m.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    out
}

#[test]
fn every_member_that_spawns_git_scrubs_its_tests() {
    let members = other_members();
    assert!(
        members.len() >= 5,
        "the scan found {} other members; it is looking in the wrong place",
        members.len()
    );
    assert_eq!(
        members_that_spawn_git_unscrubbed(&members),
        Vec::<String>::new(),
        "these members spawn git and their `src/lib.rs` neither includes `tests/common/ambient.rs` nor expands `scrub_before_main!`"
    );
}

#[test]
fn a_member_that_spawns_git_without_the_scrub_is_named() {
    let dir = tempfile::tempdir().unwrap();
    for (name, lib) in [
        ("bare", "fn f() { std::process::Command::new(\"git\"); }\n"),
        (
            "kept",
            "fn f() { std::process::Command::new(\"git\"); }\n#[path = \"../../tests/common/ambient.rs\"]\nmod a;\n",
        ),
        (
            "inside",
            "fn f() { std::process::Command::new(\"git\"); }\nmod s { mockspace_manifest::scrub_before_main!(); }\n",
        ),
        ("quiet", "fn f() {}\n"),
    ] {
        fs::create_dir_all(dir.path().join(name).join("src")).unwrap();
        fs::write(dir.path().join(name).join("src/lib.rs"), lib).unwrap();
    }
    let members: Vec<PathBuf> = ["bare", "kept", "inside", "quiet"]
        .iter()
        .map(|n| dir.path().join(n))
        .collect();
    assert_eq!(members_that_spawn_git_unscrubbed(&members), vec![
        "bare".to_string()
    ]);
}
