//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The local delegate: a program a workspace places beside its repositories to take
//! `registry-append` over, whole.
//!
//! The file is `.mockspace/hooks/registry-append` in the repository's root or in any directory
//! above it, the nearest one winning. It is a local file and no repository tracks it: a workspace
//! puts it there when it has something of its own to do with the words an answer holds, and a
//! repository cloned anywhere else has none and writes the row itself. Nothing here names the
//! workspace tool that installs it.
//!
//! The hook is run as `registry-append <ns>::<id> <field> <value>` in the repository root with
//! `MOCKSPACE_REPO_ROOT` and `MOCKSPACE_MOCK_DIR` set. Its exit status is the answer: 0 means it
//! did the whole step and mockspace writes nothing, 3 means it declines and mockspace writes the
//! row itself, anything else is a failure and is reported with what the hook printed.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The exit status of a hook that leaves the step to mockspace.
pub(crate) const DECLINED: i32 = 3;

/// What the hook did.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    /// The hook did the whole step.
    Whole,
    /// The hook left the step to mockspace.
    Declined,
}

/// The nearest `.mockspace/hooks/registry-append` from `start` upward that is a file.
pub(crate) fn find(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|d| d.join(".mockspace/hooks/registry-append"))
        .find(|p| p.is_file())
}

/// Runs `hook` on one append.
pub(crate) fn run(
    hook: &Path,
    repo_root: &Path,
    mock_dir: &Path,
    target: &str,
    field: &str,
    value: &str,
) -> Result<Taken, String> {
    let out = Command::new(hook)
        .args([target, field, value])
        .current_dir(repo_root)
        .env("MOCKSPACE_REPO_ROOT", repo_root)
        .env("MOCKSPACE_MOCK_DIR", mock_dir)
        .output()
        .map_err(|e| format!("could not run {}: {e}", hook.display()))?;
    match out.status.code() {
        Some(0) => Ok(Taken::Whole),
        Some(DECLINED) => Ok(Taken::Declined),
        code => {
            let said = String::from_utf8_lossy(&out.stderr);
            let said = said.trim();
            Err(format!(
                "{} failed ({}){}{}",
                hook.display(),
                code.map_or_else(|| "killed by a signal".to_string(), |c| format!("status {c}")),
                if said.is_empty() { "" } else { ": " },
                said
            ))
        },
    }
}
