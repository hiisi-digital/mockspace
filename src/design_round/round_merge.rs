//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `cargo mock merge-round-meta`: the merge driver for a closed round's `.meta`.
//!
//! Git calls it as `merge-round-meta %O %A %B %P`: base, ours, theirs and the
//! path. A closed round writes its `.meta` exactly once, when the directory is
//! made, so a `.meta` added on both sides with different content is two rounds
//! that took the same id, which is the collision this exists to name. Git's own
//! add/add conflict on the directory reports a conflicted file and nothing about
//! why, and the repair (rename one side, on its own branch, before the merge)
//! is not something the reader of that message would guess.
//!
//! Exit 0 for a merge, 1 for a conflict; the registered command falls back to a
//! text merge past that, as the registry driver's does.

use std::process::{Command, ExitCode};

/// What the three versions of a `.meta` amount to.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Both sides hold the same text: nothing to merge.
    Same,
    /// Added on both sides with different text: two rounds, one id.
    Collision,
    /// Changed from a common base: an ordinary text merge.
    Text,
}

pub(crate) fn verdict(base: &str, ours: &str, theirs: &str) -> Verdict {
    if ours == theirs {
        Verdict::Same
    } else if base.trim().is_empty() {
        Verdict::Collision
    } else {
        Verdict::Text
    }
}

/// The round directory a `.meta` path sits in, `mock/design_rounds/<dir>/.meta`.
pub(crate) fn round_of(path: &str) -> Option<&str> {
    let mut parts = path.rsplit('/');
    (parts.next()? == ".meta").then_some(())?;
    parts.next().filter(|dir| !dir.is_empty())
}

/// The message a collision is refused with: which id, and the one command.
pub(crate) fn collision_message(path: &str) -> String {
    let dir = round_of(path).unwrap_or("<id>");
    format!(
        "round id collision: `{dir}` was closed on both branches. `{path}` is added on each \
         side with different content, so two rounds took the same id and cannot share a \
         directory.\n  rename one of them on its own branch, then merge again:\n    git merge \
         --abort\n    git switch <the branch to rename>\n    cargo mock rename-round {dir} \
         --auto-commit\n  (it renames the directory, the files that carry the id, the `round:` \
         line of `.meta` and the local `round/{dir}/*` tags, under a fresh id on the same \
         minute)"
    )
}

pub(crate) fn run(args: &[&str]) -> ExitCode {
    let [base, ours, theirs, path, ..] = args else {
        eprintln!("usage: mock merge-round-meta <base> <ours> <theirs> <path>");
        return ExitCode::from(2);
    };
    let read = |p: &&str| std::fs::read_to_string(p).unwrap_or_default();
    match verdict(&read(base), &read(ours), &read(theirs)) {
        Verdict::Same => ExitCode::SUCCESS,
        Verdict::Collision => {
            eprintln!("error: {}", collision_message(path));
            ExitCode::FAILURE
        },
        Verdict::Text => {
            let merged = Command::new("git")
                .args(["merge-file", "-L", "ours", "-L", "base", "-L", "theirs"])
                .args([ours, base, theirs])
                .status();
            match merged {
                Ok(s) if s.success() => ExitCode::SUCCESS,
                Ok(_) => ExitCode::FAILURE,
                Err(_) => ExitCode::from(2),
            }
        },
    }
}

#[cfg(test)]
#[path = "round_merge_tests.rs"]
mod tests;
