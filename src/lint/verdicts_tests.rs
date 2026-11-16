//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::time::{Duration, SystemTime};

use super::*;

fn store(base: &Path, pack: &str) -> DiskVerdicts {
    DiskVerdicts::open_in(base, Path::new("/repo"), pack).expect("a store opens in a writable directory")
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
    assert_eq!(s.replay(&key).as_deref(), Some("verdict-v1\n"), "saving twice leaves one whole entry");
    assert_eq!(s.replay(&s.digest(&[b"file", b"other"])), None);
}

#[test]
fn the_same_parts_split_differently_are_different_keys() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    assert_ne!(s.digest(&[b"ab", b"c"]), s.digest(&[b"a", b"bc"]), "parts are framed");
    assert_ne!(s.digest(&[b"abc"]), s.digest(&[b"abc", b""]), "and an empty part counts");
}

#[test]
fn a_different_pack_finds_nothing_the_first_saved() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let old = store(dir.path(), "pack-a");
    let key = old.digest(&[b"x"]);
    old.remember(&key, "saved by pack-a");

    let new = store(dir.path(), "pack-b");
    assert_ne!(key, new.digest(&[b"x"]), "the pack is part of every key");
    assert_eq!(new.replay(&new.digest(&[b"x"])), None, "a rebuilt pack must not see the old verdicts");
    assert_eq!(old.replay(&key).as_deref(), Some("saved by pack-a"), "the control: the old pack still does");
}

#[test]
fn a_file_digest_is_the_content_and_not_the_name() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let (a, b, c) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
    fs::write(&a, b"same").unwrap();
    fs::write(&b, b"same").unwrap();
    fs::write(&c, b"different").unwrap();
    assert_eq!(file_digest(&a), file_digest(&b));
    assert_ne!(file_digest(&a), file_digest(&c));
    assert_eq!(file_digest(&dir.path().join("missing")), None);
}

#[test]
fn opening_removes_the_per_verdict_layout_and_leaves_what_is_not_the_stores() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let base = dir.path();
    // the old shape: a shard directory of 62-hex entries and a half-written temporary, the daily
    // marker and the kept keys; and, beside them, things that are not the store's
    let key = "0123456789abcdef".repeat(4);
    fs::create_dir_all(base.join(&key[.. 2])).unwrap();
    fs::write(base.join(&key[.. 2]).join(&key[2 ..]), "old verdict").unwrap();
    fs::write(base.join(&key[.. 2]).join(format!(".123.{}", &key[2 ..])), "half").unwrap();
    fs::write(base.join(".swept"), "").unwrap();
    fs::create_dir_all(base.join("last-full-run")).unwrap();
    fs::write(base.join("last-full-run/live"), "key\n").unwrap();
    fs::create_dir_all(base.join("notes")).unwrap();
    fs::write(base.join("notes/keep.txt"), "mine").unwrap();
    fs::create_dir_all(base.join("ab")).unwrap();
    fs::write(base.join("ab/keep.txt"), "mine").unwrap();

    let _s = store(base, "");
    assert!(!base.join(&key[.. 2]).exists(), "the emptied shard goes");
    assert!(!base.join(".swept").exists() && !base.join("last-full-run").exists());
    assert!(base.join("notes/keep.txt").exists(), "a file outside a shard directory is not ours");
    assert!(base.join("ab/keep.txt").exists(), "a file in a shard that is not a key is not ours");
}

#[test]
fn a_packed_file_untouched_for_two_weeks_is_swept_and_a_fresh_one_stays() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    let key = s.digest(&[b"old"]);
    s.remember(&key, "old");
    // the control: it is read back while fresh
    assert_eq!(store(dir.path(), "").replay(&key).as_deref(), Some("old"));

    let long_ago = SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60);
    let times = fs::FileTimes::new().set_accessed(long_ago).set_modified(long_ago);
    fs::File::options().write(true).open(&s.pack).unwrap().set_times(times).unwrap();
    assert_eq!(store(dir.path(), "").replay(&key), None, "a month-old packed file is swept");
}

