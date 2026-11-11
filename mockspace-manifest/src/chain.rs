//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The hooks the gate only passes on to the repository's own.
//!
//! While `core.hooksPath` points at the gate's directory, git reads no hook of the repository's
//! own directory, so a hook that belongs to one repository (the copy of the files of a data ref's
//! instance before git writes over them) is put there and reached through a hook of the gate's
//! that runs it. The gate's directory is shared by every repository on the machine, so the hook
//! names no repository and carries no policy: it finds the repository's git directory from what
//! git gives a hook, without starting a program, and hands over. An event like
//! `reference-transaction` fires on every move of a reference in every repository that points
//! here, so the hook that finds nothing to run ends at once.

use crate::gate::{MANAGED_MARKER, SHOOK_KEEPS};

/// The events the gate passes on and does nothing else with.
pub const CHAIN_HOOKS: &[&str] = &["reference-transaction"];

/// The script of the gate's hook for `name`, one of [`CHAIN_HOOKS`].
#[must_use]
pub fn chain_hook(name: &str, hook_version: u32) -> String {
    format!(
        r##"#!/bin/sh
{MANAGED_MARKER}
# mockspace durable gate ({name}), v{hook_version}. Do not edit; rewritten on each
# `mock` run. Runs the repository's own hook, which git does not read while
# core.hooksPath points here, and does nothing else.
# git runs a hook in the top of the work tree, or in the git directory of a bare
# repository, with GIT_DIR set where that is not `.git` there.
gd=${{GIT_DIR:-.git}}
if [ -f "$gd" ]; then
    read -r _ gd < "$gd" || exit 0
fi
if [ -f "$gd/commondir" ]; then
    read -r common < "$gd/commondir" || exit 0
    case "$common" in /*) gd=$common ;; *) gd="$gd/$common" ;; esac
fi
own="$gd/hooks/{name}"
[ -x "$own" ] || exit 0
{SHOOK_KEEPS}
if [ -d "$gd/shook" ] && ms_shook_keeps "$gd" {name}; then
    exit 0
fi
exec "$own" "$@"
"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mutation: drop the hand-over; a hook of the repository's own is then never run while the gate
    // holds the key.
    #[test]
    fn the_chain_hook_hands_over_and_names_no_repository() {
        for name in CHAIN_HOOKS {
            let h = chain_hook(name, 3);
            assert!(h.contains(MANAGED_MARKER), "the installer cannot tell it is its own");
            assert!(h.contains("exec \"$own\" \"$@\""), "{h}");
            assert!(!h.contains("data-refs"), "the shared hook carries a repository's policy: {h}");
            let f = std::env::temp_dir().join(format!("ms_chain_{name}_{}.sh", std::process::id()));
            std::fs::write(&f, &h).unwrap();
            let out = std::process::Command::new("sh").arg("-n").arg(&f).output().unwrap();
            let _ = std::fs::remove_file(&f);
            assert!(out.status.success(), "sh -n rejected {name}: {}", String::from_utf8_lossy(&out.stderr));
        }
    }
}
