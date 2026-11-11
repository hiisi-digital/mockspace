//! What the daily commands and each migration cost on a mock directory of hundreds of files and on
//! one five times its size, counted in git processes started and objects written rather than in
//! time. A command whose work is one edited file costs the same on both; a migration that moves a
//! directory starts a process for each batch of the files it moves, never one for each file.
//! A cost that grows with the tree where the change did not is a finding.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::consumer::{self, Shape};
use refs_e2e::spy::{self, Spy, loose_objects};
use refs_e2e::Clone;

const SMALL: usize = 400;
const LARGE: usize = 2000;
const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";

fn shape(bulk: usize) -> Shape {
    Shape { name: "scaling", bulk, ..consumer::LARGE }
}

/// What one command cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cost {
    spawns:  usize,
    objects: usize,
}

fn measure(c: &Clone, spy: &Spy, args: &[&str]) -> Cost {
    let git_dir = c.root.join(".git");
    let before = loose_objects(&git_dir);
    spy.clear();
    c.ok(args);
    Cost { spawns: spy.spawns(), objects: loose_objects(&git_dir).saturating_sub(before) }
}

fn strs(a: &[String]) -> Vec<&str> {
    a.iter().map(String::as_str).collect()
}

/// A person's clone of a migrated consumer of `bulk` research files, every ref instantiated, the
/// stamps of every file taken, nothing packed while it is measured.
fn daily_costs(bulk: usize) -> Vec<(&'static str, Cost)> {
    let s = shape(bulk);
    let w = consumer::migrated(&s);
    let a = w.clone_of("a");
    let b = w.clone_of("b");
    for c in [&a, &b] {
        c.git(&["config", "gc.auto", "0"]);
        c.ok(&["data-refs", "instantiate"]);
    }
    // a stamp taken within two seconds of the file's write is not trusted, so a write after it in
    // the same second still shows; past that every file's stamp is taken once and kept
    std::thread::sleep(std::time::Duration::from_secs(3));
    for c in [&a, &b] {
        c.ok(&["data-refs", "status"]);
    }
    let spy = spy::install(&w);
    let mut out = Vec::new();
    out.push(("status, nothing edited", measure(&a, &spy, &["data-refs", "status", "research"])));
    a.write(NOTES, b"one file edited\n");
    // past the two seconds, so whether the edit's stamp is trusted does not hang on the clock
    std::thread::sleep(std::time::Duration::from_secs(3));
    out.push(("status, one file edited", measure(&a, &spy, &["data-refs", "status", "research"])));
    out.push(("the hook's copy, one file edited", measure(&a, &spy, &["data-refs", "snapshot"])));
    out.push(("commit, one file edited", measure(&a, &spy, &["data-refs", "commit", "research"])));
    out.push(("update, one file from another clone", measure(&b, &spy, &["data-refs", "update", "research"])));
    out
}

fn migration_costs(bulk: usize) -> Vec<(String, Cost)> {
    let s = shape(bulk);
    let w = consumer::remote(&s);
    let c = w.clone_of("a");
    c.git(&["config", "gc.auto", "0"]);
    let _ = c.run(&["status"]);
    let spy = spy::install(&w);
    let mut out = Vec::new();
    for args in s.calls() {
        let what = if args[1] == "directory-to-ref" { format!("directory-to-ref {}", args[3]) } else { args[1].clone() };
        out.push((what, measure(&c, &spy, &strs(&args))));
        c.commit_all("chore: a migration");
    }
    out
}

/// The costs at both sizes side by side, for the failure message and the report.
fn table<K: std::fmt::Display>(small: &[(K, Cost)], large: &[(K, Cost)]) -> String {
    small
        .iter()
        .zip(large)
        .map(|((what, s), (_, l))| format!("{what}: spawns {} -> {}, objects {} -> {}", s.spawns, l.spawns, s.objects, l.objects))
        .collect::<Vec<_>>()
        .join("\n")
}

// Mutation: list the instance's files with a git call each, or hash every file on each look, or
// write the whole tree's blobs on a commit of one file; the costs at the larger size then grow.
#[test]
fn a_command_on_one_edited_file_costs_the_same_on_a_tree_five_times_larger() {
    let (small, large) = (daily_costs(SMALL), daily_costs(LARGE));
    let shown = table(&small, &large);
    eprintln!("{shown}");
    for ((what, s), (_, l)) in small.iter().zip(&large) {
        assert_eq!(s.spawns, l.spawns, "{what}: git processes grow with the tree\n{shown}");
        assert_eq!(s.objects, l.objects, "{what}: objects written grow with the tree\n{shown}");
    }
    let (status, commit) = (large[1].1, large[3].1);
    // the copy: the blob, the trees of its directory, of `edit`, of the worktree and of the root,
    // and the commit
    assert!(status.objects <= 6, "a look at one edited file writes no more than its copy\n{shown}");
    assert!(commit.objects <= 8, "a commit of one file writes its blob, trees and commit, and the copy's\n{shown}");
}

/// Files the engine hands one git process at a time, where it hashes or writes many.
const BATCH: usize = 200;

// Mutation: write each moved file's blob with a git call of its own, or untrack the moved files
// one call each; the migrations then start a process more for every file more.
#[test]
fn a_migration_starts_no_more_git_processes_on_a_tree_five_times_larger() {
    let (small, large) = (migration_costs(SMALL), migration_costs(LARGE));
    let shown = table(&small, &large);
    eprintln!("{shown}");
    // a migration reads every file it moves, so a process for each batch of the extra files is
    // its change; a process for each file is not
    let batches = (LARGE - SMALL).div_ceil(BATCH);
    for ((what, s), (_, l)) in small.iter().zip(&large) {
        assert!(l.spawns <= s.spawns + batches, "{what}: git processes grow with each file\n{shown}");
    }
    // the move of `research` writes nothing new: every blob it puts on the ref is the branch's
    let research = large.iter().find(|(w, _)| w == "directory-to-ref research").unwrap().1;
    assert!(research.objects <= 4 + 200, "the move writes trees, not blobs the branch has\n{shown}");
}
