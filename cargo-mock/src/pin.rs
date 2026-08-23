//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Resolving a pinned engine to concrete build attempts.
//!
//! The pin *schema* (which `mockspace.toml` keys, the `Cargo.lock` fallback,
//! the [`Pin`] / [`Reference`] types) lives in the shared `mockspace-manifest`
//! crate, so the launcher and the engine parse it identically. This module is
//! the launcher-only half: turning a [`Pin`] into `cargo install` attempts,
//! which means resolving a branch pin to a concrete rev via `git ls-remote`
//! (cached with a TTL) and naming the pin-matched lint-rules dep.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use mockspace_manifest::{Pin, Reference};

use crate::hash::Fnv;

/// The crates.io package name of the engine.
pub const ENGINE_CRATE: &str = "mockspace";

/// A branch pin re-resolves to a concrete rev at most this often; a fresh
/// resolution within the window is reused without a network round-trip. Kept
/// short so a repo tracking a branch (e.g. `mockspace_branch = "dev"`) picks up
/// new heads within the hour, matching the launcher's self-update cadence.
const BRANCH_TTL: Duration = Duration::from_secs(60 * 60);

/// A pin resolved to concrete build attempts.
pub struct Resolved {
    /// The stable component of the cache key: `v:<version>` for a release,
    /// the concrete rev for a rev/branch pin, `tag:<t>` for a git tag.
    pub key_rev:        String,
    /// One or more `cargo install` argument lists (source selectors, package
    /// name included; `--root`/`--force` are added by the cache), tried in
    /// order until one succeeds. A `version` pin tries crates.io first, then
    /// the matching git tag.
    pub attempts:       Vec<Vec<String>>,
    /// The cargo dependency *value* for `mockspace-lint-rules`, renamed to the
    /// package `mockspace`, pinned to the same source the engine is built
    /// from. Passed to the engine so a custom-lint cdylib links the identical
    /// lint-rules and its `Box<dyn Lint>` vtables match. Always a git ref (the
    /// lint-rules crate lives in the same repo at the same tag/rev).
    pub lint_rules_dep: String,
}

/// Resolve a pin to concrete build attempts. A branch resolves to its current
/// head via `git ls-remote`, cached with a TTL; a rev, tag, or version is
/// already immutable.
pub fn resolve(pin: &Pin, cache_root: &Path) -> Result<Resolved, String> {
    let git = |sel: &[&str]| -> Vec<String> {
        let mut a = vec!["--git".to_string(), pin.url.clone()];
        a.extend(sel.iter().map(|s| s.to_string()));
        a.push(ENGINE_CRATE.to_string());
        a
    };
    // the lint-rules dep, renamed to `mockspace`, pinned by the same git ref
    // (kind = "tag" | "rev") so a lint cdylib links identical types.
    let lint_dep = |kind: &str, val: &str| -> String {
        format!(
            "{{ package = \"mockspace-lint-rules\", git = \"{}\", {kind} = \"{val}\" }}",
            pin.url
        )
    };
    match &pin.reference {
        Reference::Version(v) => {
            Ok(Resolved {
                key_rev:        format!("v:{v}"),
                attempts:       vec![
                    // crates.io release first ("maps to crates.io directly").
                    // Forward-looking: the engine is `publish = false` today, so
                    // this attempt currently fails on a cold build and falls
                    // through to the git tag below (silently, since ensure_built
                    // only reports failure when every attempt fails). It becomes
                    // the fast path once the engine publishes.
                    vec![ENGINE_CRATE.into(), "--version".into(), v.clone()],
                    // the matching git tag: works before the engine is published
                    // and for git-only consumers.
                    git(&["--tag", v]),
                ],
                lint_rules_dep: lint_dep("tag", v),
            })
        },
        Reference::Rev(r) => {
            Ok(Resolved {
                key_rev:        r.clone(),
                attempts:       vec![git(&["--rev", r])],
                lint_rules_dep: lint_dep("rev", r),
            })
        },
        Reference::Tag(t) => {
            Ok(Resolved {
                key_rev:        format!("tag:{t}"),
                attempts:       vec![git(&["--tag", t])],
                lint_rules_dep: lint_dep("tag", t),
            })
        },
        Reference::Branch(b) => {
            let sha = resolve_branch(pin, b, cache_root)?;
            Ok(Resolved {
                key_rev:        sha.clone(),
                attempts:       vec![git(&["--rev", &sha])],
                lint_rules_dep: lint_dep("rev", &sha),
            })
        },
    }
}

