//! Losses a review of the data-ref branch found: git writing over an edited instance file in a
//! merge, a file a `.gitignore` matches that never reaches the ref, and a sparse block that hides
//! a tracked file.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{World, fixture};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";

/// A repository whose `feat` branch edits the notes before the migration, with the migration on
/// `dev` and the instance of `research` edited and not committed.
fn dirty_instance_and_an_old_branch(w: &World) -> refs_e2e::Clone<'_> {
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.git(&["checkout", "-q", "-b", "feat"]);
    a.write(NOTES, b"edited on the old branch\n");
    a.commit_all("docs: edit the notes on the branch");
    a.git(&["checkout", "-q", "dev"]);
    fixture::migrate_all(&a);
    a.push();
    a.write(NOTES, b"DIRTY instance edit\n");
    a
}

// Mutation: a pattern with a `*` writes only the file line of the sparse block, so a directory
// below it that the branch tracks is skip-worktree.
#[test]
fn a_star_pattern_does_not_hide_a_branch_file_in_a_subdirectory() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.write("mock/research/notes/a.md", b"held\n");
    a.write("mock/research/notes/sub/keep.md", b"branch file\n");
    a.commit_all("docs: notes");
    fixture::migrate_one(&a, "research", "research", &["notes/*"]);
    a.push();
    a.git(&["checkout", "-q", "-b", "x"]);
    a.git(&["checkout", "-q", "dev"]);
    let v = a.git(&["ls-files", "-v", "--", "mock/research/notes/sub/keep.md"]);
    a.write("mock/research/notes/sub/keep.md", b"an edit to a branch file\n");
    let st = a.git(&["status", "--porcelain", "--", "mock/research/notes/sub/keep.md"]);
    assert!(v.starts_with("H "), "the branch file is skip-worktree: {v:?}");
    assert!(st.contains("keep.md"), "git does not see the edit to a branch file: {st:?}");
}

// ---- what the engine keeps of an edit, so git writing over it loses nothing ----

const BACKUP_NOTES: &str = "refs/mock-backup/research:20261001_alpha/NOTES.md";

/// What the local backup ref holds of the notes, where it holds any.
fn backup_of(c: &refs_e2e::Clone) -> Option<String> {
    let o = c.git_out(&["show", BACKUP_NOTES]);
    o.ok.then_some(o.stdout)
}

// Mutation: leave the `reference-transaction` hook out, so nothing copies the edit before the merge
// writes the branch's bytes over it.
#[test]
fn a_merge_overwrites_the_file_and_the_edit_is_in_the_backup_ref() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    assert_eq!(backup_of(&a).as_deref(), Some("DIRTY instance edit\n"), "the backup ref holds the edit");
}

// Mutation: as above, for the rebase that replays the edit of the old branch.
#[test]
fn a_rebase_of_an_old_branch_over_a_dirty_instance_leaves_the_edit_in_the_backup_ref() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    a.git(&["checkout", "-q", "feat"]);
    let _ = a.git_out(&["rebase", "dev"]);
    let _ = a.git_out(&["rebase", "--abort"]);
    assert_eq!(backup_of(&a).as_deref(), Some("DIRTY instance edit\n"), "the backup ref holds the edit");
}

// Mutation: none; this pins the window that remains. `git cherry-pick` writes the file before it
// updates any reference, so no hook runs ahead of the write, and an edit no engine command saw
// since it was made has no copy. When git changes that, this test fails and says so.
#[test]
fn a_cherry_pick_straight_after_an_edit_is_the_window_no_hook_covers() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let o = a.git_out(&["cherry-pick", "feat"]);
    assert_ne!(a.read(NOTES), b"DIRTY instance edit\n", "the cherry-pick no longer overwrites ({})", o.all());
    assert_eq!(backup_of(&a), None, "a copy exists now: the window is closed, so remove this test");
}

// Mutation: snapshot only from the hook; an engine run between the edit and the cherry-pick would
// then not have kept the edit.
#[test]
fn an_engine_run_between_the_edit_and_the_cherry_pick_closes_that_window() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.ends_with("changed\n"), "{}", s.all());
    let _ = a.git_out(&["cherry-pick", "feat"]);
    assert_eq!(backup_of(&a).as_deref(), Some("DIRTY instance edit\n"));
}

// Mutation: write a new backup commit on every run; a gated run on every command would then fill
// the ref with copies of one state.
#[test]
fn an_unchanged_edit_is_backed_up_once() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    for _ in 0..3 {
        a.ok(&["data-refs", "status", "research"]);
    }
    assert_eq!(a.git(&["rev-list", "--count", "refs/mock-backup/research"]).trim(), "1");
    a.write(NOTES, b"a second edit\n");
    a.ok(&["data-refs", "status", "research"]);
    assert_eq!(a.git(&["rev-list", "--count", "refs/mock-backup/research"]).trim(), "2", "a new state is a new copy");
}

// Mutation: make the backup ref even where nothing is edited.
#[test]
fn a_clean_instance_makes_no_backup() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.ok(&["data-refs", "status"]);
    assert!(!a.git_out(&["rev-parse", "--verify", "-q", "refs/mock-backup/research"]).ok);
}

