//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Real git against a bare repository on disk: the clones, the pushes and the races are git's.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;
use crate::pairing::tree_digest;
use crate::test_remote::{Remote, git};

const T1: &str = "id = \"t1\"\n";
const T2: &str = "id = \"t2\"\n";

/// A repository on the remote with a trunk that holds code and, beside it, the registry.
fn trunk(remote: &Remote, name: &str) -> PathBuf {
    let dir = remote.clone_dir(name);
    git(&dir, &["config", "user.name", "tester"]);
    git(&dir, &["config", "user.email", "tester@local"]);
    git(&dir, &["config", "push.negotiate", "false"]);
    for (path, text) in [
        ("mock/crates/a/lib.rs", "fn a() {}\n"),
        ("mock/registry/ticket/items/t1.toml", T1),
        ("mock/registry/ticket/items/t2.toml", T2),
    ] {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "trunk"]);
    dir
}

fn registry(dir: &Path) -> DataRef {
    DataRef::new(dir, "registry").unwrap()
}

fn names(dir: &Path, rev: &str) -> String {
    git(dir, &["ls-tree", "-r", "--name-only", rev]).replace('\n', " ")
}

#[test]
fn a_ref_is_seeded_from_a_subtree_with_no_parent_and_is_not_a_branch() {
    let remote = Remote::new();
    let dir = trunk(&remote, "a");
    let reg = registry(&dir);
    assert_eq!(reg.tip().unwrap(), None, "no ref before it is seeded");
    let commit = reg.seed("HEAD:mock/registry", "registry: seed").unwrap();
    assert_eq!(reg.tip().unwrap(), Some(commit.clone()));
    assert_eq!(
        names(&dir, "refs/mock/registry"),
        "ticket/items/t1.toml ticket/items/t2.toml"
    );
    assert_eq!(
        git(&dir, &["rev-list", "--parents", "-n1", &commit]),
        commit,
        "no parent"
    );
    assert!(
        !git(&dir, &["branch", "-a"]).contains("mock"),
        "not a branch"
    );
    assert!(git(&dir, &["for-each-ref", "refs/mock"]).contains("refs/mock/registry"));
    // seeding over it is refused, and so is a tree that is not one and a name that is not a name
    assert!(
        reg.seed("HEAD:mock/registry", "again")
            .unwrap_err()
            .contains("exists")
    );
    // a spec that names no tree is refused and leaves no ref behind
    let other = DataRef::new(&dir, "other").unwrap();
    assert!(other.seed("HEAD:no/such/dir", "x").is_err());
    assert_eq!(other.tip().unwrap(), None);
    assert!(DataRef::new(&dir, "Registry").is_err());
    assert!(DataRef::new(&dir, "a/b").is_err());
    assert!(DataRef::new(&dir, "").is_err());
}

#[test]
fn an_instance_is_made_in_an_empty_directory_and_the_branch_never_sees_it() {
    let remote = Remote::new();
    let dir = trunk(&remote, "a");
    let reg = registry(&dir);
    assert!(
        reg.instantiate(Path::new(".mock/registry")).is_err(),
        "no ref to instantiate yet"
    );
    reg.seed("HEAD:mock/registry", "seed").unwrap();
    let inst = Path::new(".mock/registry");
    reg.instantiate(inst).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.join(".mock/registry/ticket/items/t1.toml")).unwrap(),
        T1
    );
    assert_eq!(
        git(&dir, &["status", "--porcelain"]),
        "",
        "excluded from the branch's status"
    );
    assert_eq!(
        git(&dir, &["diff", "--cached", "--name-only"]),
        "",
        "the branch's index is untouched"
    );
    assert!(
        reg.instantiate(inst).unwrap_err().contains("empty"),
        "a second instance over it"
    );
    // the control for the exclusion: the same directory unexcluded shows up
    std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
    std::fs::write(dir.join("elsewhere/f"), "x").unwrap();
    assert!(git(&dir, &["status", "--porcelain"]).contains("elsewhere"));
}

