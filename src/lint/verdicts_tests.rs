//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::record::HEAD;
use super::*;

fn store(base: &Path, pack: &str) -> DiskVerdicts {
    DiskVerdicts::open_in(base, Path::new("/repo"), pack)
        .expect("a store opens in a writable directory")
}

#[test]
fn a_verdict_saved_is_replayed_and_an_unknown_key_is_not() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    let key = s.digest(&[b"file", b"text"]);
    assert_eq!(s.replay(&key), None, "the control: nothing is saved yet");
    s.remember(&key, "verdict-v1\n");
    assert_eq!(s.replay(&key).as_deref(), Some("verdict-v1\n"));
    s.remember(&key, "verdict-v1\n");
    assert_eq!(
        s.replay(&key).as_deref(),
        Some("verdict-v1\n"),
        "saving twice leaves one whole entry"
    );
    assert_eq!(s.replay(&s.digest(&[b"file", b"other"])), None);
}

#[test]
fn the_same_parts_split_differently_are_different_keys() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    assert_ne!(
        s.digest(&[b"ab", b"c"]),
        s.digest(&[b"a", b"bc"]),
        "parts are framed"
    );
    assert_ne!(
        s.digest(&[b"abc"]),
        s.digest(&[b"abc", b""]),
        "and an empty part counts"
    );
}

#[test]
fn a_different_pack_finds_nothing_the_first_saved() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let old = store(dir.path(), "pack-a");
    let key = old.digest(&[b"x"]);
    old.remember(&key, "saved by pack-a");

    let new = store(dir.path(), "pack-b");
    assert_ne!(key, new.digest(&[b"x"]), "the pack is part of every key");
    assert_eq!(
        new.replay(&new.digest(&[b"x"])),
        None,
        "a rebuilt pack must not see the old verdicts"
    );
    assert_eq!(
        old.replay(&key).as_deref(),
        Some("saved by pack-a"),
        "the control: the old pack still does"
    );
}

#[test]
fn a_file_digest_is_the_content_and_not_the_name() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let (a, b, c) = (
        dir.path().join("a"),
        dir.path().join("b"),
        dir.path().join("c"),
    );
    fs::write(&a, b"same").unwrap();
    fs::write(&b, b"same").unwrap();
    fs::write(&c, b"different").unwrap();
    assert_eq!(file_digest(&a), file_digest(&b));
    assert_ne!(file_digest(&a), file_digest(&c));
    assert_eq!(file_digest(&dir.path().join("missing")), None);
}

/// Every regular file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else { out.push(p) }
        }
    }
    out
}

#[test]
fn two_writers_of_one_store_lose_and_corrupt_nothing() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let base = dir.path().to_path_buf();
    let writers: Vec<_> = (0 .. 2u8)
        .map(|w| {
            let base = base.clone();
            std::thread::spawn(move || {
                let s = DiskVerdicts::open_in(&base, Path::new("/repo"), "pack").unwrap();
                for n in 0 .. 150u16 {
                    let key = s.digest(&[&[w], &n.to_le_bytes()]);
                    s.remember(&key, &format!("writer {w} entry {n}\nwith a second line\n"));
                }
            })
        })
        .collect();
    for w in writers {
        w.join().unwrap();
    }
    let s = DiskVerdicts::open_in(&base, Path::new("/repo"), "pack").unwrap();
    for w in 0 .. 2u8 {
        for n in 0 .. 150u16 {
            let key = s.digest(&[&[w], &n.to_le_bytes()]);
            assert_eq!(
                s.replay(&key).as_deref(),
                Some(format!("writer {w} entry {n}\nwith a second line\n").as_str()),
                "entry {w}/{n} survived whole"
            );
        }
    }
}

/// The bytes the store writes for one record, read off a store of their own.
fn record_bytes(parts: &[&[u8]], value: &str) -> Vec<u8> {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    s.remember(&s.digest(parts), value);
    fs::read(pack_path(&s)).unwrap()
}

fn pack_path(s: &DiskVerdicts) -> PathBuf {
    s.current.clone()
}

