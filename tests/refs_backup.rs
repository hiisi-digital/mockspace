//! The copy of an instance's edits in `refs/mock-backup/<namespace>`: an edit git wrote over is
//! said by `status` and `commit` and brought back by `restore`, two worktrees copying at once both
//! keep theirs, and a copy that cannot be made never fails a command that only reads.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{Clone, World, fixture};
use fixture::{NOTES, dirty_instance_and_an_old_branch};
const IN_REF: &str = "20261001_alpha/NOTES.md";

/// Every blob the latest copy holds at a path ending in `rel`, by content.
fn copies_of(c: &Clone, rel: &str) -> Vec<String> {
    let tree = c.git_out(&["ls-tree", "-r", "--name-only", "refs/mock-backup/research"]);
    if !tree.ok {
        return Vec::new();
    }
    tree.stdout
        .lines()
        .filter(|p| p.ends_with(rel))
        .map(|p| c.git(&["show", &format!("refs/mock-backup/research:{p}")]))
        .collect()
}

// Mutation: copy the edit and say nothing when the file no longer holds it; the next `commit` then
// pushes the old branch's bytes over the ref and the edit is in a local ref nobody reads.
#[test]
fn an_edit_a_merge_wrote_over_is_said_refused_and_restored() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    assert_ne!(a.exists(NOTES).then(|| a.read(NOTES)), Some(b"DIRTY instance edit\n".to_vec()), "the control: git wrote over the edit");
    let restore = format!("data-refs restore research {IN_REF}");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains("written over") && s.all().contains(&restore), "status says nothing of it: {}", s.all());
    // said again on the next look, not only the first
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&restore), "the second status forgot it: {}", s.all());
    let before = w.remote_tip("refs/heads/mock/research");
    let c = a.fails(&["data-refs", "commit", "research"]);
    assert!(c.all().contains("written over") && c.all().contains("--accept-overwritten"), "{}", c.all());
    assert_eq!(w.remote_tip("refs/heads/mock/research"), before, "the refusal pushed");
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n", "restore did not bring the edit back");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(!s.all().contains("written over"), "still said after the restore: {}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    let on_remote = w.remote_git(&["show", &format!("refs/heads/mock/research:{IN_REF}")]);
    assert_eq!(on_remote, "DIRTY instance edit\n");
}

// Mutation: let the flag be ignored, or let it push without clearing what was said; the person who
// means the bytes on disk has no way to commit them.
#[test]
fn the_flag_commits_the_bytes_on_disk_and_ends_the_warning() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    a.write(NOTES, b"what the person means now\n");
    a.ok(&["data-refs", "commit", "research", "--accept-overwritten"]);
    assert_eq!(w.remote_git(&["show", &format!("refs/heads/mock/research:{IN_REF}")]), "what the person means now\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(!s.all().contains("written over"), "{}", s.all());
}

