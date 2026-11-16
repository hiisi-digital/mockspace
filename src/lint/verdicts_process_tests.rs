//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The claims about the store that are about processes, each tested with processes: the test
//! binary starts itself again, running [`child`] in the role the test names.

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Output, Stdio};

use super::*;

/// The variable naming the role a started copy of this binary plays.
const ROLE: &str = "MOCKSPACE_VERDICTS_TEST_ROLE";

/// The full name of [`child`], which the started copy runs alone.
const CHILD_TEST: &str = "lint::verdicts::process_tests::child";

/// The value writer `w` saves under its `n`th key: two lines, so a record holds a newline.
fn value_of(w: u8, n: u16) -> String {
    format!("writer {w} entry {n}\nwith a second line\n")
}

/// A value larger than any limit a test sets above the packed file, so a write of it is cut.
fn big_value(i: u16) -> String {
    format!("big {i}\n{}\n", "x".repeat(2000))
}

fn key_of(s: &DiskVerdicts, w: u8, n: u16) -> String {
    s.digest(&[&[w], &n.to_le_bytes()])
}

fn big_key(s: &DiskVerdicts, i: u16) -> String {
    s.digest(&[b"big", &i.to_le_bytes()])
}

fn open(base: &Path) -> DiskVerdicts {
    DiskVerdicts::open_in(base, Path::new("/repo"), "pack").expect("a store opens")
}

/// The packed file the store writes to.
fn pack_of(s: &DiskVerdicts) -> PathBuf {
    s.pack.clone()
}

/// A copy of this binary in `role`, with `vars` set, its output kept.
fn command(role: &str, base: &Path, vars: &[(&str, String)]) -> Command {
    let mut c = Command::new(std::env::current_exe().expect("the test binary"));
    c.args([CHILD_TEST, "--exact", "--test-threads=1", "--nocapture"])
        .env(ROLE, role)
        .env("MOCKSPACE_VERDICTS_TEST_BASE", base)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in vars {
        c.env(k, v);
    }
    c
}

fn run(role: &str, base: &Path, vars: &[(&str, String)]) -> Output {
    command(role, base, vars).output().expect("the child starts")
}

fn assert_ok(out: &Output, what: &str) {
    assert!(
        out.status.success(),
        "{what} failed with {:?}:\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is set for the child"))
}

fn num<T: std::str::FromStr>(name: &str) -> T {
    var(name).parse().unwrap_or_else(|_| panic!("{name} is a number"))
}

/// Caps the size any file this process writes may reach. Over it, a write is cut where the cap
/// falls and the next one fails: by `SIGXFSZ`, which ends the process, or with `EFBIG` once the
/// signal is ignored, which is how a full disk looks to the writer.
fn cap_file_size(bytes: u64, survive: bool) {
    // SAFETY: plain calls with valid arguments; this process is a test child with one thread
    // doing work.
    unsafe {
        if survive {
            libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        }
        let lim = libc::rlimit { rlim_cur: bytes as libc::rlim_t, rlim_max: libc::RLIM_INFINITY };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &lim), 0, "the cap is set");
    }
}

fn uncap_file_size() {
    cap_file_size(libc::RLIM_INFINITY as u64, true);
}

/// Waits `PAUSE_US` microseconds when set, so the children of a race interleave.
fn pause() {
    if let Ok(us) = std::env::var("PAUSE_US") {
        std::thread::sleep(std::time::Duration::from_micros(us.parse().unwrap_or(0)));
    }
}

/// What a started copy of this binary does, by its role. Run directly, it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else { return };
    let base = PathBuf::from(var("MOCKSPACE_VERDICTS_TEST_BASE"));
    match role.as_str() {
        // saves `COUNT` entries of writer `WRITER`
        "write" => {
            let s = open(&base);
            let w: u8 = num("WRITER");
            for n in 0 .. num::<u16>("COUNT") {
                s.remember(&key_of(&s, w, n), &value_of(w, n));
                pause();
            }
        }
        // opens a store for another repository in the same base, which sweeps it
        "open-other" => {
            let _ = DiskVerdicts::open_in(&base, Path::new("/another-repo"), "pack").expect("a store opens");
        }
        // saves a value too big for the cap, and dies part way through writing it
        "die-mid-write" => {
            let s = open(&base);
            cap_file_size(num("CAP"), false);
            s.remember(&big_key(&s, 0), &big_value(0));
            unreachable!("the write over the cap ends the process");
        }
        // saves a value too big for the cap, which fails, then a small one, which fits
        "fail-then-write" => {
            let s = open(&base);
            cap_file_size(num("CAP"), true);
            s.remember(&big_key(&s, 0), &big_value(0));
            s.remember(&key_of(&s, 9, 9), &value_of(9, 9));
            uncap_file_size();
        }
        // saves big values with the cap set just above the file each time, so most are cut
        "flaky" => {
            let s = open(&base);
            let pack = pack_of(&s);
            for i in 0 .. num::<u16>("COUNT") {
                let len = fs::metadata(&pack).map(|m| m.len()).unwrap_or(0);
                cap_file_size(len + 1 + u64::from(i) * 37 % 300, true);
                s.remember(&big_key(&s, i), &big_value(i));
                uncap_file_size();
                pause();
            }
        }
        // opens the store over and over and checks every value it replays
        "read" => {
            for _ in 0 .. num::<u16>("ROUNDS") {
                check_replays(&open(&base), num("WRITERS"), num("COUNT"), num("BIG"));
            }
        }
        other => panic!("no role {other}"),
    }
}

