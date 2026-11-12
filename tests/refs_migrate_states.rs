//! `directory-to-ref` from the states a real repository is in: stopped part way, run twice, dirty,
//! carrying untracked files, a symlink, an unreachable remote.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{Held, World, check, content_only, fixture, held_on_remote, no_loss, tip};

const CONFIG: &[&str] = &["mock/mockspace.toml"];
const RESEARCH: Held = Held { ns: "research", path: "research" };
const RES: [&str; 5] = ["directory-to-ref", "--namespace", "research", "--path", "research"];

fn research(a: &refs_e2e::Clone) -> refs_e2e::Out {
    let mut v = vec!["migrate"];
    v.extend_from_slice(&RES);
    a.run(&v)
}

fn settled(w: &World, a: &refs_e2e::Clone, before: &refs_e2e::Manifest) {
    a.commit_all("chore: settle the migration");
    let held = held_on_remote(w, &[RESEARCH]);
    no_loss(&content_only(before), &content_only(&check::branch(a)), &content_only(&held), CONFIG).unwrap();
    refs_e2e::instance_matches(a, &held, false).unwrap();
}

// Mutation: make the migration push the ref after it has appended the config and untracked the
// files, with an unreachable remote the branch is then left declaring a ref that does not exist.
#[test]
fn an_unreachable_remote_stops_the_migration_before_it_changes_anything() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    let (cfg, tracked) = (a.read("mock/mockspace.toml"), a.tracked());
    a.git(&["remote", "set-url", "origin", "/nonexistent/remote.git"]);
    let o = research(&a);
    assert!(!o.ok, "{}", o.all());
    assert_eq!(a.read("mock/mockspace.toml"), cfg, "the config is untouched");
    assert_eq!(a.tracked(), tracked, "the index is untouched");
    // and once the remote is back the same call goes through
    a.git(&["remote", "set-url", "origin", w.remote.to_str().unwrap()]);
    let before = check::at(&a, "HEAD");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    settled(&w, &a, &before);
}

// Mutation: let a second call append a second table or remake the ref from the instance.
#[test]
fn a_migration_run_twice_changes_nothing_the_second_time() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    let before = check::at(&a, "HEAD");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    let (cfg, t, index) = (a.read("mock/mockspace.toml"), tip(&w, "research"), a.tracked());
    let o = research(&a);
    assert!(!o.ok && o.all().contains("made here before"), "{}", o.all());
    assert_eq!((a.read("mock/mockspace.toml"), tip(&w, "research"), a.tracked()), (cfg.clone(), t.clone(), index));
    settled(&w, &a, &before);
    let o = research(&a);
    assert!(!o.ok, "{}", o.all());
    assert_eq!((a.read("mock/mockspace.toml"), tip(&w, "research")), (cfg, t));
}

// Mutation: let the migration read the held set off the disk instead of the index, or skip the
// dirty check; the edit would then be pushed as if it had been committed.
#[test]
fn a_changed_or_untracked_file_in_the_directory_refuses_with_nothing_written() {
    for (what, path, bytes) in [
        ("changed", "mock/research/empty.txt", &b"changed\n"[..]),
        ("untracked", "mock/research/scratch.md", &b"scratch\n"[..]),
    ] {
        let w = World::new();
        let a = w.first_clone("a");
        fixture::build_tracked(&a);
        a.write(path, bytes);
        let (cfg, tracked) = (a.read("mock/mockspace.toml"), a.tracked());
        let o = research(&a);
        assert!(!o.ok && o.all().contains(path), "{what}: {}", o.all());
        assert!(w.remote_tip("refs/heads/mock/research").is_none(), "{what}: no ref was pushed");
        assert_eq!(a.read("mock/mockspace.toml"), cfg, "{what}");
        assert_eq!(a.tracked(), tracked, "{what}");
        assert_eq!(a.read(path), bytes, "{what}: the file is as it was");
    }
}

