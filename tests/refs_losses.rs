//! Losses a review of the data-ref branch found: git writing over an edited instance file in a
//! merge, build output that never reaches the ref, a mode a filesystem makes up, and a sparse
//! block that hides a tracked file.

#[path = "common/ambient.rs"]
mod ambient;
#[path = "common/exec.rs"]
mod exec;
mod refs_e2e;

use refs_e2e::{World, fixture};
use fixture::{NOTES, dirty_instance_and_an_old_branch};

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

/// Every copy the local backup ref holds of the notes, by content: the ref keeps the copies of each
/// worktree's instance apart, so the path of one is not known here, only that it ends in the notes.
fn backup_of(c: &refs_e2e::Clone) -> Vec<String> {
    let tree = c.git_out(&["ls-tree", "-r", "--name-only", "refs/mock-backup/research"]);
    tree.stdout
        .lines()
        .filter(|p| p.ends_with("20261001_alpha/NOTES.md"))
        .map(|p| c.git(&["show", &format!("refs/mock-backup/research:{p}")]))
        .collect()
}

fn backup_holds(c: &refs_e2e::Clone, text: &str) -> bool {
    backup_of(c).iter().any(|t| t == text)
}

// Mutation: leave the `reference-transaction` hook out, so nothing copies the edit before the merge
// writes the branch's bytes over it.
#[test]
fn a_merge_overwrites_the_file_and_the_edit_is_in_the_backup_ref() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    assert!(backup_holds(&a, "DIRTY instance edit\n"), "the backup ref holds the edit: {:?}", backup_of(&a));
}

// Mutation: none; this pins that git refuses a plain rebase over the edit, since the old branch
// tracks the file and the edit shows there as an unstaged change.
#[test]
fn a_rebase_of_an_old_branch_over_a_dirty_instance_is_refused_by_git_and_the_edit_stays() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    a.git(&["checkout", "-q", "feat"]);
    let rb = a.git_out(&["rebase", "dev"]);
    assert!(!rb.ok && rb.all().contains("unstaged changes"), "{}", rb.all());
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n");
}

// Mutation: leave the edit nowhere when an autostashed rebase writes the branch's bytes: it is in the
// stash, the backup ref or the file.
#[test]
fn an_autostashed_rebase_of_an_old_branch_leaves_the_edit_somewhere() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    a.git(&["checkout", "-q", "feat"]);
    let _ = a.git_out(&["rebase", "--autostash", "dev"]);
    let on_disk = a.exists(NOTES) && a.read(NOTES) == b"DIRTY instance edit\n";
    // a rebase that stops keeps its autostash as a commit its own state file names
    let state = a.git(&["rev-parse", "--path-format=absolute", "--git-path", "rebase-merge/autostash"]);
    let stashed = std::fs::read_to_string(state.trim())
        .map(|id| a.git_out(&["show", id.trim()]).stdout.contains("DIRTY instance edit"))
        .unwrap_or(false);
    let backed_up = backup_holds(&a, "DIRTY instance edit\n");
    assert!(on_disk || stashed || backed_up, "the edit is in the file, the stash and the backup ref: nowhere");
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
    assert_eq!(backup_of(&a), Vec::<String>::new(), "a copy exists now: the window is closed, so remove this test");
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
    assert!(backup_holds(&a, "DIRTY instance edit\n"), "{:?}", backup_of(&a));
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

/// The repository's own `reference-transaction` hook, which git runs directly or through the gate's.
fn own_hook(c: &refs_e2e::Clone) -> std::path::PathBuf {
    let common = c.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    std::path::Path::new(common.trim()).join("hooks/reference-transaction")
}

// Mutation: let `verify` pass where the hook that copies an edit before git writes over it is gone.
#[test]
fn verify_names_a_worktree_with_no_backup_hook_and_instantiate_repairs_it() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.ok(&["data-refs", "verify"]);
    let hook = own_hook(&a);
    std::fs::remove_file(&hook).unwrap();
    let o = a.fails(&["data-refs", "verify"]);
    assert!(o.all().contains("reference-transaction") && o.all().contains("instantiate"), "{}", o.all());
    let _ = a.run(&["data-refs", "instantiate"]);
    a.ok(&["data-refs", "verify"]);
}

