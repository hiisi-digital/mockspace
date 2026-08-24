//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `GIT_*` variables git exports into a hook, and dropping them.
//!
//! The engine runs routinely as a grandchild of a git hook, and it spawns `git`
//! with its working directory set to the mock workspace rather than the repo
//! root. With `GIT_DIR` inherited and `GIT_WORK_TREE` unset, git reads that
//! directory as the top of the work tree: every index path (`mock/crates/...`)
//! then misses the `crates/...` pathspec, and the changelist gates read the
//! whole tree as untracked. With a *relative* `GIT_DIR=.git` the same
//! inheritance makes those invocations fail outright and the gates run blind.
//! Found live, on a worktree commit that reported all 84 doc templates as
//! untracked and changed.
//!
//! Dropping `GIT_INDEX_FILE` means a `git commit -a` temporary index is not
//! consulted. For *detection* that changes nothing, since the gates scan
//! staged, unstaged and untracked state alike. For *content* it can: a file
//! staged and then further modified reads as its index blob while a `commit -a`
//! in flight would commit the worktree blob. The case is catalogued in the
//! changelist-required lint, at `staged_or_worktree`.
//!
//! This also strips a `GIT_DIR`/`GIT_WORK_TREE` pair a user exported on purpose,
//! the bare dotfiles-repo pattern. Running the engine inside such an environment
//! falls back to ordinary repo discovery from the working directory.
//!
//! The launcher does the same thing at its own entry, from its own copy. Two
//! processes, two entries, and neither can reach the other's: sharing one list
//! would mean the engine depending on the launcher's crate, which is a
//! dependency in the wrong direction for the sake of seven strings.

/// The variables that say *where the repository is*. Everything else git
/// exports, and the authoring identity in particular, is left alone.
///
/// Public so the set is testable without mutating the environment, which a
/// multi-threaded test harness cannot do safely.
pub const GIT_REPO_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_COMMON_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_PREFIX",
];

/// Drop them, so every child `git` rediscovers the repo from its own working
/// directory.
///
/// # Safety
///
/// Call first thing in the process entry, before any other thread exists. The
/// environment is process-global and unsynchronised; mutating it while another
/// thread reads it, including indirectly through `Command::spawn` or any libc
/// function that walks `environ`, is undefined behaviour.
pub unsafe fn sanitize_git_env() {
    for var in GIT_REPO_ENV {
        // SAFETY: the caller guarantees no other thread exists yet.
        unsafe { std::env::remove_var(var) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_is_the_repo_location_variables_and_nothing_else() {
        // it fails by narrowing: a variable dropped from the list goes on being
        // inherited and nothing reports it. So the count is the assertion that
        // does the work, and the names are what say which seven.
        assert_eq!(GIT_REPO_ENV.len(), 7, "the set changed without a decision");
        for want in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            assert!(GIT_REPO_ENV.contains(&want), "{want} is no longer scrubbed");
        }
    }

    #[test]
    fn the_authoring_variables_survive() {
        // the control, and the reason this is a list rather than a `GIT_*`
        // sweep: scrubbing the author and committer would rewrite who a commit
        // came from.
        for keep in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "GIT_CONFIG_PARAMETERS",
            "GIT_SSH_COMMAND",
        ] {
            assert!(!GIT_REPO_ENV.contains(&keep), "{keep} must survive");
        }
    }
}
