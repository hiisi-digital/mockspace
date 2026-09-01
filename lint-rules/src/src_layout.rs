//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where a project's source actually lives, and the git walk over it.
//!
//! Three phase-gate lints each ran the same three git commands with `crates/`
//! written into them as a literal, then filtered the results with their own copy
//! of `starts_with("crates/")` and pulled a package name out with their own copy
//! of `strip_prefix("crates/")`. Seventeen sites for one fact, counted rather
//! than estimated: five in `changelist_doc_gate`, six in `changelist_lock`, six
//! in `changelist_required`.
//!
//! And the fact was wrong. `src_dirs` has been configurable for a while, is
//! honoured by discovery and by every renderer, and was honoured by none of the
//! gates. A project that renamed or grouped its packages got gates pointed at a
//! directory it does not have, which is to say gates that found nothing and
//! said nothing, which is the shape a working gate has too.
//!
//! **This fixes where the source is and not what counts as source there.** The
//! gates still decide that with `ends_with(".rs")`, which nothing in
//! `mockspace.toml` names, so a project writing zig or typescript under a
//! declared source directory still gets the failure described above with the
//! other half of the cause. Two `FIXME`s mark it, and
//! `a_source_gate_guards_the_extensions_the_project_writes` asserts the
//! intended behaviour and is ignored until there is one.
//!
//! So the fact lives here once and the lints ask. What stays with each lint is
//! the part that really is its own, which is what it considers a file worth
//! caring about: source for one, doc templates for another.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::merge;

/// The source directories a project declares, relative to its mock directory.
///
/// Relative because that is what git wants as a pathspec under `--relative`, and
/// what git gives back, so the same strings both ask the question and answer it.
pub struct SrcLayout {
    dirs: Vec<String>,
}

impl SrcLayout {
    /// Build the layout from the absolute directories the config carries.
    ///
    /// A directory that does not sit under the mock dir cannot be written as a
    /// relative pathspec, and dropping one silently would narrow every gate that
    /// consults it without anything saying so. Nothing in the config can produce
    /// one today, since every entry is `mock_dir.join(..)` of a declared name, so
    /// this warns and carries on rather than refusing.
    pub fn new(mock_dir: &Path, src_dirs: &[PathBuf]) -> Self {
        let mut dirs = Vec::new();
        for d in src_dirs {
            match d.strip_prefix(mock_dir) {
                Ok(rel) => {
                    let s = rel.to_string_lossy().replace('\\', "/");
                    let s = s.trim_end_matches('/').to_string();
                    if !s.is_empty() {
                        dirs.push(s);
                    }
                },
                Err(_) => {
                    eprintln!(
                        "  [lint-config] source directory `{}` is not under the mock dir `{}`, \
                     so the phase gates cannot address it and will not guard what is in it.",
                        d.display(),
                        mock_dir.display(),
                    )
                },
            }
        }
        Self {
            dirs,
        }
    }

    /// Whether the project declares any source at all.
    ///
    /// True for a documentation repository, and the reason every walk below
    /// returns early rather than calling git: a git pathspec list that is empty
    /// means *everything*, so an empty layout asked naively would hand a phase
    /// gate the whole worktree.
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty()
    }

    /// The pathspecs to hand git, one per declared directory.
    pub fn pathspecs(&self) -> Vec<String> {
        self.dirs.iter().map(|d| format!("{d}/")).collect()
    }

    /// Whether a repo-relative path sits under one of the directories.
    ///
    /// The trailing slash is what makes this a component match, so a sibling
    /// named `crates-old` does not answer to `crates`.
    pub fn holds(&self, file: &str) -> bool {
        !file.is_empty() && self.dirs.iter().any(|d| file.starts_with(&format!("{d}/")))
    }

    /// The package a path belongs to, meaning the directory component directly
    /// under whichever source directory holds it.
    pub fn package_name(&self, file: &str) -> Option<String> {
        for d in &self.dirs {
            let Some(rest) = file.strip_prefix(&format!("{d}/")) else {
                continue;
            };
            let end = rest.find('/')?;
            return Some(rest[.. end].to_string());
        }
        None
    }

    /// That package's directory on disk, given the mock dir.
    pub fn package_dir(&self, mock_dir: &Path, file: &str) -> Option<PathBuf> {
        for d in &self.dirs {
            let Some(rest) = file.strip_prefix(&format!("{d}/")) else {
                continue;
            };
            let end = rest.find('/')?;
            return Some(mock_dir.join(d).join(&rest[.. end]));
        }
        None
    }
}

