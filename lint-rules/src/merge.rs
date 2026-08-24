//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Which staged content a merge brought in rather than an author wrote.
//!
//! The phase gates judge every changed file as an authored change, which is
//! right for a commit somebody typed and wrong for a merge. Merging a trunk
//! into a feature branch stages whatever the two sides changed, and that
//! routinely spans doc templates and source at once. No phase permits both, so
//! before this a round could not carry a merge at all: the only ways past were
//! rebasing the whole branch or bypassing the hook.
//!
//! What separates the two is content. A file the merge resolved by taking one
//! side is byte-identical to that side, and nobody authored anything. A file
//! somebody resolved by hand matches neither parent, and that IS an authored
//! change which the gate should still hold.
//!
//! So the excuse is per file and is exactly that test. It never fires outside a
//! merge, and it never fires on a hand resolution.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// The parents of a merge in progress, or nothing when no merge is in flight.
///
/// Cheap to construct and constructed once per lint run, because every query
/// afterwards is one `git rev-parse` per file per parent.
#[derive(Debug, Default, Clone)]
pub struct Merge {
    /// `HEAD` plus every `MERGE_HEAD`. Empty when no merge is in progress,
    /// which is what makes [`Merge::inherited`] a constant `false` off the
    /// merge path.
    parents: Vec<String>,
}

impl Merge {
    /// Read the merge state of the repository containing `dir`.
    pub fn detect(dir: &Path) -> Self {
        // The file, not `rev-parse --verify MERGE_HEAD`, which returns the
        // FIRST line only. An octopus writes one sha per parent, and reading
        // just the first would silently gate every file inherited from the
        // second onward. It exists only while a merge is in flight, so its
        // absence is the whole of the off-the-merge-path test.
        let Some(path) = git(dir, &["rev-parse", "--git-path", "MERGE_HEAD"]) else {
            return Self::default();
        };
        // `--git-path` answers relative to the working directory it was asked
        // from, so from a subdirectory it prints `../.git/MERGE_HEAD`. That is
        // `dir`, which is where the query ran. Every test here detects from a
        // subdirectory for exactly this reason.
        // `join` returns an absolute argument unchanged, which is the linked
        // worktree case: there `--git-path` answers absolutely and points at
        // that worktree's own MERGE_HEAD rather than the main repository's.
        let path = dir.join(Path::new(path.trim()));
        let Ok(heads) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let mut parents: Vec<String> =
            heads.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        if parents.is_empty() {
            return Self::default();
        }
        match git(dir, &["rev-parse", "-q", "--verify", "HEAD"]) {
            Some(h) if !h.trim().is_empty() => parents.push(h.trim().to_string()),
            // An unborn HEAD cannot be merged into, so there is nothing to add.
            _ => {},
        }
        Self {
            parents,
        }
    }

    /// Whether a merge is in progress at all.
    pub fn in_progress(&self) -> bool {
        !self.parents.is_empty()
    }

    /// Whether the staged content of `file` is byte-identical to one of the
    /// merge parents, and therefore came from the merge rather than from an
    /// author.
    ///
    /// `file` is relative to `dir`, matching what `git diff --relative` prints
    /// and what the gates carry around.
    ///
    /// A path absent from the index compares equal to a parent that also lacks
    /// it, which is the deletion case: a merge that takes the other side's
    /// removal authored nothing either.
    pub fn inherited(&self, dir: &Path, file: &str) -> bool {
        if self.parents.is_empty() {
            return false;
        }
        let staged = blob(dir, ":", file);
        let at_parents: Vec<Option<String>> =
            self.parents.iter().map(|p| blob(dir, &format!("{p}:"), file)).collect();
        // A path nothing carries is not a match of two absences. It cannot
        // reach here through the index, since a walk over staged changes never
        // names one, but the answer should not depend on that.
        if staged.is_none() && at_parents.iter().all(Option::is_none) {
            return false;
        }
        if at_parents.iter().any(|p| *p == staged) {
            return true;
        }
        self.is_the_auto_merge(dir, file, staged.as_deref())
    }

