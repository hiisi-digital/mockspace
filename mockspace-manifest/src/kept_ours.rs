//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The record a merge driver leaves when it keeps one side of a generated file.
//!
//! The driver decides at merge time that a later hook will regenerate, and
//! nothing else checks afterwards that it did: the hook may be missing from a
//! cleaned `target/`, `cargo` may be off the hook's `PATH`, a launcher pinned to
//! an older engine may ignore the flags and exit 0 having generated nothing. So
//! the driver writes a file and only a regeneration the engine has verified
//! removes it. While it stands, a commit and a push are refused and the follow-up
//! hooks say the files may be stale.

/// The file's name under the git directory. Asked for with
/// `git rev-parse --git-path`, which resolves it per working tree, so a
/// linked worktree keeps its own.
pub const KEPT_OURS_FILE: &str = "mockspace-kept-ours";

/// The shell every hook that reads the record shares. `ms_kept_live` says
/// whether the record still describes something; `ms_kept_say` names it.
///
/// The first line of the file is `HEAD` when the driver first kept ours, and
/// the paths follow. A merge or rebase still going is the promise itself. After
/// it, the record holds only if that `HEAD` is a proper ancestor of this one: a
/// merge aborted, a reset, or a rebase aborted leaves the file behind with
/// nothing kept, and it is removed there rather than refusing the next commit.
pub const KEPT_OURS_SHELL: &str = r##"ms_kept="$(git rev-parse --git-path mockspace-kept-ours 2>/dev/null)"
ms_kept_live() {
    [ -f "$ms_kept" ] || return 1
    for s in MERGE_HEAD SQUASH_MSG rebase-merge rebase-apply; do
        [ -e "$(git rev-parse --git-path "$s")" ] && return 0
    done
    rec=$(head -n 1 "$ms_kept")
    if [ "$rec" != "$(git rev-parse HEAD 2>/dev/null)" ] \
        && git merge-base --is-ancestor "$rec" HEAD 2>/dev/null; then
        return 0
    fi
    rm -f "$ms_kept"
    return 1
}
ms_kept_say() {
    echo "mockspace: generated files kept from one side of a merge or rebase ($(tail -n +2 "$ms_kept" | tr '\n' ' ')) have not been regenerated since, so they may be stale. Run \`cargo mock\`, then commit what it changes." >&2
}
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shared_shell_is_valid_and_names_the_same_file_the_driver_writes() {
        assert!(KEPT_OURS_SHELL.contains(KEPT_OURS_FILE));
        let f = std::env::temp_dir().join(format!("ms_kept_{}.sh", std::process::id()));
        std::fs::write(&f, KEPT_OURS_SHELL).unwrap();
        let out = std::process::Command::new("bash").arg("-n").arg(&f).output().unwrap();
        let _ = std::fs::remove_file(&f);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}