/// Every path under the layout that differs from `HEAD`, with how it differs.
///
/// Staged, unstaged and untracked, in that order, first mention winning, which
/// is what the three lints each did separately and is the ordering their
/// messages assume. The gate is deliberately global rather than staged-only: an
/// unstaged edit to a file the phase forbids blocks the commit too, because
/// otherwise the rule is only ever enforced against whoever remembered to stage.
pub fn changed_files(
    mock_dir: &Path,
    layout: &SrcLayout,
    keep: impl Fn(&str) -> bool,
) -> Vec<(String, String)> {
    if layout.is_empty() {
        return Vec::new();
    }
    let specs = layout.pathspecs();

    let mut files: Vec<(String, String)> = Vec::new();

    // The two diff views ask for status rather than names alone, so each one
    // recognises a rename in the same invocation that reports the change.
    //
    // Asking separately is what a first attempt did and it is wrong twice over.
    // A rename query passing `-M100%` overrides whatever `diff.renames` the
    // config chain sets while a bare `--name-only` inherits it, so under
    // `renames = false` the walk reported a delete and an add while the query
    // still paired them, the old path survived the filter, and the gate blocked
    // on a file no longer on disk. And one set of pairs shared between the views
    // let a staged rename hide an unstaged edit to the same file, which
    // reinstates exactly the staged-only hole the paragraph above exists to
    // close.
    let diffs: [(&[&str], &str); 2] = [
        (
            &["diff", "--cached", "--name-status", "-M100%", "--relative", "--"],
            "staged",
        ),
        (
            &["diff", "--name-status", "-M100%", "--relative", "--"],
            "unstaged",
        ),
    ];

    for (args, source) in diffs {
        let mut argv: Vec<&str> = args.to_vec();
        argv.extend(specs.iter().map(String::as_str));
        let Some(output) = run_git(mock_dir, &argv) else {
            continue;
        };
        for line in output.lines() {
            let Some(file) = changed_path(line) else {
                continue;
            };
            if keep(file) && !files.iter().any(|(f, _)| f == file) {
                files.push((file.to_string(), source.to_string()));
            }
        }
    }

    // Untracked files have no status to read and no rename to detect: git has
    // never seen the path before, so a shell `mv` arrives here as an add and in
    // the walk above as a delete, and both sides are reported. Moving a
    // directory with `git mv`, or staging the move, is what puts it in front of
    // the carve-out.
    {
        let mut argv: Vec<&str> = vec!["ls-files", "--others", "--exclude-standard", "--"];
        argv.extend(specs.iter().map(String::as_str));
        if let Some(output) = run_git(mock_dir, &argv) {
            for line in output.lines() {
                let file = line.trim();
                if keep(file) && !files.iter().any(|(f, _)| f == file) {
                    files.push((file.to_string(), "untracked".to_string()));
                }
            }
        }
    }

    // What a merge brought in is not what an author wrote, and the phase gates
    // judge authored changes. Only the staged walk is asked, because that is
    // the only one a merge writes to; an unstaged or untracked file during a
    // merge was dirty beforehand and is nobody's resolution.
    //
    // Costs one `git rev-parse` off the merge path and nothing per file, since
    // `Merge::detect` finds no MERGE_HEAD and `inherited` is then constant.
    let merge = merge::Merge::detect(mock_dir);
    if merge.in_progress() {
        files.retain(|(file, source)| source != "staged" || !merge.inherited(mock_dir, file));
    }

    files
}