    /// Whether the staged content is exactly what git's own three-way merge
    /// produces for this path.
    ///
    /// The byte-identical test above catches a file only one side touched and a
    /// conflict resolved by taking one side whole. It does not catch the common
    /// case: **both sides changed the file in different places, and git
    /// combined them**. That result matches neither parent while still being
    /// something git assembled and nobody wrote, and treating it as authored
    /// gates most of what an ordinary merge stages.
    ///
    /// So the question is asked of git directly. Reproduce the merge for this
    /// one path from the three blobs and compare. A clean reproduction that
    /// equals what is staged is git's own work. A hand resolution differs,
    /// which is the whole point, and where it does not differ it *is* the auto
    /// merge and excusing it is right.
    ///
    /// **Two parents only, and an octopus is therefore left with the weaker
    /// coverage rather than served by it.** An octopus has no single merge
    /// base, so there is nothing to reproduce from. The byte comparison above
    /// still answers for a file that matches one parent whole; a file an
    /// octopus auto-merged out of combined hunks stays reported as an authored
    /// change there, which is exactly the case this function exists for.
    ///
    /// The reproduction is bare `git merge-file`, which knows none of the
    /// repository's merge configuration: a `.gitattributes` `merge=` driver,
    /// `merge.renormalize`, a non-default `diff.algorithm`, or a criss-cross
    /// history where `ort` builds a virtual base out of several. Any of those
    /// produces a different blob and the file is reported. That is the safe
    /// direction, and it is written down because the visible effect is the gate
    /// firing on an ordinary merge with nothing saying why.
    fn is_the_auto_merge(&self, dir: &Path, file: &str, staged: Option<&str>) -> bool {
        // `detect` collects MERGE_HEAD first and pushes HEAD last, and git
        // calls HEAD `ours` and MERGE_HEAD `theirs`. Naming them the other way
        // round costs nothing today, because a clean `merge-file` is
        // order-independent, and misleads the next person to reach for a label
        // flag or `--ours`.
        let (Some(staged), [theirs, ours]) = (staged, &self.parents[..]) else {
            return false;
        };
        let Some(base) = git(dir, &["merge-base", ours, theirs]) else {
            return false;
        };
        let base = base.trim();
        // `merge-file` wants three files. The blobs go to a scratch directory
        // that is removed whatever happens below.
        //
        // The name carries a counter and not the path, because two calls about
        // the same path must not share a directory: the first one's cleanup
        // deletes the second one's inputs mid-run, and the second then reports
        // an authored change. Found exactly that way, as a test that failed
        // beside its neighbours and passed alone.
        static NTH: AtomicU64 = AtomicU64::new(0);
        let scratch = std::env::temp_dir().join(format!(
            "mockspace-merge-{}-{}",
            std::process::id(),
            NTH.fetch_add(1, Ordering::Relaxed)
        ));
        // `create_dir`, not `create_dir_all`. The path is a pid and a small
        // counter, so it is predictable, and `create_dir_all` succeeds through
        // a symlink somebody planted there first: three files of repository
        // content would then land in a directory of their choosing, and the
        // cleanup below would remove only the link. `create_dir` refuses an
        // existing entry of any kind.
        if std::fs::create_dir(&scratch).is_err() {
            return false;
        }
        let write = |name: &str, rev: &str| -> Option<PathBuf> {
            let at = scratch.join(name);
            // An absent side is an empty file, which is what `merge-file` means
            // by one side having nothing there. `git show` already fails on an
            // absent path, so it answers presence and content in one spawn;
            // asking `rev-parse` first doubled the process count on the hot
            // path, which is every file the byte test missed at every commit.
            let body = git_bytes(dir, &["show", &format!("{rev}:./{file}")]).unwrap_or_default();
            std::fs::write(&at, body).ok()?;
            Some(at)
        };
        let verdict = (|| {
            let o = write("ours", ours)?;
            let b = write("base", base)?;
            let t = write("theirs", theirs)?;
            let out = Command::new("git")
                .arg("merge-file")
                .arg("-p")
                .arg("--quiet")
                .arg(&o)
                .arg(&b)
                .arg(&t)
                .current_dir(dir)
                .output()
                .ok()?;
            // A non-zero status is a conflict count. Today no author can match
            // a conflicted reproduction anyway, because `merge-file` labels its
            // markers with the paths it was handed and those carry the pid and
            // counter above, so nobody can stage the same bytes. That makes
            // this guard redundant and it stays, because the property it would
            // enforce rests entirely on the scratch name being unpredictable:
            // give that directory a fixed name and this line is the only thing
            // standing between a staged conflict and an excuse.
            if !out.status.success() {
                return Some(false);
            }
            let produced = git_hash_object(dir, &out.stdout)?;
            Some(produced == staged)
        })()
        .unwrap_or(false);
        let _ = std::fs::remove_dir_all(&scratch);
        verdict
    }
}