// Mutation: build each copy from the edits of one worktree alone, or give up on a lost
// compare-and-swap; the copy of the other worktree's edit is then gone from the ref's tip, or a
// command fails because another worktree copied at the same moment.
#[test]
fn two_worktrees_copying_at_once_both_keep_their_edits() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    let wt = a.at(a.worktree_add("wt", "other"));
    wt.ok(&["data-refs", "instantiate"]);
    a.write(NOTES, b"the edit of the first worktree\n");
    wt.write(NOTES, b"the edit of the second worktree\n");
    for round in 0 .. 6 {
        let extra = format!("mock/research/round{round}.md");
        a.write(&extra, b"a\n");
        wt.write(&extra, b"wt\n");
        let spawn = |c: &Clone| {
            let mut cmd = c.w.command(refs_e2e::world::BIN, &c.root);
            cmd.arg("--dir").arg(c.mock()).args(["data-refs", "snapshot", "research"]);
            cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn().unwrap()
        };
        let (p, q) = (spawn(&a), spawn(&wt));
        for (who, out) in [("first", p.wait_with_output().unwrap()), ("second", q.wait_with_output().unwrap())] {
            assert!(out.status.success(), "round {round}, {who}: {}", String::from_utf8_lossy(&out.stderr));
            assert!(out.stderr.is_empty(), "round {round}, {who} said: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
    let held = copies_of(&a, IN_REF);
    assert!(held.contains(&"the edit of the first worktree\n".to_string()), "{held:?}");
    assert!(held.contains(&"the edit of the second worktree\n".to_string()), "{held:?}");
}

// Mutation: let a copy that cannot be written fail the command; a status, a verify or a gated run
// then stops for a reason that is not the instance's.
#[test]
fn a_copy_that_cannot_be_made_fails_no_command_that_only_reads() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.write(NOTES, b"an edit\n");
    let refs = a.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let lock = std::path::Path::new(refs.trim()).join("refs/mock-backup/research.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, b"").unwrap();
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
    assert!(s.stderr.contains("copy"), "the failed copy is not said: {}", s.all());
    let v = a.fails(&["data-refs", "verify", "research"]);
    assert!(v.all().contains("`commit`"), "verify fails for the edit, its own reason: {}", v.all());
}

// Mutation: copy only the edited files before git writes; a merge that writes an old branch's bytes
// over a file nobody edited then reads as an edit, and the next commit pushes the old bytes over the
// ref without a word.
#[test]
fn a_clean_file_a_merge_wrote_over_is_said_and_not_pushed() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.git(&["checkout", "-q", "-b", "feat"]);
    a.write(NOTES, b"edited on the old branch\n");
    a.commit_all("docs: edit the notes on the branch");
    a.git(&["checkout", "-q", "dev"]);
    fixture::migrate_all(&a);
    a.push();
    let instance = a.read(NOTES);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    assert_ne!(a.exists(NOTES).then(|| a.read(NOTES)), Some(instance.clone()), "the control: git wrote over the file");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&format!("data-refs restore research {IN_REF}")), "{}", s.all());
    let before = w.remote_tip("refs/heads/mock/research");
    a.fails(&["data-refs", "commit", "research"]);
    assert_eq!(w.remote_tip("refs/heads/mock/research"), before, "the old bytes were pushed");
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), instance);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.stdout.trim_end().ends_with("clean") && !s.all().contains("written over"), "{}", s.all());
}

// Mutation: take every file that differs from the copy made before a git command for one git wrote
// over; an edit made after a merge then reads as written over, and `commit` refuses the person's own
// work.
#[test]
fn an_edit_made_after_a_merge_is_not_said_to_be_written_over() {
    let w = fixture::migrated_remote();
    let a = fixture::seat(&w, "a");
    a.git(&["checkout", "-q", "-b", "side"]);
    a.write("README.md", b"# fixture, changed on the side\n");
    a.commit_all("docs: change the readme on the side");
    a.git(&["checkout", "-q", "dev"]);
    a.write(NOTES, b"before the merge\n");
    a.git(&["merge", "-q", "--no-edit", "side"]);
    assert!(copies_of(&a, IN_REF).contains(&"before the merge\n".to_string()), "the control: the hook kept the file");
    a.write(NOTES, b"after the merge\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(!s.all().contains("written over"), "an edit of the person's read as git's: {}", s.all());
    assert!(s.stdout.trim_end().ends_with("changed"), "{}", s.all());
    a.ok(&["data-refs", "commit", "research"]);
    assert_eq!(w.remote_git(&["show", &format!("refs/heads/mock/research:{IN_REF}")]), "after the merge\n");
}

// Mutation: keep no copy at the reset that left the edit in place; the reset back to the migration,
// which tracks none of the instance's files, removes every one of them, and the edit is in no commit
// and no copy.
#[test]
fn an_edit_a_hard_reset_and_back_removed_is_said_and_restored() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let dev = a.git(&["rev-parse", "HEAD"]);
    let o = a.git_out(&["reset", "-q", "--hard", "feat"]);
    assert!(o.ok, "{}", o.all());
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n", "the sparse checkout left the edit in place");
    a.git(&["reset", "-q", "--hard", dev.trim()]);
    assert!(!a.exists(NOTES), "the control: leaving the commit that tracks the file removed it");
    let s = a.fails(&["data-refs", "status", "research"]);
    assert!(s.all().contains("`restore` it") && s.all().contains(IN_REF), "the edit is not named beside the removal: {}", s.all());
    a.ok(&["data-refs", "restore", "research"]);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&format!("data-refs restore research {IN_REF}")), "{}", s.all());
    a.fails(&["data-refs", "commit", "research"]);
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n");
}

