//! An instance against real git: a directory that is a view of a ref, moved by deltas.
#![allow(unused_imports)]
mod common;

use std::path::PathBuf;

use common::{MOCK_REF, NS, T1, T2, World, git, inst_dir, read, seeded, store};
use mockspace_store::{Edit, Instance, Reader, Reapply, RefPair, RefStore, Store, StoreError};

/// A second clone that has fetched the ref.
fn second(w: &World, name: &str) -> RefStore {
    let b = store(w, name);
    b.refresh().unwrap();
    b
}

fn clone_path(w: &World, name: &str) -> PathBuf {
    w.dir.path().join(name)
}

fn on_remote(w: &World) -> String {
    git(&w.bare, &["ls-tree", "-r", "--name-only", MOCK_REF]).replace('\n', " ")
}

#[test]
fn materialize_writes_the_tip_records_the_base_and_the_branch_never_sees_it() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    assert_eq!(i.base().unwrap(), None, "nothing made here yet");
    let tip = i.materialize().unwrap();
    assert_eq!(Some(tip.clone()), a.local_tip().unwrap());
    assert_eq!(read(&dir, "ticket/t1.toml"), T1);
    assert_eq!(read(&dir, "ticket/t2.toml"), T2);
    assert_eq!(i.base().unwrap().map(|b| b.to_string()), Some(tip));
    assert!(!i.is_dirty().unwrap());
    assert_eq!(
        git(&clone_path(&w, "a"), &["status", "--porcelain"]),
        "",
        "the branch's status does not see the instance"
    );
    // the control: a directory that was not excluded does show
    std::fs::create_dir_all(clone_path(&w, "a").join("elsewhere")).unwrap();
    std::fs::write(clone_path(&w, "a").join("elsewhere/f"), "x").unwrap();
    assert!(git(&clone_path(&w, "a"), &["status", "--porcelain"]).contains("elsewhere"));
    // made again after the directory was removed, the exclude line is not written twice
    std::fs::remove_dir_all(&dir).unwrap();
    i.materialize().unwrap();
    let exclude = std::fs::read_to_string(clone_path(&w, "a").join(".git/info/exclude")).unwrap();
    assert_eq!(
        exclude.lines().filter(|l| *l == "/.mock/registry/").count(),
        1
    );
}

#[test]
fn materialize_refuses_a_directory_with_files_and_a_ref_that_is_not_there() {
    let w = World::new();
    let a = seeded(&w);
    let busy = inst_dir(&w, "a", "busy");
    std::fs::create_dir_all(&busy).unwrap();
    std::fs::write(busy.join("mine"), "x").unwrap();
    let err = Instance::new(&a, &busy).unwrap().materialize().unwrap_err();
    assert!(matches!(err, StoreError::Instance { .. }), "{err}");
    assert!(err.to_string().contains("not an empty directory"), "{err}");
    assert_eq!(
        std::fs::read_to_string(busy.join("mine")).unwrap(),
        "x",
        "left as it was"
    );
    // a clone that never fetched has no ref to show, and says so rather than showing nothing
    let c = store(&w, "c");
    let none = inst_dir(&w, "c", "registry");
    let err = Instance::new(&c, &none).unwrap().materialize().unwrap_err();
    assert!(matches!(err, StoreError::NotFetched { .. }), "{err}");
    assert!(!none.exists(), "no directory made for it");
}

