use std::cell::Cell;
use std::time::Duration;

use super::*;

fn fake_build(dir: &Path, bytes: &str) -> PathBuf {
    let p = dir.join("libfake.so");
    std::fs::write(&p, bytes).unwrap();
    p
}

#[test]
fn an_identity_built_once_is_not_built_again() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let calls = Cell::new(0);
    let build = || {
        calls.set(calls.get() + 1);
        Ok(fake_build(&out, "first"))
    };
    let first = dylib_for(&base, "id-one", build).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(std::fs::read_to_string(first).unwrap(), "first");

    // The build output is overwritten by another tree's run in between.
    std::fs::write(out.join("libfake.so"), "something else").unwrap();
    let second = dylib_for(&base, "id-one", build).unwrap();
    assert_eq!(calls.get(), 1, "a kept library is loaded, not rebuilt");
    assert_eq!(std::fs::read_to_string(second).unwrap(), "first");
}

#[test]
fn another_identity_is_built() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    let calls = Cell::new(0);
    for id in ["id-one", "id-two"] {
        dylib_for(&base, id, || {
            calls.set(calls.get() + 1);
            Ok(fake_build(tmp.path(), id))
        })
        .unwrap();
    }
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_failed_build_keeps_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    assert!(dylib_for(&base, "id", || Err("no".to_string())).is_err());
    let calls = Cell::new(0);
    dylib_for(&base, "id", || {
        calls.set(1);
        Ok(fake_build(tmp.path(), "x"))
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn only_libraries_unused_for_long_are_reaped() {
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("store");
    for id in ["old", "new"] {
        dylib_for(&base, id, || Ok(fake_build(tmp.path(), id))).unwrap();
    }
    let later = SystemTime::now() + Duration::from_secs(10 * 24 * 3600);
    assert_eq!(reap(&base, later, Duration::from_secs(14 * 24 * 3600)), 0, "inside the window");
    let much_later = SystemTime::now() + Duration::from_secs(20 * 24 * 3600);
    assert_eq!(reap(&base, much_later, Duration::from_secs(14 * 24 * 3600)), 2);
    let calls = Cell::new(0);
    dylib_for(&base, "old", || {
        calls.set(1);
        Ok(fake_build(tmp.path(), "old"))
    })
    .unwrap();
    assert_eq!(calls.get(), 1, "a reaped identity is only a rebuild");
}
