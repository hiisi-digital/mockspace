//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a lint's verdict depends on, declared by the lint.
//!
//! A gate run that repeats work is repeating a verdict whose inputs did not move. To skip it the
//! engine has to know the inputs, and only the lint knows them, so the lint says. The declaration
//! is a promise about reading: **a lint declaring a reach reads nothing outside it**, and the
//! engine acts on exactly what the reach names, in one of two ways. [`Reach::File`], [`Reach::Crate`]
//! and [`Reach::Workspace`] key the lint's saved verdict on the content they cover, so the lint is
//! replayed when none of it moved. [`Reach::Paths`] is for a repo lint and names repository files
//! instead: it is never saved, and the commit and push gates do not run it when the commit or the
//! unpushed commits change none of its globs (`lint::touched`).
//!
//! **Declaring none is declaring the widest.** [`Lint::reach`](crate::Lint::reach) defaults to
//! [`Reach::Repository`], which the engine never caches, so a lint written without knowing this
//! exists, a pack lint included, is run on every gate as it always was. Narrowing is the lint's
//! choice, and [`reach_check`](crate::reach_check) samples the promise: a lint that reads outside
//! what it declared gives different findings when only that outside input moves, and the check
//! fails. A sample is not a proof, which is why a lint that finds nothing on its corpus fails the
//! built-in test instead of passing it. For `Paths` the check is
//! [`reach_check::repo_lint`](crate::reach_check::repo_lint), which changes, removes and adds files
//! no glob matches.

/// How far a lint reads, from one file to the whole repository.
///
/// Ordered narrowest to widest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reach {
    /// The verdict is a function of one source file's text, the crate's configuration scalars
    /// (its name, the prefix, whether it is a proc-macro crate) and the lint's own
    /// configuration. It reads no sibling file, no document, no dependency list.
    File,
    /// The verdict is a function of one crate: its source files, its design and doc templates,
    /// its dependency list, the set of crate names in the workspace, and the lint's own
    /// configuration. It reads nothing outside the crate directory through `workspace_root`.
    Crate,
    /// The verdict is a function of every crate the lint is handed, and nothing outside them.
    Workspace,
    /// The verdict is a function of the repository files matching these globs, relative to the
    /// repository root (`dir/**`, `dir/*.toml`), and of the lint's configuration. **The lint's own
    /// source and the tooling it runs are inputs**, so they are listed too: a commit changing any
    /// file the globs match, or the lint set itself, runs the lint, and any other commit does not.
    /// An empty list names nothing and is read as [`Reach::Repository`].
    ///
    /// Not saved by the engine, which has no digest of files it cannot enumerate; a lint that wants
    /// replay asks [`RepoContext::remembered`](crate::RepoContext::remembered) itself.
    Paths(&'static [&'static str]),
    /// The verdict may depend on anything in the repository: the registry, the design rounds,
    /// git state, any file. The engine never saves such a verdict. The default.
    Repository,
}

/// What a gate run changes, as far as deciding which lints it runs goes.
///
/// Paths are relative to the repository root and `/`-separated.
#[derive(Debug, Clone, Copy)]
pub enum Touched<'a> {
    /// The run is judged whole: a plain run, a commit or push git could not list, one that changed
    /// the lint set, or one run under a lint set no clean full run of this tree has covered. Every
    /// lint runs.
    Everything,
    /// The files the commit changes, or the commits a push carries, and nothing else changed.
    Files(&'a [String]),
}

impl Reach {
    /// Whether a lint of this reach runs for a run that changes `touched`.
    ///
    /// Only [`Reach::Paths`] can be skipped, and only when none of the changed files matches its
    /// globs. Every other reach, and a `Paths` naming nothing, runs always: a lint that declares
    /// nothing is never skipped.
    #[must_use]
    pub fn runs_for(self, touched: &Touched) -> bool {
        match (self, touched) {
            (Self::Paths(globs), Touched::Files(files)) if !globs.is_empty() => {
                files
                    .iter()
                    .any(|f| globs.iter().any(|g| crate::glob_match_anchored(g, f)))
            },
            _ => true,
        }
    }

    /// Whether a verdict of this reach can be saved and replayed at all.
    #[must_use]
    pub const fn is_cacheable(self) -> bool {
        matches!(self, Self::File | Self::Crate | Self::Workspace)
    }

    /// The label the lint listing and the timings use.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Crate => "crate",
            Self::Workspace => "workspace",
            Self::Paths(_) => "paths",
            Self::Repository => "repository",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    const ASSETS: Reach = Reach::Paths(&["content/**", "mock/crates/kaski-asset-lints/**"]);

    #[test]
    fn a_commit_changing_a_file_the_paths_name_runs_the_lint() {
        let changed = files(&["README.md", "content/packs/a/manifest.toml"]);
        assert!(ASSETS.runs_for(&Touched::Files(&changed)));
    }

    #[test]
    fn a_commit_changing_the_lints_own_source_runs_it() {
        let changed = files(&["mock/crates/kaski-asset-lints/src/check.rs"]);
        assert!(ASSETS.runs_for(&Touched::Files(&changed)));
    }

    #[test]
    fn a_commit_changing_nothing_the_paths_name_skips_the_lint() {
        let changed = files(&["mock/crates/kaski-engine-hash/src/lib.rs", "contentious/x"]);
        assert!(!ASSETS.runs_for(&Touched::Files(&changed)));
    }

    #[test]
    fn a_commit_listing_no_file_skips_a_paths_lint() {
        assert!(!ASSETS.runs_for(&Touched::Files(&[])));
    }

    #[test]
    fn a_run_judged_whole_runs_every_lint() {
        assert!(ASSETS.runs_for(&Touched::Everything));
    }

    #[test]
    fn a_lint_declaring_nothing_is_never_skipped() {
        let changed = files(&["unrelated.txt"]);
        let t = Touched::Files(&changed);
        for reach in [Reach::Repository, Reach::File, Reach::Crate, Reach::Workspace, Reach::Paths(&[])] {
            assert!(reach.runs_for(&t), "{reach:?} must run");
        }
    }

    #[test]
    fn only_the_three_content_reaches_are_cacheable() {
        assert!(Reach::File.is_cacheable());
        assert!(Reach::Crate.is_cacheable());
        assert!(Reach::Workspace.is_cacheable());
        assert!(!Reach::Repository.is_cacheable(), "the default must never be saved");
        assert!(!Reach::Paths(&["a/**"]).is_cacheable(), "a path reach is skipped, never replayed");
    }
}