/// Every value `s` replays for the writers' and the big keys is the one saved under it.
fn check_replays(s: &DiskVerdicts, writers: u8, count: u16, big: u16) {
    for w in 0 .. writers {
        for n in 0 .. count {
            if let Some(v) = s.replay(&key_of(s, w, n)) {
                assert_eq!(v, value_of(w, n), "a replayed value is the one saved");
            }
        }
    }
    for i in 0 .. big {
        if let Some(v) = s.replay(&big_key(s, i)) {
            assert_eq!(v, big_value(i), "a replayed value is the one saved");
        }
    }
}

#[test]
fn writers_in_separate_processes_lose_and_corrupt_nothing() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let children: Vec<_> = (0 .. 4u8)
        .map(|w| {
            command("write", dir.path(), &[("WRITER", w.to_string()), ("COUNT", "200".into())])
                .spawn()
                .expect("a writer starts")
        })
        .collect();
    for c in children {
        assert_ok(&c.wait_with_output().unwrap(), "a writer");
    }
    let s = open(dir.path());
    for w in 0 .. 4u8 {
        for n in 0 .. 200u16 {
            assert_eq!(s.replay(&key_of(&s, w, n)), Some(value_of(w, n)), "entry {w}/{n} survived whole");
        }
    }
}

/// Repro (a) of the review: a store keeps writing after another process died part way through a
/// record, at cuts in the record's head and in its value.
#[test]
fn a_store_writing_after_a_process_died_mid_record_keeps_every_whole_record() {
    for cut in [1u64, 7, 20, 40, 70, 300] {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let s = open(dir.path());
        s.remember(&key_of(&s, 0, 0), &value_of(0, 0));
        let before = fs::metadata(pack_of(&s)).unwrap().len();

        let out = run("die-mid-write", dir.path(), &[("CAP", (before + cut).to_string())]);
        assert_eq!(out.status.signal(), Some(libc::SIGXFSZ), "the control: the child died writing");
        assert_eq!(fs::metadata(pack_of(&s)).unwrap().len(), before + cut, "the control: it left {cut} bytes");

        s.remember(&key_of(&s, 0, 1), &value_of(0, 1));
        let again = open(dir.path());
        assert_eq!(again.replay(&key_of(&again, 0, 0)), Some(value_of(0, 0)), "cut {cut}: the record before");
        assert_eq!(again.replay(&big_key(&again, 0)), None, "cut {cut}: the cut record is no value");
        assert_eq!(again.replay(&key_of(&again, 0, 1)), Some(value_of(0, 1)), "cut {cut}: the record after");
    }
}

/// An open store whose write fails part way leaves the file as it was and keeps working.
#[test]
fn a_write_that_fails_part_way_leaves_the_pack_as_it_was() {
    for cut in [1u64, 20, 300] {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let s = open(dir.path());
        s.remember(&key_of(&s, 0, 0), &value_of(0, 0));
        let before = fs::read(pack_of(&s)).unwrap();
        // what the small record adds, measured on a store of its own
        let alone = mockspace_manifest::scratch::tempdir().unwrap();
        let lone = open(alone.path());
        lone.remember(&key_of(&lone, 9, 9), &value_of(9, 9));
        let small = fs::read(pack_of(&lone)).unwrap();
        // the cap leaves room for the small record and not for the big one
        let cap = before.len() as u64 + cut.max(small.len() as u64 + 1);

        assert_ok(&run("fail-then-write", dir.path(), &[("CAP", cap.to_string())]), "the failing writer");
        let after = fs::read(pack_of(&s)).unwrap();
        assert_eq!(after, [before.as_slice(), small.as_slice()].concat(), "cut {cut}: only the small record was added");
        let again = open(dir.path());
        assert_eq!(again.replay(&big_key(&again, 0)), None);
        assert_eq!(again.replay(&key_of(&again, 9, 9)), Some(value_of(9, 9)));
    }
}

/// Readers open the store over and over while writers save and a writer's saves keep failing
/// part way and are cut back: no reader replays a value that was not saved, and no whole record
/// is lost.
#[test]
fn a_reader_racing_failing_writes_replays_only_what_was_saved() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let base = dir.path();
    open(base).remember(&key_of(&open(base), 7, 0), &value_of(7, 0));
    let counts = [("WRITERS", "2".to_string()), ("COUNT", "300".into()), ("BIG", "200".into())];
    let mut children = vec![
        command("flaky", base, &[("COUNT", "200".into()), ("PAUSE_US", "1500".into())]).spawn().unwrap(),
        command("read", base, &[&counts[..], &[("ROUNDS", "40".into())]].concat()).spawn().unwrap(),
    ];
    for w in 0 .. 2u8 {
        children.push(command("write", base, &[("WRITER", w.to_string()), ("COUNT", "300".into()), ("PAUSE_US", "1000".into())]).spawn().unwrap());
    }
    for c in children {
        assert_ok(&c.wait_with_output().unwrap(), "a child");
    }
    let s = open(base);
    check_replays(&s, 2, 300, 200);
    for w in 0 .. 2u8 {
        for n in 0 .. 300u16 {
            assert_eq!(s.replay(&key_of(&s, w, n)), Some(value_of(w, n)), "entry {w}/{n} survived");
        }
    }
}

/// Finding 4: another process's sweep removes the repository's directory, still empty, between
/// this store opening and its first write; the write still lands.
#[test]
fn a_sweep_in_another_process_does_not_lose_a_fresh_stores_write() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = open(dir.path());
    let repo_dir = s.dir.clone();
    assert_ok(&run("open-other", dir.path(), &[]), "the sweeping process");
    assert!(!repo_dir.exists(), "the control: the other process swept the empty directory");
    s.remember(&key_of(&s, 0, 0), &value_of(0, 0));
    assert_eq!(open(dir.path()).replay(&key_of(&s, 0, 0)), Some(value_of(0, 0)));
}
