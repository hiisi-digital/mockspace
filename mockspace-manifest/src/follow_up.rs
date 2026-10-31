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
    // what the hook runs itself when this working tree has no generated hook yet, a fresh worktree
    // having none until a generation has run in it: the same call the generated hook makes
    let (follow_only, flag) = if reads_stdin {
        ("[ \"${1:-}\" = rebase ] || ms_chain \"$@\"", "--commit-regenerated=rebase")
    } else {
        (
            "[ \"$(git rev-list --parents -n 1 HEAD | wc -w)\" -lt 3 ] && ms_chain \"$@\"",
            "--commit-regenerated",
        )
    };
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
ms_can_generate() {{
    [ -n "$cfg" ] && command -v cargo >/dev/null 2>&1
}}
if [ -n "${{MOCKSPACE_PROBE:-}}" ]; then
    [ -x "$generated" ] || ms_can_generate
    exit $?
fi
if [ -x "$generated" ]; then
    {stdin_replay}"$generated" "$@"
    exit $?
fi

# no generated hook in this working tree: build it by regenerating, which is what the merge driver
# was promised when it kept ours, rather than leave a fresh worktree to a conflict
{KEPT_OURS_SHELL}if ms_can_generate; then
    {follow_only}
    if ! cargo mock --generate-only {flag} 2>&1; then
        echo "{name}: the generated files in this tree may be STALE, the regeneration failed. Run \`cargo mock\` and commit what it changes." >&2
    elif ms_kept_live; then
        ms_kept_say
    fi
    ms_chain "$@"
fi

# nothing to regenerate with: if the merge driver kept one side
# of a generated file on the promise that something would, say it did not
if ms_kept_live; then
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
        crate::exec::write_executable(&own, "#!/bin/sh\ntouch ran-own\n").unwrap();
        let hook = dir.join("hook");
        std::fs::write(&hook, durable_follow_up_hook("post-merge", 3)).unwrap();
        // the tools the hook reads and no `cargo`: with one, the hook can build what is missing
        let bare = tool_dir(&dir.join("bin"), false);
        let probe_on = |path: &std::ffi::OsStr| {
            std::process::Command::new("bash")
                .arg(&hook)
                .current_dir(&dir)
                .env("PATH", path)
                .env("MOCKSPACE_PROBE", "1")
                .env_remove("MOCK_ROOT")
                .status()
                .unwrap()
                .code()
        };
        let probe = || probe_on(&bare);
        assert_eq!(probe(), Some(1), "no generated hook under linux/target/hooks and no cargo to build one");
        std::fs::create_dir_all(dir.join("mock/target/hooks")).unwrap();
        crate::exec::write_executable(&dir.join("mock/target/hooks/post-merge"), "#!/bin/sh\n").unwrap();
        assert_eq!(probe(), Some(1), "a leftover under another directory does not count");
        let with_cargo = tool_dir(&dir.join("bin2"), true);
        assert_eq!(
            probe_on(&with_cargo),
            Some(0),
            "a fresh worktree with cargo would build the generated hook"
        );
        std::fs::create_dir_all(dir.join("linux/target/hooks")).unwrap();
        crate::exec::write_executable(&dir.join("linux/target/hooks/post-merge"), "#!/bin/sh\n").unwrap();
        assert_eq!(probe(), Some(0), "this working tree would hand over");
        assert!(!dir.join("ran-own").exists(), "a probe runs no hook");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A directory of the tools the hooks read, git and the coreutils, with a `cargo` that records its
    /// arguments into `cargo.log` beside it when asked. Returned as a `PATH` value.
    fn tool_dir(bin: &std::path::Path, with_cargo: bool) -> std::ffi::OsString {
        std::fs::create_dir_all(bin).unwrap();
        for tool in [
            "git", "bash", "sh", "awk", "basename", "dirname", "head", "tail", "tr", "wc", "cat", "sed", "grep",
            "env", "rm", "mkdir", "ls", "sort", "cut",
        ] {
            let found = std::process::Command::new("sh")
                .args(["-c", &format!("command -v {tool}")])
                .output()
                .unwrap();
            let real = String::from_utf8_lossy(&found.stdout).trim().to_string();
            if !real.is_empty() {
                let _ = std::os::unix::fs::symlink(real, bin.join(tool));
            }
        }
        if with_cargo {
            let log = bin.join("cargo.log");
            crate::exec::write_executable(
                &bin.join("cargo"),
                &format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display()),
            )
            .unwrap();
        }
        bin.as_os_str().to_owned()
    }

    #[test]
    fn a_worktree_with_no_generated_hook_regenerates_after_a_merge_and_after_a_rebase_only() {
        for (name, args, parents, flag, runs) in [
            ("post-merge", vec![], 2usize, "--commit-regenerated", true),
            ("post-merge", vec![], 1, "--commit-regenerated", false),
            ("post-rewrite", vec!["rebase"], 2, "--commit-regenerated=rebase", true),
            ("post-rewrite", vec!["amend"], 2, "--commit-regenerated=rebase", false),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let git = |a: &[&str]| {
                let out = std::process::Command::new("git").args(a).current_dir(root).output().unwrap();
                assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            };
            git(&["init", "-q", "-b", "dev"]);
            let commit = |m: &str| {
                git(&["-c", "user.name=t", "-c", "user.email=t@e.c", "commit", "-q", "--allow-empty", "-m", m]);
            };
            commit("one");
            if parents == 2 {
                git(&["switch", "-q", "-c", "side"]);
                commit("side");
                git(&["switch", "-q", "dev"]);
                commit("two");
                git(&["-c", "user.name=t", "-c", "user.email=t@e.c", "merge", "-q", "--no-ff", "--no-edit", "side"]);
            }
            std::fs::create_dir_all(root.join("mock")).unwrap();
            std::fs::write(root.join("mock/mockspace.toml"), "project_name = \"x\"\n").unwrap();
            let hook = root.join("hook");
            std::fs::write(&hook, durable_follow_up_hook(name, 3)).unwrap();
            let bin = tool_dir(&root.join("bin"), true);
            let status = std::process::Command::new("bash")
                .arg(&hook)
                .args(&args)
                .current_dir(root)
                .env("PATH", &bin)
                .env_remove("MOCK_ROOT")
                .stdin(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "{name} {args:?}: a follow-up never blocks");
            let log = std::fs::read_to_string(root.join("bin/cargo.log")).unwrap_or_default();
            if runs {
                assert_eq!(log.trim(), format!("mock --generate-only {flag}"), "{name} {args:?}");
            } else {
                assert_eq!(log, "", "{name} {args:?}: nothing to follow up");
            }
        }
    }
}
