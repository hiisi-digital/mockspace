#![allow(unused_imports)]
use super::*;

/// Resolve the cargo home the way cargo does: `CARGO_HOME` when set,
/// else `$HOME/.cargo`. Pure over its arguments for testability.
pub(crate) fn resolve_cargo_home(
    cargo_home_env: Option<std::ffi::OsString>,
    home_env: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    cargo_home_env
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_env.map(|h| PathBuf::from(h).join(".cargo")))
}


/// Read sudo's identity handoff. `Ok(None)` when not under sudo,
/// `Ok(Some((uid, gid)))` when sudo published both ids, `Err` when the
/// sudo marker is present but the ids are missing or malformed (a state
/// the caller must treat as fatal rather than guess through).
pub(crate) fn parse_sudo_ids(
    user_env: Option<std::ffi::OsString>,
    uid_env: Option<std::ffi::OsString>,
    gid_env: Option<std::ffi::OsString>,
) -> Result<Option<(u32, u32)>, String> {
    match (uid_env, gid_env) {
        // No ids and no marker: not under sudo. No ids but the marker is
        // set: something sudo-shaped without an identity handoff, which
        // must fail closed rather than install root-owned unrepaired.
        (None, None) => {
            if user_env.is_some() {
                Err("SUDO_USER is set but SUDO_UID/SUDO_GID are absent".to_string())
            } else {
                Ok(None)
            }
        }
        (Some(uid), Some(gid)) => {
            let parse = |v: &std::ffi::OsStr, name: &str| {
                v.to_str()
                    .and_then(|s| s.parse::<u32>().ok())
                    .ok_or_else(|| format!("{name} is not a numeric id: {v:?}"))
            };
            let uid = parse(&uid, "SUDO_UID")?;
            let gid = parse(&gid, "SUDO_GID")?;
            Ok(Some((uid, gid)))
        }
        (uid, gid) => Err(format!(
            "sudo id handoff incomplete: SUDO_UID {}, SUDO_GID {}",
            if uid.is_some() { "present" } else { "missing" },
            if gid.is_some() { "present" } else { "missing" },
        )),
    }
}


/// Chown `path` and, when it is a real directory, everything under it.
///
/// No-follow throughout: a symlink planted in a bootstrap-owned dir must
/// never redirect this root chown onto an external file (2026-07 security
/// review). `lchown` chowns the link itself without dereferencing, and
/// `symlink_metadata` reports the link rather than its target, so a
/// symlink is never seen as a directory and never descended.
#[cfg(unix)]
pub(crate) fn chown_tree(path: &Path, uid: u32, gid: u32) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(path)?;
    std::os::unix::fs::lchown(path, Some(uid), Some(gid))?;
    if md.is_dir() {
        for entry in std::fs::read_dir(path)? {
            chown_tree(&entry?.path(), uid, gid)?;
        }
    }
    Ok(())
}


/// Return the bootstrap's output roots to the invoking user after a
/// sudo-run install. Covers exactly what the bootstrap writes: broad
/// consumer build trees are cargo's outputs, not ours, and stay put.
#[cfg(unix)]
pub(crate) fn repair_ownership(
    repo_root: &Path,
    mock_dir: &Path,
    uid: u32,
    gid: u32,
) -> std::io::Result<()> {
    // (path, recursive). `.git/config` is in the set because activation
    // runs `git config --local core.hooksPath`, and git rewrites the file
    // via a lock-and-rename that, under sudo, leaves it root-owned. Missing
    // it would reintroduce the exact EPERM class this repair prevents:
    // every later unprivileged `git config --local` would fail. The git
    // dir is resolved (not assumed at `.git/`) because `.git` may be a file
    // pointing elsewhere in a worktree or submodule.
    let targets: [(PathBuf, bool); 7] = [
        (repo_root.join(".cargo"), true),
        (repo_root.join("target"), false),
        (repo_root.join("target/mockspace-proxy"), true),
        (mock_dir.join("target"), false),
        (mock_dir.join("target/hooks"), true),
        (repo_root.join(".gitignore"), false),
        // one file, never .git recursively.
        (resolve_git_dir(repo_root).join("config"), false),
    ];
    for (path, recursive) in targets {
        if !path.exists() {
            continue;
        }
        if recursive {
            chown_tree(&path, uid, gid)?;
        } else {
            // lchown, not chown: never dereference a symlink at an output
            // root either (e.g. a .gitignore replaced by a link).
            std::os::unix::fs::lchown(&path, Some(uid), Some(gid))?;
        }
    }
    Ok(())
}


/// `true` when `manifest_dir` lies inside the cargo home (the shared
/// dependency cache): the build is compiling a git checkout or registry
/// copy, not a working repo, and the bootstrap must not install there.
///
/// Pure over its arguments so the guard is testable without touching
/// process env. Canonicalizes each side where the path exists and falls
/// back to the path as given, so nonexistent inputs never panic.
pub(crate) fn is_inside_cargo_home(manifest_dir: &Path, cargo_home: Option<&Path>) -> bool {
    let Some(home) = cargo_home else {
        return false;
    };
    let dir = manifest_dir
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.to_path_buf());
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    dir.starts_with(&home)
}