/// The object id `content` would have as a blob, without writing it.
fn git_hash_object(dir: &Path, content: &[u8]) -> Option<String> {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin"])
        .current_dir(dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(content).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_bytes(cwd: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    out.status.success().then_some(out.stdout)
}


/// The object id of `file` at `rev`, or `None` where the path is absent there.
///
/// `rev` is the whole rev-spec prefix up to and including its colon: `":"` for
/// the index, `"<sha>:"` for a commit. The `./` is load-bearing and is the same
/// reason it is load-bearing
/// in `changelist_required::staged_or_worktree`: the bare `<rev>:<path>` form
/// resolves from the repository root, so from a mock directory it would read
/// `<repo>/crates/...` rather than `<repo>/mock/crates/...`, silently comparing
/// the wrong blob where both exist.
fn blob(dir: &Path, rev: &str, file: &str) -> Option<String> {
    let spec = format!("{rev}./{file}");
    let out = git(dir, &["rev-parse", "-q", "--verify", &spec])?;
    let id = out.trim();
    (!id.is_empty()).then(|| id.to_string())
}

fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct Repo {
        root: PathBuf,
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl Repo {
        /// A repo with a `mock/` subdirectory, because the gates run from
        /// there and the relative-path handling is half of what is under test.
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("mockspace-merge-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("mock/crates/foo/src")).unwrap();
            let r = Repo {
                root,
            };
            r.git(&["init", "-q", "-b", "trunk"]);
            r.git(&["config", "user.email", "t@example.com"]);
            r.git(&["config", "user.name", "t"]);
            r.git(&["config", "commit.gpgsign", "false"]);
            r
        }

        /// Refuses a git that failed. A no-op that exits non-zero looks
        /// exactly like a step that worked, and a fixture built on one asserts
        /// against state nobody set up: that is how the `--ours` case below
        /// came to re-assert the `--theirs` case on identical content.
        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            String::from_utf8_lossy(&out.stdout).to_string()
        }

        /// The same, for a command whose failure is the expected outcome.
        fn git_may_fail(&self, args: &[&str]) -> bool {
            Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        fn mock(&self) -> PathBuf {
            self.root.join("mock")
        }

        fn write(&self, rel: &str, body: &str) {
            let p = self.mock().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }

        fn commit(&self, msg: &str) {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", msg, "--no-gpg-sign"]);
        }

        /// Trunk and a branch each move the same file to a different body, then
        /// the branch merges trunk. Returns whether the merge conflicted.
        fn diverge_and_merge(&self, rel: &str, trunk_body: &str, branch_body: &str) -> bool {
            self.write(rel, "base\n");
            self.commit("base");
            self.git(&["switch", "-q", "-c", "feature"]);
            self.write(rel, branch_body);
            self.commit("on the branch");
            self.git(&["switch", "-q", "trunk"]);
            self.write(rel, trunk_body);
            self.commit("on trunk");
            self.git(&["switch", "-q", "feature"]);
            !self.git_may_fail(&["merge", "--no-commit", "--no-ff", "trunk"])
        }
    }

    /// The scratch directory is created with `create_dir` and not
    /// `create_dir_all`, because its name is a pid and a small counter and is
    /// therefore predictable. Measured rather than assumed: `create_dir_all`
    /// succeeds through a symlink somebody planted at that path first, so
    /// three files of repository content would land wherever it points, and
    /// the cleanup afterwards removes the link rather than what it wrote.
    #[test]
    fn the_scratch_directory_refuses_a_path_something_already_holds() {
        let root = std::env::temp_dir().join(format!("mockspace-scratchguard-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("elsewhere")).unwrap();
        let planted = root.join("planted");
        std::os::unix::fs::symlink(root.join("elsewhere"), &planted).unwrap();

        assert!(
            std::fs::create_dir_all(&planted).is_ok(),
            "the reason this test exists went away: create_dir_all now refuses a symlink"
        );
        assert!(
            std::fs::create_dir(&planted).is_err(),
            "create_dir accepted a path already held, which is the whole guard"
        );
        assert!(
            std::fs::create_dir(root.join("fresh")).is_ok(),
            "create_dir refused a clean path, so the guard would refuse every real call"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn off_a_merge_nothing_is_ever_inherited() {
        // the control that keeps every excuse below meaningful: an ordinary
        // staged edit, in a repo with no merge in flight, is an authored change
        // and the gates must still see it.
        let r = Repo::new("plain");
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n");
        r.commit("seed");
        r.write("crates/foo/src/lib.rs", "pub fn b() {}\n");
        r.git(&["add", "-A"]);

        let m = Merge::detect(&r.mock());
        assert!(!m.in_progress());
        assert!(!m.inherited(&r.mock(), "crates/foo/src/lib.rs"));
    }

    #[test]
    fn a_file_only_trunk_touched_is_inherited_from_that_parent() {
        // the case that blocked a real merge: the branch never touched this
        // file, so the merge takes trunk's copy verbatim and nobody authored
        // anything.
        let r = Repo::new("theirs");
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n");
        r.write("crates/foo/OTHER.md.tmpl", "untouched\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/OTHER.md.tmpl", "branch moved something else\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\npub fn trunk_added() {}\n");
        r.commit("on trunk");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress(), "the fixture did not leave a merge in flight");
        assert!(
            m.inherited(&r.mock(), "crates/foo/src/lib.rs"),
            "a file the branch never touched is not an authored change"
        );
    }

    #[test]
    fn a_file_both_sides_changed_in_different_places_is_inherited() {
        // The common shape of an ordinary merge, and the one a byte comparison
        // against each parent cannot see: git combines two non-overlapping
        // hunks, so the result matches neither side while still being nobody's
        // work. Without this, most of what a real merge stages reads as
        // authored and the gate fires on all of it.
        let r = Repo::new("automerge");
        r.write("crates/foo/src/lib.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/src/lib.rs", "one\nBRANCH\nthree\nfour\nfive\nsix\nseven\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        r.write("crates/foo/src/lib.rs", "one\ntwo\nthree\nfour\nfive\nsix\nTRUNK\n");
        r.commit("on trunk");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);

        let body = std::fs::read_to_string(r.mock().join("crates/foo/src/lib.rs")).unwrap();
        assert!(
            body.contains("BRANCH") && body.contains("TRUNK"),
            "the fixture did not auto-merge both hunks: {body:?}"
        );

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress());
        assert!(
            m.inherited(&r.mock(), "crates/foo/src/lib.rs"),
            "git assembled this and nobody wrote it"
        );
    }

    #[test]
    fn an_octopus_does_not_reach_the_reproduction_and_keeps_the_weaker_answer() {
        // Pins the two-parent restriction, and records what it costs. An
        // octopus has no single merge base, so there is nothing to reproduce
        // from and only the byte comparison answers. A file it combined out of
        // hunks from two parents matches none of them and is therefore reported
        // as an authored change, which is the very case the reproduction
        // exists to stop reporting.
        //
        // The assertion is the current limitation rather than the wanted
        // behaviour, deliberately: without it, dropping the restriction and
        // feeding an octopus its first two parents leaves every test green,
        // and that is a merge base nobody chose.
        let r = Repo::new("octopus-hunks");
        r.write("crates/foo/src/lib.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "one"]);
        r.write("crates/foo/src/lib.rs", "one\nONE\nthree\nfour\nfive\nsix\nseven\n");
        r.commit("on one");
        r.git(&["switch", "-q", "trunk"]);
        r.git(&["switch", "-q", "-c", "two"]);
        r.write("crates/foo/src/lib.rs", "one\ntwo\nthree\nfour\nfive\nsix\nTWO\n");
        r.commit("on two");
        r.git(&["switch", "-q", "trunk"]);
        r.git(&["merge", "--no-commit", "--no-ff", "one", "two"]);

        let body = std::fs::read_to_string(r.mock().join("crates/foo/src/lib.rs")).unwrap();
        assert!(
            body.contains("ONE") && body.contains("TWO"),
            "the fixture did not combine both hunks: {body:?}"
        );
        let m = Merge::detect(&r.mock());
        assert_eq!(m.parents.len(), 3, "HEAD plus both merged heads");
        assert!(
            !m.inherited(&r.mock(), "crates/foo/src/lib.rs"),
            "an octopus reached the two-parent reproduction, which has no base to stand on"
        );
    }

    #[test]
    fn an_edit_made_on_top_of_an_auto_merge_is_not_inherited() {
        // The control on the case above, and the one that keeps the reproduction
        // honest: take git's own combined result and change one line of it. It
        // still matches neither parent, exactly as the auto merge did, and it is
        // an authored change.
        let r = Repo::new("automerge-then-edit");
        r.write("crates/foo/src/lib.rs", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/src/lib.rs", "one\nBRANCH\nthree\nfour\nfive\nsix\nseven\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        r.write("crates/foo/src/lib.rs", "one\ntwo\nthree\nfour\nfive\nsix\nTRUNK\n");
        r.commit("on trunk");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);

        r.write(
            "crates/foo/src/lib.rs",
            "one\nBRANCH\nthree\nSMUGGLED\nfive\nsix\nTRUNK\n",
        );
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);

        let m = Merge::detect(&r.mock());
        assert!(
            !m.inherited(&r.mock(), "crates/foo/src/lib.rs"),
            "a line nobody committed on either side rode in under a merge"
        );
    }

    #[test]
    fn a_hand_resolution_matching_neither_parent_is_not_inherited() {
        // the load-bearing negative. A blanket skip-during-a-merge would let
        // this through, and this is exactly the authored change the gate is
        // for: somebody wrote a third body while resolving.
        let r = Repo::new("hand");
        let conflicted = r.diverge_and_merge("crates/foo/src/lib.rs", "trunk\n", "branch\n");
        assert!(conflicted, "the fixture was meant to conflict");

        r.write("crates/foo/src/lib.rs", "a third thing nobody committed\n");
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress());
        assert!(
            !m.inherited(&r.mock(), "crates/foo/src/lib.rs"),
            "a hand resolution IS an authored change and stays gated"
        );
    }

    #[test]
    fn resolving_a_conflict_by_taking_the_other_side_whole_is_inherited() {
        // `--theirs` reproduces a parent byte for byte, so it is not an
        // authored change however it was reached.
        let r = Repo::new("theirs-resolve");
        assert!(r.diverge_and_merge("crates/foo/src/lib.rs", "trunk\n", "branch\n"));
        r.git(&["checkout", "--theirs", "--", "mock/crates/foo/src/lib.rs"]);
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);

        let m = Merge::detect(&r.mock());
        assert_eq!(
            std::fs::read_to_string(r.mock().join("crates/foo/src/lib.rs")).unwrap(),
            "trunk\n",
            "the resolution did not take the other side"
        );
        assert!(m.inherited(&r.mock(), "crates/foo/src/lib.rs"));
    }

    #[test]
    fn resolving_a_conflict_by_keeping_our_own_side_is_inherited() {
        // its own fixture, and that is the point rather than tidiness. Staging
        // a resolution collapses the conflict stages, so a second `checkout
        // --ours` in the same tree is a no-op that changes nothing and leaves
        // the previous assertion re-run on identical content. This is the only
        // case where the staged blob matches HEAD rather than MERGE_HEAD, so
        // without a fresh conflict nothing anywhere covers HEAD as a parent.
        let r = Repo::new("ours-resolve");
        assert!(r.diverge_and_merge("crates/foo/src/lib.rs", "trunk\n", "branch\n"));
        r.git(&["checkout", "--ours", "--", "mock/crates/foo/src/lib.rs"]);
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);

        let m = Merge::detect(&r.mock());
        assert_eq!(
            std::fs::read_to_string(r.mock().join("crates/foo/src/lib.rs")).unwrap(),
            "branch\n",
            "the resolution did not keep our own side"
        );
        assert!(m.inherited(&r.mock(), "crates/foo/src/lib.rs"));
    }

    #[test]
    fn an_octopus_merge_counts_every_parent_and_not_only_the_first() {
        // `git rev-parse --verify MERGE_HEAD` prints the first line only, so a
        // reader built on it treats parent two onward as absent and gates
        // every file inherited from them. Reading the file is what makes this
        // pass, and swapping it back for `--verify` is what this catches.
        let r = Repo::new("octopus");
        r.write("crates/foo/src/lib.rs", "base\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "one"]);
        r.write("crates/foo/src/from_one.rs", "one\n");
        r.commit("on one");
        r.git(&["switch", "-q", "trunk"]);
        r.git(&["switch", "-q", "-c", "two"]);
        r.write("crates/foo/src/from_two.rs", "two\n");
        r.commit("on two");
        r.git(&["switch", "-q", "trunk"]);
        r.git(&["merge", "--no-commit", "--no-ff", "one", "two"]);

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress(), "the fixture did not leave a merge in flight");
        assert_eq!(m.parents.len(), 3, "HEAD plus both merged heads");
        // the file from the SECOND merged head is the one a first-line-only
        // reader would report as an authored change
        assert!(m.inherited(&r.mock(), "crates/foo/src/from_two.rs"));
        assert!(m.inherited(&r.mock(), "crates/foo/src/from_one.rs"));
    }

    #[test]
    fn a_deletion_the_merge_brought_in_is_inherited() {
        // absent in the index and absent in a parent compare equal, which is
        // the deletion case. Without it a merge of a branch that removed a file
        // is gated on a file nobody can revert.
        let r = Repo::new("delete");
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n");
        r.write("crates/foo/src/gone.rs", "pub fn b() {}\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n// branch\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        std::fs::remove_file(r.mock().join("crates/foo/src/gone.rs")).unwrap();
        r.commit("trunk removed it");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress());
        assert!(m.inherited(&r.mock(), "crates/foo/src/gone.rs"));
    }

    #[test]
    fn a_file_neither_parent_carries_is_not_inherited() {
        // the control on the deletion case above, which compares two absences.
        // A path nothing has must not read as inherited, or every untracked
        // file would be excused during a merge.
        let r = Repo::new("absent");
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n// branch\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        r.write("crates/foo/src/other.rs", "pub fn c() {}\n");
        r.commit("on trunk");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress());
        // staged and absent from both parents cannot happen through the index,
        // so this asks the question the untracked walk would ask
        assert!(
            !m.inherited(&r.mock(), "crates/foo/src/never_existed.rs"),
            "two absences must not read as a match"
        );
    }

    #[test]
    fn the_relative_form_reads_the_mock_copy_not_the_repo_root_one() {
        // the `./` in the rev-spec. With both `<repo>/x.rs` and
        // `<repo>/mock/x.rs` present and differing, a bare `<rev>:x.rs` reads
        // the repo-root one, which would compare the wrong blob and excuse a
        // real change.
        let r = Repo::new("relative");
        r.write("crates/foo/src/lib.rs", "the mock copy\n");
        std::fs::create_dir_all(r.root.join("crates/foo/src")).unwrap();
        std::fs::write(r.root.join("crates/foo/src/lib.rs"), "the root copy\n").unwrap();
        r.commit("base");

        let from_mock = blob(&r.mock(), "HEAD:", "crates/foo/src/lib.rs").unwrap();
        let from_root = blob(&r.root, "HEAD:", "crates/foo/src/lib.rs").unwrap();
        assert_ne!(from_mock, from_root, "the two copies resolved to one blob");
        let mock_body = r.git(&["cat-file", "-p", &from_mock]);
        assert_eq!(mock_body, "the mock copy\n");
    }

    /// A linked worktree answers `--git-path` absolutely, and `detect` joins
    /// that answer onto the directory it asked from. `Path::join` returns an
    /// absolute argument unchanged, which is what makes one line serve both
    /// shapes, and nothing pinned it: the code carried the claim in a comment
    /// and the suite never built a worktree.
    ///
    /// Silently, too. Were the join wrong, the read would miss, `detect` would
    /// return the default, and every file a merge staged would be judged as
    /// authored. No error, no diagnostic, and a gate that refuses an ordinary
    /// merge for a reason nobody can see.
    #[test]
    fn a_merge_inside_a_linked_worktree_is_detected() {
        let r = Repo::new("linked");
        r.write("crates/foo/src/lib.rs", "base\n");
        r.commit("base");

        let wt = r.root.join("wt");
        r.git(&["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "feature"]);
        let wt_mock = wt.join("mock");

        // the branch moves one file, trunk adds another, so the merge is clean
        // and stages a file the branch never touched.
        std::fs::write(wt_mock.join("crates/foo/src/lib.rs"), "on the branch\n").unwrap();
        git(&wt_mock, &["add", "-A"]);
        git(&wt_mock, &[
            "commit",
            "-q",
            "-m",
            "on the branch",
            "--no-gpg-sign",
        ]);
        r.write("crates/foo/src/other.rs", "on trunk\n");
        r.commit("on trunk");
        assert!(
            Command::new("git")
                .args(["merge", "--no-commit", "--no-ff", "trunk"])
                .current_dir(&wt_mock)
                .output()
                .expect("git merge")
                .status
                .success(),
            "the fixture's merge conflicted, so it is not the case under test"
        );

        // the control on the fixture, and the whole reason this test is not the
        // one above it wearing a different name: the two shapes must actually
        // differ, or a worktree proves nothing a plain repository did not.
        let in_the_worktree = git(&wt_mock, &["rev-parse", "--git-path", "MERGE_HEAD"]).unwrap();
        assert!(
            Path::new(in_the_worktree.trim()).is_absolute(),
            "a linked worktree answered relatively, so this fixture is not \
             exercising the join at all: {in_the_worktree:?}"
        );
        let in_the_main = git(&r.mock(), &["rev-parse", "--git-path", "MERGE_HEAD"]).unwrap();
        assert!(
            !Path::new(in_the_main.trim()).is_absolute(),
            "the main worktree answered absolutely too, so the two shapes are \
             not distinguishable here: {in_the_main:?}"
        );

        let m = Merge::detect(&wt_mock);
        assert!(
            m.in_progress(),
            "the merge was not seen from the linked worktree"
        );
        assert!(
            m.inherited(&wt_mock, "crates/foo/src/other.rs"),
            "a file the merge brought in whole was judged as authored"
        );

        // `remove_dir_all` on the root leaves git's administrative copy behind
        // otherwise, and the next run of this test reuses that root.
        r.git(&["worktree", "remove", "--force", wt.to_str().unwrap()]);
    }
}
