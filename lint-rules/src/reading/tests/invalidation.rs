//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A saved unit is replayed exactly while every question in its trace has the answer it had.
//!
//! Each test runs a probe that asks one kind of question, then moves the answer, or moves
//! something the probe never asked, and counts the times the probe was judged.

use std::path::Path;
use std::sync::atomic::Ordering;

use super::*;

#[test]
fn a_read_file_changing_reruns_the_unit() {
    let dir = Dir::new("read");
    dir.write("a.txt", "one");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| v.text("a.txt").unwrap_or("absent").to_string());
    let pack = pack_of(probe);

    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    assert_eq!(judged.load(Ordering::SeqCst), 1, "nothing moved, so the second run replays");

    dir.write("a.txt", "two");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["two"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

#[test]
fn a_file_appearing_in_a_listed_directory_reruns_the_unit() {
    let dir = Dir::new("list");
    dir.write("d/a.txt", "");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| {
        v.list("d")
            .map(|l| l.entries().iter().map(|e| e.name().to_string()).collect::<Vec<_>>().join(","))
            .unwrap_or_default()
    });
    let pack = pack_of(probe);

    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a.txt"]);
    dir.write("d/b.txt", "");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["a.txt,b.txt"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

#[test]
fn an_unread_file_changing_replays_the_unit() {
    let dir = Dir::new("unread");
    dir.write("a.txt", "one").write("b.txt", "other");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| v.text("a.txt").unwrap_or("absent").to_string());
    let pack = pack_of(probe);

    let _ = run_repo(&dir, &cache, &pack, None);
    dir.write("b.txt", "changed");
    dir.write("c.txt", "new beside it");
    let again = run_repo(&dir, &cache, &pack, None);
    assert_eq!(messages(&again.found), ["one"]);
    assert_eq!(judged.load(Ordering::SeqCst), 1, "the probe never read `b.txt`, so it replays");
    assert!(again.counts.reads <= 1, "only the file the trace holds may be read: {:?}", again.counts);

    // The control: the file it did read moving reruns it, so the replay above is the trace
    // holding and not every unit replaying.
    dir.write("a.txt", "two");
    let _ = run_repo(&dir, &cache, &pack, None);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

#[test]
fn a_git_answer_changing_reruns_the_unit() {
    let dir = Dir::new("git");
    dir.git(&["init", "-q"]);
    dir.git(&["config", "probe.value", "one"]);
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| {
        v.git(&["config", "--get", "probe.value"])
            .text()
            .unwrap_or_default()
            .trim()
            .to_string()
    });
    let pack = pack_of(probe);

    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    let again = run_repo(&dir, &cache, &pack, None);
    assert_eq!(judged.load(Ordering::SeqCst), 1);
    assert_eq!(again.counts.spawns, 1, "a git question is checked by asking it again, once");

    dir.git(&["config", "probe.value", "two"]);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["two"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

#[test]
fn a_run_input_changing_reruns_the_unit_and_a_replay_starts_nothing() {
    let dir = Dir::new("run");
    dir.write("in.txt", "one");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| {
        v.run("cat", &["in.txt"], v.root(), &[Path::new("in.txt")])
            .text()
            .unwrap_or_default()
            .to_string()
    });
    let pack = pack_of(probe);

    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    let again = run_repo(&dir, &cache, &pack, None);
    assert_eq!(judged.load(Ordering::SeqCst), 1);
    assert_eq!(again.counts.spawns, 0, "a command is checked by its inputs, never by running it");

    dir.write("in.txt", "two");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["two"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

#[test]
fn a_file_restored_with_a_new_time_replays_the_unit() {
    let dir = Dir::new("restore");
    dir.write("a.txt", "one");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| v.text("a.txt").unwrap_or("absent").to_string());
    let pack = pack_of(probe);

    let _ = run_repo(&dir, &cache, &pack, None);
    dir.write("a.txt", "two");
    let _ = run_repo(&dir, &cache, &pack, None);
    assert_eq!(judged.load(Ordering::SeqCst), 2);

    // Back to what it was, under a time it never had: a branch switched back.
    dir.write("a.txt", "one");
    touch(&dir.0.join("a.txt"), 30);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2, "the earlier trace holds again, so it replays");
}

#[test]
fn a_file_rewritten_within_the_clock_tick_is_read_again() {
    // Two contents of one size under one time: the table alone cannot tell them apart, so a
    // digest taken that close to the file's time is not trusted on a later run.
    let dir = Dir::new("racy");
    dir.write("a.txt", "one");
    touch(&dir.0.join("a.txt"), 0);
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| v.text("a.txt").unwrap_or("absent").to_string());
    let pack = pack_of(probe);
    let _ = run_repo(&dir, &cache, &pack, None);

    let when = std::fs::metadata(dir.0.join("a.txt")).unwrap().modified().unwrap();
    dir.write("a.txt", "two");
    std::fs::File::options()
        .write(true)
        .open(dir.0.join("a.txt"))
        .unwrap()
        .set_modified(when)
        .unwrap();
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["two"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}
