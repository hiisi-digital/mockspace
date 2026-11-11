//! The controls of the whole-tree check every scenario ends on: each makes by hand one loss a
//! command could cause outside the files of the work tree, and the check must name it. A check that
//! passes these over says nothing of the scenarios that lean on it.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::whole::{self, Allowed, Ledger};
use refs_e2e::{Clone, World};

/// A clone with one commit on `dev`, pushed.
fn committed(w: &World) -> Clone<'_> {
    let c = w.first_clone("a");
    c.write("notes/kept.md", b"in a commit\n");
    c.commit_all("feat: a note");
    c.push();
    c
}

fn seen(c: &Clone, before: &whole::Whole, what: &str) {
    match whole::kept(c, before, &whole::capture(c), &Allowed::default()) {
        Ok(()) => panic!("the check passed over it: {what}"),
        Err(e) => assert!(e.contains(what), "the check names `{what}`: {e}"),
    }
}

#[test]
fn head_moved_to_another_branch_is_seen() {
    let w = World::new();
    let c = committed(&w);
    let before = whole::capture(&c);
    c.git(&["checkout", "-q", "-b", "other"]);
    seen(&c, &before, "HEAD");
}

#[test]
fn a_staged_change_in_no_commit_taken_off_the_index_is_seen() {
    let w = World::new();
    let c = committed(&w);
    c.write("notes/staged.md", b"staged and never committed\n");
    c.git(&["add", "notes/staged.md"]);
    let before = whole::capture(&c);
    // the bytes stay on disk; what the person staged is gone
    c.git(&["rm", "-q", "--cached", "notes/staged.md"]);
    seen(&c, &before, "notes/staged.md");
}

#[test]
fn a_config_key_of_the_clone_taken_is_seen() {
    let w = World::new();
    let c = committed(&w);
    c.git(&["config", "branch.dev.description", "the person's own words"]);
    let before = whole::capture(&c);
    c.git(&["config", "--unset", "branch.dev.description"]);
    seen(&c, &before, "branch.dev.description");
}

#[test]
fn a_hook_of_the_persons_own_rewritten_is_seen() {
    let w = World::new();
    let c = committed(&w);
    let hook = c.root.join(".git/hooks/pre-push");
    mockspace_manifest::write_executable(&hook, "#!/bin/sh\n# the person's own\nexit 0\n").unwrap();
    let before = whole::capture(&c);
    std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    seen(&c, &before, "pre-push");
}

#[test]
fn a_line_of_the_persons_own_sparse_checkout_dropped_is_seen() {
    let w = World::new();
    let c = committed(&w);
    c.git(&["sparse-checkout", "set", "--no-cone", "/*", "!/vendor/private/"]);
    let before = whole::capture(&c);
    c.git(&["sparse-checkout", "set", "--no-cone", "/*"]);
    seen(&c, &before, "!/vendor/private/");
}

#[test]
#[should_panic(expected = "reading")]
fn a_directory_that_cannot_be_read_stops_the_capture() {
    let w = World::new();
    let _ = whole::disk(&w.base().join("not there"));
}

#[test]
fn a_text_only_at_another_path_than_its_own_is_lost() {
    let w = World::new();
    let c = committed(&w);
    let mut l = Ledger::default();
    l.write(&c, "notes/draft.md", b"written and never committed\n");
    std::fs::rename(c.path("notes/draft.md"), c.path("notes/elsewhere.md")).unwrap();
    let e = l.kept(&w, &[&c]).expect_err("a text at a path nothing named is not where the person finds it");
    assert!(e.contains("notes/draft.md"), "{e}");
}

#[test]
fn a_text_at_a_path_a_stated_move_took_it_to_is_kept() {
    let w = World::new();
    let c = committed(&w);
    let mut l = Ledger::default();
    l.write(&c, "notes/draft.md", b"written and never committed\n");
    std::fs::rename(c.path("notes/draft.md"), c.path("notes/elsewhere.md")).unwrap();
    l.moved("notes/draft.md", "notes/elsewhere.md");
    l.kept(&w, &[&c]).unwrap();
}

// the control of every test above: a tree with all of it, and nothing done to it, passes
#[test]
fn nothing_done_is_nothing_seen() {
    let w = World::new();
    let c = committed(&w);
    c.git(&["config", "branch.dev.description", "the person's own words"]);
    mockspace_manifest::write_executable(&c.root.join(".git/hooks/pre-push"), "#!/bin/sh\nexit 0\n").unwrap();
    c.git(&["sparse-checkout", "set", "--no-cone", "/*", "!/vendor/private/"]);
    c.write("notes/staged.md", b"staged and never committed\n");
    c.git(&["add", "notes/staged.md"]);
    let before = whole::capture(&c);
    assert!(before.git.sparse.as_ref().is_some_and(|l| l.iter().any(|l| l == "!/vendor/private/")), "{:?}", before.git);
    assert!(before.git.hooks.keys().any(|k| k == b"pre-push"), "{:?}", before.git.hooks.keys());
    whole::kept(&c, &before, &whole::capture(&c), &Allowed::default()).unwrap();
}