// Mutation: none; this pins the window that remains. A reset writes the work tree before it moves
// `ORIG_HEAD`, so an edit made while the branch sits at a commit from before the migration, which
// tracks the file, is copied by no hook before the reset back removes it, as git's reset drops an
// edit of any tracked file. The day a hook runs first, this test says so.
#[test]
fn an_edit_made_at_a_commit_from_before_the_migration_is_the_window_a_reset_leaves() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let dev = a.git(&["rev-parse", "HEAD"]);
    a.git(&["reset", "-q", "--hard", "feat"]);
    a.write(NOTES, b"edited at the old commit\n");
    a.git(&["reset", "-q", "--hard", dev.trim()]);
    assert!(!a.exists(NOTES), "the control: leaving the commit that tracks the file removed it");
    a.ok(&["data-refs", "restore", "research"]);
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(
        a.read(NOTES),
        b"DIRTY instance edit\n",
        "the edit made at the old commit is kept now; close the window in the design text"
    );
    assert!(!copies_of(&a, IN_REF).contains(&"edited at the old commit\n".to_string()));
}

/// Holds the lock of the backup ref, as another writer, or the transaction a hook runs inside of
/// under the reftable backend, does. Taken away by removing the file it returns.
fn lock_backup_ref(c: &Clone) -> std::path::PathBuf {
    let common = c.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let lock = std::path::Path::new(common.trim()).join("refs/mock-backup/research.lock");
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, b"").unwrap();
    lock
}

/// Whether any copy in the history of the backup ref held `text`.
fn history_holds(c: &Clone, text: &str) -> bool {
    let commits = c.git(&["rev-list", "refs/mock-backup/research"]);
    commits.lines().any(|commit| {
        let tree = c.git(&["ls-tree", "-r", "--name-only", commit]);
        tree.lines().filter(|p| p.ends_with(IN_REF)).any(|p| c.git(&["show", &format!("{commit}:{p}")]) == text)
    })
}

// Mutation: write the bytes from before where the copy of what the file holds now could not be
// made; what the person wrote since is then in no copy and no file.
#[test]
fn restore_refuses_where_the_bytes_on_disk_cannot_be_copied_first() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains("written over"), "the control: the merge wrote over the edit: {}", s.all());
    a.write(NOTES, b"written since, in no copy yet\n");
    let lock = lock_backup_ref(&a);
    let r = a.fails(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"written since, in no copy yet\n", "restore wrote over bytes no copy holds: {}", r.all());
    std::fs::remove_file(&lock).unwrap();
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n");
    assert!(history_holds(&a, "written since, in no copy yet\n"), "the bytes written over by the restore are in no copy");
}

// Mutation: take a guarded edit that holds new bytes for one the person made, whatever git did; an
// edit git wrote over and the person edited again before any engine command is then named nowhere,
// and the next commit pushes the second edit as though the first never was.
#[test]
fn an_edit_git_wrote_over_and_edited_again_before_any_engine_command_is_said() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    assert_eq!(a.read(NOTES), b"edited on the old branch\n", "the control: git wrote the branch's bytes over the edit");
    a.write(NOTES, b"edited again while the merge stands\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&format!("data-refs restore research {IN_REF}")), "{}", s.all());
    let before = w.remote_tip("refs/heads/mock/research");
    a.fails(&["data-refs", "commit", "research"]);
    assert_eq!(w.remote_tip("refs/heads/mock/research"), before, "the second edit was pushed over the first");
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n");
    assert!(history_holds(&a, "edited again while the merge stands\n"));
}