/// The path a `--name-status` line reports as changed, or nothing where the
/// line reports something no phase protects anything by refusing.
///
/// A file that only moved says exactly what it said before. The gates freeze
/// what a document states and what source declares, and a path is neither, so
/// a rename with identical content contributes neither of its two sides.
///
/// Without that a crate cannot be renamed at all. Its directory carries both
/// templates and source, the doc gate refuses the templates in every phase but
/// DOC and the source gate refuses the source in every phase but IMPL, so one
/// of the two fires whichever phase the move is made in. Splitting the move
/// across the two phases is not a way out either, since a crate is required to
/// have both a `src/lib.rs` and a `README.md.tmpl` and the halfway state leaves
/// one crate missing each.
///
/// This does permit a single document to move between crates during a phase
/// that would otherwise refuse it, which is wider than the argument above.
/// Accepted rather than overlooked: the two are indistinguishable per file, a
/// crate rename being a directory of them and nothing more, and the source side
/// self-heals because a moved `.rs` forces a `mod` line to change in a `lib.rs`
/// that stays gated.
fn changed_path(line: &str) -> Option<&str> {
    let mut parts = line.split('\t');
    let status = parts.next()?;

    // `R` and `C` carry a source and a destination; every other status carries
    // one path. Parsed on the shape rather than on the flag, so a later change
    // to the rename options cannot silently turn a destination into a source.
    if status.starts_with('R') || status.starts_with('C') {
        let _from = parts.next()?;
        let to = parts.next()?.trim();
        // The whole of the carve-out, and only a rename. A copy leaves the
        // original where it was and declares the content a second time, which
        // is a new declaration however identical it is.
        if status == "R100" {
            return None;
        }
        return (!to.is_empty()).then_some(to);
    }

    let path = parts.next()?.trim();
    (!path.is_empty()).then_some(path)
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(test)]
mod merge_walk_tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    /// A repo with a `mock/crates/foo` package, on branch `trunk`.
    struct Repo {
        root: PathBuf,
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    impl Repo {
        fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("mockspace-walk-{name}-{}", std::process::id()));
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

        fn git(&self, args: &[&str]) {
            Command::new("git")
                .args(args)
                .current_dir(&self.root)
                .output()
                .unwrap();
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

        fn layout(&self) -> SrcLayout {
            SrcLayout::new(&self.mock(), &[self.mock().join("crates")])
        }

        fn walk(&self) -> Vec<(String, String)> {
            changed_files(&self.mock(), &self.layout(), |_| true)
        }
    }

    /// The whole reason the merge filter exists. A merge of trunk into a
    /// feature branch stages doc templates and source at once, and no phase
    /// permits both, so before this no design round could carry a merge.
    #[test]
    fn a_merge_that_staged_both_a_doc_template_and_source_reports_neither() {
        let r = Repo::new("both");
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n");
        r.write("crates/foo/DESIGN.md.tmpl", "base\n");
        r.write("crates/foo/README.md", "base\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/README.md", "the branch moved something else\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        r.write(
            "crates/foo/src/lib.rs",
            "pub fn a() {}\npub fn trunk_added() {}\n",
        );
        r.write("crates/foo/DESIGN.md.tmpl", "trunk rewrote the design\n");
        r.commit("on trunk");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);

        let files = r.walk();
        assert!(
            files.is_empty(),
            "a merge authored nothing, yet the walk reported {files:?}"
        );
    }

    /// The control that makes the assertion above mean something: the same
    /// walk, in the same merge, still reports a file somebody resolved by hand.
    #[test]
    fn a_hand_resolved_file_in_the_same_merge_is_still_reported() {
        let r = Repo::new("hand");
        r.write("crates/foo/src/lib.rs", "base\n");
        r.write("crates/foo/src/quiet.rs", "base\n");
        r.commit("base");
        r.git(&["switch", "-q", "-c", "feature"]);
        r.write("crates/foo/src/lib.rs", "branch\n");
        r.commit("on the branch");
        r.git(&["switch", "-q", "trunk"]);
        r.write("crates/foo/src/lib.rs", "trunk\n");
        r.write("crates/foo/src/quiet.rs", "trunk took this one alone\n");
        r.commit("on trunk");
        r.git(&["switch", "-q", "feature"]);
        r.git(&["merge", "--no-commit", "--no-ff", "trunk"]);
        r.write("crates/foo/src/lib.rs", "a third body nobody committed\n");
        r.git(&["add", "mock/crates/foo/src/lib.rs"]);

        let files = r.walk();
        let names: Vec<&str> = files.iter().map(|(f, _)| f.as_str()).collect();
        assert_eq!(
            names,
            vec!["crates/foo/src/lib.rs"],
            "the hand resolution must stay gated and the inherited file must not"
        );
    }

    /// And off a merge the walk is exactly what it always was, so the filter
    /// cannot be excusing anything in an ordinary commit.
    #[test]
    fn off_a_merge_an_ordinary_staged_edit_is_still_reported() {
        let r = Repo::new("plain");
        r.write("crates/foo/src/lib.rs", "pub fn a() {}\n");
        r.commit("base");
        r.write("crates/foo/src/lib.rs", "pub fn b() {}\n");
        r.git(&["add", "-A"]);

        assert_eq!(r.walk(), vec![(
            "crates/foo/src/lib.rs".to_string(),
            "staged".to_string()
        )]);
    }
}

