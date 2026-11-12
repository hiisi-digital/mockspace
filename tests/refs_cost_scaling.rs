//! What the daily commands and each migration cost on a mock directory of hundreds of files and on
//! one five times its size, its research, templates and registry rows grown alike, counted in git
//! processes started, in the bytes those processes are handed as arguments and input, and in
//! objects written, rather than in time. A command whose work is one edited file costs the same on
//! both, in every count: a look, a copy, a commit and an update of `research`, the same of the ref
//! by pattern that holds the templates, `verify`, a `restore` of one file, and the lists of the held
//! files the gates are told on every lint run. A command that writes every file of an instance,
//! `instantiate` and a `restore` of a directory taken off whole, and a migration that moves a
//! directory, start a process for each batch of the files, never one for each file, and the move
//! writes no blob the branch has. A cost that grows with the tree where the change did not is a
//! finding.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::consumer::{self, Shape};
use refs_e2e::spy::{self, Spy, loose_objects};
use refs_e2e::Clone;

const SMALL: usize = 400;
const LARGE: usize = 2000;
const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
const TEMPLATE: &str = "mock/crates/alpha/docs/gen/t00000.md.tmpl";

/// A consumer of `bulk` research files, and a template and a row file for every four of them.
fn shape(bulk: usize) -> Shape {
    Shape { name: "scaling", bulk, cites: bulk / 4, ..consumer::LARGE }
}

/// What one command cost.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cost {
    spawns:  usize,
    handed:  usize,
    objects: usize,
    /// The calls by the bytes they were handed, the largest first, for the failure message.
    largest: Vec<(String, usize)>,
}

fn measure(c: &Clone, spy: &Spy, args: &[&str]) -> Cost {
    let git_dir = c.root.join(".git");
    let before = loose_objects(&git_dir);
    spy.clear();
    c.ok(args);
    let mut largest = spy.handed_by_call();
    largest.truncate(3);
    Cost { spawns: spy.spawns(), handed: spy.handed(), objects: loose_objects(&git_dir).saturating_sub(before), largest }
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
    spy.measure_input();
    let mut out = Vec::new();
    out.push(("status, nothing edited", measure(&a, &spy, &["data-refs", "status", "research"])));
    a.write(NOTES, b"one file edited\n");
    // past the two seconds, so whether the edit's stamp is trusted does not hang on the clock
    std::thread::sleep(std::time::Duration::from_secs(3));
    out.push(("status, one file edited", measure(&a, &spy, &["data-refs", "status", "research"])));
    out.push(("the hook's copy, one file edited", measure(&a, &spy, &["data-refs", "snapshot"])));
    out.push(("commit, one file edited", measure(&a, &spy, &["data-refs", "commit", "research"])));
    out.push(("update, one file from another clone", measure(&b, &spy, &["data-refs", "update", "research"])));
    // the ref by pattern, whose instance is the crates directory and holds its templates alone
    a.write(TEMPLATE, b"one template edited\n");
    std::thread::sleep(std::time::Duration::from_secs(3));
    out.push(("status of the ref by pattern, one template edited", measure(&a, &spy, &["data-refs", "status", "templates"])));
    out.push(("commit of the ref by pattern, one template edited", measure(&a, &spy, &["data-refs", "commit", "templates"])));
    out.push(("verify, nothing edited", measure(&a, &spy, &["data-refs", "verify"])));
    a.remove(NOTES);
    out.push(("restore, one file taken off", measure(&a, &spy, &["data-refs", "restore", "research"])));
    // the gates are told the files the refs hold and which of them changed, on every lint run
    a.write(NOTES, b"one file edited for the gates\n");
    std::thread::sleep(std::time::Duration::from_secs(3));
    out.push(("the gates' lists of the held files, one file edited", measure(&a, &spy, &["--lint-only", "--scope", "infra"])));
    out
}

/// What the commands that write every file of an instance cost: `instantiate` in a fresh clone,
/// and `restore` of a directory taken off whole.
fn whole_instance_costs(bulk: usize) -> Vec<(&'static str, Cost)> {
    let s = shape(bulk);
    let w = consumer::migrated(&s);
    let a = w.clone_of("a");
    a.git(&["config", "gc.auto", "0"]);
    let _ = a.run(&["status"]);
    let spy = spy::install(&w);
    spy.measure_input();
    let mut out = Vec::new();
    out.push(("instantiate, every ref", measure(&a, &spy, &["data-refs", "instantiate"])));
    std::fs::remove_dir_all(a.path("mock/research")).unwrap();
    out.push(("restore, every file of research", measure(&a, &spy, &["data-refs", "restore", "research"])));
    out
}

