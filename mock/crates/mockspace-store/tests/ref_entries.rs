//! What a ref store refuses to read as a file: a symbolic link, a submodule, a name that is not
//! text. Each would otherwise be taken for something it is not, and no later check could tell.
mod common;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::Command;

use common::{World, store};
use mockspace_store::{Edit, Reapply, RefStore, Store, StoreError};

/// A commit in the store's own repository holding exactly `entries` (mode, object, path as
/// bytes), made with plumbing and a private index, and not on any ref.
fn commit_of(store_dir: &std::path::Path, entries: &[(&str, &str, &[u8])]) -> String {
    let idx = store_dir.join(".git/entries.idx");
    // each commit holds exactly its own entries, so the index of the last one is not kept
    let _ = std::fs::remove_file(&idx);
    let run = |args: &[&OsStr], input: Option<&[u8]>| -> String {
        use std::io::Write;
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(store_dir)
            .args(args)
            .env("GIT_INDEX_FILE", &idx)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.unwrap_or(b""))
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    for (mode, content, path) in entries {
        let id = if *mode == "160000" {
            "1234567890123456789012345678901234567890".to_owned()
        } else {
            run(
                &[OsStr::new("hash-object"), OsStr::new("-w"), OsStr::new("--stdin")],
                Some(content.as_bytes()),
            )
        };
        let mut spec = format!("{mode},{id},").into_bytes();
        spec.extend_from_slice(path);
        run(
            &[
                OsStr::new("update-index"),
                OsStr::new("--add"),
                OsStr::new("--cacheinfo"),
                OsStr::from_bytes(&spec),
            ],
            None,
        );
    }
    let tree = run(&[OsStr::new("write-tree")], None);
    run(
        &[
            OsStr::new("commit-tree"),
            OsStr::new(&tree),
            OsStr::new("-m"),
            OsStr::new("entries"),
        ],
        None,
    )
}

fn plain(w: &World) -> RefStore {
    let a = store(w, "a");
    a.apply(&Edit::new().put("f", "x\n"), "seed", &Reapply)
        .unwrap();
    a
}

#[test]
fn a_regular_file_and_an_executable_one_are_read() {
    let w = World::new();
    let a = plain(&w);
    let dir = w.dir.path().join("a");
    let c = commit_of(&dir, &[
        ("100644", "one\n", b"a"),
        ("100755", "two\n", b"b"),
    ]);
    let files = a.files_of(&c).unwrap();
    assert_eq!(files.get("a").map(Vec::as_slice), Some(&b"one\n"[..]));
    assert_eq!(files.get("b").map(Vec::as_slice), Some(&b"two\n"[..]));
}

#[test]
fn a_symbolic_link_is_refused_and_not_read_as_a_file_of_its_target_text() {
    let w = World::new();
    let a = plain(&w);
    let dir = w.dir.path().join("a");
    let linked = commit_of(&dir, &[("100644", "x\n", b"f"), ("120000", "f", b"link")]);
    let err = a.files_of(&linked).unwrap_err();
    assert!(matches!(err, StoreError::InvalidPath { .. }), "{err}");
    assert!(err.to_string().contains("link"), "{err}");
    // the control: the same commit without the link is read
    let plain_commit = commit_of(&dir, &[("100644", "x\n", b"f")]);
    assert_eq!(a.files_of(&plain_commit).unwrap().len(), 1);
}

#[test]
fn a_submodule_is_refused_and_not_left_out() {
    let w = World::new();
    let a = plain(&w);
    let dir = w.dir.path().join("a");
    let c = commit_of(&dir, &[("100644", "x\n", b"f"), ("160000", "", b"mod")]);
    let err = a.files_of(&c).unwrap_err();
    assert!(matches!(err, StoreError::InvalidPath { .. }), "{err}");
    assert!(err.to_string().contains("mod"), "{err}");
}

#[test]
fn two_names_that_are_not_text_stay_two_names_or_are_refused() {
    let w = World::new();
    let a = plain(&w);
    let dir = w.dir.path().join("a");
    let c = commit_of(&dir, &[
        ("100644", "one\n", b"a\xff"),
        ("100644", "two\n", b"a\xfe"),
    ]);
    let err = a.files_of(&c).unwrap_err();
    assert!(matches!(err, StoreError::InvalidPath { .. }), "{err}");
    assert!(err.to_string().contains("text"), "{err}");
}
