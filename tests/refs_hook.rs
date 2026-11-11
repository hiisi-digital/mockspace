//! The `reference-transaction` hook that copies an edited instance file before git writes over it:
//! it never stops the person's git command, it lands where git runs it in an activated repository
//! without touching a directory other repositories share, and turning on per-worktree config for
//! the sparse checkout leaves a bare clone and its linked worktrees working.

#[path = "common/ambient.rs"]
mod ambient;
mod refs_e2e;

use std::path::{Path, PathBuf};

use refs_e2e::{Clone, Out, World, fixture};

use refs_e2e::fixture::NOTES;

/// A seat on the migrated remote whose instance holds an edit no commit has, with a branch `side`
/// that merges cleanly (it changes a file of the branch only) and a branch `behind` cut before the
/// side commit, which rebases onto it.
fn seat_with_branches(w: &World) -> Clone<'_> {
    let a = fixture::seat(w, "a");
    a.git(&["checkout", "-q", "-b", "behind"]);
    a.write("BEHIND.md", b"behind\n");
    a.commit_all("docs: a file of the branch behind");
    a.git(&["checkout", "-q", "dev"]);
    a.git(&["checkout", "-q", "-b", "side"]);
    a.write("README.md", b"# fixture, changed on the side\n");
    a.commit_all("docs: change the readme on the side");
    a.git(&["checkout", "-q", "dev"]);
    a.write(NOTES, b"an edit no commit holds\n");
    a
}