// Mutation: tell a file git wrote by its bytes on disk alone; git writes through the filters the
// attributes name, so a file it wrote with `eol=crlf` holds bytes no commit has and reads as an edit.
#[test]
fn a_file_git_wrote_through_a_filter_is_said() {
    let w = World::new();
    let a = w.first_clone("a");
    fixture::build_tracked(&a);
    a.write(".gitattributes", b"*.md text eol=crlf\n");
    a.commit_all("chore: write markdown with crlf");
    a.git(&["checkout", "-q", "-b", "feat"]);
    a.write(NOTES, b"edited on the old branch\n");
    a.commit_all("docs: edit the notes on the branch");
    a.git(&["checkout", "-q", "dev"]);
    fixture::migrate_all(&a);
    a.push();
    let instance = a.read(NOTES);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    assert_eq!(a.read(NOTES), b"edited on the old branch\r\n", "the control: git wrote the file through the filter");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&format!("data-refs restore research {IN_REF}")), "{}", s.all());
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), instance);
}

// Mutation: give the copy up where the backup ref cannot be moved from the hook; under the reftable
// backend the transaction the hook runs inside of holds that lock every time, and an edit no engine
// command saw is then in no copy when the merge writes over it.
#[test]
fn a_copy_before_git_writes_that_the_ref_cannot_take_is_kept_and_taken_by_the_next_look() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let lock = lock_backup_ref(&a);
    let m = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    std::fs::remove_file(&lock).unwrap();
    let held = a.git_out(&["rev-parse", "--verify", "-q", "refs/mock-backup/research"]);
    assert!(!held.ok, "the control: the ref took a copy while it was locked: {}", m.all());
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&format!("data-refs restore research {IN_REF}")), "{}", s.all());
    assert!(history_holds(&a, "DIRTY instance edit\n"), "the copy is not in the ref after the look");
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n");
}

// Mutation: the same as the one before, under the reftable backend itself. A git that cannot move a
// repository to it (`git refs migrate`, 2.46 and later) skips this, and says so.
#[test]
fn under_the_reftable_backend_the_copy_before_git_writes_is_kept() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let moved = a.git_out(&["refs", "migrate", "--ref-format=reftable"]);
    if !moved.ok {
        eprintln!("skipped: this git cannot move a repository to the reftable backend: {}", moved.all());
        return;
    }
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    let _ = a.git_out(&["merge", "--abort"]);
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(s.all().contains(&format!("data-refs restore research {IN_REF}")), "{}", s.all());
    a.ok(&["data-refs", "restore", "research", IN_REF]);
    assert_eq!(a.read(NOTES), b"DIRTY instance edit\n");
}

// Mutation: none; this pins the window that remains of an edit git wrote over and the person edited
// again before any engine command. Where the person took the path off the branch's index first,
// nothing git left says it wrote there, the file holds new bytes, and the edit from before is kept
// only in the history of the backup ref. The day it is named, this test says so.
#[test]
fn an_edit_written_over_then_taken_off_the_index_and_edited_again_is_the_window_left() {
    let w = World::new();
    let a = dirty_instance_and_an_old_branch(&w);
    let _ = a.git_out(&["merge", "--no-edit", "feat"]);
    assert_eq!(a.read(NOTES), b"edited on the old branch\n", "the control: git wrote the branch's bytes over the edit");
    a.git(&["rm", "-q", "--cached", "--sparse", "--", NOTES]);
    a.write(NOTES, b"edited again off the index\n");
    let s = a.ok(&["data-refs", "status", "research"]);
    assert!(!s.all().contains("written over"), "it is named now; close the window in the design text: {}", s.all());
    assert!(history_holds(&a, "DIRTY instance edit\n"), "the edit from before is in no copy");
}
