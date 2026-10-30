//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A run that replays saved verdicts reports what a full run reports, commit by commit, over a real
//! repository's history.
//!
//! The suite cannot hold a consumer repository, so this is opt in. It walks the last
//! `MOCKSPACE_HISTORY_COMMITS` commits of the repository checked out at `MOCKSPACE_HISTORY_WORKTREE`
//! (a scratch worktree the test is free to move) oldest first. At each it runs the launcher at
//! `MOCKSPACE_HISTORY_LAUNCHER` twice with `--lint-only --commit`: once with the verdict store off,
//! which is a full run, and once with a store shared across every commit of the walk, which is the
//! incremental one. The two reports must be the same text once the timing table and the build chatter
//! are taken out.
//!
//! The store is shared across the commits on purpose: replay across a changing history is what is
//! under test, and a store emptied at every commit would only test the cold path.
//!
//! `MOCKSPACE_HISTORY_PIN` names the engine branch every commit is pinned to, so an older commit
//! that pins `dev` still runs the engine under test. `MOCKSPACE_HISTORY_PACKS`, a list of
//! `name=path` pairs, redirects a pack to a patched copy for the same reason.

use std::path::{Path, PathBuf};
use std::process::Command;

const WORKTREE: &str = "MOCKSPACE_HISTORY_WORKTREE";
const LAUNCHER: &str = "MOCKSPACE_HISTORY_LAUNCHER";
const COMMITS: &str = "MOCKSPACE_HISTORY_COMMITS";
const PIN: &str = "MOCKSPACE_HISTORY_PIN";
const PACKS: &str = "MOCKSPACE_HISTORY_PACKS";

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// What two runs of one tree must agree on: the report without its timings and its build chatter.
fn report(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.starts_with("--- timings") {
            break;
        }
        let t = line.trim_start();
        if t.starts_with("Compiling")
            || t.starts_with("Finished")
            || t.starts_with("Updating")
            || t.starts_with("Locking")
            || t.starts_with("Fresh")
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn run(launcher: &Path, tree: &Path, store: Option<&Path>) -> String {
    let mut cmd = Command::new(launcher);
    cmd.current_dir(tree).args(["--lint-only", "--commit"]).env("MOCKSPACE_TIMINGS", "1");
    match store {
        Some(dir) => cmd.env("MOCKSPACE_VERDICTS_DIR", dir).env_remove("MOCKSPACE_NO_VERDICTS"),
        None => cmd.env("MOCKSPACE_NO_VERDICTS", "1"),
    };
    let out = cmd.output().expect("the launcher runs");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The config of the commit under test, pointed at the engine and packs under test.
fn repoint(tree: &Path, pin: &str, packs: &[(String, String)]) {
    let path = tree.join("mockspace.toml");
    let text = std::fs::read_to_string(&path).expect("the tree has a mockspace.toml");
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("mockspace_branch") {
            out.push(format!("mockspace_branch = \"{pin}\""));
        } else if let Some((name, dir)) =
            packs.iter().find(|(n, _)| trimmed.starts_with(&format!("{n} =")))
        {
            out.push(format!("{name} = {{ path = \"{dir}\" }}"));
        } else {
            out.push(line.to_string());
        }
    }
    std::fs::write(&path, out.join("\n") + "\n").expect("the config is written");
}

#[test]
#[ignore = "walks a real repository's history; set MOCKSPACE_HISTORY_WORKTREE and friends"]
fn an_incremental_run_equals_a_full_one_over_a_history() {
    let Some(tree) = std::env::var_os(WORKTREE).map(PathBuf::from) else {
        panic!("{WORKTREE} names the scratch worktree to walk");
    };
    let launcher = PathBuf::from(std::env::var_os(LAUNCHER).expect("the launcher to run"));
    let n: usize = std::env::var(COMMITS).ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let pin = std::env::var(PIN).expect("the engine branch to pin every commit to");
    let packs: Vec<(String, String)> = std::env::var(PACKS)
        .unwrap_or_default()
        .split(',')
        .filter_map(|p| p.split_once('=').map(|(a, b)| (a.to_string(), b.to_string())))
        .collect();

    let walked = git(&tree, &["rev-list", "--first-parent", "--reverse", &format!("-{n}"), "HEAD"]);
    let commits: Vec<&str> = walked.lines().collect();
    assert!(commits.len() >= 2, "a history of one commit proves nothing about replay");
    let store = tempfile::tempdir().unwrap();

    let mut differing = Vec::new();
    for sha in &commits {
        git(&tree, &["checkout", "-q", "--detach", "-f", sha]);
        repoint(&tree, &pin, &packs);
        let full = report(&run(&launcher, &tree, None));
        let incremental = report(&run(&launcher, &tree, Some(store.path())));
        // the control: a run that reported nothing at all would agree with anything
        assert!(
            full.contains("--- running lints ---"),
            "{sha}: the full run never reached the lints:\n{full}"
        );
        if full != incremental {
            differing.push(format!("{sha}\n--- full\n{full}\n--- incremental\n{incremental}"));
        }
    }
    assert!(differing.is_empty(), "{} of {} commits differ:\n{}", differing.len(), commits.len(), differing.join("\n"));
}

#[test]
fn the_report_drops_the_timings_and_the_build_chatter_and_keeps_findings() {
    let text = "   Compiling x v1\n--- running lints ---\n  [a] f.rs:1: [error] x\n--- timings (ms) ---\n  3.0 1 lint:a\n";
    assert_eq!(report(text), "--- running lints ---\n  [a] f.rs:1: [error] x\n");
    assert_ne!(
        report(text),
        report("--- running lints ---\n  [a] f.rs:2: [error] x\n"),
        "a different finding must be a different report"
    );
}