/// Each migration's cost, and the directories of `research` on the branch before it moved, the
/// root of it counted.
fn migration_costs(bulk: usize) -> (Vec<(String, Cost)>, usize) {
    let s = shape(bulk);
    let w = consumer::remote(&s);
    let c = w.clone_of("a");
    c.git(&["config", "gc.auto", "0"]);
    let _ = c.run(&["status"]);
    let spy = spy::install(&w);
    spy.measure_input();
    let dirs = 1 + c.git(&["ls-tree", "-r", "-d", "--name-only", "HEAD", "--", "mock/research"]).lines().count();
    let mut out = Vec::new();
    for args in s.calls() {
        let what = if args[1] == "directory-to-ref" { format!("directory-to-ref {}", args[3]) } else { args[1].clone() };
        out.push((what, measure(&c, &spy, &strs(&args))));
        c.commit_all("chore: a migration");
    }
    (out, dirs)
}

/// The costs at both sizes side by side, for the failure message and the report.
fn table<K: std::fmt::Display>(small: &[(K, Cost)], large: &[(K, Cost)]) -> String {
    small
        .iter()
        .zip(large)
        .map(|((what, s), (_, l))| {
            format!(
                "{what}: spawns {} -> {}, handed {} -> {} bytes, objects {} -> {}; largest {:?}",
                s.spawns, l.spawns, s.handed, l.handed, s.objects, l.objects, l.largest
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// Mutation: list the instance's files with a git call each, or hash every file on each look, or
// hand git every path of the tree on a look, or write the whole tree's blobs on a commit of one
// file; the costs at the larger size then grow.
#[test]
fn a_command_on_one_edited_file_costs_the_same_on_a_tree_five_times_larger() {
    let (small, large) = (daily_costs(SMALL), daily_costs(LARGE));
    let shown = table(&small, &large);
    eprintln!("{shown}");
    for ((what, s), (_, l)) in small.iter().zip(&large) {
        assert_eq!(s.spawns, l.spawns, "{what}: git processes grow with the tree\n{shown}");
        assert_eq!(s.handed, l.handed, "{what}: what git is handed grows with the tree\n{shown}");
        assert_eq!(s.objects, l.objects, "{what}: objects written grow with the tree\n{shown}");
    }
    let (status, commit) = (&large[1].1, &large[3].1);
    // the copy: the blob, the trees of its directory, of `edit`, of the worktree and of the root,
    // and the commit
    assert!(status.objects <= 6, "a look at one edited file writes no more than its copy\n{shown}");
    assert!(commit.objects <= 8, "a commit of one file writes its blob, trees and commit, and the copy's\n{shown}");
}

/// Files the engine hands one git process at a time, where it hashes or writes many.
const BATCH: usize = 200;

// Mutation: write each moved file's blob with a git call of its own, or untrack the moved files
// one call each; the migrations then start a process more for every file more. Write the blobs of
// the moved files again; the move of `research` then writes one object for every file.
#[test]
fn a_migration_starts_no_more_git_processes_on_a_tree_five_times_larger() {
    let ((small, _), (large, dirs)) = (migration_costs(SMALL), migration_costs(LARGE));
    let shown = table(&small, &large);
    eprintln!("{shown}");
    // a migration reads every file it moves, so a process for each batch of the extra files is
    // its change; a process for each file is not. The extra files are the research, the templates
    // and the row files the larger tree has more of.
    let extra = (LARGE - SMALL) + 2 * (shape(LARGE).cites - shape(SMALL).cites);
    let batches = extra.div_ceil(BATCH);
    for ((what, s), (_, l)) in small.iter().zip(&large) {
        assert!(l.spawns <= s.spawns + batches, "{what}: git processes grow with each file\n{shown}");
    }
    // the move of `research` writes nothing the branch has: one tree for each directory of it,
    // as the ref holds it at its root, and the ref's commit; every blob it puts there is the
    // branch's already
    let research = &large.iter().find(|(w, _)| w == "directory-to-ref research").unwrap().1;
    let bound = dirs + 1;
    assert!(research.objects <= bound, "the move writes {dirs} trees and a commit, not blobs the branch has\n{shown}");
}

// Mutation: write each file of an instance with a git call of its own, or read each blob with one;
// the commands then start a process more for every file more.
#[test]
fn a_command_that_writes_every_file_starts_no_more_git_processes_on_a_tree_five_times_larger() {
    let (small, large) = (whole_instance_costs(SMALL), whole_instance_costs(LARGE));
    let shown = table(&small, &large);
    eprintln!("{shown}");
    let extra = (LARGE - SMALL) + (shape(LARGE).cites - shape(SMALL).cites);
    let batches = extra.div_ceil(BATCH);
    for ((what, s), (_, l)) in small.iter().zip(&large) {
        assert!(l.spawns <= s.spawns + batches, "{what}: git processes grow with each file\n{shown}");
        assert_eq!(s.objects, l.objects, "{what}: objects written grow with the tree\n{shown}");
    }
}