// Mutation: let `verify` pass where the hook that copies an edit before git writes over it is gone.
#[test]
fn verify_names_a_worktree_with_no_backup_hook_and_instantiate_repairs_it() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.ok(&["data-refs", "verify"]);
    let hook = a.git(&["rev-parse", "--path-format=absolute", "--git-path", "hooks/reference-transaction"]);
    std::fs::remove_file(hook.trim()).unwrap();
    let o = a.fails(&["data-refs", "verify"]);
    assert!(o.all().contains("reference-transaction") && o.all().contains("instantiate"), "{}", o.all());
    let _ = a.run(&["data-refs", "instantiate"]);
    a.ok(&["data-refs", "verify"]);
}

// Mutation: overwrite a hook of the person's own to install ours.
#[test]
fn a_reference_transaction_hook_of_the_persons_own_is_refused_and_kept() {
    let w = fixture::migrated_remote();
    let a = w.clone_of("a");
    let hook = a.git(&["rev-parse", "--path-format=absolute", "--git-path", "hooks/reference-transaction"]);
    std::fs::create_dir_all(std::path::Path::new(hook.trim()).parent().unwrap()).unwrap();
    std::fs::write(hook.trim(), "#!/bin/sh\n# my own\nexit 0\n").unwrap();
    let o = a.run(&["data-refs", "instantiate"]);
    assert!(!o.ok && o.all().contains("reference-transaction"), "{}", o.all());
    assert_eq!(std::fs::read_to_string(hook.trim()).unwrap(), "#!/bin/sh\n# my own\nexit 0\n");
}

// ---- the sparse block follows the declarations ----

// Mutation: leave the block where the last `[[data_ref]]` was removed; the paths of a ref that is
// gone stay hidden from git.
#[test]
fn a_removed_declaration_leaves_no_block_behind() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write("mock/mockspace.toml", b"project_name = \"fixture\"\ncrate_prefix = \"fixture\"\n");
    let v = a.fails(&["data-refs", "verify"]);
    assert!(v.all().contains("instantiate"), "verify names the stale block: {}", v.all());
    a.ok(&["data-refs", "instantiate"]);
    let file = a.git(&["rev-parse", "--path-format=absolute", "--git-path", "info/sparse-checkout"]);
    let text = std::fs::read_to_string(file.trim()).unwrap_or_default();
    assert!(!text.contains("mockspace data refs"), "the block is still there: {text}");
    a.ok(&["data-refs", "verify"]);
}

// Mutation: write the unescaped directory into the block, so `[1]` is a class and matches `1`.
#[test]
fn a_directory_with_glob_characters_is_protected_as_itself() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.write("mock/odd[1]/n.md", b"held\n");
    a.commit_all("docs: odd directory");
    a.git(&["branch", "old"]);
    fixture::migrate_one(&a, "odd", "odd[1]", &[]);
    a.push();
    a.write("mock/odd[1]/n.md", b"edited\n");
    let o = a.git_out(&["checkout", "-q", "old"]);
    assert!(o.ok, "{}", o.all());
    assert_eq!(a.read("mock/odd[1]/n.md"), b"edited\n");
}

// ---- what a ref holds is what its patterns match ----

// Mutation: read a `.gitignore` of the branch, as the first version did.
#[test]
fn a_new_file_a_gitignore_matches_is_data_and_reaches_the_ref() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write(".gitignore", b"*.pdf\n");
    a.commit_all("chore: ignore pdfs");
    a.write("mock/research/paper.pdf", b"%PDF a fetched paper\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    let listing = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/research"]);
    assert!(listing.contains("paper.pdf"), "the ref does not hold the paper: {listing}");
}

const CACHE_TAG: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n# This file is a cache directory tag created by cargo.\n";

// Mutation: read every file of a cargo build directory as data, so `target/` of a sketch fills the ref.
#[test]
fn a_cache_directory_is_not_data_though_no_gitignore_names_it() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write(".gitignore", b"");
    a.commit_all("chore: no ignores");
    a.write("mock/research/sketches/probe/build/CACHEDIR.TAG", CACHE_TAG);
    a.write("mock/research/sketches/probe/build/debug/x", &fixture::bytes(6, 512));
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("clean"), "{}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    let listing = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/research"]);
    assert!(!listing.contains("probe/build"), "build output reached the ref: {listing}");
}

// Mutation: skip every directory named `target`, as the ignore rule did; a directory of data that
// happens to be so named was dropped without a word.
#[test]
fn a_directory_named_target_with_no_cache_tag_is_data() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write("mock/research/target/keep.md", b"findings about a target\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    let listing = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/research"]);
    assert!(listing.contains("target/keep.md"), "{listing}");
}

// ---- modes ----

fn chmod_all(dir: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            chmod_all(&p, mode);
        } else {
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        }
    }
}

// Mutation: read the executable bit with `core.fileMode` off; a filesystem that reports every file
// executable then reads every file as changed and a commit sets 100755 on all of them.
#[test]
fn core_filemode_false_reads_no_file_as_changed_for_its_mode() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.git(&["config", "core.fileMode", "false"]);
    chmod_all(&a.path("mock/research"), 0o755);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("clean"), "a mode alone is a change: {}", s.all());
    a.write(NOTES, b"one real edit\n");
    a.ok(&["data-refs", "commit", "research"]);
    let tree = w.remote_git(&["ls-tree", "-r", "refs/heads/mock/research"]);
    assert!(!tree.contains("100755"), "a commit set the executable bit: {tree}");
    assert!(tree.contains("100644"), "{tree}");
}

// Mutation: with `core.fileMode` on, ignore a changed bit.
#[test]
fn core_filemode_true_reads_a_changed_executable_bit() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    chmod_all(&a.path("mock/research/20261001_alpha"), 0o755);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}
