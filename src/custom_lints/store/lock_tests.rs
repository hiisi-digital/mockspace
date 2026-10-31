use super::*;
use crate::custom_lints::store::LOCK;

#[test]
fn the_lock_is_held_until_it_is_dropped() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("0123456789abcdef");
    let held = lock_dir(&dir).unwrap();
    let peer = File::open(dir.join(LOCK)).unwrap();
    assert!(
        peer.try_lock().is_err(),
        "a second run must wait while the first holds it"
    );
    drop(held);
    assert!(
        peer.try_lock().is_ok(),
        "the control: released once dropped"
    );
}

#[test]
fn a_directory_that_cannot_be_locked_is_named_with_the_way_out() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("a-file");
    std::fs::write(&file, "").unwrap();
    // a directory below a file cannot be made
    let dir = file.join("0123456789abcdef");
    let err = lock_dir(&dir).unwrap_err();
    assert!(err.contains(&dir.display().to_string()), "{err}");
    assert!(
        err.contains("XDG_CACHE_HOME") && err.contains("CARGO_TARGET_DIR"),
        "{err}"
    );
}

#[cfg(unix)]
#[test]
fn a_lock_on_a_file_that_was_replaced_is_not_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(LOCK);
    std::fs::write(&path, "").unwrap();
    let file = File::open(&path).unwrap();
    assert!(
        is_the_file_at(&file, &path),
        "the control: the file at the path"
    );
    // what a reaper does, and a peer's recreation of it
    std::fs::remove_file(&path).unwrap();
    assert!(!is_the_file_at(&file, &path), "removed");
    std::fs::write(&path, "").unwrap();
    assert!(!is_the_file_at(&file, &path), "recreated is another file");
}
