//! An instance against writers that win a race: the same file written twice, a file turning
//! into a directory, a merge landing, instances made at the same moment.
mod common;

use std::path::{Path, PathBuf};

use common::{MOCK_REF, World, git, store};
use mockspace_store::{
    Edit,
    Instance,
    MergeRefusal,
    Reader,
    Reapply,
    RefStore,
    Refuse,
    Store,
    StoreError,
};

const T1: &str = "id = \"t1\"\n";
const T2: &str = "id = \"t2\"\n";
const MINE: &str = "id = \"t1\"\nmine = 1\n";
const THEIRS: &str = "id = \"t1\"\ntheirs = 1\n";

fn seeded(w: &World) -> RefStore {
    let a = store(w, "a");
    a.apply(
        &Edit::new()
            .put("ticket/t1.toml", T1)
            .put("ticket/t2.toml", T2),
        "seed",
        &Reapply,
    )
    .unwrap();
    a
}

fn dir_of(w: &World, clone: &str, leaf: &str) -> PathBuf {
    w.dir.path().join(clone).join(".mock").join(leaf)
}

/// The remote's file, with the newline the shared helper trims put back: every file here ends
/// in one.
fn remote_t1(w: &World) -> String {
    format!("{}\n", w.file(MOCK_REF, "ticket/t1.toml").unwrap())
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

/// The lines `ours` added to the base, appended to what the tip holds: a three-way merge of
/// a file of lines, with the base read through the store that holds the instance.
fn merging<'r>(
    base: &'r dyn Reader,
) -> impl Fn(&dyn Reader, &Edit) -> Result<Edit, MergeRefusal> + 'r {
    move |tip, ours| {
        let mut next = Edit::new();
        for (path, bytes) in ours.entries() {
            let text = |r: &dyn Reader| {
                r.read(path)
                    .unwrap()
                    .map(|b| String::from_utf8(b).unwrap())
                    .unwrap_or_default()
            };
            let (was, theirs) = (text(base), text(tip));
            let mut merged = theirs.clone();
            let ours = String::from_utf8(bytes.unwrap_or_default().to_vec()).unwrap();
            for line in ours.lines() {
                if !was.lines().any(|l| l == line) && !theirs.lines().any(|l| l == line) {
                    merged.push_str(line);
                    merged.push('\n');
                }
            }
            next = next.put(path, merged);
        }
        Ok(next)
    }
}

#[test]
fn a_stale_edit_of_a_file_the_winner_changed_goes_through_the_hook() {
    let w = World::new();
    let a = seeded(&w);
    let dir = dir_of(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    let base = i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new().put("ticket/t1.toml", THEIRS),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let r = i.apply("mine", &Refuse);
    assert!(matches!(r, Err(StoreError::Merge { .. })), "{r:?}");
    assert_eq!(
        remote_t1(&w),
        THEIRS,
        "the winner's file is still the remote's"
    );
    assert_eq!(
        read(&dir, "ticket/t1.toml"),
        MINE,
        "what was written here is kept"
    );
    assert_eq!(i.base().unwrap(), Some(base), "the base did not move");
}

#[test]
fn the_same_after_a_refresh_which_is_what_every_run_of_the_launcher_does() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = dir_of(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new().put("ticket/t1.toml", THEIRS),
        "theirs",
        &Reapply,
    )
    .unwrap();
    b.refresh().unwrap();
    let r = i.apply("mine", &Refuse);
    assert!(matches!(r, Err(StoreError::Merge { .. })), "{r:?}");
    assert_eq!(remote_t1(&w), THEIRS);
}

