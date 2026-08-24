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

use std::path::Path;
use std::process::Command;

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
        // `--verify` keeps this quiet and false outside a merge: MERGE_HEAD
        // exists only while one is in flight. An octopus merge writes several
        // lines and every one of them is a parent.
        let Some(heads) = git(dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]) else {
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
        at_parents.iter().any(|p| *p == staged)
    }
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

        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
            String::from_utf8_lossy(&out.stdout).to_string()
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
            let out = Command::new("git")
                .args(["merge", "--no-commit", "--no-ff", "trunk"])
                .current_dir(&self.root)
                .output()
                .unwrap();
            !out.status.success()
        }
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
    fn a_hand_resolution_matching_neither_parent_is_not_inherited() {
        // the load-bearing negative. A blanket skip-during-a-merge would let
        // this through, and this is exactly the authored change the gate is
        // for: somebody wrote a third body while resolving.
        let r = Repo::new("hand");
        let conflicted = r.diverge_and_merge("crates/foo/src/lib.rs", "trunk\n", "branch\n");
        assert!(conflicted, "the fixture was meant to conflict");

        r.write("crates/foo/src/lib.rs", "a third thing nobody committed\n");
        r.git(&["add", "crates/foo/src/lib.rs"]);

        let m = Merge::detect(&r.mock());
        assert!(m.in_progress());
        assert!(
            !m.inherited(&r.mock(), "crates/foo/src/lib.rs"),
            "a hand resolution IS an authored change and stays gated"
        );
    }

    #[test]
    fn resolving_a_conflict_by_taking_one_side_whole_is_inherited() {
        // and the other half of the same fixture: `--theirs` reproduces a
        // parent byte for byte, so it is not an authored change however it was
        // reached.
        let r = Repo::new("theirs-resolve");
        assert!(r.diverge_and_merge("crates/foo/src/lib.rs", "trunk\n", "branch\n"));
        r.git(&["checkout", "--theirs", "--", "mock/crates/foo/src/lib.rs"]);
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);

        let m = Merge::detect(&r.mock());
        assert!(m.inherited(&r.mock(), "crates/foo/src/lib.rs"));

        // taking our own side is the same statement about the other parent
        r.git(&["checkout", "--ours", "--", "mock/crates/foo/src/lib.rs"]);
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);
        assert!(m.inherited(&r.mock(), "crates/foo/src/lib.rs"));
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
}
