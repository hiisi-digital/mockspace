//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The durable gate's script bodies and installer, shared by the launcher and
//! the engine.
//!
//! # Why the launcher needs these
//!
//! The gate used to be installed only by the engine, on invocation. That leaves a
//! window nothing covers: if the *engine* cannot run, nothing is installed. And
//! the engine has real reasons not to run. Its build can fail on a bad pin, on a
//! network outage, or on a compile error in the pinned revision; and it can fail
//! on the repo's own contents, which is not hypothetical, since a workspace with
//! no members made `mock` exit non-zero before it reached any setup.
//!
//! In every one of those cases the repo is left with no gate at all, silently, and
//! the next commit goes unchecked. The launcher runs *before* the engine and
//! cannot fail for any of those reasons, so it plants the gate first. The engine
//! then keeps it current.
//!
//! Living in this crate rather than in either binary is what makes that possible:
//! both already depend on it for the pin schema, and a second copy of the script
//! in the launcher is exactly the drift this arc has been removing.
//!
//! # What the gate does
//!
//! Nothing, by itself. It resolves the repo, then either delegates to the
//! generated per-repo hook or blocks at the configured scope. It carries no
//! policy, because it is machine-global (one body serves every repo on this
//! version) while policy is per-repo.

use std::path::{Path, PathBuf};

/// The gate's version, keying the durable hooks directory.
///
/// Lives here because the launcher and the engine both install and read the same
/// hooks. Two copies of this number is how a repo ends up with hooks written by
/// one era and wired by another, so there is exactly one.
///
/// Bumping it makes every existing install regenerate, which is why the bump is
/// held until a change is ready to ship rather than done as it is developed.
pub const HOOK_VERSION: u32 = 3;

/// The hooks a repo's gate consists of.
pub const HOOK_NAMES: &[&str] = &["pre-commit", "pre-push", "commit-msg"];

/// Marks a file as generated, so a reader knows not to edit it and the installer
/// knows it may overwrite it.
pub const MANAGED_MARKER: &str = "# mockspace-managed";

/// The durable hooks directory for a given hook version.
///
/// Under the user config home, so it survives a `target/` clean. Version-keyed so
/// several mockspace versions coexist without clobbering each other.
#[must_use]
pub fn durable_hooks_dir_from(
    xdg: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    hook_version: u32,
) -> Option<PathBuf> {
    let base = xdg
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))?;
    Some(
        base.join("mockspace")
            .join(format!("hooks-v{hook_version}")),
    )
}

/// The durable hooks directory, resolved from the process environment.
#[must_use]
pub fn durable_hooks_dir(hook_version: u32) -> Option<PathBuf> {
    durable_hooks_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        hook_version,
    )
}

/// The one shell implementation of mockspace-project discovery.
///
/// Resolves `root`, `cfg`, `mockdir`, `cfgrel` and `mockrel`, then exits 0 when
/// the repo is not a mockspace project at all. Also defines `ms_read_key`, which
/// reads a top-level scalar from a `mockspace.toml`.
///
/// Mirrors the launcher's `discover::locate`, including the single-config rule: a
/// repo has exactly one `mockspace.toml`, and more than one is a hard error.
pub const DISCOVERY: &str = r##"# resolve the repo root (MOCK_ROOT wins, else the .git ancestor)
root="${MOCK_ROOT:-}"
if [ -z "$root" ] || [ ! -d "$root" ]; then
    root=$(git rev-parse --show-toplevel 2>/dev/null) || {
        echo "mockspace gate: not inside a git repository." >&2
        exit 1
    }
fi