/// Repro (b) of the review, over every cut: a record cut short at any byte, then a whole record,
/// replays the whole one and never the cut one, and never a value made of the two.
#[test]
fn a_record_cut_at_any_byte_is_no_value_and_hides_no_later_record() {
    let long = "a value with a newline\n".repeat(12);
    let (a, b, c) = (
        record_bytes(&[b"a"], "before"),
        record_bytes(&[b"b"], &long),
        record_bytes(&[b"c"], "after"),
    );
    for cut in 1 .. b.len() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let s = store(dir.path(), "");
        s.remember(&s.digest(&[b"a"]), "before");
        assert_eq!(
            fs::read(pack_path(&s)).unwrap(),
            a,
            "the control: one record is what was measured"
        );
        fs::write(
            pack_path(&s),
            [a.as_slice(), &b[.. cut], c.as_slice()].concat(),
        )
        .unwrap();
        let again = store(dir.path(), "");
        assert_eq!(
            again.replay(&again.digest(&[b"a"])).as_deref(),
            Some("before"),
            "cut {cut}"
        );
        assert_eq!(
            again.replay(&again.digest(&[b"b"])),
            None,
            "cut {cut}: the cut record is no value"
        );
        assert_eq!(
            again.replay(&again.digest(&[b"c"])).as_deref(),
            Some("after"),
            "cut {cut}: the later record"
        );
    }
}

/// Finding 5: two engines in use on one repository each keep their verdicts.
#[test]
fn two_engines_in_use_on_one_repository_keep_each_others_verdicts() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let (a, b) = (store(dir.path(), "pack-a"), store(dir.path(), "pack-b"));
    for n in 0 .. 5u8 {
        a.remember(&a.digest(&[&[n]]), "saved by pack-a");
        b.remember(&b.digest(&[&[n]]), "saved by pack-b");
    }
    let (a, b) = (store(dir.path(), "pack-a"), store(dir.path(), "pack-b"));
    for n in 0 .. 5u8 {
        assert_eq!(
            a.replay(&a.digest(&[&[n]])).as_deref(),
            Some("saved by pack-a")
        );
        assert_eq!(
            b.replay(&b.digest(&[&[n]])).as_deref(),
            Some("saved by pack-b")
        );
    }
}

fn set_times(path: &Path, accessed: SystemTime, modified: SystemTime) {
    let times = fs::FileTimes::new()
        .set_accessed(accessed)
        .set_modified(modified);
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(times)
        .unwrap();
}

/// Finding 3 and 5: an entry of the one-file-per-verdict layout, which a released engine still
/// writes, is kept while either of its times is within two weeks, and goes once both are older.
/// A mount that keeps no access times leaves the access time old while the entry is rewritten.
#[test]
fn the_old_layout_is_kept_while_either_time_is_fresh_and_goes_once_both_are_old() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let base = dir.path();
    let (now, old) = (
        SystemTime::now(),
        SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60),
    );
    let shard = base.join("01");
    fs::create_dir_all(&shard).unwrap();
    let entry = |n: u8| shard.join(format!("{n:02x}").repeat(31));
    for (n, (accessed, modified)) in [(now, now), (old, now), (now, old), (old, old)]
        .into_iter()
        .enumerate()
    {
        fs::write(entry(n as u8), "old verdict").unwrap();
        set_times(&entry(n as u8), accessed, modified);
    }
    fs::create_dir_all(base.join("last-full-run")).unwrap();
    fs::write(base.join("last-full-run/live"), "key\n").unwrap();
    // beside them, things that are not the store's, however old
    fs::create_dir_all(base.join("notes")).unwrap();
    fs::write(base.join("notes/keep.txt"), "mine").unwrap();
    fs::create_dir_all(base.join("ab")).unwrap();
    fs::write(base.join("ab/keep.txt"), "mine").unwrap();
    for mine in ["notes/keep.txt", "ab/keep.txt"] {
        set_times(&base.join(mine), old, old);
    }

    let _s = store(base, "");
    assert!(entry(0).exists(), "both times fresh");
    assert!(
        entry(1).exists(),
        "read long ago and written lately, as on a mount without access times"
    );
    assert!(entry(2).exists(), "written long ago and read lately");
    assert!(!entry(3).exists(), "both times old");
    assert!(
        base.join("last-full-run/live").exists(),
        "kept keys written lately"
    );
    assert!(
        base.join("notes/keep.txt").exists() && base.join("ab/keep.txt").exists(),
        "what is not the store's stays"
    );
}

