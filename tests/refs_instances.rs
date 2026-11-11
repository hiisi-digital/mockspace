//! The instance in the working tree: worktrees, vanished files, a clone with no instance, branch
//! switches and rebases, a clean that wipes it.

mod refs_e2e;

use refs_e2e::{Held, check, fixture, held_on_remote, instance_matches, status_of};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
const ALL: [Held; 4] = [
    Held { ns: "research", path: "research" },
    Held { ns: "benches", path: "benches" },
    Held { ns: "rounds", path: "design_rounds" },
    Held { ns: "templates", path: "crates" },
];

// Mutation: make `instantiate` write into the clone's own directory instead of the worktree it is
// run from; the worktree would then stay without files.
#[test]
fn a_worktree_gets_its_own_instance_and_a_removed_one_made_again_is_restored() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let path = a.worktree_add("wt", "work/wt");
    let wt = a.at(path.clone());
    // a worktree of the branch has the config and none of the files
    assert!(!wt.exists(NOTES));
    let blocked = wt.fails(&["--generate-only"]);
    assert!(blocked.all().contains("BLOCKED"), "{}", blocked.all());
    wt.ok(&["data-refs", "instantiate"]);
    let held = held_on_remote(&w, &ALL);
    instance_matches(&wt, &held, false).unwrap();
    // an edit committed from the worktree
    wt.write(NOTES, b"edited in the worktree\n");
    wt.ok(&["data-refs", "commit", "research"]);
    // remove the worktree and make it again at the same path
    a.git(&["worktree", "remove", "--force", path.to_str().unwrap()]);
    let again = a.worktree_add("wt", "work/wt2");
    let wt = a.at(again);
    assert!(!wt.exists(NOTES));
    let s = wt.run(&["data-refs", "status", "research"]);
    assert!(s.all().contains("none of its") && s.all().contains("restore"), "{}", s.all());
    let blocked = wt.fails(&["--generate-only"]);
    assert!(blocked.all().contains("restore"), "the refusal names the way out: {}", blocked.all());
    wt.ok(&["data-refs", "restore"]);
    assert_eq!(wt.read(NOTES), b"edited in the worktree\n", "restored from the commit the instance was at");
    let held = held_on_remote(&w, &ALL);
    instance_matches(&wt, &held, false).unwrap();
    // the original clone is unaffected throughout
    assert_eq!(a.read(NOTES), b"findings on alpha, revised\n");
}

// Mutation: let `restore` write every file of the commit over the disk; the modified file below
// would be reset to the ref's bytes.
#[test]
fn restore_writes_back_what_vanished_and_leaves_what_is_there_alone() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let gone = ["mock/research/empty.txt", "mock/research/notes with spaces.md", "mock/research/large.bin"];
    for g in gone {
        a.remove(g);
    }
    a.write(NOTES, b"modified, not committed\n");
    a.ok(&["data-refs", "restore", "research"]);
    for g in gone {
        assert!(a.exists(g), "{g} is back");
    }
    assert_eq!(a.read(NOTES), b"modified, not committed\n");
    let held = held_on_remote(&w, &ALL);
    for g in gone {
        assert_eq!(refs_e2e::check::blob_id(&a, g), held[g].1, "{g}");
    }
}

// Mutation: have the standing check ignore an instance with no files; the other commands would
// then read an empty directory as the data.
#[test]
fn the_whole_directory_gone_blocks_every_other_command_until_it_is_restored() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.remove("mock/research");
    let o = a.fails(&["--generate-only"]);
    assert!(o.all().contains("restore"), "{}", o.all());
    assert!(!a.exists("mock/research"), "a refused command does not make the directory");
    a.ok(&["data-refs", "restore", "research"]);
    a.ok(&["--generate-only"]);
    instance_matches(&a, &held_on_remote(&w, &ALL), false).unwrap();
}

// Mutation: make a command with no instance read the mock directory anyway.
#[test]
fn a_clone_with_no_instance_runs_the_recovery_verbs_and_is_stopped_for_the_rest() {
    let w = fixture::migrated_remote();
    let b = w.clone_of("b");
    for args in [&["--generate-only"][..], &["--lint-only", "--scope", "infra"], &["data-refs", "commit"], &["data-refs", "trailers"]] {
        let o = b.fails(args);
        assert!(o.all().contains("instantiate") || o.all().contains("instance"), "{args:?}: {}", o.all());
    }
    for args in [&["status"][..], &["data-refs", "status"]] {
        let o = b.run(args);
        assert!(o.ok || o.all().contains("research"), "{args:?} is a recovery verb: {}", o.all());
    }
    assert!(!b.exists("mock/research") && !b.exists("mock/benches"), "nothing was made while stopped");
    b.ok(&["data-refs", "instantiate"]);
    b.ok(&["--generate-only"]);
}

// Mutation: let the instance's files be tracked by the branch's index after a switch; the
// untracked check would then list them.
#[test]
fn a_branch_switch_and_back_leaves_the_instance_and_its_edits_alone() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write(NOTES, b"edited, not committed\n");
    a.git(&["checkout", "-q", "-b", "other"]);
    a.write("mock/crates/alpha/src/lib.rs", b"pub fn other() {}\n");
    a.commit_all("feat: a source change on the other branch");
    a.git(&["checkout", "-q", "dev"]);
    assert_eq!(a.read(NOTES), b"edited, not committed\n");
    assert_eq!(a.read("mock/crates/alpha/src/lib.rs"), b"pub fn alpha() {}\n");
    assert!(status_of(&a, "research").ends_with("changed"));
    assert!(!a.tracked().iter().any(|p| p.starts_with("mock/research/")));
}


// Mutation: have the migration's removal commit also remove the files from disk; a rebase over
// it would then lose the instance.
#[test]
fn a_rebase_onto_a_moved_dev_keeps_the_instance() {
    let w = fixture::migrated_remote();
    let (a, b) = (fixture::seat(&w, "a"), fixture::seat(&w, "b"));
    b.write("mock/crates/beta/src/lib.rs", b"pub fn beta2() {}\n");
    b.commit_all("feat: a change on dev");
    b.push();
    a.git(&["checkout", "-q", "-b", "feature"]);
    a.write("mock/crates/alpha/src/lib.rs", b"pub fn alpha2() {}\n");
    a.commit_all("feat: a feature");
    a.write(NOTES, b"edited, not committed\n");
    a.git(&["fetch", "-q", "origin", "dev"]);
    a.git(&["rebase", "-q", "origin/dev"]);
    assert_eq!(a.read("mock/crates/beta/src/lib.rs"), b"pub fn beta2() {}\n");
    assert_eq!(a.read(NOTES), b"edited, not committed\n");
    a.ok(&["--generate-only"]);
    let held = held_on_remote(&w, &ALL);
    for p in held.keys().filter(|p| *p != NOTES) {
        assert!(a.exists(p), "{p}");
    }
}

// Mutation: have `restore` only act on a directory with none of its files; a `git clean -fdx`
// that took all of them would still be restored, but a partial one would not.
#[test]
fn a_git_clean_that_wipes_the_instance_is_recovered_to_the_committed_state() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.git(&["clean", "-q", "-f", "-d", "-x"]);
    assert!(!a.exists(NOTES));
    let o = a.fails(&["--generate-only"]);
    assert!(o.all().contains("restore"), "{}", o.all());
    a.ok(&["data-refs", "restore"]);
    instance_matches(&a, &held_on_remote(&w, &ALL), false).unwrap();
    let _ = check::branch;
}