#[test]
fn an_edit_is_the_delta_against_the_base_and_nothing_more() {
    let w = World::new();
    let a = seeded(&w);
    a.apply(
        &Edit::new().put("ticket/t4.toml", "id = \"t4\"\n"),
        "t4",
        &Reapply,
    )
    .unwrap();
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    std::fs::write(dir.join("ticket/t3.toml"), "id = \"t3\"\n").unwrap();
    std::fs::write(
        dir.join("ticket/t1.toml"),
        "id = \"t1\"\nstate = \"done\"\n",
    )
    .unwrap();
    std::fs::remove_file(dir.join("ticket/t2.toml")).unwrap();
    assert!(i.is_dirty().unwrap());
    let edit = i.edit().unwrap();
    let got: Vec<(String, Option<String>)> = edit
        .entries()
        .map(|(p, b)| {
            (
                p.to_owned(),
                b.map(|b| String::from_utf8(b.to_vec()).unwrap()),
            )
        })
        .collect();
    assert_eq!(got, vec![
        (
            "ticket/t1.toml".to_owned(),
            Some("id = \"t1\"\nstate = \"done\"\n".to_owned())
        ),
        ("ticket/t2.toml".to_owned(), None),
        (
            "ticket/t3.toml".to_owned(),
            Some("id = \"t3\"\n".to_owned())
        ),
    ]);
}

#[test]
fn a_stale_instance_adds_to_the_tip_that_won_and_erases_nothing() {
    let w = World::new();
    let a = seeded(&w);
    let b = second(&w, "b");
    let (da, db) = (inst_dir(&w, "a", "registry"), inst_dir(&w, "b", "registry"));
    let (ia, ib) = (
        Instance::new(&a, &da).unwrap(),
        Instance::new(&b, &db).unwrap(),
    );
    ia.materialize().unwrap();
    ib.materialize().unwrap();
    std::fs::write(da.join("ticket/won.toml"), "id = \"won\"\n").unwrap();
    ia.apply("won", &Reapply).unwrap();
    // b still holds the seed as its base, and now writes a file of its own
    std::fs::write(db.join("ticket/late.toml"), "id = \"late\"\n").unwrap();
    let applied = ib.apply("late", &Reapply).unwrap();
    assert!(applied.changed);
    assert_eq!(
        on_remote(&w),
        "ticket/late.toml ticket/t1.toml ticket/t2.toml ticket/won.toml",
        "the winner's file is still there"
    );
    assert_eq!(w.commits(MOCK_REF), 3, "seed, won, late");
    assert_eq!(
        read(&db, "ticket/won.toml"),
        "id = \"won\"\n",
        "b's directory followed the tip"
    );
    assert!(!ib.is_dirty().unwrap());
    // the control: a removal is a removal of that file only
    std::fs::remove_file(db.join("ticket/t1.toml")).unwrap();
    ib.apply("drop t1", &Reapply).unwrap();
    assert_eq!(
        on_remote(&w),
        "ticket/late.toml ticket/t2.toml ticket/won.toml"
    );
}

#[test]
fn a_fetch_that_moves_the_tip_leaves_the_instance_clean_and_a_commit_after_it_reverts_nothing() {
    let w = World::new();
    let a = seeded(&w);
    let b = second(&w, "b");
    let db = inst_dir(&w, "b", "registry");
    let ib = Instance::new(&b, &db).unwrap();
    ib.materialize().unwrap();
    a.apply(
        &Edit::new().put("ticket/t3.toml", "id = \"t3\"\n"),
        "t3",
        &Reapply,
    )
    .unwrap();
    ib.update().unwrap();
    assert_eq!(read(&db, "ticket/t3.toml"), "id = \"t3\"\n");
    assert!(
        !ib.is_dirty().unwrap(),
        "measured against the base, not against a tip that moved"
    );
    std::fs::write(db.join("ticket/t5.toml"), "id = \"t5\"\n").unwrap();
    ib.apply("t5", &Reapply).unwrap();
    assert_eq!(
        on_remote(&w),
        "ticket/t1.toml ticket/t2.toml ticket/t3.toml ticket/t5.toml"
    );
}