/// Finding 3 for the packed file: written lately, read long ago as a mount without access times
/// leaves it, the file is in use and stays.
#[test]
fn a_packed_file_written_lately_is_kept_whatever_its_access_time() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    let key = s.digest(&[b"k"]);
    s.remember(&key, "v");
    let old = SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60);
    set_times(&pack_path(&s), old, SystemTime::now());
    assert_eq!(store(dir.path(), "").replay(&key).as_deref(), Some("v"));
}

/// The start of a generation, far enough from the epoch for two before it.
fn generation_start() -> SystemTime {
    UNIX_EPOCH + KEPT * 2000
}

fn store_at(base: &Path, pack: &str, now: SystemTime) -> DiskVerdicts {
    DiskVerdicts::open_at(base, Path::new("/repo"), pack, now).expect("a store opens")
}

/// Whether `s` loaded `key`, read without replaying it, which would count as a use.
fn holds(s: &DiskVerdicts, key: &str) -> bool {
    s.state.lock().unwrap().entries.contains_key(key)
}

/// A verdict is kept at least [`KEPT`] after it was last read or written, whenever in its
/// generation that was, and is gone within twice that.
#[test]
fn a_verdict_lasts_at_least_kept_after_its_last_use_and_goes_within_twice_that() {
    let second = Duration::from_secs(1);
    let t = generation_start();
    // the earliest use in a generation, and the latest
    for (used, kept_until) in
        [(t, t + KEPT * 2 - second), (t + KEPT - second, t + KEPT * 2 - second)]
    {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let s = store_at(dir.path(), "", used);
        let key = s.digest(&[b"k"]);
        s.remember(&key, "v");
        assert!(kept_until.duration_since(used).unwrap() >= KEPT - second);
        assert!(
            holds(&store_at(dir.path(), "", kept_until), &key),
            "still kept"
        );
        assert_eq!(
            store_at(dir.path(), "", t + KEPT * 2).replay(&key),
            None,
            "gone at twice the window"
        );
        assert!(
            files_under(dir.path()).is_empty(),
            "and its file with it: {:?}",
            files_under(dir.path())
        );
    }
}

/// A verdict replayed from the generation before is copied into this one, so it lasts as long
/// as it is used; one nobody reads goes.
#[test]
fn a_verdict_read_is_carried_into_the_next_generation_and_one_not_read_goes() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let t = generation_start();
    let s = store_at(dir.path(), "", t);
    let (read, unread) = (s.digest(&[b"read"]), s.digest(&[b"unread"]));
    s.remember(&read, "r");
    s.remember(&unread, "u");
    let day = Duration::from_secs(24 * 60 * 60);
    for g in 1 .. 6u32 {
        let s = store_at(dir.path(), "", t + KEPT * g + day);
        assert_eq!(
            s.replay(&read).as_deref(),
            Some("r"),
            "generation {g}: the verdict in use"
        );
    }
    assert_eq!(store_at(dir.path(), "", t + KEPT * 6).replay(&unread), None);
}

