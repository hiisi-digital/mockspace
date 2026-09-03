//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The repo-side gate: durable hooks, activation, and custom-lint discovery.
//!
//! The launcher (`mock` / `cargo-mock`) is the sole entry to mockspace; it
//! plants the gate before the engine builds, and the engine keeps it current
//! through [`ensure_gate`]. The `build.rs` bootstrap, the `.cargo` alias and
//! the generated proxy crate are gone; [`bootstrap_from_buildscript`] remains
//! only as a tombstone that fails the build with the migration steps.
//!
//! # Hook model
//!
//! mockspace never touches `.git/hooks/`. Those are the repository's own
//! hooks, whatever tool put them there, and they always run: with or without
//! mockspace. A generated hook runs mockspace validation first and then the
//! repository's `.git/hooks/<name>`, with the same arguments and the same
//! stdin, and never past a refusal; the durable gate does the same on the
//! passes it takes itself. Activation is explicit and reversible
//! (`cargo mock activate` / `deactivate` drive `core.hooksPath`);
//! deactivated, git falls back to the repository's hooks as if mockspace was
//! never there.
//!
//! # Custom lints
//!
//! In-tree lint files under `{mock_dir}/lints/` and external packs declared
//! under `[lint-crates]` in `mockspace.toml` are discovered here and compiled
//! by the engine into one cdylib (see `custom_lints`).
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::{env, fs};

mod gitignore;
pub(crate) use gitignore::*;
mod activate;
pub use activate::{activate, deactivate, is_active};
mod lints;
pub(crate) use lints::*;
mod hooks;
pub(crate) use hooks::*;
mod durable;
pub(crate) use durable::*;
#[cfg(test)]
mod gitignore_tests;
#[cfg(test)]
mod lint_crates_tests;

/// Marker in generated hooks for identification and versioning.
const MANAGED_MARKER: &str = "# mockspace-managed";

use mockspace_manifest::gate::HOOK_VERSION;

/// Hook names that mockspace generates.
const HOOK_NAMES: &[&str] = &["pre-commit", "pre-push", "commit-msg"];

// ──────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────

/// The retired `build.rs` bootstrap entry, kept only to fail with guidance.
///
/// The launcher is the sole entry now. A silent no-op here would leave a
/// repo believing it is gated while nothing installed anything, so this
/// fails the build and names the migration. Per the anomalous-state rule:
/// error, inform, guide; never silently degrade.
pub fn bootstrap_from_buildscript() {
    eprintln!(
        "mockspace: the build.rs bootstrap is removed. Migrate to the launcher:\n\
         \n\
         1. install it once per machine:\n\
            cargo install --git https://github.com/hiisi-digital/mockspace.git cargo-mock\n\
         2. pin the engine in the repo's root mockspace.toml:\n\
            mockspace_version = \"<release>\"   # or mockspace_branch = \"dev\"\n\
         3. delete the build.rs call to bootstrap_from_buildscript and the\n\
            [build-dependencies] mockspace entry that carried it, and remove any\n\
            legacy `mock = ...` alias from .cargo/config.toml\n\
         \n\
         then run `mock` from the repo; the launcher plants the gate itself."
    );
    std::process::exit(1);
}