#[test]
fn an_update_keeps_a_local_edit_the_tip_left_alone_and_refuses_a_file_both_changed() {
    let w = World::new();
    let a = seeded(&w);
    let b = second(&w, "b");
    let db = inst_dir(&w, "b", "registry");
    let ib = Instance::new(&b, &db).unwrap();
    ib.materialize().unwrap();
    std::fs::write(db.join("ticket/t1.toml"), "id = \"t1\"\nmine = 1\n").unwrap();
    a.apply(
        &Edit::new().put("ticket/t2.toml", "id = \"t2\"\ntheirs = 1\n"),
        "t2",
        &Reapply,
    )
    .unwrap();
    ib.update().unwrap();
    assert_eq!(
        read(&db, "ticket/t2.toml"),
        "id = \"t2\"\ntheirs = 1\n",
        "taken from the tip"
    );
    assert_eq!(
        read(&db, "ticket/t1.toml"),
        "id = \"t1\"\nmine = 1\n",
        "kept"
    );
    let paths: Vec<String> = ib
        .edit()
        .unwrap()
        .entries()
        .map(|(p, _)| p.to_owned())
        .collect();
    assert_eq!(
        paths,
        vec!["ticket/t1.toml"],
        "still the one pending change"
    );
    // now the tip changes the same file
    a.apply(
        &Edit::new().put("ticket/t1.toml", "id = \"t1\"\ntheirs = 2\n"),
        "t1",
        &Reapply,
    )
    .unwrap();
    let base = ib.base().unwrap();
    let err = ib.update().unwrap_err();
    match &err {
        StoreError::Instance {
            paths,
            ..
        } => assert_eq!(paths, &vec!["ticket/t1.toml".to_owned()]),
        other => panic!("{other}"),
    }
    assert_eq!(
        read(&db, "ticket/t1.toml"),
        "id = \"t1\"\nmine = 1\n",
        "nothing written"
    );
    assert_eq!(ib.base().unwrap(), base, "the base did not move");
}

// Mutation: read the repository's `info/exclude` as an ignore rule; the line that hides an instance
// from its branch, or a pattern one person keeps there, then turns data into files no commit carries.
#[test]
fn a_pattern_of_the_exclude_file_does_not_keep_a_file_out() {
    let w = World::new();
    let a = seeded(&w);
    std::fs::write(clone_path(&w, "a").join(".git/info/exclude"), "*.log\n").unwrap();
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    std::fs::write(dir.join("ticket/run.log"), "x\n").unwrap();
    assert!(i.is_dirty().unwrap(), "a name only the exclude file matches is data");
    assert!(i.ignored().unwrap().is_empty());
    i.apply("log", &Reapply).unwrap();
    assert!(on_remote(&w).contains("ticket/run.log"));
}

// Mutation: read every file as data whatever a `.gitignore` says; a file the work tree keeps out of
// every commit then reaches the ref.
#[test]
fn a_file_a_gitignore_matches_is_not_data_and_is_named() {
    let w = World::new();
    let a = seeded(&w);
    std::fs::write(clone_path(&w, "a").join(".gitignore"), "*.log\n").unwrap();
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    std::fs::write(dir.join("ticket/run.log"), "x\n").unwrap();
    assert!(!i.is_dirty().unwrap(), "an ignored file read as an edit");
    assert_eq!(i.ignored().unwrap(), vec!["ticket/run.log".to_string()]);
    // the control: a file no pattern matches beside it is an edit
    std::fs::write(dir.join("ticket/run.txt"), "x\n").unwrap();
    assert!(i.is_dirty().unwrap());
    i.apply("data", &Reapply).unwrap();
    let held = on_remote(&w);
    assert!(held.contains("ticket/run.txt") && !held.contains("ticket/run.log"), "{held}");
    assert_eq!(read(&dir, "ticket/run.log"), "x\n", "the ignored file was touched");
}

