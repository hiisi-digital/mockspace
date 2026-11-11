//! A file whose name is not text, in either backend. A store names its files as text, so such a
//! name is refused, by the name read lossily, wherever the store lists it: listed lossily, it
//! names no file, reads as absent, and whoever walks the store passes it over without a word.
#![cfg(unix)]
mod common;

use std::os::unix::ffi::OsStrExt;

use common::{World, git, store};
use mockspace_store::{DirStore, Edit, Reader, Reapply, Store, StoreError};

// Mutation: list the directory's names lossily, as before; the file is then listed by a name that
// reads as absent.
#[test]
fn the_directory_backend_refuses_to_list_a_name_that_is_not_text() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("own.toml"), "o = 0").unwrap();
    std::fs::write(d.path().join(std::ffi::OsStr::from_bytes(b"caf\xe9.toml")), "c = 1").unwrap();
    let err = DirStore::new(d.path()).list("").unwrap_err();
    assert!(matches!(err, StoreError::InvalidPath { .. }), "{err}");
    assert!(err.to_string().contains("caf") && err.to_string().contains("text"), "{err}");
}

// Mutation: list the tip's names lossily, as before; the file is then listed by a name that reads
// as absent.
#[test]
fn the_ref_backend_refuses_to_list_a_name_that_is_not_text() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("f", "x\n"), "seed", &Reapply).unwrap();
    let dir = w.dir.path().join("a");
    // a commit on the remote that holds a name git keeps as bytes, and nothing else
    let scratch = dir.join(".git/odd.idx");
    let run = |args: &[&std::ffi::OsStr]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .env("GIT_INDEX_FILE", &scratch)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    let os = |s: &str| std::ffi::OsString::from(s);
    let empty = run(&[&os("hash-object"), &os("-w"), &os("/dev/null")]);
    let mut spec = format!("100644,{empty},").into_bytes();
    spec.extend_from_slice(b"caf\xe9.toml");
    run(&[&os("update-index"), &os("--add"), &os("--cacheinfo"), std::ffi::OsStr::from_bytes(&spec)]);
    let tree = run(&[&os("write-tree")]);
    let tip = git(&dir, &["rev-parse", common::MOCK_REF]);
    let commit = git(&dir, &["commit-tree", &tree, "-p", &tip, "-m", "odd"]);
    git(&dir, &["push", "-q", &w.remote(), &format!("{commit}:{}", common::MOCK_REF)]);
    a.refresh().unwrap();
    let err = a.list("").unwrap_err();
    assert!(matches!(err, StoreError::InvalidPath { .. }), "{err}");
    assert!(err.to_string().contains("caf") && err.to_string().contains("text"), "{err}");
}
