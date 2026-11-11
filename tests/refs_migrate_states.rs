//! `directory-to-ref` from the states a real repository is in: stopped part way, run twice, dirty,
//! carrying untracked files, a symlink, an unreachable remote.

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
    a.git(&["add", "-f", "mock/research"]);
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
    a.git(&["add", "-f", "mock/research"]);
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
    a.git(&["add", "-f", "mock/research"]);
    a.write("mock/research/notes with spaces.md", b"changed since the ref was made\n");
    let o = research(&a);
    assert!(!o.ok, "{}", o.all());
    assert_eq!(a.read("mock/research/notes with spaces.md"), b"changed since the ref was made\n");
}
