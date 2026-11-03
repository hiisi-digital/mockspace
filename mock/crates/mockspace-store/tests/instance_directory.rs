//! An instance and the shape of its directory: files and directories changing places, a file in
//! the way, instances made together, two spellings of one directory, a materialize that fails.
mod common;

use common::{T1, World, git, inst_dir, read, seeded, store, write};
use mockspace_store::{Edit, Instance, Reapply, Store};

#[test]
fn an_empty_directory_where_the_tip_puts_a_file_gives_way_to_it() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    i.materialize().unwrap();
    std::fs::create_dir_all(dir.join("ticket/new")).unwrap();
    a.apply(
        &Edit::new()
            .remove("ticket/t2.toml")
            .put("ticket/new", "a file\n"),
        "theirs",
        &Reapply,
    )
    .unwrap();
    i.update().unwrap();
    assert_eq!(read(&dir, "ticket/new"), "a file\n");
    assert!(!dir.join("ticket/t2.toml").exists());
    assert!(!i.is_dirty().unwrap());
}

#[test]
fn an_instance_whose_files_were_all_removed_is_not_filled_again_over_the_removals() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    std::fs::remove_file(dir.join("ticket/t1.toml")).unwrap();
    std::fs::remove_file(dir.join("ticket/t2.toml")).unwrap();
    std::fs::remove_dir(dir.join("ticket")).unwrap();
    assert!(i.is_dirty().unwrap(), "two removals are waiting");
    let err = i.materialize().unwrap_err();
    assert!(err.to_string().contains("every file was removed"), "{err}");
    assert!(
        i.is_dirty().unwrap(),
        "the removals are still waiting after the refusal"
    );
    // the way on: apply the removals, or remove the directory and start again
    std::fs::remove_dir_all(&dir).unwrap();
    i.materialize().unwrap();
    assert_eq!(read(&dir, "ticket/t1.toml"), T1);
}

#[test]
fn exclude_lines_of_other_writers_are_kept_whatever_they_end_with() {
    let w = World::new();
    let a = seeded(&w);
    let exclude = w.dir.path().join("a/.git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "/theirs/").unwrap();
    for leaf in ["one", "two"] {
        let dir = inst_dir(&w, "a", leaf);
        Instance::new(&a, &dir).unwrap().materialize().unwrap();
    }
    let held = std::fs::read_to_string(&exclude).unwrap();
    for line in ["/theirs/", "/.mock/one/", "/.mock/two/"] {
        assert_eq!(held.lines().filter(|l| *l == line).count(), 1, "{held:?}");
    }
}

#[test]
fn a_tip_that_turns_a_file_into_a_directory_is_followed() {
    let w = World::new();
    let a = seeded(&w);
    a.apply(&Edit::new().put("x", "file\n"), "x", &Reapply)
        .unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    i.materialize().unwrap();
    a.apply(
        &Edit::new().remove("x").put("x/y", "nested\n"),
        "x to dir",
        &Reapply,
    )
    .unwrap();
    i.update().unwrap();
    assert_eq!(read(&dir, "x/y"), "nested\n");
    assert!(!i.is_dirty().unwrap());
}

#[test]
fn a_tip_that_turns_a_directory_into_a_file_is_followed() {
    let w = World::new();
    let a = seeded(&w);
    a.apply(&Edit::new().put("x/y", "nested\n"), "x", &Reapply)
        .unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    i.materialize().unwrap();
    a.apply(
        &Edit::new().remove("x/y").put("x", "file\n"),
        "x to file",
        &Reapply,
    )
    .unwrap();
    i.update().unwrap();
    assert_eq!(read(&dir, "x"), "file\n");
    assert!(!i.is_dirty().unwrap());
}

#[test]
fn a_file_here_in_the_way_of_the_tip_is_named_and_nothing_is_written() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    let base = i.materialize().unwrap();
    write(&dir, "ticket/new/z", "mine\n");
    a.apply(
        &Edit::new()
            .put("ticket/new", "a file\n")
            .put("ticket/t9.toml", "t9\n"),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let err = i.update().unwrap_err();
    assert!(err.to_string().contains("ticket/new"), "{err}");
    assert!(!dir.join("ticket/t9.toml").exists(), "nothing was written");
    assert_eq!(read(&dir, "ticket/new/z"), "mine\n");
    assert_eq!(i.base().unwrap(), Some(base));
    // the control: with the file moved out of the way the same update goes through
    std::fs::remove_dir_all(dir.join("ticket/new")).unwrap();
    i.update().unwrap();
    assert_eq!(read(&dir, "ticket/new"), "a file\n");
}

#[test]
fn instances_made_at_the_same_moment_keep_both_records_and_both_exclude_lines() {
    let w = World::new();
    let a = seeded(&w);
    let mut lost = Vec::new();
    for n in 0 .. 40 {
        let (d1, d2) = (
            inst_dir(&w, "a", &format!("p{n}")),
            inst_dir(&w, "a", &format!("q{n}")),
        );
        let (i1, i2) = (
            Instance::new(&a, &d1).unwrap(),
            Instance::new(&a, &d2).unwrap(),
        );
        std::thread::scope(|s| {
            let h1 = s.spawn(|| i1.materialize());
            let h2 = s.spawn(|| i2.materialize());
            h1.join().unwrap().unwrap();
            h2.join().unwrap().unwrap();
        });
        if i1.base().unwrap().is_none() || i2.base().unwrap().is_none() {
            lost.push(n);
        }
    }
    assert!(lost.is_empty(), "records lost in rounds {lost:?}");
    assert_eq!(
        git(&w.dir.path().join("a"), &["status", "--porcelain"]),
        "",
        "every instance is excluded"
    );
}

#[test]
fn two_spellings_of_one_directory_are_one_instance() {
    let w = World::new();
    let a = seeded(&w);
    let real = w.dir.path().join("a").join(".mock");
    std::fs::create_dir_all(&real).unwrap();
    let alias = w.dir.path().join("alias");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let by_alias = Instance::new(&a, alias.join("registry")).unwrap();
    let by_path = Instance::new(&a, real.join("sub/../registry")).unwrap();
    assert_eq!(by_alias.dir(), by_path.dir());
    let tip = by_alias.materialize().unwrap();
    assert_eq!(by_path.base().unwrap(), Some(tip));
}

#[test]
fn a_materialize_that_fails_after_the_files_are_written_leaves_nothing_behind() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    // the records directory is a file, so the record cannot be written
    let common = w.dir.path().join("a").join(".git").join("mock-instances");
    std::fs::write(&common, "in the way").unwrap();
    let i = Instance::new(&a, &dir).unwrap();
    assert!(i.materialize().is_err());
    assert!(!dir.exists(), "the directory it made is gone");
    // and once the way is clear it can be made
    std::fs::remove_file(&common).unwrap();
    i.materialize().unwrap();
    assert_eq!(read(&dir, "ticket/t1.toml"), T1);
}

#[test]
fn a_pre_existing_empty_directory_is_left_empty_when_materialize_fails() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    std::fs::create_dir_all(&dir).unwrap();
    let common = w.dir.path().join("a").join(".git").join("mock-instances");
    std::fs::write(&common, "in the way").unwrap();
    let i = Instance::new(&a, &dir).unwrap();
    assert!(i.materialize().is_err());
    assert!(dir.is_dir());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "emptied again");
    std::fs::remove_file(&common).unwrap();
    i.materialize().unwrap();
}
