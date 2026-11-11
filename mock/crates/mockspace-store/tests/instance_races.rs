//! An instance against writers that win a race: the same file written by both, a merge hook
//! that merges, refuses, keeps the tip's file or leaves one out, and what the directory holds
//! afterwards.
mod common;

use common::{MOCK_REF, T2, World, inst_dir, read, seeded, store, write};
use mockspace_store::{Edit, Instance, MergeRefusal, Reader, Reapply, Refuse, Store, StoreError};

const MINE: &str = "id = \"t1\"\nmine = 1\n";
const THEIRS: &str = "id = \"t1\"\ntheirs = 1\n";

/// The remote's file, with the newline the shared helper trims put back: every file here ends
/// in one.
fn remote_t1(w: &World) -> String {
    format!("{}\n", w.file(MOCK_REF, "ticket/t1.toml").unwrap())
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
    let dir = inst_dir(&w, "a", "registry");
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
    assert_eq!(i.base().unwrap().map(|b| b.to_string()), Some(base), "the base did not move");
}

#[test]
fn the_same_after_a_refresh_which_is_what_every_run_of_the_launcher_does() {
    let w = World::new();
    let a = seeded(&w);
    let b = store(&w, "b");
    b.refresh().unwrap();
    let dir = inst_dir(&w, "b", "registry");
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
    let dir = inst_dir(&w, "a", "registry");
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
    let dir = inst_dir(&w, "b", "registry");
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
    assert_eq!(i.base().unwrap().map(|b| b.to_string()), first.commit);
    assert!(!i.is_dirty().unwrap());
    let commits = w.commits(MOCK_REF);
    let second = i.apply("mine again", &merge).unwrap();
    assert!(!second.changed, "{second:?}");
    assert_eq!(w.commits(MOCK_REF), commits, "nothing more was written");
    assert_eq!(remote_t1(&w), merged, "the merge was not written over");
}

#[test]
fn a_hook_that_puts_the_winners_file_leaves_the_directory_at_the_winners_file() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new().put("ticket/t1.toml", THEIRS),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let keep_theirs = |tip: &dyn Reader, _: &Edit| -> Result<Edit, MergeRefusal> {
        Ok(Edit::new().put(
            "ticket/t1.toml",
            tip.read("ticket/t1.toml").unwrap().unwrap(),
        ))
    };
    let applied = i.apply("mine", &keep_theirs).unwrap();
    assert!(!applied.changed, "the tip already holds it");
    assert_eq!(
        read(&dir, "ticket/t1.toml"),
        THEIRS,
        "the hook said so, with a put"
    );
    assert!(!i.is_dirty().unwrap());
}

#[test]
fn a_hook_that_returns_nothing_keeps_the_file_here_and_names_the_conflict() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    a.apply(
        &Edit::new().put("ticket/t1.toml", THEIRS),
        "theirs",
        &Reapply,
    )
    .unwrap();
    let nothing = |_: &dyn Reader, _: &Edit| -> Result<Edit, MergeRefusal> { Ok(Edit::new()) };
    let err = i.apply("mine", &nothing).unwrap_err();
    assert!(err.to_string().contains("ticket/t1.toml"), "{err}");
    assert_eq!(
        read(&dir, "ticket/t1.toml"),
        MINE,
        "the file here is not erased by a hook that said nothing of it"
    );
}

#[test]
fn a_file_the_hook_leaves_out_of_its_edit_is_kept_and_goes_with_the_next_apply() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
    let i = Instance::new(&a, &dir).unwrap();
    i.materialize().unwrap();
    write(&dir, "ticket/t1.toml", MINE);
    write(&dir, "ticket/t2.toml", "id = \"t2\"\nmine = 2\n");
    // an unrelated file lands first, which is all it takes to send the edit through the hook
    a.apply(&Edit::new().put("ticket/t9.toml", "t9\n"), "t9", &Reapply)
        .unwrap();
    let only_t1 = |_: &dyn Reader, ours: &Edit| -> Result<Edit, MergeRefusal> {
        let mut next = Edit::new();
        for (path, bytes) in ours.entries() {
            if path == "ticket/t1.toml" {
                next = next.put(path, bytes.unwrap().to_vec());
            }
        }
        Ok(next)
    };
    let applied = i.apply("t1 only", &only_t1).unwrap();
    assert!(applied.changed);
    assert_eq!(remote_t1(&w), MINE);
    assert_eq!(
        read(&dir, "ticket/t2.toml"),
        "id = \"t2\"\nmine = 2\n",
        "the file the hook left out is still here"
    );
    assert_eq!(
        w.file(MOCK_REF, "ticket/t2.toml").unwrap(),
        T2.trim_end(),
        "and was not written"
    );
    assert!(i.is_dirty().unwrap(), "it is still to be sent");
    i.apply("t2 now", &Reapply).unwrap();
    assert!(
        w.file(MOCK_REF, "ticket/t2.toml")
            .unwrap()
            .contains("mine = 2")
    );
}

#[test]
fn a_file_removed_here_and_on_the_tip_is_no_conflict() {
    let w = World::new();
    let a = seeded(&w);
    let dir = inst_dir(&w, "a", "registry");
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
    let dir = inst_dir(&w, "b", "registry");
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
    assert_eq!(i.base().unwrap().map(|b| b.to_string()), Some(base.clone()));
    let was = b.reader_of(&base).unwrap();
    i.apply("merge", &merging(&was)).unwrap();
    assert!(read(&dir, "ticket/t1.toml").contains("theirs = 1"));
    assert_eq!(
        read(&dir, "ticket/t9.toml"),
        "t9\n",
        "and the tip's other file arrived"
    );
}
