//! The copy of an instance's edits in `refs/mock-backup/<namespace>`: an edit git wrote over is
//! said by `status` and `commit` and brought back by `restore`, two worktrees copying at once both
//! keep theirs, and a copy that cannot be made never fails a command that only reads.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use refs_e2e::{Clone, World, fixture};

const NOTES: &str = "mock/research/20261001_alpha/NOTES.md";
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

/// The fixture with a branch cut before the migration that edits the notes, the migration on `dev`,
/// and the instance's notes edited and not committed.
fn dirty_instance_and_an_old_branch(w: &World) -> Clone<'_> {
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