/// A repository used every few days under one salt holds at most two files, and no verdict in
/// them more than twice, however long it is used: what is loaded is what was used lately.
#[test]
fn a_repository_in_steady_use_stays_bounded_by_what_it_used_lately() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let t = generation_start();
    let day = Duration::from_secs(24 * 60 * 60);
    let mut used_at: HashMap<String, SystemTime> = HashMap::new();
    for run in 0 .. 60u32 {
        let now = t + day * 3 * run;
        let s = store_at(dir.path(), "", now);
        // a hot set every run reads, saving what it misses, and a fresh set only this run writes
        for n in 0 .. 20u32 {
            let key = s.digest(&[b"hot", &n.to_le_bytes()]);
            if s.replay(&key).is_none() {
                s.remember(&key, "hot verdict");
            }
            used_at.insert(key, now);
        }
        for n in 0 .. 20u32 {
            let key = s.digest(&[b"fresh", &run.to_le_bytes(), &n.to_le_bytes()]);
            s.remember(&key, "fresh verdict");
            used_at.insert(key, now);
        }
        let files = files_under(dir.path());
        assert!(files.len() <= 2, "run {run}: {files:?}");
        let records: Vec<_> = files
            .iter()
            .flat_map(|f| parse(&fs::read(f).unwrap()))
            .collect();
        let lately = used_at
            .values()
            .filter(|&&u| now.duration_since(u).unwrap() < KEPT * 2)
            .count();
        assert!(
            records.len() <= 2 * lately,
            "run {run}: {} records for {lately} verdicts used lately",
            records.len()
        );
        for (key, _) in &records {
            assert!(
                now.duration_since(used_at[key]).unwrap() < KEPT * 2,
                "run {run}: a verdict unused for twice the window"
            );
        }
    }
}

/// Finding 5: another salt's files stay while that engine is in use, and go once it has been
/// untouched past [`KEPT`].
#[test]
fn another_salt_is_kept_while_in_use_and_goes_once_untouched_past_the_window() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let t = generation_start();
    let day = Duration::from_secs(24 * 60 * 60);
    let (a, b) = (
        store_at(dir.path(), "pack-a", t),
        store_at(dir.path(), "pack-b", t),
    );
    let (ka, kb) = (a.digest(&[b"x"]), b.digest(&[b"x"]));
    a.remember(&ka, "a");
    b.remember(&kb, "b");
    // only pack-b runs from here; pack-a was last used at `t`
    for d in [KEPT - day, KEPT + day] {
        assert_eq!(
            store_at(dir.path(), "pack-b", t + d).replay(&kb).as_deref(),
            Some("b")
        );
        assert_eq!(
            store_at(dir.path(), "pack-a", t + d).replay(&ka).as_deref(),
            Some("a"),
            "untouched {d:?}: kept"
        );
        assert_eq!(
            store_at(dir.path(), "pack-b", t + d + day)
                .replay(&kb)
                .as_deref(),
            Some("b")
        );
    }
    // pack-a was read at `t + KEPT + day`, so its verdict is in that generation's file
    let later = t + KEPT * 3;
    assert_eq!(
        store_at(dir.path(), "pack-b", later).replay(&kb),
        None,
        "pack-b unused since t + KEPT + 2 days"
    );
    let left = files_under(dir.path());
    assert!(left.is_empty(), "both untouched past the window: {left:?}");
}

/// A record whose bytes are not the ones written, as a crash can leave a file's end zeroed, is
/// no value, at every byte of its key and value, and the records around it are still read.
#[test]
fn a_record_with_any_byte_not_as_written_is_no_value() {
    let long = "a value with a newline\n".repeat(4);
    let (a, b, c) = (
        record_bytes(&[b"a"], "before"),
        record_bytes(&[b"b"], &long),
        record_bytes(&[b"c"], "after"),
    );
    for at in HEAD .. b.len() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let s = store(dir.path(), "");
        let mut changed = b.clone();
        changed[at] = if changed[at] == 0 { b'0' } else { 0 };
        fs::create_dir_all(pack_path(&s).parent().unwrap()).unwrap();
        fs::write(
            pack_path(&s),
            [a.as_slice(), &changed, c.as_slice()].concat(),
        )
        .unwrap();
        let again = store(dir.path(), "");
        assert_eq!(
            again.replay(&again.digest(&[b"b"])),
            None,
            "byte {at}: the changed record is no value"
        );
        assert_eq!(
            again.replay(&again.digest(&[b"a"])).as_deref(),
            Some("before"),
            "byte {at}"
        );
        assert_eq!(
            again.replay(&again.digest(&[b"c"])).as_deref(),
            Some("after"),
            "byte {at}"
        );
    }
}
