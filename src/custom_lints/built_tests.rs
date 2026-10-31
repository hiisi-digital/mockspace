use std::cell::Cell;
use std::time::Duration;

use super::*;

/// A "build": a library and the dep-info cargo would leave beside it, naming `sources`.
fn fake_build(out: &Path, bytes: &str, sources: &[&Path]) -> Built {
    let release = out.join("target").join("release");
    std::fs::create_dir_all(&release).unwrap();
    let dylib = release.join("libfake.so");
    std::fs::write(&dylib, bytes).unwrap();
    let list: Vec<String> = sources.iter().map(|p| p.display().to_string()).collect();
    std::fs::write(release.join("libfake.d"), format!("out.d: {}\n", list.join(" "))).unwrap();
    std::fs::create_dir_all(release.join("deps")).unwrap();
    std::fs::write(release.join("deps").join("fake.d"), format!("out.d: {}\n", list.join(" "))).unwrap();
    Built { dylib, started: SystemTime::now() + Duration::from_secs(1), units: Vec::new(), fresh: false }
}

struct Fixture {
    tmp:    tempfile::TempDir,
    base:   PathBuf,
    ran:    PathBuf,
    source: PathBuf,
    roots:  Roots,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("lints").join("probe.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "v1").unwrap();
    let mut roots = Roots::default();
    roots.add("lints", source.parent().unwrap());
    Fixture {
        base: tmp.path().join("store"),
        ran: tmp.path().join("gen"),
        source,
        roots,
        tmp,
    }
}

#[test]
fn an_identity_built_once_is_not_built_again() {
    let f = fixture();
    let calls = Cell::new(0);
    let build = || {
        calls.set(calls.get() + 1);
        Ok(fake_build(&f.ran, "first", &[&f.source]))
    };
    let first = dylib_for(&f.base, "key", &f.roots, &f.ran, Duration::ZERO, build).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(std::fs::read_to_string(&first.path).unwrap(), "first");
    assert!(first.identity.as_deref().is_some_and(|i| i.starts_with("key-")));

    // The build output is overwritten by another tree's run in between.
    std::fs::write(&first.path, "something else").unwrap();
    let second = dylib_for(&f.base, "key", &f.roots, &f.ran, Duration::ZERO, build).unwrap();
    assert_eq!(calls.get(), 1, "a kept library is loaded, not rebuilt");
    assert_eq!(std::fs::read_to_string(second.path).unwrap(), "first");
    assert_eq!(second.identity, first.identity);
}

#[test]
fn a_kept_library_is_not_replayed_for_a_file_that_changed_since() {
    let f = fixture();
    let calls = Cell::new(0);
    let load = |bytes: &'static str| {
        dylib_for(&f.base, "key", &f.roots, &f.ran, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            Ok(fake_build(&f.ran, bytes, &[&f.source]))
        })
        .unwrap()
    };
    let one = load("one");
    std::fs::write(&f.source, "v2").unwrap();
    let two = load("two");
    assert_eq!(calls.get(), 2, "the edit is a rebuild");
    assert_eq!(std::fs::read_to_string(&two.path).unwrap(), "two");
    assert_ne!(one.identity, two.identity, "and the code is another code");

    // Back to the first content: its entry is still there, and is loaded without a build.
    std::fs::write(&f.source, "v1").unwrap();
    let again = load("never built");
    assert_eq!(calls.get(), 2);
    assert_eq!(again.identity, one.identity);
}

#[test]
fn another_key_is_built() {
    let f = fixture();
    let calls = Cell::new(0);
    for key in ["one", "two"] {
        dylib_for(&f.base, key, &f.roots, &f.ran, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            Ok(fake_build(&f.ran, key, &[&f.source]))
        })
        .unwrap();
    }
    assert_eq!(calls.get(), 2);
}

#[test]
fn a_failed_build_keeps_nothing() {
    let f = fixture();
    assert!(dylib_for(&f.base, "key", &f.roots, &f.ran, Duration::ZERO, || Err("no".to_string())).is_err());
    let calls = Cell::new(0);
    dylib_for(&f.base, "key", &f.roots, &f.ran, Duration::ZERO, || {
        calls.set(1);
        Ok(fake_build(&f.ran, "x", &[&f.source]))
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_build_that_cannot_be_tied_to_its_files_is_returned_and_not_kept() {
    let f = fixture();
    let calls = Cell::new(0);
    let build_late = || {
        dylib_for(&f.base, "key", &f.roots, &f.ran, Duration::ZERO, || {
            calls.set(calls.get() + 1);
            let mut b = fake_build(&f.ran, "x", &[&f.source]);
            // A source modified after the build began.
            b.started = SystemTime::now() - Duration::from_secs(3600);
            Ok(b)
        })
        .unwrap()
    };
    let first = build_late();
    assert!(first.path.is_file(), "still returned");
    assert_eq!(first.identity, None, "and identified by its bytes");
    build_late();
    assert_eq!(calls.get(), 2, "nothing was kept");
}

#[test]
fn only_files_unused_for_long_are_reaped() {
    let f = fixture();
    for key in ["old", "new"] {
        dylib_for(&f.base, key, &f.roots, &f.ran, Duration::ZERO, || Ok(fake_build(&f.ran, key, &[&f.source]))).unwrap();
    }
    let later = SystemTime::now() + Duration::from_secs(10 * 24 * 3600);
    assert_eq!(reap(&f.base, later, Duration::from_secs(14 * 24 * 3600)), 0, "inside the window");
    let much_later = SystemTime::now() + Duration::from_secs(20 * 24 * 3600);
    assert_eq!(reap(&f.base, much_later, Duration::from_secs(14 * 24 * 3600)), 4, "each is two files");
    let calls = Cell::new(0);
    dylib_for(&f.base, "old", &f.roots, &f.ran, Duration::ZERO, || {
        calls.set(1);
        Ok(fake_build(&f.ran, "old", &[&f.source]))
    })
    .unwrap();
    assert_eq!(calls.get(), 1, "a reaped entry is only a rebuild");
    drop(f.tmp);
}
