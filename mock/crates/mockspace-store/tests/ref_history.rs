//! What a ref store says of the commits on its local ref, against real git.
#![allow(unused_imports)]
mod common;

use std::process::Command;

use common::{EMPTY_TREE, MOCK_REF, NS, World, git, store};
use mockspace_store::{Edit, Reader, Reapply, RefPair, RefStore, Store, StoreError};

/// A commit of the empty tree whose author time and committer time are the two given.
fn commit_at(dir: &std::path::Path, parent: Option<&str>, author: i64, committer: i64) -> String {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(["commit-tree", "-m", "x", EMPTY_TREE]);
    if let Some(p) = parent {
        cmd.args(["-p", p]);
    }
    let out = cmd
        .env_remove("GIT_DIR")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .env("GIT_AUTHOR_DATE", format!("{author} +0000"))
        .env("GIT_COMMITTER_DATE", format!("{committer} +0000"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[test]
fn there_is_no_history_before_there_is_a_ref() {
    let w = World::new();
    let a = store(&w, "a");
    assert!(a.history().unwrap().is_empty());
}

#[test]
fn history_lists_newest_first_with_the_author_time_and_not_the_committer_time() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = w.dir.path().join("a");
    // the committer time is what a rebase resets; the author time is what it keeps
    let first = commit_at(&dir, None, 1_700_000_000, 1_700_000_500);
    let second = commit_at(&dir, Some(&first), 1_700_001_000, 1_800_000_000);
    git(&dir, &["update-ref", MOCK_REF, &second]);
    let got = a.history().unwrap();
    assert_eq!(got.len(), 2);
    assert_eq!(
        (got[0].id.as_str(), got[0].author_time),
        (second.as_str(), 1_700_001_000)
    );
    assert_eq!(
        (got[1].id.as_str(), got[1].author_time),
        (first.as_str(), 1_700_000_000)
    );
}

#[test]
fn a_commit_is_read_by_its_id_whatever_the_tip_is_now() {
    let w = World::new();
    let a = store(&w, "a");
    let one = a
        .apply(&Edit::new().put("f", "one\n"), "one", &Reapply)
        .unwrap()
        .commit
        .unwrap();
    a.apply(&Edit::new().put("f", "two\n"), "two", &Reapply)
        .unwrap();
    let then = a.reader_of(&one).unwrap();
    assert_eq!(then.read("f").unwrap().unwrap(), b"one\n");
    assert_eq!(
        a.read("f").unwrap().unwrap(),
        b"two\n",
        "the control: the tip moved on"
    );
}

#[test]
fn a_name_that_git_would_take_for_an_option_or_a_branch_is_not_an_object_id() {
    let w = World::new();
    let a = store(&w, "a");
    for bad in ["", "HEAD", "--output=x", "refs/mock/x", "abc def", "-1"] {
        match a.reader_of(bad) {
            Err(StoreError::InvalidRef {
                ..
            }) => {},
            other => panic!("{bad:?}: {:?}", other.map(|_| ())),
        }
    }
}

#[test]
fn a_clone_is_shallow_only_when_a_depth_cut_its_history() {
    let w = World::new();
    let a = store(&w, "a");
    a.apply(&Edit::new().put("f", "1\n"), "one", &Reapply)
        .unwrap();
    a.apply(&Edit::new().put("f", "2\n"), "two", &Reapply)
        .unwrap();
    assert!(!a.is_shallow().unwrap(), "the writer holds all of it");
    let b =
        RefStore::new(w.clone_dir("b"), w.remote(), RefPair::ref_form(NS).unwrap()).with_depth(1);
    b.refresh().unwrap();
    assert!(b.is_shallow().unwrap());
    assert_eq!(
        b.history().unwrap().len(),
        1,
        "only the commit the depth reached"
    );
}

#[test]
fn every_file_of_a_commit_comes_back_with_its_bytes_across_the_batches() {
    let w = World::new();
    let a = store(&w, "a");
    let mut edit = Edit::new();
    // more than two batches of 200, spread over directories
    for n in 0 .. 450 {
        edit = edit.put(format!("d{}/f{n}.toml", n % 7), format!("n = {n}\n"));
    }
    let odd: Vec<u8> = vec![0, 255, 10, 13, 10, 0];
    edit = edit.put("bin", odd.clone());
    let commit = a.apply(&edit, "many", &Reapply).unwrap().commit.unwrap();
    let files = a.files_of(&commit).unwrap();
    assert_eq!(files.len(), 451);
    assert_eq!(files["bin"], odd);
    assert_eq!(files["d3/f10.toml"], b"n = 10\n");
    assert_eq!(
        files["d1/f449.toml"], b"n = 449\n",
        "the last of the last batch"
    );
    assert!(a.files_of("--all").is_err(), "an option is not a commit");
}
