//! The claim the shared store rests on, run for real: two worktrees of one repo
//! with different lint sources, loaded in turn and at once from one store, each
//! get their own lint and never the other's.
//!
//! `#[ignore]` because every case runs `cargo build` of a release cdylib
//! against the real `mockspace-lint-rules`, and the first one in a store builds
//! that crate's whole dependency graph. Run with `--include-ignored`.
//!
//! The control is the same race with the lock skipped. Without it the cases
//! above would pass for an engine that had no lock at all, which is what this
//! file existed to catch: deleting the `lock_dir` call left every other test
//! green.
//!
//! Every case keeps its store in a directory of its own, deleted with it.

use std::time::Duration;

use std::path::{Path, PathBuf};

use super::key::is_key_name;
use super::support::git;
use crate::config::Config;
use super::*;
use crate::custom_lints::{LoadedLints, load_in};

/// A lint named `name`, in the shape the consumer lints are written.
fn lint_source(name: &str) -> String {
    format!(
        r#"use mockspace::{{CrateLint, Lint, LintContext, LintError}};
pub fn lint() -> Box<dyn CrateLint> {{ Box::new(Probe) }}
struct Probe;
impl Lint for Probe {{
    fn name(&self) -> &'static str {{ "{name}" }}
}}
impl CrateLint for Probe {{
    fn check(&self, _ctx: &LintContext) -> Vec<LintError> {{ Vec::new() }}
}}
"#
    )
}

/// Two worktrees of one clone, each carrying a lint the other does not.
struct Pair {
    _tmp:  tempfile::TempDir,
    cache: PathBuf,
    a:     PathBuf,
    b:     PathBuf,
    /// A third worktree, whose lint neither of the two shares, for warming the dependency graph.
    warm:  PathBuf,
}

/// `tag` goes in the remote, so each case has a key, and so a package name and a
/// built library path, of its own: two cases sharing a key would be one crate
/// built by two tests at once, and the race under test would be theirs.
fn pair(tag: &str) -> Pair {
    let tmp = tempfile::tempdir().unwrap();
    let main = tmp.path().join("clone");
    std::fs::create_dir_all(main.join("mock")).unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &[
        "remote",
        "add",
        "origin",
        &format!("ssh://git@example.invalid/o/probe-{tag}.git"),
    ]);
    std::fs::write(
        main.join("mock").join("mockspace.toml"),
        "project_name = \"probe\"\n",
    )
    .unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-q", "-m", "one"]);

    let mut mocks = Vec::new();
    for (who, lint) in [("a", "lint-from-a"), ("b", "lint-from-b"), ("w", "lint-warm")] {
        let wt = tmp.path().join("wt").join(who);
        git(&main, &[
            "worktree",
            "add",
            "-q",
            wt.to_str().unwrap(),
            "-b",
            who,
        ]);
        let lints = wt.join("mock").join("lints");
        std::fs::create_dir_all(&lints).unwrap();
        std::fs::write(
            lints.join(format!("{}.rs", lint.replace('-', "_"))),
            lint_source(lint),
        )
        .unwrap();
        mocks.push(wt.join("mock"));
    }
    Pair {
        cache: tmp.path().join("cache"),
        warm:  mocks.pop().unwrap(),
        b:     mocks.pop().unwrap(),
        a:     mocks.pop().unwrap(),
        _tmp:  tmp,
    }
}

fn dep() -> String {
    let lint_rules = concat!(env!("CARGO_MANIFEST_DIR"), "/lint-rules");
    format!("{{ package = \"mockspace-lint-rules\", path = \"{lint_rules}\" }}")
}

fn load_of(cache: &Path, mock: &Path) -> Result<LoadedLints, String> {
    let cfg = Config::from_dir(mock);
    load_in(cache, &cfg, &mock.join("mockspace.toml"), &dep())
        .and_then(|l| l.ok_or_else(|| "nothing to load".to_string()))
}

