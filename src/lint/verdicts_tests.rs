//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::time::{Duration, SystemTime};

use super::*;

fn store(base: &Path, pack: &str) -> DiskVerdicts {
    DiskVerdicts::open_in(base, pack).expect("a store opens in a writable directory")
}

#[test]
fn a_verdict_saved_is_replayed_and_an_unknown_key_is_not() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path(), "");
    assert_ne!(s.digest(&[b"ab", b"c"]), s.digest(&[b"a", b"bc"]), "parts are framed");
    assert_ne!(s.digest(&[b"abc"]), s.digest(&[b"abc", b""]), "and an empty part counts");
}

#[test]
fn a_different_pack_finds_nothing_the_first_saved() {
    let dir = tempfile::tempdir().unwrap();
    let old = store(dir.path(), "pack-a");
    let key = old.digest(&[b"x"]);
    old.remember(&key, "saved by pack-a");

    let new = store(dir.path(), "pack-b");
    assert_ne!(key, new.digest(&[b"x"]), "the pack is part of every key");
    assert_eq!(new.replay(&new.digest(&[b"x"])), None, "a rebuilt pack must not see the old verdicts");
    assert_eq!(old.replay(&key).as_deref(), Some("saved by pack-a"), "the control: the old pack still does");
}

#[test]
fn the_sweep_removes_what_nothing_has_read_for_two_weeks_and_keeps_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path(), "");
    let old = s.digest(&[b"old"]);
    let new = s.digest(&[b"new"]);
    s.remember(&old, "old");
    s.remember(&new, "new");

    let long_ago = SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60);
    let times = fs::FileTimes::new().set_accessed(long_ago).set_modified(long_ago);
    fs::File::options().write(true).open(s.path_of(&old)).unwrap().set_times(times).unwrap();
    // a store opened for the first time sweeps; remove the marker the first open left
    fs::remove_file(s.dir.join(".swept")).unwrap();

    let again = store(dir.path(), "");
    assert_eq!(again.replay(&old), None, "a month-old entry is swept");
    assert_eq!(again.replay(&new).as_deref(), Some("new"), "a fresh entry stays");
}

#[test]
fn a_file_digest_is_the_content_and_not_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, c) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
    fs::write(&a, b"same").unwrap();
    fs::write(&b, b"same").unwrap();
    fs::write(&c, b"different").unwrap();
    assert_eq!(file_digest(&a), file_digest(&b));
    assert_ne!(file_digest(&a), file_digest(&c));
    assert_eq!(file_digest(&dir.path().join("missing")), None);
}

#[test]
fn the_sweep_leaves_alone_what_the_store_did_not_write() {
    let dir = tempfile::tempdir().unwrap();
    let s = store(dir.path(), "");
    // a directory the user shares with other things: a non-shard directory and a non-key file in a
    // shard-looking one, both a month old
    fs::create_dir_all(dir.path().join("notes")).unwrap();
    fs::write(dir.path().join("notes/keep.txt"), "mine").unwrap();
    fs::create_dir_all(dir.path().join("ab")).unwrap();
    fs::write(dir.path().join("ab/keep.txt"), "mine").unwrap();
    let long_ago = SystemTime::now() - Duration::from_secs(30 * 24 * 60 * 60);
    let times = fs::FileTimes::new().set_accessed(long_ago).set_modified(long_ago);
    for f in ["notes/keep.txt", "ab/keep.txt"] {
        fs::File::options().write(true).open(dir.path().join(f)).unwrap().set_times(times).unwrap();
    }
    // the control: one of the store's own entries of the same age is swept
    let own = s.digest(&[b"own"]);
    s.remember(&own, "own");
    fs::File::options().write(true).open(s.path_of(&own)).unwrap().set_times(times).unwrap();
    fs::remove_file(s.dir.join(".swept")).unwrap();

    let again = store(dir.path(), "");
    assert_eq!(again.replay(&own), None, "the control: the store's own old entry is swept");
    assert!(dir.path().join("notes/keep.txt").exists(), "a file outside a shard directory is not ours");
    assert!(dir.path().join("ab/keep.txt").exists(), "a file in a shard that is not a key is not ours");
}