/// The walk over real git repositories, which is most of what this module owes
/// a test and none of what `SrcLayout` itself does. Kept in its own file so the
/// two kinds do not have to be read past each other.
#[cfg(test)]
#[path = "src_layout_walk_tests.rs"]
mod walk_tests;

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(dirs: &[&str]) -> SrcLayout {
        let mock = Path::new("/m");
        let abs: Vec<PathBuf> = dirs.iter().map(|d| mock.join(d)).collect();
        SrcLayout::new(mock, &abs)
    }

    #[test]
    fn the_default_layout_behaves_as_the_hardcoded_prefix_did() {
        let l = layout(&["crates"]);
        assert!(l.holds("crates/foo/src/lib.rs"));
        assert!(!l.holds("docs/foo.md"));
        assert_eq!(
            l.package_name("crates/foo/src/lib.rs").as_deref(),
            Some("foo")
        );
        assert_eq!(l.pathspecs(), vec!["crates/"]);
    }

    /// The whole point: a project that renamed the directory is guarded there
    /// and not under a name it no longer uses.
    #[test]
    fn a_renamed_source_directory_is_the_one_that_is_guarded() {
        let l = layout(&["libs"]);
        assert!(l.holds("libs/foo/src/lib.rs"));
        assert!(!l.holds("crates/foo/src/lib.rs"));
        assert_eq!(
            l.package_name("libs/foo/src/lib.rs").as_deref(),
            Some("foo")
        );
    }

    #[test]
    fn a_grouped_project_is_guarded_in_every_group() {
        let l = layout(&["libs", "tools"]);
        assert!(l.holds("libs/a/src/lib.rs"));
        assert!(l.holds("tools/b/src/main.rs"));
        assert_eq!(l.package_name("tools/b/src/main.rs").as_deref(), Some("b"));
        assert_eq!(l.pathspecs(), vec!["libs/", "tools/"]);
    }

    #[test]
    fn a_nested_source_directory_keeps_its_whole_path() {
        let l = layout(&["sub/crates"]);
        assert!(l.holds("sub/crates/foo/src/lib.rs"));
        assert!(!l.holds("crates/foo/src/lib.rs"));
        assert_eq!(
            l.package_name("sub/crates/foo/src/lib.rs").as_deref(),
            Some("foo")
        );
    }

    /// A prefix match without the separator would claim this one, and the
    /// neighbour it claims is exactly the kind of directory that gets left
    /// behind by a rename.
    #[test]
    fn a_directory_whose_name_merely_starts_the_same_is_not_held() {
        let l = layout(&["crates"]);
        assert!(!l.holds("crates-old/foo/src/lib.rs"));
        assert_eq!(l.package_name("crates-old/foo/src/lib.rs"), None);
    }

    /// A file sitting directly in a source directory belongs to no package, and
    /// saying so is better than naming the file as though it were one.
    #[test]
    fn a_file_with_no_package_component_names_no_package() {
        let l = layout(&["crates"]);
        assert!(l.holds("crates/README.md"));
        assert_eq!(l.package_name("crates/README.md"), None);
    }

    #[test]
    fn package_dir_is_rooted_at_the_mock_dir() {
        let l = layout(&["libs"]);
        assert_eq!(
            l.package_dir(Path::new("/m"), "libs/foo/src/lib.rs"),
            Some(PathBuf::from("/m/libs/foo")),
        );
    }

    /// The control for the early return in `changed_files`. An empty pathspec
    /// list is not "nothing", it is "everything", so a documentation repository
    /// would have had its whole worktree handed to a source gate.
    #[test]
    fn an_empty_layout_walks_nothing_rather_than_walking_everything() {
        let l = SrcLayout::new(Path::new("/m"), &[]);
        assert!(l.is_empty());
        assert!(l.pathspecs().is_empty());
        assert!(!l.holds("crates/foo/src/lib.rs"));
        // the real repo is this checkout, so a walk that ignored the guard would
        // return a great many files rather than none
        let cwd = std::env::current_dir().unwrap();
        assert!(changed_files(&cwd, &l, |_| true).is_empty());
    }

    #[test]
    fn a_source_directory_outside_the_mock_dir_contributes_nothing_addressable() {
        let l = SrcLayout::new(Path::new("/m"), &[PathBuf::from("/elsewhere/crates")]);
        assert!(l.is_empty());
    }

    /// Catalogued. The layout answers where source lives and the gates still
    /// decide what counts as source with `.rs`, so half the hardcode this module
    /// exists to remove is still standing one layer up.
    ///
    /// The assertion is what a project should be able to say. Un-ignore it once
    /// a source directory can carry the extensions its language uses; the two
    /// `FIXME`s in `changelist_required.rs` and `changelist_lock.rs` are the
    /// sites that would then read it.
    #[test]
    #[ignore = "catalogue: a source directory cannot declare which extensions \
                are source in it, so a non-rust project's gate matches nothing"]
    fn a_source_gate_guards_the_extensions_the_project_writes() {
        let l = layout(&["src"]);
        // what the gates ask today, spelled out, so the gap is visible here
        let is_source_today = |f: &str| l.holds(f) && f.ends_with(".rs");
        assert!(
            is_source_today("src/main.zig"),
            "a zig project's source is not guarded, and the gate reports cleanly"
        );
    }
}
