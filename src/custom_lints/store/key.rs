//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where the generated crate lives, the key that names the directory, and the revision it is
//! built at.
//!
//! The key hashes the repo and the one tool a build is cut to, and nothing about where a worktree
//! sits or which revisions it builds against, so every worktree and every revision of one repo is
//! one cargo package. Cargo then replaces the library in place when a revision moves, and the
//! target holds one library per package. The revision (the engine's lint-rules and the packs) is
//! carried beside the crate, where the store reads which kept libraries it has made old.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

/// Where the shared generated lint crates live, one directory per key.
///
/// Under the machine's shared cargo target directory when one is set, which is
/// where the rest of the build already is, and otherwise in the user's cache.
/// A relative target directory is read against the repo root, so the answer
/// does not move with the directory the engine happens to run in.
pub(crate) fn shared_base(repo_root: &Path) -> Result<PathBuf, String> {
    base_from(
        crate::build_dir::inherited_target_dir(),
        std::env::var_os("XDG_CACHE_HOME"),
        std::env::var_os("HOME"),
        repo_root,
    )
}

/// [`shared_base`] with the environment passed in, so it is testable without a
/// test changing the process environment under every other test.
///
/// There is no fallback to the temp directory. A directory every user can write
/// to is a place another user can plant a `Cargo.toml` that this engine then
/// builds and loads, so a machine with no home gets an error naming what to set.
fn base_from(
    target: Option<OsString>,
    xdg_cache: Option<OsString>,
    home: Option<OsString>,
    repo_root: &Path,
) -> Result<PathBuf, String> {
    if let Some(target) = target.filter(|t| !t.is_empty()) {
        let target = PathBuf::from(target);
        let target = if target.is_absolute() { target } else { repo_root.join(target) };
        return Ok(target.join("mockspace-lints"));
    }
    // The XDG specification says a relative value is invalid and to be ignored.
    let cache = xdg_cache
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            home.filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
                .filter(|p| p.is_absolute())
        });
    match cache {
        Some(cache) => Ok(cache.join("mockspace").join("lints")),
        None => {
            Err(
                "nowhere to put the generated lint crate: none of CARGO_TARGET_DIR, an absolute \
             XDG_CACHE_HOME or an absolute HOME is set. The crate is built and loaded, so it \
             cannot live in a directory every user can write to; set one of them."
                    .to_string(),
            )
        },
    }
}

/// Where the generated lint crate's manifest goes: `<base>/<key>/Cargo.toml`,
/// outside every worktree. Pure apart from reading the repo's git config.
///
/// The revisions it builds against are not in the key: a changed rev rewrites
/// this one crate, and cargo builds its library over the old one rather than
/// beside it (see [`revision`]).
///
/// `scope` is the one tool a build is cut to (`None` for the whole content),
/// and it is part of the key: a build of one tool is not the build of every
/// lint and tool, and the two must not overwrite each other's generated crate.
pub(crate) fn lints_manifest_path(base: &Path, cfg: &Config, scope: Option<&str>) -> PathBuf {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir);
    let key = lint_crate_key(&repo_identity(&cfg.repo_root), mock_rel, scope);
    base.join(format!("{key:016x}")).join("Cargo.toml")
}

/// The revision a build is made at: the engine's lint-rules dependency and the
/// packs, as cargo will be handed them, a branch already replaced by the rev of
/// its tip (see [`resolve_packs`]). A kept library of another revision of the
/// same crate is one the build has made old.
pub(crate) fn revision(lint_rules_dep: &str, packs: &[(String, String)]) -> String {
    let mut text = format!("{lint_rules_dep}\n");
    for (name, spec) in packs {
        text.push_str(&format!("{name}={spec}\n"));
    }
    format!("{:016x}", crate::pack_pin::fnv1a(text.as_bytes()))
}

/// Each pack's spec with a branch replaced by the rev of its tip.
///
/// Called once per run: the note it prints when a tip could not be read is
/// printed once, and the key and the written manifest cannot disagree about a
/// tip that moved between two reads.
pub(crate) fn resolve_packs(
    packs: &[(String, String)],
    pins: &Path,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Vec<(String, String)> {
    packs
        .iter()
        .map(|(name, spec)| {
            let said = format!("lint pack `{name}`");
            (
                name.clone(),
                crate::pack_pin::spec_for_cargo(&said, spec, pins, resolve),
            )
        })
        .collect()
}

/// FNV-1a rather than std's hasher, whose output may change between releases:
/// the key names a directory that outlives the binary that made it.
fn lint_crate_key(identity: &str, mock_rel: &Path, scope: Option<&str>) -> u64 {
    let mut text = format!("{identity}\n{}\n", mock_rel.display());
    if let Some(tool) = scope {
        text.push_str(&format!("scope={tool}\n"));
    }
    crate::pack_pin::fnv1a(text.as_bytes())
}

/// What names the repo across worktrees and clones: its `origin` url with the
/// scheme, user and `.git` taken off, else the git directory every worktree of
/// the clone shares, else the path.
pub(crate) fn repo_identity(repo_root: &Path) -> String {
    if let Some(url) = git_line(repo_root, &["config", "--get", "remote.origin.url"]) {
        return format!("remote:{}", normalise_remote(&url));
    }
    if let Some(dir) = git_line(repo_root, &[
        "rev-parse",
        "--path-format=absolute",
        "--git-common-dir",
    ]) {
        return format!("git-dir:{dir}");
    }
    format!("path:{}", repo_root.display())
}

fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .env("LC_ALL", "C")
        .current_dir(dir)
        .output()
        .ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
}

/// `https://h/o/r.git`, `ssh://git@h/o/r` and `git@h:o/r.git` are one remote.
fn normalise_remote(url: &str) -> String {
    let mut s = url.trim().to_lowercase();
    let mut had_scheme = false;
    if let Some(i) = s.find("://") {
        s = s[i + 3 ..].to_string();
        had_scheme = true;
    }
    if let Some(at) = s.find('@')
        && s.find('/').is_none_or(|slash| at < slash)
    {
        s = s[at + 1 ..].to_string();
    }
    if !had_scheme
        && let Some(colon) = s.find(':')
        && !s[.. colon].contains('/')
    {
        s.replace_range(colon ..= colon, "/");
    }
    s.trim_end_matches('/').trim_end_matches(".git").to_string()
}

/// A pack's `path` made absolute against `from`, where it was written to be
/// read from, and folded to its shortest form. Every other spec is returned as
/// it came.
pub(crate) fn absolutize_path_pack(spec: &str, from: &Path) -> String {
    let Ok(mut doc) = format!("p = {spec}").parse::<toml_edit::DocumentMut>() else {
        return spec.to_string();
    };
    let Some(table) = doc.get_mut("p").and_then(|i| i.as_inline_table_mut()) else {
        return spec.to_string();
    };
    let Some(path) = table
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        return spec.to_string();
    };
    if Path::new(&path).is_absolute() {
        return spec.to_string();
    }
    let abs = crate::build_dir::lexically_normal(&from.join(path))
        .to_string_lossy()
        .replace('\\', "/");
    table.insert("path", toml_edit::Value::from(abs));
    table.fmt();
    table.to_string().trim().to_string()
}

/// A key directory's name: sixteen lowercase hex digits, as the key is written.
/// Anything else under the base (the pack pins, a stray file) is not ours to
/// remove.
pub(crate) fn is_key_name(name: &str) -> bool {
    name.len() == 16
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a' ..= b'f').contains(&b))
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod tests;
