//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The durable hooks that follow an event rather than gate it.

use crate::gate::{DISCOVERY, MANAGED_MARKER, chain_tail};
use crate::kept_ours::KEPT_OURS_SHELL;

/// The hooks that follow an event rather than gate it. They never block: a
/// merge or a rebase has already happened, so the only thing left to do at
/// `post-merge` and `post-rewrite` is bring the generated files to the merged
/// tree. `post-rewrite` is handed the rewritten commits on stdin, which the
/// repository's own hook of that name is owed.
pub const FOLLOW_UP_HOOKS: &[&str] = &["post-merge", "post-rewrite"];

/// A hook that follows an event: discover, delegate to the generated per-repo
/// hook when there is one, and otherwise hand over to the repository's own.
/// There is no block arm, since nothing here can refuse what already happened,
/// and a repo where mockspace is not initialised has nothing to follow up.
pub(crate) fn durable_follow_up_hook(name: &str, hook_version: u32) -> String {
    // post-rewrite reads the rewritten commits from stdin, and the generated hook
    // and the repository's own are each owed the same lines
    let reads_stdin = name == "post-rewrite";
    let stdin_capture = if reads_stdin { "REWRITE_STDIN=$(cat)\n" } else { "" };
    let stdin_replay = if reads_stdin { "printf '%s\\n' \"$REWRITE_STDIN\" | " } else { "" };
    let chain = chain_tail(name, stdin_replay);
    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# mockspace durable follow-up ({name}), v{hook_version}. Do not edit; rewritten on each
# `mock` run. Delegates to the generated per-repo hook when mockspace is
# initialised, and never blocks.
set -u
{stdin_capture}
{chain}
{DISCOVERY}
generated="$mockdir/target/hooks/{name}"
# the merge driver asks whether this hook would hand over to a generated one in
# this working tree, and a hook that would not is not one that regenerates
if [ -n "${{MOCKSPACE_PROBE:-}}" ]; then
    [ -x "$generated" ]
    exit $?
fi
if [ -x "$generated" ]; then
    {stdin_replay}"$generated" "$@"
    exit $?
fi

# nothing generated here to regenerate with: if the merge driver kept one side
# of a generated file on the promise that something would, say it did not
{KEPT_OURS_SHELL}if ms_kept_live; then
    ms_kept_say
fi
ms_chain "$@"
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::durable_hook;

    #[test]
    fn a_follow_up_hook_delegates_and_never_blocks() {
        for name in FOLLOW_UP_HOOKS {
            let h = durable_hook(name, 3);
            assert!(h.contains("$generated\" \"$@\""), "{name}: delegates");
            assert!(!h.contains("BLOCKED"), "{name}: nothing here can refuse");
            let f = std::env::temp_dir().join(format!("ms_gate_fu_{name}_{}.sh", std::process::id()));
            std::fs::write(&f, &h).unwrap();
            let out = std::process::Command::new("bash").arg("-n").arg(&f).output().unwrap();
            let _ = std::fs::remove_file(&f);
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    #[test]
    fn the_rewrite_hook_hands_its_stdin_to_both_hooks_it_delegates_to() {
        let h = durable_hook("post-rewrite", 3);
        assert!(h.contains("REWRITE_STDIN=$(cat)"), "reads the rewritten commits once");
        assert!(h.contains("\"$REWRITE_STDIN\" | \"$generated\""), "the generated hook is owed them");
        assert!(h.contains("\"$REWRITE_STDIN\" | \"$own\""), "and so is the repository's own");
        assert!(!durable_hook("post-merge", 3).contains("REWRITE_STDIN"), "post-merge has none");
    }


    #[test]
    fn a_probe_answers_from_the_mock_directory_this_working_tree_has_and_runs_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ms_probe_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("linux")).unwrap();
        let sh = |args: &[&str]| {
            let out = std::process::Command::new("git").args(args).current_dir(&dir).output().unwrap();
            assert!(out.status.success());
        };
        sh(&["init", "-q"]);
        // the mock directory here is `linux/`, which no string kept elsewhere knows
        std::fs::write(dir.join("linux/mockspace.toml"), "project_name = \"x\"\n").unwrap();
        // the repository's own hook must not run on a probe
        let own = dir.join(".git/hooks/post-merge");
        std::fs::write(&own, "#!/bin/sh\ntouch ran-own\n").unwrap();
        std::fs::set_permissions(&own, std::fs::Permissions::from_mode(0o755)).unwrap();
        let hook = dir.join("hook");
        std::fs::write(&hook, durable_follow_up_hook("post-merge", 3)).unwrap();
        let probe = || {
            std::process::Command::new("bash")
                .arg(&hook)
                .current_dir(&dir)
                .env("MOCKSPACE_PROBE", "1")
                .env_remove("MOCK_ROOT")
                .status()
                .unwrap()
                .code()
        };
        assert_eq!(probe(), Some(1), "no generated hook under linux/target/hooks");
        std::fs::create_dir_all(dir.join("mock/target/hooks")).unwrap();
        std::fs::write(dir.join("mock/target/hooks/post-merge"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(
            dir.join("mock/target/hooks/post-merge"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(probe(), Some(1), "a leftover under another directory does not count");
        std::fs::create_dir_all(dir.join("linux/target/hooks")).unwrap();
        std::fs::write(dir.join("linux/target/hooks/post-merge"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(
            dir.join("linux/target/hooks/post-merge"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(probe(), Some(0), "this working tree would hand over");
        assert!(!dir.join("ran-own").exists(), "a probe runs no hook");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