#[test]
fn a_filter_the_repository_sets_does_not_change_what_the_instance_hashes() {
    let w = World::new();
    let a = store(&w, "a");
    let ident = "x = \"$Id: abc$\"\n";
    a.apply(
        &Edit::new().put("ticket/ident.toml", ident),
        "ident",
        &Reapply,
    )
    .unwrap();
    // `ident` rewrites `$Id: ...$` to `$Id$` whenever git hashes a file by its path
    std::fs::write(
        clone_path(&w, "a").join(".git/info/attributes"),
        "* ident\n",
    )
    .unwrap();
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    let filtered = git(&clone_path(&w, "a"), &[
        "hash-object",
        dir.join("ticket/ident.toml").to_str().unwrap(),
    ]);
    let stored = git(&w.bare, &[
        "rev-parse",
        &format!("{MOCK_REF}:ticket/ident.toml"),
    ]);
    assert_ne!(
        filtered, stored,
        "the control: with the filter on, git hashes the file differently"
    );
    assert!(
        !i.is_dirty().unwrap(),
        "the instance hashes the bytes as they are"
    );
}

#[test]
fn an_instance_in_a_linked_worktree_is_excluded_from_that_worktree() {
    let w = World::new();
    seeded(&w);
    let main = clone_path(&w, "a");
    git(&main, &["commit", "--allow-empty", "-q", "-m", "base"]);
    let wt = w.dir.path().join("wt");
    git(&main, &[
        "worktree",
        "add",
        "-q",
        "--detach",
        wt.to_str().unwrap(),
    ]);
    let s = RefStore::new(&wt, w.remote(), RefPair::ref_form(NS).unwrap());
    s.refresh().unwrap();
    let dir = wt.join(".mock").join("registry");
    Instance::new(&s, &dir).unwrap().materialize().unwrap();
    assert_eq!(
        git(&wt, &["status", "--porcelain"]),
        "",
        "excluded in the worktree it sits in"
    );
    // the control: without the line the same directory is untracked there
    std::fs::create_dir_all(wt.join("other")).unwrap();
    std::fs::write(wt.join("other/f"), "x").unwrap();
    assert!(git(&wt, &["status", "--porcelain"]).contains("other"));
}

#[test]
fn an_instance_outside_the_repository_needs_no_exclusion() {
    let w = World::new();
    let a = seeded(&w);
    let outside = tempfile::tempdir().unwrap();
    let dir = outside.path().join("registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    assert_eq!(read(&dir, "ticket/t1.toml"), T1);
    assert_eq!(git(&clone_path(&w, "a"), &["status", "--porcelain"]), "");
    let exclude = std::fs::read_to_string(clone_path(&w, "a").join(".git/info/exclude")).unwrap();
    assert!(!exclude.contains("registry"), "{exclude}");
}

#[test]
fn a_symlink_and_a_vanished_directory_are_refused_rather_than_read_as_edits() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    std::os::unix::fs::symlink("/etc/hostname", dir.join("ticket/link")).unwrap();
    let err = i.edit().unwrap_err();
    assert!(err.to_string().contains("symlink"), "{err}");
    std::fs::remove_file(dir.join("ticket/link")).unwrap();
    assert!(
        i.edit().unwrap().is_empty(),
        "the control: without the link it reads clean"
    );
    std::fs::remove_dir_all(&dir).unwrap();
    let err = i.edit().unwrap_err();
    assert!(err.to_string().contains("not a directory"), "{err}");
    // and an instance nobody made is not read as an empty base
    let other = Instance::new(&a, inst_dir(&w, "a", "unmade")).unwrap();
    std::fs::create_dir_all(other.dir()).unwrap();
    assert!(
        other
            .edit()
            .unwrap_err()
            .to_string()
            .contains("not made by")
    );
}