#[test]
fn a_commit_carries_an_added_file_an_edit_and_a_deletion_onto_the_tip() {
    let remote = Remote::new();
    let dir = trunk(&remote, "a");
    let reg = registry(&dir);
    let seed = reg.seed("HEAD:mock/registry", "seed").unwrap();
    let inst = Path::new(".mock/registry");
    reg.instantiate(inst).unwrap();
    assert!(!reg.dirty(inst).unwrap(), "nothing changed yet");
    assert_eq!(
        reg.commit(inst, "nothing").unwrap(),
        None,
        "an unchanged instance commits nothing"
    );
    assert_eq!(reg.tip().unwrap(), Some(seed.clone()));
    let items = dir.join(".mock/registry/ticket/items");
    std::fs::write(items.join("t3.toml"), "id = \"t3\"\n").unwrap();
    std::fs::write(items.join("t1.toml"), "id = \"t1\"\nstate = \"done\"\n").unwrap();
    std::fs::remove_file(items.join("t2.toml")).unwrap();
    assert!(reg.dirty(inst).unwrap());
    let new = reg
        .commit(inst, "registry: three changes")
        .unwrap()
        .expect("a commit");
    assert_eq!(reg.tip().unwrap(), Some(new.clone()));
    assert_eq!(
        git(&dir, &["rev-parse", &format!("{new}^")]),
        seed,
        "on the old tip"
    );
    assert_eq!(
        names(&dir, &new),
        "ticket/items/t1.toml ticket/items/t3.toml"
    );
    assert!(git(&dir, &["show", &format!("{new}:ticket/items/t1.toml")]).contains("done"));
    assert!(
        !reg.dirty(inst).unwrap(),
        "committed back, nothing left over"
    );
    assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}

#[test]
fn a_commit_that_finds_the_ref_moved_since_it_started_is_refused_and_moves_nothing() {
    let remote = Remote::new();
    let dir = trunk(&remote, "a");
    let reg = registry(&dir);
    let seed = reg.seed("HEAD:mock/registry", "seed").unwrap();
    let inst = Path::new(".mock/registry");
    reg.instantiate(inst).unwrap();
    let items = dir.join(".mock/registry/ticket/items");
    std::fs::write(items.join("t3.toml"), "id = \"t3\"\n").unwrap();
    let winner = reg.commit(inst, "winner").unwrap().unwrap();
    std::fs::write(items.join("t4.toml"), "id = \"t4\"\n").unwrap();
    // a writer that read the tip before the winner moved it
    let err = reg.commit_expecting(inst, "loser", &seed).unwrap_err();
    assert!(err.contains("update-ref"), "{err}");
    assert_eq!(
        reg.tip().unwrap(),
        Some(winner),
        "the ref is where the winner left it"
    );
    // the control: the same change goes in when it is made against the tip
    assert!(reg.commit(inst, "retried").unwrap().is_some());
}

#[test]
fn a_ref_travels_by_name_and_a_plain_push_does_not_carry_it() {
    let remote = Remote::new();
    let a = trunk(&remote, "a");
    let reg = registry(&a);
    reg.seed("HEAD:mock/registry", "seed").unwrap();
    git(&a, &["push", "-q", "origin", "HEAD:refs/heads/dev"]);
    assert!(
        remote.tip("refs/heads/dev").is_some(),
        "the control: the branch landed"
    );
    assert_eq!(
        remote.tip("refs/mock/registry"),
        None,
        "a plain push carries no data ref"
    );
    assert_eq!(reg.push("origin").unwrap(), Pushed::Landed);
    assert_eq!(remote.tip("refs/mock/registry"), reg.tip().unwrap());
    assert!(
        remote
            .file("refs/mock/registry", "ticket/items/t1.toml")
            .is_some()
    );
    assert!(
        git(&a, &["ls-remote", "origin", "refs/mock/*"]).contains("refs/mock/registry"),
        "the control: the remote lists it once pushed by name"
    );
}

