//! Git writing over the instance: a checkout of a commit from before the migration, a worktree, an
//! explicit overwrite.
//!
//! The instance is hidden by `.git/info/exclude`, so git treats its files as expendable. What the
//! engine sets up against that is a sparse-checkout block per worktree (`data_refs/sparse.rs`).

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{Held, World, fixture, held_on_remote, instance_matches};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
const ALL: [Held; 4] = [
    Held { ns: "research", path: "research" },
    Held { ns: "benches", path: "benches" },
    Held { ns: "rounds", path: "design_rounds" },
    Held { ns: "templates", path: "crates" },
];

/// Checks out `branch` and says so when git refused or HEAD did not move there: a test of what
/// survives a checkout means nothing where the checkout did not happen.
fn switch(c: &refs_e2e::Clone, branch: &str) {
    let o = c.git_out(&["checkout", "-q", branch]);
    assert!(o.ok, "checkout of {branch} was refused: {}", o.all());
    assert_eq!(c.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(), branch, "HEAD is not on {branch}");
}

/// Leaves for `branch` where git may refuse to protect an edit: either the checkout happened and HEAD is on
/// `branch`, or it was refused and HEAD stays where it was. Gives whether it happened.
fn try_switch(c: &refs_e2e::Clone, branch: &str) -> bool {
    let at = c.git(&["rev-parse", "HEAD"]);
    let o = c.git_out(&["checkout", "-q", branch]);
    if o.ok {
        assert_eq!(c.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(), branch, "HEAD is not on {branch}");
    } else {
        assert_eq!(c.git(&["rev-parse", "HEAD"]), at, "a refused checkout moved HEAD: {}", o.all());
        assert!(o.all().contains("overwritten"), "refused for another reason: {}", o.all());
    }
    o.ok
}

/// A repository whose `old` branch was cut before the migration, with the migration on `dev`.
fn migrated_with_an_old_branch(w: &World) -> refs_e2e::Clone<'_> {
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.git(&["branch", "old"]);
    fixture::migrate_all(&a);
    a.push();
    a
}

// Mutation: leave out the sparse-checkout block `migrate` writes; the checkout then overwrites the
// edit with the old commit's bytes, as plain git does for an ignored file.
#[test]
fn a_checkout_of_a_commit_from_before_the_migration_keeps_an_uncommitted_edit() {
    let w = World::new();
    let a = migrated_with_an_old_branch(&w);
    a.write(NOTES, b"edited after the migration\n");
    switch(&a, "old");
    assert_eq!(a.read(NOTES), b"edited after the migration\n", "on the old branch");
    // leaving it again must not take the edit either, whether git refuses or lets it be
    try_switch(&a, "dev");
    assert_eq!(a.read(NOTES), b"edited after the migration\n", "back on dev");
}

// Mutation: as above, for a file the ref changed since the old commit was cut and nobody edited.
#[test]
fn a_file_newer_on_the_ref_than_the_old_commit_is_not_replaced_by_the_old_bytes() {
    let w = World::new();
    let a = migrated_with_an_old_branch(&w);
    a.write(NOTES, b"newer, committed to the ref\n");
    a.ok(&["data-refs", "commit", "research"]);
    switch(&a, "old");
    assert_eq!(a.read(NOTES), b"newer, committed to the ref\n");
    try_switch(&a, "dev");
    assert_eq!(a.read(NOTES), b"newer, committed to the ref\n");
}

// Mutation: none; this pins the one window that remains for a clean instance. A file equal to the
// old commit's is up to date there, and leaving that commit removes it. No byte is lost, since the
// old commit and the ref both hold it, and `restore` writes it again. A `post-checkout` hook could do
// that by itself, but the gate holds `core.hooksPath`, so the hook is the hook machinery's to add.
#[test]
fn leaving_an_old_commit_removes_the_clean_files_and_restore_gives_back_the_same_bytes() {
    let w = World::new();
    let a = migrated_with_an_old_branch(&w);
    let held = held_on_remote(&w, &ALL);
    a.git(&["checkout", "-q", "old"]);
    a.git(&["checkout", "-q", "dev"]);
    let blocked = a.fails(&["--generate-only"]);
    assert!(blocked.all().contains("restore"), "{}", blocked.all());
    a.ok(&["data-refs", "restore"]);
    instance_matches(&a, &held, false).unwrap();
}

// Mutation: write the block only in the clone that migrated; a worktree made later would not carry
// it and would lose its edit to the checkout.
#[test]
fn a_worktree_made_after_the_migration_keeps_its_edit_through_the_same_checkout() {
    let w = World::new();
    let a = migrated_with_an_old_branch(&w);
    let path = a.worktree_add("wt", "work/wt");
    let wt = a.at(path);
    wt.ok(&["data-refs", "instantiate"]);
    wt.write(NOTES, b"edited in the worktree\n");
    switch(&wt, "old");
    assert_eq!(wt.read(NOTES), b"edited in the worktree\n");
}

// Mutation: let `verify` pass without looking at the sparse-checkout block.
#[test]
fn verify_names_a_worktree_that_git_could_write_over_and_instantiate_repairs_it() {
    let w = World::new();
    let a = migrated_with_an_old_branch(&w);
    a.ok(&["data-refs", "verify"]);
    let file = a.git(&["rev-parse", "--path-format=absolute", "--git-path", "info/sparse-checkout"]);
    std::fs::write(file.trim(), "/*\n").unwrap();
    let o = a.fails(&["data-refs", "verify"]);
    assert!(o.all().contains("instantiate"), "{}", o.all());
    // `instantiate` on an instantiated clone has nothing to fetch and sets the protection up again
    let _ = a.run(&["data-refs", "instantiate"]);
    a.ok(&["data-refs", "verify"]);
}

// Mutation: none. `checkout -f` and `reset --hard` leave the edit alone under the sparse block, but a
// command that names the path overwrites it: git writes a path it was asked for whatever the
// patterns say, and there is no hook before that write and no configuration that refuses it for an
// ignored file. This pins that window, so the day git closes it this test fails and says so.
#[test]
fn naming_the_path_overwrites_a_dirty_instance_file_and_that_is_the_window_git_leaves_open() {
    for overwrite in [
        &["checkout", "-q", "old", "--", NOTES][..],
        &["restore", "-q", "--source=old", "--", NOTES],
    ] {
        let w = World::new();
        let a = migrated_with_an_old_branch(&w);
        a.write(NOTES, b"edited, not committed\n");
        let o = a.git_out(overwrite);
        assert!(o.ok, "{overwrite:?}: {}", o.all());
        assert_ne!(a.read(NOTES), b"edited, not committed\n", "{overwrite:?} no longer overwrites; remove this test");
    }
}

// Mutation: drop the block; `checkout -f` and `reset --hard` then overwrite the edit as well.
#[test]
fn a_forced_checkout_and_a_hard_reset_keep_the_edit_too() {
    for overwrite in [&["checkout", "-q", "-f", "old"][..], &["reset", "-q", "--hard", "old"]] {
        let w = World::new();
        let a = migrated_with_an_old_branch(&w);
        a.write(NOTES, b"edited, not committed\n");
        let o = a.git_out(overwrite);
        assert!(o.ok, "{overwrite:?} was refused: {}", o.all());
        let old = a.git(&["rev-parse", "old"]);
        assert_eq!(a.git(&["rev-parse", "HEAD"]), old, "{overwrite:?} did not move HEAD to the old commit");
        assert_eq!(a.read(NOTES), b"edited, not committed\n", "{overwrite:?}");
    }
}