fn resolve_branch(pin: &Pin, branch: &str, cache_root: &Path) -> Result<String, String> {
    let cache = branch_resolution_path(cache_root, &pin.url, branch);
    if let Some(sha) = fresh_resolution(&cache) {
        return Ok(sha);
    }
    let sha = ls_remote_head(&pin.url, branch)?;
    if let Some(parent) = cache.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let now = unix_now();
    let _ = std::fs::write(&cache, format!("{now}\n{sha}\n"));
    Ok(sha)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The recorded resolution if present and younger than the TTL.
fn fresh_resolution(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let ts: u64 = lines.next()?.trim().parse().ok()?;
    let sha = lines.next()?.trim().to_string();
    if sha.is_empty() {
        return None;
    }
    if unix_now().saturating_sub(ts) <= BRANCH_TTL.as_secs() {
        Some(sha)
    } else {
        None
    }
}

pub(crate) fn ls_remote_head(url: &str, branch: &str) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(["ls-remote", url, branch])
        .output()
        .map_err(|e| format!("could not run git ls-remote: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-remote {url} {branch} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let sha = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or("")
        .to_string();
    if sha.len() < 7 {
        return Err(format!(
            "branch '{branch}' not found on {url} (ls-remote returned no rev)"
        ));
    }
    Ok(sha)
}

fn branch_resolution_path(cache_root: &Path, url: &str, branch: &str) -> std::path::PathBuf {
    let mut h = Fnv::new();
    h.write_field(url);
    h.write_field(branch);
    cache_root.join("branch-resolutions").join(h.hex())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_maps_to_cratesio_then_git_tag() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin {
            url:       mockspace_manifest::CANONICAL_URL.to_string(),
            reference: Reference::Version("0.0.0-d05".into()),
        };
        let r = resolve(&pin, dir.path()).unwrap();
        assert_eq!(r.key_rev, "v:0.0.0-d05");
        assert_eq!(r.attempts.len(), 2);
        assert_eq!(r.attempts[0], vec!["mockspace", "--version", "0.0.0-d05"]);
        assert_eq!(r.attempts[1], vec![
            "--git",
            mockspace_manifest::CANONICAL_URL,
            "--tag",
            "0.0.0-d05",
            "mockspace"
        ]);
    }

    #[test]
    fn rev_resolves_to_single_git_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin {
            url:       "u".into(),
            reference: Reference::Rev("sha1".into()),
        };
        let r = resolve(&pin, dir.path()).unwrap();
        assert_eq!(r.key_rev, "sha1");
        assert_eq!(r.attempts, vec![vec![
            "--git",
            "u",
            "--rev",
            "sha1",
            "mockspace"
        ]]);
    }

    #[test]
    fn tag_resolves_to_git_tag_only() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin {
            url:       "u".into(),
            reference: Reference::Tag("nightly".into()),
        };
        let r = resolve(&pin, dir.path()).unwrap();
        assert_eq!(r.key_rev, "tag:nightly");
        assert_eq!(r.attempts, vec![vec![
            "--git",
            "u",
            "--tag",
            "nightly",
            "mockspace"
        ]]);
    }

    #[test]
    fn branch_resolution_ttl_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = branch_resolution_path(dir.path(), "u", "dev");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let now = unix_now();
        std::fs::write(&path, format!("{now}\nfeedface99\n")).unwrap();
        assert_eq!(fresh_resolution(&path), Some("feedface99".into()));
        let old = now - BRANCH_TTL.as_secs() - 1;
        std::fs::write(&path, format!("{old}\nfeedface99\n")).unwrap();
        assert_eq!(fresh_resolution(&path), None);
    }
}