#[test]
fn a_dirty_file_outside_the_directory_neither_blocks_nor_is_touched() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    let before = check::at(&a, "HEAD");
    a.write("mock/crates/alpha/src/lib.rs", b"edited, not committed\n");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    assert_eq!(a.read("mock/crates/alpha/src/lib.rs"), b"edited, not committed\n");
    a.git(&["checkout", "-q", "HEAD", "--", "mock/crates/alpha/src/lib.rs"]);
    settled(&w, &a, &before);
}

// Mutation: untrack the files before the ref is pushed; the stop below then leaves the files in
// neither place.
#[test]
fn a_tracked_symlink_stops_the_migration_with_everything_still_tracked_and_the_way_out_works() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    fixture::add_research_symlink(&a);
    let before = check::at(&a, "HEAD");
    let (cfg, tracked) = (a.read("mock/mockspace.toml"), a.tracked());
    let o = research(&a);
    assert!(!o.ok && o.all().contains("symlink"), "{}", o.all());
    assert_eq!((a.read("mock/mockspace.toml"), a.tracked()), (cfg, tracked), "a stop leaves the branch whole");
    assert!(a.exists("mock/research/latest"));
    // the way out the message points to: take the link away and run the same call again
    a.git(&["rm", "-q", "mock/research/latest"]);
    a.commit_all("chore: drop the link");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    let mut b = before.clone();
    b.remove("mock/research/latest");
    settled(&w, &a, &b);
}

// Mutation: build the ref from a walk of the directory; the ignored output is then pushed, and
// the call that finishes a stopped migration refuses over files the branch never tracked.
#[test]
fn a_stopped_migration_is_finished_in_a_tree_that_has_build_output() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build(&a);
    let before = check::at(&a, "HEAD");
    let cfg = a.read("mock/mockspace.toml");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    // stopped after the ref was pushed: the config is as it was and the branch tracks the files
    a.write("mock/mockspace.toml", &cfg);
    a.git(&["read-tree", "HEAD"]);
    let o = research(&a);
    assert!(o.ok, "the same call finishes it: {}", o.all());
    settled(&w, &a, &before);
}

// Mutation: let the finishing call remake the ref; the tip would then differ.
#[test]
fn a_stopped_migration_is_finished_by_the_steps_that_are_missing_and_no_other() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    let before = check::at(&a, "HEAD");
    let cfg = a.read("mock/mockspace.toml");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    let t = tip(&w, "research");
    a.write("mock/mockspace.toml", &cfg);
    a.git(&["read-tree", "HEAD"]);
    let o = research(&a);
    assert!(o.ok, "{}", o.all());
    assert_eq!(tip(&w, "research"), t, "the ref is not made again");
    let text = String::from_utf8(a.read("mock/mockspace.toml")).unwrap();
    assert_eq!(text.matches("[[data_ref]]").count(), 1, "{text}");
    settled(&w, &a, &before);
}

// Mutation: let the finishing call take an instance edited since the ref was made as the ref's.
#[test]
fn a_stopped_migration_is_not_finished_over_a_directory_changed_since() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    let cfg = a.read("mock/mockspace.toml");
    a.ok(&["migrate", RES[0], RES[1], RES[2], RES[3], RES[4]]);
    a.write("mock/mockspace.toml", &cfg);
    a.git(&["read-tree", "HEAD"]);
    a.write("mock/research/notes with spaces.md", b"changed since the ref was made\n");
    let o = research(&a);
    assert!(!o.ok, "{}", o.all());
    assert_eq!(a.read("mock/research/notes with spaces.md"), b"changed since the ref was made\n");
}