# Read a top-level scalar key from a mockspace.toml. $1 = file, $2 = key.
# Stops at the first table header, so a same-named key inside a [table] is not
# mistaken for the top-level one.
ms_read_key() {
    [ -f "$1" ] || return 0
    awk -v key="$2" '
        /^[[:space:]]*\[/ { exit }
        $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            sub(/^[^=]*=[[:space:]]*/, ""); gsub(/"/, ""); sub(/[[:space:]].*$/, "");
            print; exit
        }' "$1" 2>/dev/null
}

# Locate the one mockspace.toml + the mock dir (root first, then subdirs).
cfg=""; mockdir=""; cfgcount=0; cfglist=""
if [ -f "$root/mockspace.toml" ]; then
    cfgcount=$((cfgcount + 1)); cfglist="${cfglist}  $root/mockspace.toml
"
    cfg="$root/mockspace.toml"
    md=$(ms_read_key "$cfg" mock_dir); [ -z "$md" ] && md="mock"
    mockdir="$root/$md"
fi
for d in "$root"/.*/ "$root"/*/; do
    [ -d "$d" ] || continue
    base=$(basename "$d")
    case "$base" in .|..|.git|target|node_modules) continue ;; esac
    if [ -f "${d}mockspace.toml" ]; then
        cfgcount=$((cfgcount + 1)); cfglist="${cfglist}  ${d}mockspace.toml
"
        if [ -z "$cfg" ]; then
            cfg="${d}mockspace.toml"
            md=$(ms_read_key "$cfg" mock_dir); [ -z "$md" ] && md="."
            if [ "$md" = "." ]; then mockdir="${d%/}"; else mockdir="${d%/}/$md"; fi
        fi
    fi
done
if [ "$cfgcount" -gt 1 ]; then
    echo "mockspace gate: found more than one mockspace.toml; a repo must have exactly one. Remove the extras, keep one:" >&2
    printf '%s' "$cfglist" >&2
    exit 1
fi

# Not a mockspace project: the gate governs nothing here.
[ -z "$cfg" ] && exit 0

mockrel="${mockdir#"$root"/}"
cfgrel="${cfg#"$root"/}"
"##;

/// The full script for one durable hook.
///
/// Discover, then delegate to the generated per-repo hook when one exists, or
/// block at the repo's configured `uninitialised_blocks` scope when it does not.
#[must_use]
pub fn durable_hook(name: &str, hook_version: u32) -> String {
    let stdin_capture = if name == "pre-push" { "PREPUSH_STDIN=$(cat)\n" } else { "" };
    let stdin_replay = if name == "pre-push" { "printf '%s\\n' \"$PREPUSH_STDIN\" | " } else { "" };
    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# mockspace durable gate ({name}), v{hook_version}. Do not edit; rewritten on each
# `mock` run. core.hooksPath points here: invisible to the repo, and it survives a
# target/ clean. Carries no policy: it delegates to the generated per-repo hook
# when mockspace is initialised, and blocks at the configured scope when it is not.
set -u
{stdin_capture}
{DISCOVERY}
# --- initialised? then the generated per-repo hook owns this ---
generated="$mockdir/target/hooks/{name}"
if [ -x "$generated" ]; then
    {stdin_replay}"$generated" "$@"
    exit $?
fi

# --- not initialised: block at the configured scope ---
scope=$(ms_read_key "$cfg" uninitialised_blocks)
[ -z "$scope" ] && scope="surface"

if [ "$scope" != "all" ]; then
    # `surface` scope: only changes to the design surface are the gate's
    # business. Work elsewhere in the repo passes untouched.
    surface=""
    [ -n "$mockrel" ] && surface=$(git diff --cached --name-only -- "$mockrel" 2>/dev/null || true)
    if [ -z "$surface" ] && [ -n "$cfgrel" ]; then
        surface=$(git diff --cached --name-only -- "$cfgrel" 2>/dev/null || true)
    fi
    [ -z "$surface" ] && exit 0
fi

echo "" >&2
echo "BLOCKED: mockspace is not initialised in this repo, so the {name} gate cannot run." >&2
echo "" >&2
echo "  expected the generated hook at:" >&2
echo "    $generated" >&2
echo "" >&2
if [ "$scope" = "all" ]; then
    echo "  scope: all (uninitialised_blocks = \"all\"), so every commit and push" >&2
    echo "  is blocked until mockspace is initialised." >&2
else
    echo "  scope: surface (the default), so this is blocked because it changes" >&2
    echo "  ${{mockrel:-mock}} or the mockspace config. Work outside that passes." >&2
fi
echo "" >&2
echo "  to initialise:  mock" >&2
echo "  if the launcher is missing:  cargo install cargo-mock" >&2
exit 1
"##
    )
}

/// A stable fingerprint of a script, so an unchanged hook is not rewritten.
#[must_use]
pub fn fingerprint(content: &str) -> u64 {
    // FNV-1a: vendored rather than pulled in, since this crate is deliberately
    // dependency-light and the hash only needs to be stable, not strong.
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in content.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Write the durable hooks, skipping any already current.
///
/// Idempotent by fingerprint. Returns the actions taken, for reporting. Every
/// failure is reported rather than raised: a gate that cannot be written is worth
/// telling the user about, but it must not stop the command they actually ran.
pub fn install_durable_hooks(dir: &Path, hook_version: u32) -> Vec<String> {
    let mut actions = Vec::new();
    if let Err(e) = std::fs::create_dir_all(dir) {
        actions.push(format!(
            "could not create the durable hooks dir {}: {e}",
            dir.display()
        ));
        return actions;
    }

    for name in HOOK_NAMES {
        let path = dir.join(name);
        let content = durable_hook(name, hook_version);
        let fp_line = format!(
            "{MANAGED_MARKER} v{hook_version} fp:{:016x}",
            fingerprint(&content)
        );

        if let Ok(current) = std::fs::read_to_string(&path)
            && current.contains(&fp_line)
        {
            continue;
        }

        let final_content = content.replacen(MANAGED_MARKER, &fp_line, 1);
        if let Err(e) = std::fs::write(&path, &final_content) {
            actions.push(format!("failed to write the durable {name}: {e}"));
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&path, perms);
            }
        }
        actions.push(format!("wrote the durable {name} to {}", dir.display()));
    }
    actions
}

/// The repo-location `GIT_*` variables, and dropping them.
///
/// Re-exported rather than restated: `renki` owns this because every launcher
/// built on it runs as a grandchild of a git hook and needs the same scrub, and
/// two copies of the list is how one of them quietly narrows.
///
/// Why it matters here: the engine spawns `git` with its working directory set
/// to the mock workspace rather than the repo root. With `GIT_DIR` inherited
/// and `GIT_WORK_TREE` unset, git reads that directory as the top of the work
/// tree, so every index path misses its pathspec and the changelist gates read
/// the whole tree as untracked. Found live, on a worktree commit that reported
/// all 84 doc templates as changed and untracked.
pub use renki::{GIT_REPO_ENV, sanitize_git_env};

/// Point the repo's `core.hooksPath` at `dir`, unless it already points somewhere
/// mockspace owns.
///
/// Never overrides a path outside mockspace: that would silently disable whatever
/// the user installed, and taking over another tool's hooks without asking is not
/// this gate's business.
pub fn activate(repo_root: &Path, dir: &Path) -> Vec<String> {
    let mut actions = Vec::new();
    let current = std::process::Command::new("git")
        .args(["config", "--local", "--get", "core.hooksPath"])
        .current_dir(repo_root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let want = dir.display().to_string();
    if current == want {
        return actions;
    }
    if !current.is_empty() && !current.contains("mockspace") && !current.contains("target/hooks") {
        actions.push(format!(
            "core.hooksPath points at {current}, which mockspace does not own; leaving it alone"
        ));
        return actions;
    }
    let ok = std::process::Command::new("git")
        .args(["config", "--local", "core.hooksPath", &want])
        .current_dir(repo_root)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        actions.push(format!("pointed core.hooksPath at {want}"));
    } else {
        actions.push("could not set core.hooksPath".to_string());
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hook_delegates_before_it_blocks() {
        let h = durable_hook("pre-commit", 3);
        let delegate = h.find("$generated\" \"$@\"").expect("a delegation");
        let block = h.find("BLOCKED:").expect("a block");
        assert!(
            delegate < block,
            "delegation must be attempted before blocking, or an initialised repo would be blocked"
        );
    }

    #[test]
    fn the_hook_carries_no_policy() {
        // Machine-global: one body serves every repo, so it cannot hold per-repo
        // policy. Any attribution or lint rule here would contradict the
        // configured one.
        for name in HOOK_NAMES {
            let h = durable_hook(name, 3);
            assert!(!h.to_lowercase().contains("co-authored-by"), "{name}");
            assert!(!h.contains("--lint-only"), "{name}");
        }
    }

    #[test]
    fn a_non_project_exits_zero() {
        let h = durable_hook("pre-commit", 3);
        assert!(h.contains(r#"[ -z "$cfg" ] && exit 0"#));
    }

    #[test]
    fn only_pre_push_captures_and_replays_stdin() {
        // git hands pre-push its refs on stdin; the others get none, and reading
        // stdin they will never receive would hang the hook.
        let pp = durable_hook("pre-push", 3);
        assert!(pp.contains("PREPUSH_STDIN=$(cat)"));
        assert!(pp.contains(r#"printf '%s\n' "$PREPUSH_STDIN" | "$generated""#));
        for other in ["pre-commit", "commit-msg"] {
            assert!(!durable_hook(other, 3).contains("$(cat)"), "{other}");
        }
    }

    #[test]
    fn every_hook_is_valid_bash() {
        // A syntax error fails open: the hook errors, git proceeds, nothing is
        // gated, and it still looks installed.
        for name in HOOK_NAMES {
            let h = durable_hook(name, 3);
            let f = std::env::temp_dir().join(format!("ms_gate_{name}_{}.sh", std::process::id()));
            std::fs::write(&f, &h).unwrap();
            let out = std::process::Command::new("bash")
                .arg("-n")
                .arg(&f)
                .output()
                .unwrap();
            let _ = std::fs::remove_file(&f);
            assert!(
                out.status.success(),
                "bash -n rejected the durable {name}:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    #[test]
    fn the_version_keys_the_directory() {
        let a = durable_hooks_dir_from(None, Some("/h".into()), 2).unwrap();
        let b = durable_hooks_dir_from(None, Some("/h".into()), 3).unwrap();
        assert_ne!(a, b, "versions must not share a directory");
        assert!(b.ends_with("hooks-v3"));
    }

    #[test]
    fn xdg_wins_over_home_but_only_when_set() {
        assert_eq!(
            durable_hooks_dir_from(Some("/x".into()), Some("/h".into()), 3).unwrap(),
            PathBuf::from("/x/mockspace/hooks-v3")
        );
        // an empty XDG value is not a setting
        assert_eq!(
            durable_hooks_dir_from(Some("".into()), Some("/h".into()), 3).unwrap(),
            PathBuf::from("/h/.config/mockspace/hooks-v3")
        );
        assert_eq!(durable_hooks_dir_from(None, None, 3), None);
    }

    #[test]
    fn installing_twice_writes_once() {
        let dir = std::env::temp_dir().join(format!("ms_gate_idem_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = install_durable_hooks(&dir, 3);
        let second = install_durable_hooks(&dir, 3);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(first.len(), HOOK_NAMES.len(), "first run writes every hook");
        assert!(second.is_empty(), "second run rewrites nothing: {second:?}");
    }

    #[test]
    fn the_fingerprint_changes_with_the_content() {
        assert_ne!(fingerprint("a"), fingerprint("b"));
        assert_eq!(fingerprint("same"), fingerprint("same"));
    }

    // The behaviour of `remove_var` is std's to test; the contract owned here
    // is the *set*: which variables count as repo-location. These pin it
    // without touching the process environment, which the test harness's
    // sibling threads read concurrently (env mutation under the default
    // multi-threaded harness is undefined behaviour).
}