#[test]
fn a_fetch_creates_fast_forwards_and_reports_what_it_finds() {
    let remote = Remote::new();
    let a = trunk(&remote, "a");
    let ra = registry(&a);
    ra.seed("HEAD:mock/registry", "seed").unwrap();
    ra.push("origin").unwrap();
    let b = remote.clone_dir("b");
    let rb = registry(&b);
    // nothing local, so a shallow fetch makes it
    assert_eq!(rb.fetch("origin", Some(1)).unwrap(), Synced::Created);
    assert_eq!(rb.tip().unwrap(), ra.tip().unwrap());
    assert_eq!(rb.fetch("origin", None).unwrap(), Synced::UpToDate);
    // the remote moves ahead
    ra.instantiate(Path::new(".mock/registry")).unwrap();
    std::fs::write(
        a.join(".mock/registry/ticket/items/t3.toml"),
        "id = \"t3\"\n",
    )
    .unwrap();
    ra.commit(Path::new(".mock/registry"), "t3")
        .unwrap()
        .unwrap();
    ra.push("origin").unwrap();
    assert_eq!(rb.fetch("origin", None).unwrap(), Synced::FastForwarded);
    assert_eq!(rb.tip().unwrap(), ra.tip().unwrap());
    assert!(names(&b, "refs/mock/registry").contains("t3.toml"));
    // a local commit not pushed yet is ahead of the remote
    rb.instantiate(Path::new(".mock/registry")).unwrap();
    std::fs::write(
        b.join(".mock/registry/ticket/items/t5.toml"),
        "id = \"t5\"\n",
    )
    .unwrap();
    rb.commit(Path::new(".mock/registry"), "t5")
        .unwrap()
        .unwrap();
    assert_eq!(rb.fetch("origin", None).unwrap(), Synced::Ahead);
}

#[test]
fn two_writers_race_and_the_loser_is_told_at_the_push_and_at_the_fetch() {
    let remote = Remote::new();
    let a = trunk(&remote, "a");
    let ra = registry(&a);
    ra.seed("HEAD:mock/registry", "seed").unwrap();
    ra.push("origin").unwrap();
    let b = remote.clone_dir("b");
    let rb = registry(&b);
    rb.fetch("origin", Some(1)).unwrap();
    let inst = Path::new(".mock/registry");
    ra.instantiate(inst).unwrap();
    rb.instantiate(inst).unwrap();
    std::fs::write(
        a.join(".mock/registry/ticket/items/won.toml"),
        "id = \"won\"\n",
    )
    .unwrap();
    std::fs::write(
        b.join(".mock/registry/ticket/items/lost.toml"),
        "id = \"lost\"\n",
    )
    .unwrap();
    git(&b, &["config", "user.name", "tester"]);
    git(&b, &["config", "user.email", "tester@local"]);
    ra.commit(inst, "won").unwrap().unwrap();
    assert_eq!(ra.push("origin").unwrap(), Pushed::Landed);
    let theirs = rb.commit(inst, "lost").unwrap().unwrap();
    assert_eq!(rb.push("origin").unwrap(), Pushed::Raced);
    assert_eq!(
        remote.tip("refs/mock/registry"),
        ra.tip().unwrap(),
        "the remote kept the winner"
    );
    assert_eq!(rb.fetch("origin", None).unwrap(), Synced::Diverged);
    assert_eq!(
        rb.tip().unwrap(),
        Some(theirs),
        "a diverged fetch moves nothing"
    );
}

#[test]
fn a_remote_without_the_ref_is_an_error_and_not_an_empty_registry() {
    let remote = Remote::new();
    let a = trunk(&remote, "a");
    git(&a, &["push", "-q", "origin", "HEAD:refs/heads/dev"]);
    let rb = registry(&remote.clone_dir("b"));
    let err = rb.fetch("origin", Some(1)).unwrap_err();
    assert!(err.contains("refs/mock/registry"), "{err}");
    assert_eq!(rb.tip().unwrap(), None, "nothing was made out of the miss");
}