#[test]
fn nobody_else_writing_is_not_a_race_so_a_refusing_hook_is_not_asked() {
    let w = World::new();
    let a = seeded(&w);
    let dir = dir_of(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    let applied = i.apply("mine", &Refuse).unwrap();
    assert!(applied.changed);
    assert_eq!(remote_t1(&w), MINE);
}

#[test]
fn a_merging_hook_lands_the_merge_and_the_directory_takes_it_so_the_next_apply_has_nothing() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = dir_of(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    let base = i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new().put("ticket/t1.toml", THEIRS),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let was = b.reader_of(&base).unwrap();
    let merge = merging(&was);
    let first = i.apply("mine", &merge).unwrap();
    let merged = remote_t1(&w);
    assert!(
        merged.contains("theirs = 1") && merged.contains("mine = 1"),
        "{merged:?}"
    );
    assert_eq!(
        read(&dir, "ticket/t1.toml"),
        merged,
        "the directory holds the merge"
    );
    assert_eq!(i.base().unwrap(), first.commit);
    assert!(!i.is_dirty().unwrap());
    let commits = w.commits(MOCK_REF);
    let second = i.apply("mine again", &merge).unwrap();
    assert!(!second.changed, "{second:?}");
    assert_eq!(w.commits(MOCK_REF), commits, "nothing more was written");
    assert_eq!(remote_t1(&w), merged, "the merge was not written over");
}

#[test]
fn a_hook_that_keeps_the_winners_file_leaves_the_directory_at_the_winners_file() {
    let w = World::new();
    let a = seeded(&w);
    let dir = dir_of(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new().put("ticket/t1.toml", THEIRS),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let keep = |_: &dyn Reader, _: &Edit| -> Result<Edit, MergeRefusal> { Ok(Edit::new()) };
    let applied = i.apply("mine", &keep).unwrap();
    assert!(!applied.changed);
    assert_eq!(
        read(&dir, "ticket/t1.toml"),
        THEIRS,
        "the hook's word stands"
    );
    assert!(!i.is_dirty().unwrap());
}

#[test]
fn a_file_removed_here_and_on_the_tip_is_no_conflict() {
    let w = World::new();
    let a = seeded(&w);
    let dir = dir_of(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    std::fs::remove_file(dir.join("ticket/t2.toml")).unwrap();
    a.apply(&Edit::new().remove("ticket/t2.toml"), "gone", &Reapply)
        .unwrap();
    i.apply("gone here too", &Reapply).unwrap();
    assert!(!dir.join("ticket/t2.toml").exists());
    assert!(!i.is_dirty().unwrap());
}

#[test]
fn a_conflicting_update_names_the_file_writes_nothing_and_apply_with_a_merge_is_the_way_on() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = dir_of(&w, "b", "registry");
    let i = Instance::new(&b, &dir).unwrap();
    let base = i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new()
            .put("ticket/t1.toml", THEIRS)
            .put("ticket/t9.toml", "t9\n"),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let err = i.update().unwrap_err();
    assert!(err.to_string().contains("ticket/t1.toml"), "{err}");
    assert!(!dir.join("ticket/t9.toml").exists(), "nothing was written");
    assert_eq!(read(&dir, "ticket/t1.toml"), MINE);
    assert_eq!(i.base().unwrap(), Some(base.clone()));
    let was = b.reader_of(&base).unwrap();
    i.apply("merge", &merging(&was)).unwrap();
    assert!(read(&dir, "ticket/t1.toml").contains("theirs = 1"));
    assert_eq!(
        read(&dir, "ticket/t9.toml"),
        "t9\n",
        "and the tip's other file arrived"
    );
}

#[test]
fn a_tip_that_turns_a_file_into_a_directory_is_followed() {
    let w = World::new();
    let a = seeded(&w);
    a.apply(&Edit::new().put("x", "file\n"), "x", &Reapply)
        .unwrap();
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = dir_of(&w, "b", "registry");
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
    let dir = dir_of(&w, "b", "registry");
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
    let dir = dir_of(&w, "b", "registry");
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
            dir_of(&w, "a", &format!("p{n}")),
            dir_of(&w, "a", &format!("q{n}")),
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
    let dir = dir_of(&w, "a", "registry");
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
    let dir = dir_of(&w, "a", "registry");
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
