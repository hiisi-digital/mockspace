use super::*;
use crate::custom_lints::store::lock::is_the_file_at;
use crate::custom_lints::store::{STALE_AFTER, lock_dir, touch_used};

const DAYS: Duration = Duration::from_secs(24 * 60 * 60);

/// A key directory as a run leaves it, with a build in it, its owner at
/// `owner` and last used `used_ago` ago.
fn key_dir(base: &Path, key: &str, owner: &Path, used_ago: Duration) -> PathBuf {
    let dir = base.join(key);
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("target").join("build"), "x").unwrap();
    std::fs::write(dir.join(LOCK), "").unwrap();
    std::fs::write(dir.join(OWNER), owner.display().to_string()).unwrap();
    let used = dir.join(USED);
    std::fs::write(&used, "0").unwrap();
    File::options()
        .write(true)
        .open(&used)
        .unwrap()
        .set_modified(SystemTime::now() - used_ago)
        .unwrap();
    dir
}

const KEPT: &str = "00000000000000aa";
const K1: &str = "0000000000000001";
const K2: &str = "0000000000000002";
const K3: &str = "0000000000000003";

fn reap_now(base: &Path, keep: &Path) -> Vec<PathBuf> {
    reap(base, keep, SystemTime::now(), STALE_AFTER)
}

#[test]
fn a_key_whose_owner_worktree_is_gone_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let alive = tmp.path().join("wt");
    std::fs::create_dir_all(&alive).unwrap();
    let gone = key_dir(
        &base,
        K1,
        &tmp.path().join("deleted-worktree"),
        Duration::ZERO,
    );
    let live = key_dir(&base, K2, &alive, Duration::ZERO);

    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![gone.clone()]);
    assert!(!gone.exists());
    assert!(
        live.exists(),
        "the control: a key whose owner exists and is recent stays"
    );
}

#[test]
fn a_key_unused_past_the_window_is_removed_though_its_owner_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let alive = tmp.path().join("wt");
    std::fs::create_dir_all(&alive).unwrap();
    let old = key_dir(&base, K1, &alive, STALE_AFTER + DAYS);
    let recent = key_dir(&base, K2, &alive, STALE_AFTER - DAYS);

    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![old.clone()]);
    assert!(!old.exists());
    assert!(recent.exists(), "just inside the window stays");
}

#[test]
fn the_key_this_run_is_using_is_never_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    // abandoned on both counts, and ours
    let ours = key_dir(&base, KEPT, &tmp.path().join("deleted"), STALE_AFTER + DAYS);
    assert!(reap_now(&base, &ours).is_empty());
    assert!(ours.exists());
}

#[test]
fn a_key_in_use_by_another_run_survives_and_goes_once_it_is_released() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let busy = key_dir(&base, K1, &tmp.path().join("deleted"), STALE_AFTER + DAYS);

    let held = lock_dir(&busy).unwrap();
    assert!(
        reap_now(&base, &base.join(KEPT)).is_empty(),
        "removed a key a run holds the lock of"
    );
    assert!(busy.join("target").join("build").exists());

    drop(held);
    // the control, which says the lock is what saved it
    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![busy.clone()]);
    assert!(!busy.exists());
}

#[test]
fn a_run_that_waited_on_a_removed_key_starts_again_in_a_fresh_one() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let dir = key_dir(&base, K1, &tmp.path().join("deleted"), STALE_AFTER + DAYS);

    // the run opens the lock file before the reaper takes it, and blocks on it
    let before = File::open(dir.join(LOCK)).unwrap();
    assert_eq!(reap_now(&base, &base.join(KEPT)), vec![dir.clone()]);
    assert!(!is_the_file_at(&before, &dir.join(LOCK)));

    // what it does next is lock_dir's loop: a new directory, a new lock
    let fresh = lock_dir(&dir).unwrap();
    assert!(dir.join(LOCK).is_file());
    drop(fresh);
}

#[test]
fn only_key_named_directories_are_candidates() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    std::fs::create_dir_all(base.join(".pack-pins")).unwrap();
    std::fs::create_dir_all(base.join("notes")).unwrap();
    std::fs::create_dir_all(base.join("0000000000000A01")).unwrap(); // upper case is not ours
    std::fs::write(base.join("0000000000000004"), "a file").unwrap();
    key_dir(&base, K3, &tmp.path().join("deleted"), Duration::ZERO);

    let removed = reap_now(&base, &base.join(KEPT));
    assert_eq!(removed, vec![base.join(K3)]);
    for kept in [".pack-pins", "notes", "0000000000000A01", "0000000000000004"] {
        assert!(base.join(kept).exists(), "{kept}");
    }
}

#[test]
fn a_directory_a_peer_is_still_creating_is_not_taken() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    // no lock file, no owner, no stamp: between `create_dir_all` and the lock
    let new = base.join(K1);
    std::fs::create_dir_all(&new).unwrap();
    assert!(reap_now(&base, &base.join(KEPT)).is_empty());
    assert!(new.exists());

    // the control: the same shape, left alone past the window, is junk
    let later = SystemTime::now() + STALE_AFTER + DAYS;
    assert_eq!(reap(&base, &base.join(KEPT), later, STALE_AFTER), vec![
        new.clone()
    ]);
}

#[test]
fn a_missing_store_reaps_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(reap_now(&tmp.path().join("never-made"), &tmp.path().join("x")).is_empty());
}

#[test]
fn a_run_stamps_its_key_as_used() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let alive = tmp.path().join("wt");
    std::fs::create_dir_all(&alive).unwrap();
    let dir = key_dir(&base, K1, &alive, STALE_AFTER + DAYS);
    assert!(abandoned(&dir, SystemTime::now(), STALE_AFTER));
    touch_used(&dir).unwrap();
    assert!(
        !abandoned(&dir, SystemTime::now(), STALE_AFTER),
        "a key a run has just used is not abandoned"
    );
}
