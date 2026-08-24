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
                Err(_) => eprintln!(
                    "  [lint-config] source directory `{}` is not under the mock dir `{}`, \
                     so the phase gates cannot address it and will not guard what is in it.",
                    d.display(),
                    mock_dir.display(),
                ),
            }
        }
        Self { dirs }
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
    let walks: [(&[&str], &str); 3] = [
        (&["diff", "--cached", "--name-only", "--relative", "--"], "staged"),
        (&["diff", "--name-only", "--relative", "--"], "unstaged"),
        (&["ls-files", "--others", "--exclude-standard", "--"], "untracked"),
    ];

    for (args, source) in walks {
        let mut argv: Vec<&str> = args.to_vec();
        argv.extend(specs.iter().map(String::as_str));
        let Some(output) = run_git(mock_dir, &argv) else {
            continue;
        };
        for line in output.lines() {
            let file = line.trim();
            if keep(file) && !files.iter().any(|(f, _)| f == file) {
                files.push((file.to_string(), source.to_string()));
            }
        }
    }

    files
}

fn run_git(cwd: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

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
        assert_eq!(l.package_name("crates/foo/src/lib.rs").as_deref(), Some("foo"));
        assert_eq!(l.pathspecs(), vec!["crates/"]);
    }

    /// The whole point: a project that renamed the directory is guarded there
    /// and not under a name it no longer uses.
    #[test]
    fn a_renamed_source_directory_is_the_one_that_is_guarded() {
        let l = layout(&["libs"]);
        assert!(l.holds("libs/foo/src/lib.rs"));
        assert!(!l.holds("crates/foo/src/lib.rs"));
        assert_eq!(l.package_name("libs/foo/src/lib.rs").as_deref(), Some("foo"));
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
        assert_eq!(l.package_name("sub/crates/foo/src/lib.rs").as_deref(), Some("foo"));
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