// A fresh clone of a repository the migration was made in has no instance and no sparse block
// yet: nothing was stopped here, so the run is refused as made, and `data-refs instantiate` is
// what the clone wants.
//
// Mutation: key the finishing step on the missing sparse block alone; the fresh clone's run then
// says it finishes a stopped run, and succeeds.
#[test]
fn a_fresh_clone_of_a_migrated_repository_refuses_the_migration_as_made() {
    let w = fixture::migrated_remote();
    let b = w.clone_of("b");
    let before = check::at(&b, "HEAD");
    let o = research(&b);
    assert!(!o.ok, "a migration already made is refused: {}", o.all());
    assert!(o.all().contains("made here before"), "{}", o.all());
    let changed = b.git(&["status", "--porcelain", "--untracked-files=no"]);
    assert!(changed.is_empty(), "no tracked file is changed: {changed}");
    let sparse = b.git_out(&["config", "--get", "core.sparseCheckout"]);
    assert!(!sparse.ok, "no sparse block is written: {}", sparse.all());
    assert!(!b.exists("mock/research"), "no instance is made");
    assert_eq!(check::at(&b, "HEAD"), before);
    for _ in 0..2 {
        let again = research(&b);
        assert!(!again.ok, "a second run is refused too: {}", again.all());
    }
    b.ok(&["data-refs", "instantiate"]);
    b.ok(&["data-refs", "verify"]);
}

// A fresh clone that instantiated the ref and then fell out of keeping git off the instance holds a
// record of the instance, though no run of the migration went through it. The run is refused as
// made all the same, and changes nothing: the finishing step is for a clone the seed ran in.
//
// `core.hooksPath` at a directory of the person's own is taken back by the gate on every launcher
// run, before the engine reads it, so that way reaches no failed keep-off through the launcher; it
// stays here for what the run must not do to the person's directory.
//
// Mutation: key the finishing step on the record of the instance; the runs over a second ref
// declared and over a sparse checkout turned off then say they finish a stopped run, and succeed.
#[test]
fn a_fresh_clone_that_instantiated_and_fell_out_of_keep_off_refuses_the_migration_as_made() {
    type Break = fn(&refs_e2e::Clone);
    let ways: [(&str, Break); 3] = [
        ("a second ref declared", |b| {
            let cfg = String::from_utf8(b.read("mock/mockspace.toml")).unwrap();
            b.write("mock/mockspace.toml", format!("{cfg}\n[[data_ref]]\nnamespace = \"lore\"\npath = \"lore\"\n").as_bytes());
        }),
        ("the sparse checkout turned off", |b| {
            b.git(&["sparse-checkout", "disable"]);
        }),
        ("core.hooksPath at a directory of the person's own", |b| {
            let own = b.w.base().join("own-hooks");
            std::fs::create_dir_all(&own).unwrap();
            b.git(&["config", "core.hooksPath", own.to_str().unwrap()]);
        }),
    ];
    for (way, break_it) in ways {
        let w = fixture::migrated_remote();
        let b = w.clone_of("b");
        b.ok(&["data-refs", "instantiate"]);
        assert!(b.exists("mock/research"), "{way}: the instance is made");
        break_it(&b);
        // a launcher run first, so the gate has done what it does to the config before it is read
        let kept_off = b.run(&["data-refs", "verify"]);
        let before = check::at(&b, "HEAD");
        let config = b.git(&["config", "--list", "--local"]);
        let sparse = std::fs::read(b.root.join(".git/info/sparse-checkout")).ok();
        let own = b.w.base().join("own-hooks");
        let o = research(&b);
        assert!(!o.ok, "{way}: a migration no run went through here is refused: {}\nverify said: {}", o.all(), kept_off.all());
        assert!(o.all().contains("made here before"), "{way}: {}", o.all());
        assert_eq!(check::at(&b, "HEAD"), before, "{way}");
        assert_eq!(b.git(&["config", "--list", "--local"]), config, "{way}: the clone's config is as it was");
        assert_eq!(std::fs::read(b.root.join(".git/info/sparse-checkout")).ok(), sparse, "{way}: the sparse block is as it was");
        let own_hooks: Vec<_> = std::fs::read_dir(&own).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
        assert!(own_hooks.is_empty(), "{way}: nothing is written into the person's own hooks: {own_hooks:?}");
    }
}
