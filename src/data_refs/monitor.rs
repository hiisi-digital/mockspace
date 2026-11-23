//! The monitor git asks before it reads the work tree, which copies the files of each instance while
//! the index of this worktree tracks any of them.
//!
//! At a commit from before the migration the index tracks the files of an instance, skipped by the
//! sparse checkout, and the plain way back to a branch that does not track them deletes them. No
//! reference moves before that delete, and the `post-checkout` hook runs after it, so an edit made
//! there after the copy the hook took on the way in would be in no copy. git asks the program
//! `core.fsmonitor` names which files changed before it reads the index into a checkout, a switch, a
//! reset or any other command, and so before it touches the work tree; that program is this one.
//!
//! It always answers that any file may have changed (`/`), which is what git reads with no monitor
//! at all, so git looks at every file as it would without it. It runs `data-refs snapshot` only
//! where the last snapshot of this worktree found its index tracking a file of an instance, which it
//! says by a file in the worktree's git directory, so every other git command costs one `git
//! rev-parse`. A git the engine itself runs copies nothing, since the engine copies the instance
//! itself and a snapshot inside it would race it. Like the hooks, it never stops the git command.
//!
//! The setting goes where the sparse switch goes, the common config or this worktree's own where
//! per-worktree config is on, and a `core.fsmonitor` of the person's own is never written over:
//! `instantiate` names it and writes nothing, and `verify` names it.

use std::path::{Path, PathBuf};

use mockspace_store::Git;

use crate::config::Config;

/// The variable the engine sets for everything it runs, so the monitor of a git it runs copies
/// nothing.
pub const IN_ENGINE: &str = "MOCKSPACE_IN_ENGINE";

/// The file in this worktree's git directory that says its index tracks a file of an instance.
pub const TRACKED: &str = "mock-data-refs.tracked";

/// The monitor's file name in the repository's own hooks directory.
const NAME: &str = "mockspace-fsmonitor";

const MARK: &str = "# mockspace data refs: copies the files of each instance before git reads the work tree (managed, do not edit)";

/// Says that this process is the engine, for every git it runs.
///
/// # Safety
///
/// The environment is process-global; call it before any other thread exists.
pub unsafe fn mark_engine_process() {
    // SAFETY: the caller guarantees no other thread exists yet.
    unsafe { std::env::set_var(IN_ENGINE, "1") };
}

/// The monitor's text: the copy where this worktree's index tracks a file of an instance, then the
/// answer that any file may have changed, whatever the copy did.
fn text() -> String {
    format!(
        "#!/bin/sh\n{MARK}\n# Never stops the git command it runs in, and answers that any file may have changed, as git \
         reads the work tree with no monitor.\nif [ -z \"${{{IN_ENGINE}:-}}\" ] && tracked=$(git rev-parse --git-path \
         {TRACKED} 2>/dev/null) && [ -e \"$tracked\" ]; then\n    ( {} ) >&2\nfi\nprintf 'mockspace\\0/\\0'\n",
        super::hook::SNAPSHOT.trim_end().replace('\n', "\n    ")
    )
}

fn git(cfg: &Config, args: &[&str]) -> Result<String, String> {
    Git::new(cfg.repo_root.clone()).must(args, &[], None).map(|o| o.text()).map_err(|e| e.to_string())
}

fn path(cfg: &Config) -> Result<PathBuf, String> {
    let common = git(cfg, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
    Ok(PathBuf::from(common).join("hooks").join(NAME))
}

/// `core.fsmonitor` as git runs it: through a shell, so the path is quoted.
fn setting(at: &Path) -> String {
    format!("'{}'", at.to_string_lossy().replace('\'', r"'\''"))
}

/// What `core.fsmonitor` is set to, `None` where it is not.
fn current(cfg: &Config) -> Result<Option<String>, String> {
    Git::new(cfg.repo_root.clone()).config_get(&[], "core.fsmonitor").map_err(|e| e.to_string())
}

/// Whether a `core.fsmonitor` of the person's own stands where this would go.
fn foreign(cfg: &Config, at: &Path) -> Result<Option<String>, String> {
    Ok(current(cfg)?.filter(|v| !v.is_empty() && *v != setting(at)))
}

/// Writes the monitor and points `core.fsmonitor` at it, in the config `scope` names (`--worktree`
/// where per-worktree config is on). A `core.fsmonitor` of the person's own is named, and nothing is
/// written. `keep_config` keeps the config files before the setting changes.
pub(super) fn install(cfg: &Config, scope: &[&str], keep_config: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let at = path(cfg)?;
    if let Some(theirs) = foreign(cfg, &at)? {
        return Err(format!(
            "core.fsmonitor is set to {theirs}, a monitor of your own, and the copy of the files of an instance before git \
             reads the work tree needs {}; unset it (`git config --unset core.fsmonitor`) and run `cargo mock data-refs \
             instantiate`",
            at.display()
        ));
    }
    let want = text();
    if std::fs::read_to_string(&at).ok().as_deref() != Some(want.as_str()) {
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
        }
        mockspace_store::kept::keep(&at).map_err(|e| format!("keeping a copy of {} before writing over it: {e}", at.display()))?;
        mockspace_manifest::write_executable(&at, want).map_err(|e| format!("writing {}: {e}", at.display()))?;
    }
    if current(cfg)?.is_none() {
        keep_config()?;
        let value = setting(&at);
        let mut args = vec!["config"];
        args.extend_from_slice(scope);
        args.extend(["core.fsmonitor", value.as_str()]);
        git(cfg, &args)?;
    }
    Ok(())
}

/// Whether the monitor is there as this writes it and `core.fsmonitor` points at it.
pub(super) fn check(cfg: &Config) -> Result<(), String> {
    let at = path(cfg)?;
    if let Some(theirs) = foreign(cfg, &at)? {
        return Err(format!(
            "core.fsmonitor is set to {theirs}, a monitor of your own, so git deletes an edit made at a commit from before \
             the migration on the plain way back with no copy of it; see `cargo mock data-refs instantiate`"
        ));
    }
    if current(cfg)?.is_none() || std::fs::read_to_string(&at).ok().as_deref() != Some(text().as_str()) {
        return Err(format!(
            "this worktree has no monitor that copies the files of an instance before git reads the work tree ({}); `cargo \
             mock data-refs instantiate` installs it",
            at.display()
        ));
    }
    Ok(())
}

/// Says, for the monitor, whether the index of this worktree tracks a file of an instance: the file
/// [`TRACKED`] where it does, none where it does not. The file is kept in the cache before it is
/// removed.
pub(super) fn mark(cfg: &Config, tracked: bool) -> Result<(), String> {
    let file = PathBuf::from(git(cfg, &["rev-parse", "--path-format=absolute", "--git-path", TRACKED])?);
    match (tracked, file.exists()) {
        (true, false) => std::fs::write(&file, "the index of this worktree tracks a file of an instance\n")
            .map_err(|e| format!("writing {}: {e}", file.display())),
        (false, true) => {
            mockspace_store::kept::keep(&file).map_err(|e| format!("keeping a copy of {} before removing it: {e}", file.display()))?;
            std::fs::remove_file(&file).map_err(|e| format!("removing {}: {e}", file.display()))
        },
        _ => Ok(()),
    }
}
