use std::time::Duration;

use super::tests::{built, later, made};
use super::*;

/// A cargo home holding one extracted registry crate, the `.crate` it came from, and a lock in the
/// generated crate that gives `lock_checksum` for it. Returns the home, the generated crate and the
/// extracted source. Everything is written just now, so the source changed within any margin.
fn registry_crate(tmp: &Path, crate_bytes: &[u8], lock_checksum: &str) -> (PathBuf, PathBuf, PathBuf) {
    let home = tmp.join("cargo-home");
    let source = home.join("registry").join("src").join("idx-1").join("foo-1.0.0").join("src").join("lib.rs");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, "pub fn foo() {}").unwrap();
    // Cargo writes this last, when it has finished unpacking: nothing in the package is newer.
    std::fs::write(home.join("registry").join("src").join("idx-1").join("foo-1.0.0").join(".cargo-ok"), "{}").unwrap();
    let cache = home.join("registry").join("cache").join("idx-1");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("foo-1.0.0.crate"), crate_bytes).unwrap();
    let gen_dir = tmp.join("gen");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(
        gen_dir.join("Cargo.lock"),
        format!(
            "version = 4\n\n[[package]]\nname = \"foo\"\nversion = \"1.0.0\"\n\
             source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
             checksum = \"{lock_checksum}\"\n"
        ),
    )
    .unwrap();
    (home, gen_dir, source)
}

#[test]
fn a_registry_crate_extracted_just_now_is_trusted_by_its_checksum_and_not_its_change_time() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = b"the crate file as the registry served it";
    let (home, gen_dir, source) = registry_crate(tmp.path(), bytes, &digest_bytes(bytes));
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let mut roots = Roots::default();
    roots.allow_absolute(&home);
    // The margin is an hour and the source changed a moment ago, as in a container that has just
    // unpacked its registry: by change time alone nothing would be kept.
    let hour = Duration::from_secs(3600);
    let deps = record(&made(&dylib, later(), false), &gen_dir, &roots, hour)
        .expect("a crate whose file hashes to the lock's checksum needs no age");
    assert_eq!(deps.len(), 1, "{deps:?}");

    // The control: the same source under a root that is no registry is held to the margin.
    let lints = tmp.path().join("lints");
    std::fs::create_dir_all(&lints).unwrap();
    std::fs::write(lints.join("probe.rs"), "x").unwrap();
    let mut own = Roots::default();
    own.add("lints", &lints);
    let dylib = built(&tmp.path().join("gen-own"), &format!("out.d: {}\n", lints.join("probe.rs").display()));
    assert!(record(&made(&dylib, later(), false), &tmp.path().join("gen-own"), &own, hour).is_none());
}

#[test]
fn a_registry_crate_is_not_trusted_unless_its_file_matches_the_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = b"the crate file as the registry served it";
    let hour = Duration::from_secs(3600);
    for (what, served, lock) in [
        ("a file that is not the one locked", b"another file".as_slice(), digest_bytes(bytes)),
        ("a lock with no checksum for it", bytes.as_slice(), String::new()),
    ] {
        let case = tmp.path().join(what.replace(' ', "-"));
        let (home, gen_dir, source) = registry_crate(&case, served, &lock);
        let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
        let mut roots = Roots::default();
        roots.allow_absolute(&home);
        assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, hour).is_none(), "{what}");
    }

    // A package the lock does not name, and a crate file that is gone.
    let case = tmp.path().join("unlisted");
    let (home, gen_dir, source) = registry_crate(&case, bytes, &digest_bytes(bytes));
    std::fs::write(gen_dir.join("Cargo.lock"), "version = 4\n").unwrap();
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let mut roots = Roots::default();
    roots.allow_absolute(&home);
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, hour).is_none(), "a package the lock lacks");
    let case = tmp.path().join("gone");
    let (home, gen_dir, source) = registry_crate(&case, bytes, &digest_bytes(bytes));
    std::fs::remove_file(home.join("registry").join("cache").join("idx-1").join("foo-1.0.0.crate")).unwrap();
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let mut roots = Roots::default();
    roots.allow_absolute(&home);
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, hour).is_none(), "no crate file");
}

#[test]
fn a_registry_file_edited_after_cargo_unpacked_it_is_not_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = b"the crate file as the registry served it";
    let (home, gen_dir, source) = registry_crate(tmp.path(), bytes, &digest_bytes(bytes));
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let mut roots = Roots::default();
    roots.allow_absolute(&home);
    let hour = Duration::from_secs(3600);
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, hour).is_some(), "the control: as unpacked");

    // The `.crate` still matches the lock, which says nothing of the unpacked copy. Cargo does not
    // fingerprint registry sources by time, so it would reuse the unit built from the old text.
    std::thread::sleep(Duration::from_millis(60));
    std::fs::write(&source, "pub fn foo() { edited() }").unwrap();
    assert!(
        record(&made(&dylib, later(), true), &gen_dir, &roots, hour).is_none(),
        "a source newer than cargo's own mark of having finished unpacking"
    );
}


fn set_unit_age(dylib: &Path, ago: Duration) {
    std::fs::File::options()
        .write(true)
        .open(dylib.parent().unwrap().join("deps").join("gen.d"))
        .unwrap()
        .set_modified(SystemTime::now() - ago)
        .unwrap();
}

#[test]
fn a_unit_cargo_reused_is_not_vouched_for_by_an_unpack_made_after_it() {
    // A file was edited, a unit was built from the edit, the package was unpacked again, and cargo
    // reused the unit (it fingerprints a registry crate by version). The fresh files are no newer
    // than the new `.cargo-ok` and the `.crate` still matches, yet they are not what the unit read.
    let tmp = tempfile::tempdir().unwrap();
    let bytes = b"the crate file as the registry served it";
    let (home, gen_dir, source) = registry_crate(tmp.path(), bytes, &digest_bytes(bytes));
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let mut roots = Roots::default();
    roots.allow_absolute(&home);
    let hour = Duration::from_secs(3600);

    // The control: a unit built in this run, which is what unpacked the package a moment before.
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, hour).is_some(), "built in this run");
    // And a unit cargo reused that is older than the unpack.
    set_unit_age(&dylib, Duration::from_secs(1800));
    assert!(
        record(&made(&dylib, later(), true), &gen_dir, &roots, hour).is_none(),
        "a reused unit older than the unpack of its sources"
    );
}

#[test]
fn a_registry_package_cargo_left_no_mark_on_is_not_trusted() {
    let tmp = tempfile::tempdir().unwrap();
    let bytes = b"the crate file as the registry served it";
    let (home, gen_dir, source) = registry_crate(tmp.path(), bytes, &digest_bytes(bytes));
    std::fs::remove_file(source.ancestors().nth(2).unwrap().join(".cargo-ok")).unwrap();
    let dylib = built(&gen_dir, &format!("out.d: {}\n", source.display()));
    let mut roots = Roots::default();
    roots.allow_absolute(&home);
    assert!(record(&made(&dylib, later(), false), &gen_dir, &roots, Duration::from_secs(3600)).is_none());
}