// Mutation: overwrite a hook of the person's own to install ours.
#[test]
fn a_reference_transaction_hook_of_the_persons_own_is_refused_and_kept() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let hook = own_hook(&a);
    std::fs::write(&hook, "#!/bin/sh\n# my own\nexit 0\n").unwrap();
    let o = a.fails(&["data-refs", "instantiate"]);
    assert!(o.all().contains("reference-transaction"), "{}", o.all());
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), "#!/bin/sh\n# my own\nexit 0\n", "the hook is as it was");
    let v = a.fails(&["data-refs", "verify"]);
    assert!(v.all().contains("reference-transaction"), "{}", v.all());
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

fn sparse_file(c: &refs_e2e::Clone) -> std::path::PathBuf {
    std::path::PathBuf::from(c.git(&["rev-parse", "--path-format=absolute", "--git-path", "info/sparse-checkout"]).trim())
}

// Mutation: keep the lines a sparse checkout that is off left in its file; turning it on then reads
// them as patterns again, and the next checkout takes away every file of the branch they leave out.
#[test]
fn the_lines_of_a_sparse_checkout_that_was_off_are_not_revived() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.git(&["config", "--worktree", "core.sparseCheckout", "false"]);
    std::fs::write(sparse_file(&a), "/mock/\n").unwrap();
    a.ok(&["data-refs", "commit"]);
    let text = std::fs::read_to_string(sparse_file(&a)).unwrap();
    assert!(text.starts_with("/*\n") && !text.lines().any(|l| l == "/mock/"), "a stale line came back: {text}");
    a.git(&["sparse-checkout", "reapply"]);
    assert!(a.exists("README.md"), "the branch's own file is gone");
}

// Mutation: switch the sparse checkout out of cone mode without a word; the person's own patterns
// are then read another way, and nothing says so.
#[test]
fn switching_a_sparse_checkout_out_of_cone_mode_is_said() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.git(&["config", "--worktree", "core.sparseCheckoutCone", "true"]);
    let o = a.ok(&["data-refs", "commit"]);
    assert!(o.stderr.contains("cone mode"), "{}", o.all());
    assert_eq!(a.git(&["config", "--worktree", "--get", "core.sparseCheckoutCone"]).trim(), "false");
    let o = a.ok(&["data-refs", "commit"]);
    assert!(!o.stderr.contains("cone mode"), "said again with nothing switched: {}", o.all());
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

// Mutation: read every file of a cargo build directory as data, so `target/` of a sketch fills the ref.
#[test]
fn a_cache_directory_is_not_data_though_no_gitignore_names_it() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write(".gitignore", b"");
    a.commit_all("chore: no ignores");
    a.write("mock/research/sketches/probe/build/CACHEDIR.TAG", fixture::CACHE_TAG);
    a.write("mock/research/sketches/probe/build/debug/x", &fixture::bytes(6, 512));
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("clean"), "{}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    let listing = w.remote_git(&["ls-tree", "-r", "--name-only", "refs/heads/mock/research"]);
    assert!(!listing.contains("probe/build"), "build output reached the ref: {listing}");
}

// Mutation: skip every directory named `target` without a word; a directory of data that happens to
// be so named, which the `.gitignore` the engine keeps matches, is then gone from the ref and nobody
// is told.
#[test]
fn a_directory_named_target_with_no_cache_tag_is_named_as_ignored() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write("mock/research/target/keep.md", b"findings about a target\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.contains("research: ignored, not held: target/\n"), "{}", s.all());
    // the control: a cache directory beside it is a build's, and is not named
    a.write("mock/research/sketch/target/CACHEDIR.TAG", fixture::CACHE_TAG);
    a.write("mock/research/sketch/target/debug/x", b"x\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(!s.stdout.contains("sketch/target"), "{}", s.all());
    assert_eq!(a.read("mock/research/target/keep.md"), b"findings about a target\n");
}

// ---- modes ----

/// Writes every file under `dir` again as an executable, with the bytes it holds, through the one
/// writer of an executable file.
fn make_all_executable(dir: &std::path::Path) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            make_all_executable(&p);
        } else {
            exec::write_executable(&p, std::fs::read(&p).unwrap());
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
    make_all_executable(&a.path("mock/research"));
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
    make_all_executable(&a.path("mock/research/20261001_alpha"));
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
}
