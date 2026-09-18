//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use super::*;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DEV: &str = "{ git = \"https://github.com/hiisi-digital/mockspace\", branch = \"dev\" }";

fn lock_with(source: &str) -> String {
    format!(
        "version = 4\n\n[[package]]\nname = \"other\"\nversion = \"0.1.0\"\n\
         source = \"git+https://example.invalid/x?branch=dev#{B}\"\n\n\
         [[package]]\nname = \"{BENCH_CORE}\"\nversion = \"0.1.0\"\nsource = \"{source}\"\n"
    )
}

fn never(_: &str, _: &str) -> Result<String, String> {
    panic!("the remote was asked while the arms had already said")
}

// --- reading a lock -----------------------------------------------------------

#[test]
fn a_git_resolution_of_bench_core_is_read_and_nothing_else_is() {
    let git = lock_with(&format!(
        "git+https://github.com/hiisi-digital/mockspace?branch=dev#{A}"
    ));
    assert_eq!(locked_rev(&git).as_deref(), Some(A), "and not the other package's");
    let pinned = lock_with(&format!("git+https://github.com/hiisi-digital/mockspace?rev={A}#{A}"));
    assert_eq!(locked_rev(&pinned).as_deref(), Some(A));
    for source in ["registry+https://github.com/rust-lang/crates.io-index", ""] {
        assert_eq!(locked_rev(&lock_with(source)), None, "{source:?}");
    }
    // A path dependency has no source line at all.
    let path = format!("version = 4\n\n[[package]]\nname = \"{BENCH_CORE}\"\nversion = \"0.1.0\"\n");
    assert_eq!(locked_rev(&path), None);
    assert_eq!(locked_rev("not a lockfile ["), None);
    assert_eq!(locked_rev_at(Path::new("/nonexistent-lock-dir")), None);
}

// --- which commit a run is on ---------------------------------------------------

fn arms(revs: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
    revs.iter().map(|(a, r)| (a.to_string(), r.map(str::to_string))).collect()
}

#[test]
fn the_arms_locks_decide_and_the_remote_is_not_asked() {
    let cache = tempfile::tempdir().unwrap();
    for set in [
        arms(&[("h/x", Some(A))]),
        arms(&[("h/x", Some(A)), ("h/y", Some(A))]),
        arms(&[("h/x", Some(A)), ("h/y", None)]),
    ] {
        assert_eq!(run_rev(&set, DEV, cache.path(), &never), Ok(Some(A.into())), "{set:?}");
    }
}

#[test]
fn arms_locked_at_two_commits_are_refused_naming_each() {
    let cache = tempfile::tempdir().unwrap();
    let set = arms(&[("h/x", Some(A)), ("h/y", Some(B)), ("h/z", None)]);
    let err = run_rev(&set, DEV, cache.path(), &never).unwrap_err();
    for part in [&format!("h/x at {A}"), &format!("h/y at {B}"), "cargo update -p"] {
        assert!(err.contains(part), "{part} missing from {err}");
    }
    assert!(!err.contains("h/z"), "an arm with no lock took no side: {err}");
}

#[test]
fn with_no_arm_locked_the_tip_is_taken_and_kept() {
    let cache = tempfile::tempdir().unwrap();
    let asked = Cell::new(0);
    let resolve = |url: &str, branch: &str| {
        asked.set(asked.get() + 1);
        assert_eq!((url, branch), ("https://github.com/hiisi-digital/mockspace", "dev"));
        Ok(B.to_string())
    };
    let none = arms(&[("h/x", None)]);
    assert_eq!(run_rev(&none, DEV, cache.path(), &resolve), Ok(Some(B.into())));
    assert_eq!(run_rev(&[], DEV, cache.path(), &resolve), Ok(Some(B.into())));
    assert_eq!(asked.get(), 1, "a second run within the hour asked again");
}

#[test]
fn with_no_arm_locked_and_no_tip_to_be_had_nothing_is_aligned() {
    let cache = tempfile::tempdir().unwrap();
    let offline = |_: &str, _: &str| Err::<String, String>("offline".into());
    assert_eq!(run_rev(&[], DEV, cache.path(), &offline), Ok(None));
}

#[test]
fn a_spec_the_consumer_pinned_is_left_to_the_consumer() {
    let cache = tempfile::tempdir().unwrap();
    // Even arms that disagree are not this module's business then: the
    // consumer chose the commit, and the dlopen hash is what is left.
    let split = arms(&[("h/x", Some(A)), ("h/y", Some(B))]);
    for spec in [
        "{ git = \"https://github.com/hiisi-digital/mockspace\", rev = \"abc\" }",
        "{ git = \"https://github.com/hiisi-digital/mockspace\", tag = \"v1\" }",
        "{ path = \"/repo/mockspace\" }",
    ] {
        assert_eq!(run_rev(&split, spec, cache.path(), &never), Ok(None), "{spec}");
    }
}

// --- moving a lock ----------------------------------------------------------------

