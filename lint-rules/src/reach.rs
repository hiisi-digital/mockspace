//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a lint's verdict depends on, declared by the lint.
//!
//! A gate run that repeats work is repeating a verdict whose inputs did not move. To skip it the
//! engine has to know the inputs, and only the lint knows them, so the lint says. The declaration
//! is a promise about reading: **a lint declaring a reach reads nothing outside it**, and the
//! engine keys the lint's saved verdict on exactly what the reach names.
//!
//! **Declaring none is declaring the widest.** [`Lint::reach`](crate::Lint::reach) defaults to
//! [`Reach::Repository`], which the engine never caches, so a lint written without knowing this
//! exists, a pack lint included, is run on every gate as it always was. Narrowing is the lint's
//! choice and the test in `reach_tests` holds it to the promise: a lint that reads outside what it
//! declared gives different findings when only that outside input moves, and the test fails.

/// How far a lint reads, from one file to the whole repository.
///
/// Ordered narrowest to widest; [`Reach::Change`] sits apart from the others in the order below,
/// since it is not about files at all.
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
    /// The verdict may depend on anything in the repository: the registry, the design rounds,
    /// git state, any file. The engine never saves such a verdict. The default.
    Repository,
    /// The verdict is about the change being judged (a message, an author, the staged paths)
    /// rather than about files. It is run on every commit and never saved.
    Change,
}

impl Reach {
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
            Self::Repository => "repository",
            Self::Change => "change",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_three_content_reaches_are_cacheable() {
        assert!(Reach::File.is_cacheable());
        assert!(Reach::Crate.is_cacheable());
        assert!(Reach::Workspace.is_cacheable());
        assert!(!Reach::Repository.is_cacheable(), "the default must never be saved");
        assert!(!Reach::Change.is_cacheable(), "a message verdict runs on every commit");
    }
}
