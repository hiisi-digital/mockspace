//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The gate registered with git-shook instead of taking `core.hooksPath`.
//!
//! Where a repository's `core.hooksPath` is git-shook's, the key is already
//! somebody's dispatcher, and replacing it would drop every other tool chained
//! on it. The gate then goes in as one more entry: a manifest in the clone's own
//! `shook/manifests/` directory naming the durable hooks, which git-shook reads
//! beside the tracked ones. The manifest sits in the git directory because the
//! durable hooks' absolute path is per machine and a tracked `shook.toml` would
//! carry one machine's home directory to every other.

use mockspace_manifest::gate::{FOLLOW_UP_HOOKS, HOOK_NAMES};

use super::*;

/// The name the manifest, and so each registered entry, answers to.
const NAME: &str = "mockspace";

/// Whether a recorded `core.hooksPath` is git-shook's own hooks directory.
pub(crate) fn is_shook_path(path: &str) -> bool {
    path.trim().trim_end_matches('/').ends_with("shook/hooks")
}

fn shook_dir(repo_root: &Path) -> PathBuf {
    resolve_git_dir(repo_root).join("shook")
}

fn manifest_path(repo_root: &Path) -> PathBuf {
    shook_dir(repo_root).join("manifests").join(format!("{NAME}.toml"))
}

fn events() -> impl Iterator<Item = &'static str> {
    HOOK_NAMES.iter().chain(FOLLOW_UP_HOOKS).copied()
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The manifest that runs each durable hook in `hooks_dir` at its event. Git's
/// own arguments are appended by the dispatcher, which is what the hooks read.
pub(crate) fn manifest_text(hooks_dir: &Path) -> String {
    let mut out = format!("name = {}\n", quote(NAME));
    for event in events() {
        let hook = hooks_dir.join(event);
        out.push_str(&format!(
            "\n[hooks.{event}]\nrun = [{}]\n",
            quote(&hook.to_string_lossy())
        ));
    }
    out
}

/// Whether the gate is registered with git-shook and current for `hooks_dir`.
pub(crate) fn is_registered(repo_root: &Path, hooks_dir: &Path) -> bool {
    let manifest_current = fs::read_to_string(manifest_path(repo_root))
        .map(|t| t == manifest_text(hooks_dir))
        .unwrap_or(false);
    manifest_current
        && events().all(|e| shook_dir(repo_root).join("entries").join(e).join(NAME).is_file())
}

/// Whether the gate is registered at all, whichever hooks it names.
pub(crate) fn has_manifest(repo_root: &Path) -> bool {
    manifest_path(repo_root).is_file()
}

/// Write the manifest and have git-shook register it.
pub(crate) fn register(repo_root: &Path, hooks_dir: &Path) -> Result<(), String> {
    let path = manifest_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    fs::write(&path, manifest_text(hooks_dir))
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    let out = Command::new("git")
        .args(["shook", "install", "--yes"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git shook could not run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git shook install failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    if !is_registered(repo_root, hooks_dir) {
        return Err(format!(
            "git shook install ran and the gate is still not registered; see `git shook doctor`"
        ));
    }
    Ok(())
}

/// Take the gate out of git-shook: the manifest and the entries it registered.
/// Everything else registered stays, which is the point of being an entry.
pub(crate) fn unregister(repo_root: &Path) -> Result<(), String> {
    let path = manifest_path(repo_root);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("could not remove {}: {e}", path.display()))?;
    }
    let entries = shook_dir(repo_root).join("entries");
    for event in events() {
        let entry = entries.join(event).join(NAME);
        if entry.exists() {
            fs::remove_file(&entry).map_err(|e| format!("could not remove {}: {e}", entry.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "shook_tests.rs"]
mod tests;