#[test]
fn a_lock_elsewhere_or_missing_is_moved_and_one_already_there_is_not() {
    let calls: RefCell<Vec<(PathBuf, String)>> = RefCell::new(Vec::new());
    let update = |m: &Path, r: &str| {
        calls.borrow_mut().push((m.to_path_buf(), r.to_string()));
        Ok(())
    };
    let dir = tempfile::tempdir().unwrap();
    align(dir.path(), B, &update).unwrap();
    std::fs::write(
        dir.path().join("Cargo.lock"),
        lock_with(&format!("git+https://github.com/hiisi-digital/mockspace?branch=dev#{A}")),
    )
    .unwrap();
    align(dir.path(), B, &update).unwrap();
    align(dir.path(), A, &update).unwrap();
    let manifest = dir.path().join("Cargo.toml");
    assert_eq!(
        calls.into_inner(),
        [(manifest.clone(), B.to_string()), (manifest, B.to_string())],
        "missing and elsewhere are moved; already there is not"
    );
}

#[test]
fn a_failed_move_is_the_run_failing() {
    let dir = tempfile::tempdir().unwrap();
    let err = align(dir.path(), B, &|_: &Path, _: &str| Err("no network".into())).unwrap_err();
    assert_eq!(err, "no network");
}

// --- against cargo, over a git source on disk -----------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// A framework repository with bench-core at two commits on `dev`, a support
/// crate declaring it by branch the way every consumer's does, and a driver
/// taking the support crate by path.
struct Tree {
    _root:  tempfile::TempDir,
    old:    String,
    new:    String,
    url:    String,
    driver: PathBuf,
}

fn tree() -> Tree {
    let root = tempfile::tempdir().unwrap();
    let fw = root.path().join("fw");
    write(
        &fw.join("Cargo.toml"),
        &format!("[package]\nname = \"{BENCH_CORE}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    );
    write(&fw.join("src/lib.rs"), "pub const V: u8 = 1;\n");
    git(&fw, &["init", "-q", "-b", "dev"]);
    git(&fw, &["config", "user.name", "t"]);
    git(&fw, &["config", "user.email", "t@example.com"]);
    git(&fw, &["config", "commit.gpgsign", "false"]);
    git(&fw, &["add", "-A"]);
    git(&fw, &["commit", "-q", "--no-verify", "-m", "one"]);
    let old = git(&fw, &["rev-parse", "HEAD"]);
    write(&fw.join("src/lib.rs"), "pub const V: u8 = 2;\n");
    git(&fw, &["commit", "-q", "--no-verify", "-am", "two"]);
    let new = git(&fw, &["rev-parse", "HEAD"]);

    let url = format!("file://{}", fw.display());
    let branch = format!("{{ git = \"{url}\", branch = \"dev\" }}");
    let kit = root.path().join("kit");
    write(
        &kit.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"kit\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\n{BENCH_CORE} = {branch}\n"
        ),
    );
    write(&kit.join("src/lib.rs"), "");
    let driver = root.path().join("driver");
    write(
        &driver.join("Cargo.toml"),
        &format!(
            "[workspace]\n\n[package]\nname = \"driver\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\n{BENCH_CORE} = {branch}\nkit = {{ path = \"{}\" }}\n",
            kit.display()
        ),
    );
    write(&driver.join("src/main.rs"), "fn main() {}\n");
    Tree { _root: root, old, new, url, driver }
}

fn bench_core_sources(lock_dir: &Path) -> Vec<String> {
    let lock = std::fs::read_to_string(lock_dir.join("Cargo.lock")).unwrap();
    let doc: toml_edit::DocumentMut = lock.parse().unwrap();
    doc["package"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(BENCH_CORE))
        .filter_map(|p| Some(p.get("source")?.as_str()?.to_string()))
        .collect()
}

#[test]
fn a_driver_stuck_at_an_old_commit_is_moved_whole_and_its_support_crate_with_it() {
    // The defect, against cargo: the driver's lock sits at the older commit,
    // as it did at its first build, and the run is settled on the newer one.
    let t = tree();
    cargo_update(&t.driver.join("Cargo.toml"), &t.old).unwrap();
    assert_eq!(locked_rev_at(&t.driver).as_deref(), Some(t.old.as_str()));

    align(&t.driver, &t.new, &cargo_update).unwrap();
    let sources = bench_core_sources(&t.driver);
    assert_eq!(
        sources,
        [format!("git+{}?branch=dev#{}", t.url, t.new)],
        "one copy, at the new commit, shared with the support crate"
    );
}

#[test]
fn rewriting_the_driver_spec_to_a_rev_splits_the_framework_in_two() {
    // Why the lock is moved rather than the spec rewritten. Cargo keys a git
    // source by its spelling, so a driver naming `rev` beside a support crate
    // naming `branch` links two copies, and the support crate's stays where
    // its lock had it. This is the shape an earlier version of this fix took.
    let t = tree();
    cargo_update(&t.driver.join("Cargo.toml"), &t.old).unwrap();
    let manifest = t.driver.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let first = text.find("branch = \"dev\"").unwrap();
    let mut respelled = text.clone();
    respelled.replace_range(first .. first + "branch = \"dev\"".len(), &format!("rev = \"{}\"", t.new));
    std::fs::write(&manifest, respelled).unwrap();
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut sources = bench_core_sources(&t.driver);
    sources.sort();
    assert_eq!(sources.len(), 2, "{sources:?}");
    assert!(sources.contains(&format!("git+{}?branch=dev#{}", t.url, t.old)), "{sources:?}");
}
