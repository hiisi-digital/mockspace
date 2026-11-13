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

/// A command that did not start, or ended in failure, is judged again on the next run, since what
/// failed may be the machine and not the inputs. A command that succeeded replays, the control.
#[test]
fn a_command_that_failed_or_never_started_is_never_saved() {
    for (program, script, saved) in [
        ("sh", "exit 0", true),
        ("sh", "exit 3", false),
        ("mockspace-a-program-no-machine-has", "", false),
    ] {
        let dir = Dir::new("run-failed");
        dir.write("script", script);
        let cache = Mem::default();
        let (probe, judged) = Probe::new(|v| {
            let program = v.text("program").unwrap_or_default().to_string();
            let a = v.run(&program, &["script"], v.root(), &[Path::new("script")]);
            format!("started={} ok={}", a.started(), a.ok())
        });
        dir.write("program", program);
        let pack = pack_of(probe);
        let _ = run_repo(&dir, &cache, &pack, None);
        let _ = run_repo(&dir, &cache, &pack, None);
        let want = if saved { 1 } else { 2 };
        assert_eq!(judged.load(Ordering::SeqCst), want, "{program} {script:?}");
    }
}

/// An input the command writes while it runs is saved under what it was when the command started,
/// so the next run sees it moved and judges again, and never replays output of the old content
/// under the new one's digest.
#[test]
fn an_input_written_while_the_command_runs_is_saved_as_it_was_before() {
    let dir = Dir::new("run-writes-input");
    dir.write("in.txt", "old");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| {
        v.run("sh", &["-c", "cat in.txt; printf new > in.txt"], v.root(), &[Path::new("in.txt")])
            .text()
            .unwrap_or_default()
            .to_string()
    });
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["old"]);
    assert_eq!(std::fs::read_to_string(dir.0.join("in.txt")).unwrap(), "new");
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["new"], "the input moved, so it ran again");
    assert_eq!(judged.load(Ordering::SeqCst), 2);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["new"], "the control: unmoved, it replays");
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

/// A rewrite that keeps the size and puts the modification time back, as `cp -p`, `tar` or
/// `touch -d` do, long after the digest was taken, still moves the change time, so the file is read
/// again and not answered from the table.
#[cfg(unix)]
#[test]
fn a_rewrite_with_its_old_time_put_back_is_read_again() {
    let dir = Dir::new("ctime");
    let file = dir.0.join("a.txt");
    dir.write("a.txt", "one");
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    let set_old = || std::fs::File::options().write(true).open(&file).unwrap().set_modified(old).unwrap();
    set_old();
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| v.text("a.txt").unwrap_or("absent").to_string());
    let pack = pack_of(probe);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["one"]);
    assert_eq!(judged.load(Ordering::SeqCst), 1, "the control: an untouched file is answered from the table");

    dir.write("a.txt", "two");
    set_old();
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["two"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}