#[test]
fn a_torn_record_is_dropped_and_later_appends_survive() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let s = store(dir.path(), "");
    let (a, b) = (s.digest(&[b"a"]), s.digest(&[b"b"]));
    s.remember(&a, "whole");
    let mut f = fs::OpenOptions::new().append(true).open(&s.pack).unwrap();
    std::io::Write::write_all(&mut f, format!("{b} 99\nhalf a rec").as_bytes()).unwrap();
    drop(f);

    let again = store(dir.path(), "");
    assert_eq!(again.replay(&a).as_deref(), Some("whole"));
    assert_eq!(again.replay(&b), None, "the torn record is not an entry");
    let c = again.digest(&[b"c"]);
    again.remember(&c, "after");
    assert_eq!(store(dir.path(), "").replay(&c).as_deref(), Some("after"), "an append after the tear is read");
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
fn verdicts_written_under_two_salts_leave_one_file_and_one_salt() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let old = store(dir.path(), "pack-a");
    for n in 0 .. 5u8 {
        old.remember(&old.digest(&[&[n]]), "saved by pack-a");
    }
    let new = store(dir.path(), "pack-b");
    for n in 0 .. 5u8 {
        new.remember(&new.digest(&[&[n]]), "saved by pack-b");
    }
    let files = files_under(dir.path());
    assert_eq!(files.len(), 1, "one packed file for one repo and its current salt, found {files:?}");
    let reopened = store(dir.path(), "pack-b");
    for n in 0 .. 5u8 {
        assert_eq!(reopened.replay(&reopened.digest(&[&[n]])).as_deref(), Some("saved by pack-b"));
    }
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
    s.pack.clone()
}

/// Repro (b) of the review, over every cut: a record cut short at any byte, then a whole record,
/// replays the whole one and never the cut one, and never a value made of the two.
#[test]
fn a_record_cut_at_any_byte_is_no_value_and_hides_no_later_record() {
    let long = "a value with a newline\n".repeat(12);
    let (a, b, c) = (record_bytes(&[b"a"], "before"), record_bytes(&[b"b"], &long), record_bytes(&[b"c"], "after"));
    for cut in 1 .. b.len() {
        let dir = mockspace_manifest::scratch::tempdir().unwrap();
        let s = store(dir.path(), "");
        s.remember(&s.digest(&[b"a"]), "before");
        assert_eq!(fs::read(pack_path(&s)).unwrap(), a, "the control: one record is what was measured");
        fs::write(pack_path(&s), [a.as_slice(), &b[.. cut], c.as_slice()].concat()).unwrap();
        let again = store(dir.path(), "");
        assert_eq!(again.replay(&again.digest(&[b"a"])).as_deref(), Some("before"), "cut {cut}");
        assert_eq!(again.replay(&again.digest(&[b"b"])), None, "cut {cut}: the cut record is no value");
        assert_eq!(again.replay(&again.digest(&[b"c"])).as_deref(), Some("after"), "cut {cut}: the later record");
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
        assert_eq!(a.replay(&a.digest(&[&[n]])).as_deref(), Some("saved by pack-a"));
        assert_eq!(b.replay(&b.digest(&[&[n]])).as_deref(), Some("saved by pack-b"));
    }
}

fn set_times(path: &Path, accessed: SystemTime, modified: SystemTime) {
    let times = fs::FileTimes::new().set_accessed(accessed).set_modified(modified);
    fs::File::options().write(true).open(path).unwrap().set_times(times).unwrap();
}

/// Finding 3 and 5: an entry of the one-file-per-verdict layout, which a released engine still
/// writes, is kept while either of its times is within two weeks, and goes once both are older.
/// A mount that keeps no access times leaves the access time old while the entry is rewritten.
#[test]
fn the_old_layout_is_kept_while_either_time_is_fresh_and_goes_once_both_are_old() {
    let dir = mockspace_manifest::scratch::tempdir().unwrap();
    let base = dir.path();
    let (now, old) = (SystemTime::now(), SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60));
    let shard = base.join("01");
    fs::create_dir_all(&shard).unwrap();
    let entry = |n: u8| shard.join(format!("{n:02x}").repeat(31));
    for (n, (accessed, modified)) in [(now, now), (old, now), (now, old), (old, old)].into_iter().enumerate() {
        fs::write(entry(n as u8), "old verdict").unwrap();
        set_times(&entry(n as u8), accessed, modified);
    }
    fs::create_dir_all(base.join("last-full-run")).unwrap();
    fs::write(base.join("last-full-run/live"), "key\n").unwrap();

    let _s = store(base, "");
    assert!(entry(0).exists(), "both times fresh");
    assert!(entry(1).exists(), "read long ago and written lately, as on a mount without access times");
    assert!(entry(2).exists(), "written long ago and read lately");
    assert!(!entry(3).exists(), "both times old");
    assert!(base.join("last-full-run/live").exists(), "kept keys written lately");
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