#[test]
fn two_instances_of_one_ref_in_one_clone_keep_their_own_bases() {
    let w = World::new();
    let a = seeded(&w);
    let (d1, d2) = (inst_dir(&w, "a", "one"), inst_dir(&w, "a", "two"));
    let (i1, i2) = (
        Instance::new(&a, &d1).unwrap(),
        Instance::new(&a, &d2).unwrap(),
    );
    let first = i1.materialize().unwrap();
    a.apply(
        &Edit::new().put("ticket/t3.toml", "id = \"t3\"\n"),
        "t3",
        &Reapply,
    )
    .unwrap();
    let second = i2.materialize().unwrap();
    assert_ne!(first, second);
    assert_eq!(i1.base().unwrap().map(|b| b.to_string()), Some(first));
    assert_eq!(i2.base().unwrap().map(|b| b.to_string()), Some(second.clone()));
    assert!(!i1.is_dirty().unwrap(), "behind the tip is not a change");
    i1.update().unwrap();
    assert_eq!(i1.base().unwrap().map(|b| b.to_string()), Some(second));
    assert_eq!(read(&d1, "ticket/t3.toml"), "id = \"t3\"\n");
}

#[test]
fn applying_nothing_touches_neither_the_store_nor_the_remote() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    let applied = i.apply("nothing", &Reapply).unwrap();
    assert!(!applied.changed && applied.commit.is_none());
    assert_eq!(w.commits(MOCK_REF), 1);
}

#[test]
fn seed_makes_the_first_commit_of_a_ref_from_the_files_of_a_directory_and_refuses_a_ref_with_a_tip() {
    let w = World::new();
    let a = store(&w, "a");
    let dir = inst_dir(&w, "a", "research");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("note.md"), "findings\n").unwrap();
    std::fs::write(dir.join("sub/more.md"), "more\n").unwrap();
    let i = Instance::new(&a, &dir).unwrap();
    let applied = i.seed("data: research", &mockspace_store::Refuse).unwrap();
    let tip = applied.commit.expect("a commit was made");
    assert_eq!(a.local_tip().unwrap(), Some(tip.clone()));
    assert_eq!(i.base().unwrap().map(|b| b.to_string()), Some(tip));
    assert!(!i.is_dirty().unwrap(), "the directory is what the ref holds");
    assert_eq!(
        git(&clone_path(&w, "a"), &["status", "--porcelain"]),
        "",
        "the branch's status does not see the instance"
    );
    // the control: a second seed is refused, the ref has its tip and the directory its record
    let err = i.seed("again", &mockspace_store::Refuse).unwrap_err();
    assert!(matches!(err, StoreError::Instance { .. }), "{err}");
    // the control: an empty directory seeds nothing
    let empty = inst_dir(&w, "a", "empty");
    std::fs::create_dir_all(&empty).unwrap();
    let b = store(&w, "b");
    let err = Instance::new(&b, &empty).unwrap().seed("x", &mockspace_store::Refuse).unwrap_err();
    assert!(err.to_string().contains("no files"), "{err}");
}

#[test]
fn a_second_clone_with_its_own_files_cannot_seed_a_ref_another_clone_seeded() {
    let w = World::new();
    let a = store(&w, "a");
    let dir_a = inst_dir(&w, "a", "research");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::write(dir_a.join("note.md"), "findings\n").unwrap();
    let seeded_tip =
        Instance::new(&a, &dir_a).unwrap().seed("data: research", &mockspace_store::Refuse).unwrap().commit.unwrap();

    // b holds files of its own for the same ref and no record of an instance
    let b = store(&w, "b");
    let dir_b = inst_dir(&w, "b", "research");
    std::fs::create_dir_all(&dir_b).unwrap();
    std::fs::write(dir_b.join("other.md"), "mine\n").unwrap();
    b.refresh().unwrap();
    let err = Instance::new(&b, &dir_b).unwrap().seed("again", &mockspace_store::Refuse).unwrap_err();
    assert!(err.to_string().contains("already has a tip"), "{err}");
    // the control: without the fetch b sees no tip, so the refusal above is the tip branch and nothing else
    let c = store(&w, "c");
    assert_eq!(c.local_tip().unwrap(), None);
    let _ = seeded_tip;
}