/// The one lint a load returned, by name.
fn name_of(loaded: &LoadedLints) -> String {
    assert_eq!(loaded.pack.crate_lints.len(), 1);
    loaded.pack.crate_lints[0].name().to_string()
}

/// Load, read the name, unload. The library is dropped before the next load of
/// the same path because the dynamic loader answers a second `dlopen` of a path
/// it still has mapped with the mapping it has, whatever the file at the path
/// now is. One `mock` run loads once, so that never arises there; here it would
/// make the test measure the loader.
fn name_loaded(cache: &Path, mock: &Path) -> Result<String, String> {
    load_of(cache, mock).map(|l| name_of(&l))
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn worktrees_with_different_lints_each_load_their_own() {
    let p = pair("shared");

    // In turn, each time the other's build is what sits in the store.
    for (mock, want) in [
        (&p.a, "lint-from-a"),
        (&p.b, "lint-from-b"),
        (&p.a, "lint-from-a"),
        (&p.b, "lint-from-b"),
    ] {
        assert_eq!(name_loaded(&p.cache, mock).unwrap(), want, "in turn");
    }

    // At once, with the window between writing the crate and building it held
    // open so that a missing lock is a certain failure and not a lucky one.
    knobs::set(&p.cache, false, Duration::from_millis(1500));
    let results: Vec<(String, Result<String, String>)> = std::thread::scope(|s| {
        let handles: Vec<_> = [
            (&p.a, "lint-from-a"),
            (&p.b, "lint-from-b"),
            (&p.a, "lint-from-a"),
            (&p.b, "lint-from-b"),
        ]
        .into_iter()
        .map(|(mock, want)| {
            let cache = &p.cache;
            (want.to_string(), s.spawn(move || name_loaded(cache, mock)))
        })
        .collect();
        handles
            .into_iter()
            .map(|(want, h)| (want, h.join().unwrap()))
            .collect()
    });
    for (want, got) in results {
        assert_eq!(got.as_deref(), Ok(want.as_str()), "at once");
    }

    // The store holds one crate for the two, which is the sharing.
    let keys = std::fs::read_dir(&p.cache)
        .unwrap()
        .flatten()
        .filter(|e| is_key_name(&e.file_name().to_string_lossy()))
        .count();
    assert_eq!(keys, 1, "two worktrees of one repo are one key");
}

#[test]
#[ignore = "runs cargo build; run with --include-ignored"]
fn without_the_lock_the_same_race_loads_the_wrong_lint() {
    // The negative control for the case above: the same two worktrees at once,
    // the same open window, and no lock. A peer rewrites the crate between this
    // run's write and its build, so a run builds and loads the other's lints, or
    // meets a half-written file and fails. If this passes with both right, the
    // case above proves nothing about the lock.
    let p = pair("unlocked");
    // Warm the dependency graph under the lock, with a lint of neither side, so the race below
    // is about the lint crate and not about two cold builds queueing on cargo, and both sides
    // are cold identities: a kept library for either would answer without a build to race.
    assert_eq!(name_loaded(&p.cache, &p.warm).unwrap(), "lint-warm");

    knobs::set(&p.cache, true, Duration::from_millis(1500));
    let results: Vec<(&str, Result<String, String>)> = std::thread::scope(|s| {
        let handles: Vec<_> = [(&p.a, "lint-from-a"), (&p.b, "lint-from-b")]
            .into_iter()
            .map(|(mock, want)| {
                let cache = &p.cache;
                (want, s.spawn(move || name_loaded(cache, mock)))
            })
            .collect();
        handles
            .into_iter()
            .map(|(want, h)| (want, h.join().unwrap()))
            .collect()
    });
    let all_right = results
        .iter()
        .all(|(want, got)| got.as_deref() == Ok(*want));
    assert!(
        !all_right,
        "with no lock both runs got their own lint, so the race this guards is not being run: {results:?}"
    );
}