#[test]
fn a_pairing_is_the_tip_its_time_and_a_digest_worked_out_without_git() {
    let remote = Remote::new();
    let dir = trunk(&remote, "a");
    let reg = registry(&dir);
    reg.seed("HEAD:mock/registry", "seed").unwrap();
    let p = reg.pairing_of(None).unwrap();
    assert_eq!(Some(p.hash.clone()), reg.tip().unwrap());
    assert_eq!(
        p.time.to_string(),
        git(&dir, &["log", "-1", "--format=%ct", &p.hash])
    );
    // the digest is made again from the files as the test knows them, with no git object in it
    let expected = tree_digest([
        ("ticket/items/t1.toml", T1.as_bytes()),
        ("ticket/items/t2.toml", T2.as_bytes()),
    ])
    .unwrap();
    assert_eq!(p.digest, expected);
    assert_eq!(Pairing::parse(&p.render()).unwrap(), p);
    assert_eq!(reg.resolve(&p).unwrap(), Resolution::ByHash(p.hash.clone()));
}

/// A commit of the same tree as `HEAD:mock/registry`, at another time, on a ref of its own: the
/// registry as a rehoused repository holds it, with the files and none of the hashes.
fn rehoused(dir: &Path, text_of_t1: Option<&str>, when: i64) -> String {
    if let Some(text) = text_of_t1 {
        std::fs::write(dir.join("mock/registry/ticket/items/t1.toml"), text).unwrap();
        git(dir, &["commit", "-q", "-a", "-m", "changed"]);
    }
    let tree = git(dir, &["rev-parse", "HEAD:mock/registry"]);
    let date = format!("{when} +0000");
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit-tree", "-m", "registry: seed", &tree])
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .unwrap();
    assert!(out.status.success());
    let commit = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    git(dir, &["update-ref", "refs/mock/registry", &commit]);
    commit
}

#[test]
fn a_pairing_finds_its_commit_after_a_rehouse_by_the_time_and_the_digest() {
    let remote = Remote::new();
    let a = trunk(&remote, "a");
    let ra = registry(&a);
    ra.seed("HEAD:mock/registry", "seed").unwrap();
    let recorded = ra.pairing_of(None).unwrap();
    let near = recorded.time + 120;
    let far = recorded.time + 3 * 24 * 60 * 60;
    // the same files, a different hash, two minutes later
    let c = trunk(&remote, "c");
    let moved = rehoused(&c, None, near);
    assert_ne!(moved, recorded.hash, "a different commit");
    assert_eq!(
        registry(&c).resolve(&recorded).unwrap(),
        Resolution::ByTimeAndDigest(moved.clone())
    );
    // the control for the time: the same files three days off are not taken
    let d = trunk(&remote, "d");
    rehoused(&d, None, far);
    assert_eq!(
        registry(&d).resolve(&recorded).unwrap(),
        Resolution::Missing
    );
    // the control for the digest: a commit in the window with another file is not taken
    let e = trunk(&remote, "e");
    rehoused(&e, Some("id = \"t1\"\nstate = \"other\"\n"), near);
    assert_eq!(
        registry(&e).resolve(&recorded).unwrap(),
        Resolution::Missing
    );
}

#[test]
fn a_hash_that_is_there_with_other_files_is_a_mismatch_through_the_ref() {
    let remote = Remote::new();
    let dir = trunk(&remote, "a");
    let reg = registry(&dir);
    reg.seed("HEAD:mock/registry", "seed").unwrap();
    let mut recorded = reg.pairing_of(None).unwrap();
    recorded.digest = "0".repeat(64);
    match reg.resolve(&recorded).unwrap() {
        Resolution::Mismatch {
            hash,
            found,
        } => {
            assert_eq!(hash, recorded.hash);
            assert!(found.is_some());
        },
        other => panic!("{other:?}"),
    }
}