/// The slim, launcher-era setup the engine runs on a normal invocation,
/// replacing what the `build.rs` bootstrap did: write the generated
/// validator, ensure the durable hooks that delegate to it, point
/// `core.hooksPath` (with no user involvement), and keep `target/`
/// gitignored. None of the dissolved plumbing (proxy, `.cargo` alias,
/// launcher self-install, build-cache guard).
///
/// Best-effort and quiet: setup failures never block the command the user
/// actually ran. Idempotent and cheap on the common path (fingerprint-guarded
/// hook writes, a single `git config` read).
pub fn ensure_gate(repo_root: &Path, mock_dir: &Path) {
    let mut actions = Vec::new();

    // The engine is the generated validator's only writer now that the
    // build.rs bootstrap is gone. The durable hook delegates to this
    // validator and blocks while it is missing, telling the user to run
    // `cargo mock`; if that run did not materialise the validator, the
    // guidance would loop forever.
    ensure_generated_hooks(repo_root, mock_dir, &mut actions);

    // Opt-out for CI and sandboxed environments where git config edits are
    // unwanted. The hooks above still get written (they are inert files) and
    // the gitignore stays maintained; only the `git config` activation is
    // skipped.
    if std::env::var("MOCKSPACE_NO_AUTO_ACTIVATE").is_ok() {
        ensure_gitignore(repo_root, &mut actions);
        return;
    }

    let target = hooks_path_target(mock_dir);
    let want = target.to_string_lossy();
    let current = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--local", "core.hooksPath"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if current.as_deref() != Some(want.as_ref()) {
        // not pointing at the durable gate yet: activate (also (re)writes the
        // durable hooks and records the mock-dir locator).
        let _ = activate(repo_root, mock_dir);
    } else {
        // already active: just keep the hook content current.
        ensure_durable_hooks(&mut actions);
    }
    ensure_gitignore(repo_root, &mut actions);
}

/// Where `core.hooksPath` should point: the durable home dir when a
/// config home exists, otherwise the generated (non-durable) dir. The
/// single source of truth, so `activate` and `ensure_gate` agree.
fn hooks_path_target(mock_dir: &Path) -> PathBuf {
    // The durable dir only when it actually exists (was created by
    // `ensure_durable_hooks`). Resolvable-but-unwritable must fall back to
    // the generated dir, and `activate` must make the same choice, or
    // `ensure_gate` would re-activate every run chasing a target `activate`
    // never set.
    match durable_hooks_dir() {
        Some(d) if d.exists() => d,
        _ => generated_hooks_dir(mock_dir),
    }
}

/// The common git directory: `.git` in a clone, and in a linked worktree the
/// clone's `.git` that the worktree's own `.git/worktrees/<name>` points back
/// at through its `commondir` file. The hooks the generated scripts chain to
/// live there, since git reads a worktree's hooks from the common directory,
/// and the worktree's own has none.
pub(crate) fn resolve_git_dir(repo_root: &Path) -> PathBuf {
    let git_path = repo_root.join(".git");
    if git_path.is_file() {
        // Worktree: .git file contains "gitdir: <path>"
        if let Ok(content) = fs::read_to_string(&git_path) {
            if let Some(gitdir) = content.trim().strip_prefix("gitdir: ") {
                let gitdir = PathBuf::from(gitdir.trim());
                // and that directory names the common one, relative to itself
                if let Ok(common) = fs::read_to_string(gitdir.join("commondir")) {
                    let common = gitdir.join(common.trim());
                    return common.canonicalize().unwrap_or(common);
                }
                return gitdir;
            }
        }
    }
    git_path
}

#[cfg(test)]
mod git_dir_tests {
    use super::*;

    fn run(cwd: &Path, args: &[&str]) {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn a_linked_worktree_resolves_to_the_clones_git_dir_where_the_hooks_are() {
        let clone = tempfile::tempdir().unwrap();
        run(clone.path(), &["init", "-q", "-b", "main"]);
        fs::write(clone.path().join("f"), "x").unwrap();
        run(clone.path(), &["add", "f"]);
        run(clone.path(), &["commit", "-qm", "one"]);
        let wt = tempfile::tempdir().unwrap();
        run(clone.path(), &[
            "worktree",
            "add",
            "-q",
            wt.path().to_str().unwrap(),
            "-b",
            "side",
        ]);
        let expect = clone.path().canonicalize().unwrap().join(".git");
        assert_eq!(resolve_git_dir(clone.path()), clone.path().join(".git"));
        let from_wt = resolve_git_dir(wt.path());
        assert_eq!(from_wt, expect, "the worktree's hooks are the clone's");
        assert!(
            !from_wt.to_string_lossy().contains("worktrees"),
            "{}",
            from_wt.display()
        );
    }
}

fn content_fingerprint(content: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}
