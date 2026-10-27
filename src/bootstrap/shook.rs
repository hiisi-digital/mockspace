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

/// The name the manifest, and so each registered entry, answers to. It leads
/// with `00-` because git-shook runs an event's entries sorted by name and stops
/// at the first refusal, and the gate is meant to refuse before any other tool's
/// entry has run: an entry named `homma` posting a status would otherwise go
/// first. Only `00-existing`, a hook git-shook kept, sorts ahead of it.
const NAME: &str = "00-mockspace";

/// Whether a recorded `core.hooksPath` is this clone's own git-shook hooks
/// directory, the exact `<common-git-dir>/shook/hooks` git-shook writes. A
/// spelling that merely ends the same way, a tracked `tools/shook/hooks` or
/// another clone's, is somebody else's key and is not claimed.
pub(crate) fn is_shook_path(repo_root: &Path, path: &str) -> bool {
    let recorded = Path::new(path.trim().trim_end_matches('/'));
    let ours = shook_dir(repo_root).join("hooks");
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    recorded.is_absolute() && canon(recorded) == canon(&ours)
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

/// Whether the gate has a manifest in this clone, whichever hooks it names.
pub(crate) fn has_manifest(repo_root: &Path) -> bool {
    manifest_path(repo_root).is_file()
}

/// Whether the gate is registered and git-shook holds the key: the manifest is
/// there, `core.hooksPath` is git-shook's, and an entry exists for every event.
/// A manifest alone is a declaration, not a gate.
pub(crate) fn is_live(repo_root: &Path, hooks_path: Option<&str>) -> bool {
    has_manifest(repo_root)
        && hooks_path.is_some_and(|p| is_shook_path(repo_root, p))
        && events().all(|e| shook_dir(repo_root).join("entries").join(e).join(NAME).is_file())
}

/// The durable hook git-shook kept as `00-existing` when it first took the key,
/// which is the gate itself and would run beside its own entry. Removed, with
/// the copy under `kept/`; a kept hook that is anybody else's stays.
fn drop_own_kept_hooks(repo_root: &Path) {
    let sd = shook_dir(repo_root);
    for event in events() {
        let entry = sd.join("entries").join(event).join("00-existing");
        let kept = sd.join("kept").join(event);
        let ours = fs::read_to_string(&kept).is_ok_and(|t| t.contains(MANAGED_MARKER));
        if ours {
            let _ = fs::remove_file(&entry);
            let _ = fs::remove_file(&kept);
        }
    }
}

/// Write the manifest and have git-shook register it. A registration that
/// fails leaves the manifest as it was before the call, so a refusal does not
/// leave a file behind for every later run to trip over.
pub(crate) fn register(repo_root: &Path, hooks_dir: &Path) -> Result<(), String> {
    let path = manifest_path(repo_root);
    let prior = fs::read(&path).ok();
    let registered = register_inner(repo_root, hooks_dir, &path);
    if registered.is_err() {
        restore(&path, prior);
    }
    registered
}

/// Put the manifest back as it was: the text it had, or absent if it had none.
pub(crate) fn restore(path: &Path, prior: Option<Vec<u8>>) {
    match prior {
        Some(text) => {
            let _ = fs::write(path, text);
        },
        None => {
            let _ = fs::remove_file(path);
        },
    }
}

fn register_inner(repo_root: &Path, hooks_dir: &Path, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    fs::write(path, manifest_text(hooks_dir))
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
    let out = Command::new("git")
        // `--only`, so the `--yes` is consent for the gate's own manifest and
        // not for a tracked `shook.toml` that arrived through a merge and has
        // not been looked at; this runs on any `cargo mock`, not on a person's
        // say-so
        .args(["shook", "install", "--yes", "--only", NAME])
        .current_dir(repo_root)
        .output()
        .map_err(|e| format!("git shook could not run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git shook install failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    drop_own_kept_hooks(repo_root);
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