/// git with `dir` first on the `PATH`, so a `mock` placed there is the launcher the hook finds.
fn git_with(c: &Clone, first: &Path, args: &[&str]) -> Out {
    let path = format!("{}:{}:{}", first.display(), c.w.base().join("shims").display(), std::env::var("PATH").unwrap());
    let o = c.w.command("git", &c.root).env("PATH", path).args(args).output().unwrap();
    Out {
        ok:     o.status.success(),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// A directory holding a `mock` that runs `body`.
fn launcher(w: &World, name: &str, body: &str) -> PathBuf {
    let dir = w.base().join(name);
    mockspace_manifest::write_executable(&dir.join("mock"), format!("#!/bin/sh\n{body}\n")).unwrap();
    dir
}

/// The launchers the hook must survive: one that fails outright, and an engine from before the
/// `snapshot` verb, which refuses it the way such an engine does.
fn broken_launchers(w: &World) -> Vec<(&'static str, PathBuf)> {
    vec![
        ("a launcher that fails", launcher(w, "failing", "echo 'the launcher broke' >&2\nexit 3")),
        (
            "an engine with no `snapshot`",
            launcher(
                w,
                "older",
                "echo 'data-refs: `snapshot` is none of status, verify, instantiate, update, restore, commit, trailers or paired' >&2\nexit 1",
            ),
        ),
    ]
}

// Mutation: let the hook exit with the launcher's status, as the first version did; the move of
// `ORIG_HEAD` is then refused and the person's merge stops on a failure that is not theirs.
#[test]
fn a_merge_goes_through_whatever_the_launcher_does() {
    let w = fixture::migrated_remote();
    let a = seat_with_branches(&w);
    for (what, dir) in broken_launchers(&w) {
        let before = a.git(&["rev-parse", "HEAD"]);
        let o = git_with(&a, &dir, &["merge", "--no-edit", "side"]);
        assert!(o.ok, "{what}: the merge was stopped: {}", o.all());
        assert_eq!(a.git(&["rev-parse", "HEAD"]), a.git(&["rev-parse", "side"]), "{what}: the merge did not land");
        assert!(o.stderr.contains("mockspace"), "{what}: the failure is said on stderr: {}", o.all());
        a.git(&["reset", "-q", "--keep", before.trim()]);
        assert_eq!(a.read(NOTES), b"an edit no commit holds\n", "{what}: the edit is on disk");
    }
}

// Mutation: the same, on a rebase, which moves `ORIG_HEAD` before it writes.
#[test]
fn a_rebase_goes_through_whatever_the_launcher_does() {
    let w = fixture::migrated_remote();
    let a = seat_with_branches(&w);
    a.git(&["merge", "-q", "--no-edit", "side"]);
    for (what, dir) in broken_launchers(&w) {
        a.git(&["checkout", "-q", "-B", "work", "behind"]);
        let o = git_with(&a, &dir, &["rebase", "dev"]);
        assert!(o.ok, "{what}: the rebase was stopped: {}", o.all());
        let base = a.git(&["merge-base", "HEAD", "dev"]);
        assert_eq!(base, a.git(&["rev-parse", "dev"]), "{what}: the rebase did not land");
        assert!(o.stderr.contains("mockspace"), "{what}: the failure is said on stderr: {}", o.all());
        a.git(&["checkout", "-q", "dev"]);
    }
}

/// `core.hooksPath` of the clone, where it is set.
fn hooks_path(c: &Clone) -> Option<String> {
    let o = c.git_out(&["config", "--local", "core.hooksPath"]);
    o.ok.then(|| o.stdout.trim().to_string())
}

/// The repository's own hooks directory, which no other repository reads.
fn own_hooks(c: &Clone) -> PathBuf {
    let common = c.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    PathBuf::from(common.trim()).join("hooks")
}

fn backup_holds_the_edit(c: &Clone) -> bool {
    let tree = c.git_out(&["ls-tree", "-r", "--name-only", "refs/mock-backup/research"]);
    if !tree.ok {
        return false;
    }
    tree.stdout.lines().filter(|p| p.ends_with("20261001_alpha/NOTES.md")).any(|p| {
        c.git_out(&["show", &format!("refs/mock-backup/research:{p}")]).stdout == "an edit no commit holds\n"
    })
}

/// A merge of `feat`, a branch cut before the migration that edits the notes, which git answers by
/// writing the branch's bytes over the instance file; then the merge is abandoned.
fn merge_the_old_branch(c: &Clone) {
    let _ = c.git_out(&["merge", "--no-edit", "feat"]);
    let _ = c.git_out(&["merge", "--abort"]);
}

// Mutation: write the hook where `git rev-parse --git-path hooks/...` points, which follows
// `core.hooksPath`; in an activated repository that is the gate's durable directory, shared by
// every repository on the machine and replaced on a version bump.
#[test]
fn in_an_activated_repository_the_hook_is_the_repositorys_own_and_git_runs_it() {
    let w = World::new();
    let a = fixture::migrated_with_an_old_branch_editing_the_notes(&w);
    let key = hooks_path(&a).expect("the fixture is activated: the engine points `core.hooksPath` at the gate");
    let shared = Path::new(&key).join("reference-transaction");
    let text = std::fs::read_to_string(&shared).unwrap_or_default();
    assert!(!text.contains("data-refs snapshot"), "the shared directory carries the data-ref hook: {text}");
    assert!(!text.contains(a.root.to_str().unwrap()), "the shared directory names this repository: {text}");
    let own = std::fs::read_to_string(own_hooks(&a).join("reference-transaction")).unwrap_or_default();
    assert!(own.contains("data-refs snapshot"), "the repository's own hook is not there: {own}");
    a.write(NOTES, b"an edit no commit holds\n");
    merge_the_old_branch(&a);
    assert!(backup_holds_the_edit(&a), "git did not run the hook through the gate's directory");
}

// Mutation: keep the hook only in a directory a version bump or `cargo clean` replaces; the copy of
// an edit then stops with nothing said until `instantiate` runs again.
#[test]
fn the_hook_survives_the_gate_directory_made_again_and_a_clean_of_the_build() {
    let w = World::new();
    let a = fixture::migrated_with_an_old_branch_editing_the_notes(&w);
    let key = hooks_path(&a).expect("activated");
    // the durable directory as a version bump leaves it: a new, empty directory that the next run
    // of the engine fills, and the build directory gone
    std::fs::remove_dir_all(&key).unwrap();
    let _ = std::fs::remove_dir_all(a.path("mock/target"));
    a.ok(&["data-refs", "verify"]);
    a.write(NOTES, b"an edit no commit holds\n");
    merge_the_old_branch(&a);
    assert!(backup_holds_the_edit(&a), "the copy stopped after the gate directory was made again");
}

// Mutation: turn on `extensions.worktreeConfig` with `core.bare = true` left in the common config;
// every linked worktree of a bare clone then reads itself as bare and git refuses to work in it.
#[test]
fn a_bare_clone_and_its_linked_worktrees_keep_working() {
    let w = fixture::migrated_remote();
    let bare = w.base().join("bare.git");
    assert!(w.sh(&w.base(), "git", &["clone", "-q", "--bare", w.remote.to_str().unwrap(), bare.to_str().unwrap()]).ok);
    let (one, two) = (w.base().join("one"), w.base().join("two"));
    for (dir, branch) in [(&one, "first"), (&two, "second")] {
        let o = w.sh(&bare, "git", &["worktree", "add", "-q", "-b", branch, dir.to_str().unwrap(), "dev"]);
        assert!(o.ok, "{}", o.all());
    }
    let c = Clone { w: &w, root: one.clone() };
    for kv in [["user.name", "tester"], ["user.email", "tester@example.com"]] {
        c.git(&["config", kv[0], kv[1]]);
    }
    c.ok(&["data-refs", "instantiate"]);
    for dir in [&one, &two] {
        let o = w.sh(dir, "git", &["status", "--porcelain"]);
        assert!(o.ok, "git status in {}: {}", dir.display(), o.all());
        let o = w.sh(dir, "git", &["rev-parse", "--is-bare-repository"]);
        assert_eq!(o.stdout.trim(), "false", "{} reads itself as bare: {}", dir.display(), o.all());
    }
    let o = w.sh(&bare, "git", &["rev-parse", "--is-bare-repository"]);
    assert_eq!(o.stdout.trim(), "true", "the bare clone is no longer bare: {}", o.all());
    c.ok(&["data-refs", "verify"]);
}

// Mutation: take the repository's hooks from `GIT_DIR` alone; in a linked worktree that is the
// worktree's own directory, which holds no hooks, and the copy never runs there.
#[test]
fn in_a_linked_worktree_of_an_activated_repository_git_runs_the_hook() {
    let w = World::new();
    let a = fixture::migrated_with_an_old_branch_editing_the_notes(&w);
    assert!(hooks_path(&a).is_some(), "the fixture is activated");
    let wt = a.at(a.worktree_add("wt", "other"));
    wt.ok(&["data-refs", "instantiate"]);
    wt.write(NOTES, b"an edit no commit holds\n");
    merge_the_old_branch(&wt);
    assert!(backup_holds_the_edit(&wt), "git did not run the hook from the linked worktree");
}
