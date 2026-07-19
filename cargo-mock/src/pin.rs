//! Resolving which mockspace a repo runs.
//!
//! The pin is a flat top-level key in the existing `mockspace.toml` schema,
//! mirroring the existing `abi_version` scalar (not a new file, not a new
//! table):
//!
//! ```toml
//! mockspace_version = "0.0.0-d05"   # maps to the git tag AND the crates.io release
//! ```
//!
//! The released `mockspace_version` is the primary, friendly form: one string
//! that is simultaneously the git tag and the crates.io version, cut together
//! at release time, stated just like a dependency version in Cargo.toml. For
//! local development the same file also accepts explicit
//! `mockspace_rev` / `mockspace_branch` overrides against an optional
//! `mockspace_git = <url>`.
//!
//! An un-migrated repo has no `mockspace_version` yet; the launcher falls back
//! to the legacy pin (the mockspace git rev recorded in the mock workspace's
//! `Cargo.lock`), so `mock` works before `mock migrate` runs and `migrate`
//! itself can execute.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::hash::Fnv;

/// The canonical mockspace repository: the `git = <url>` default, and the
/// git-tag fallback target for a `version` pin.
pub const CANONICAL_URL: &str = "ssh://git@github.com/hiisi-digital/mockspace.git";

/// The crates.io package name of the engine.
pub const ENGINE_CRATE: &str = "mockspace";

/// A branch pin re-resolves to a concrete rev at most this often; a fresh
/// resolution within the window is reused without a network round-trip.
/// Matches the engine's proxy-freshness TTL.
const BRANCH_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Which revision of the engine to build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    /// A released version string that is both a git tag and a crates.io
    /// release. The primary form.
    Version(String),
    /// An explicit git rev (dev override).
    Rev(String),
    /// An explicit git branch (dev override); re-resolved to a rev with a TTL.
    Branch(String),
    /// An explicit git tag (dev override) that is not a crates.io release.
    Tag(String),
}

/// A resolved source for the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub url: String,
    pub reference: Reference,
}

/// A pin resolved to concrete build attempts.
pub struct Resolved {
    /// The stable component of the cache key: `v:<version>` for a release,
    /// the concrete rev for a rev/branch pin, `tag:<t>` for a git tag.
    pub key_rev: String,
    /// One or more `cargo install` argument lists (source selectors, package
    /// name included; `--root`/`--force` are added by the cache), tried in
    /// order until one succeeds. A `version` pin tries crates.io first, then
    /// the matching git tag.
    pub attempts: Vec<Vec<String>>,
    /// The cargo dependency *value* for `mockspace-lint-rules`, renamed to the
    /// package `mockspace`, pinned to the same source the engine is built
    /// from. Passed to the engine so a custom-lint cdylib links the identical
    /// lint-rules and its `Box<dyn Lint>` vtables match. Always a git ref (the
    /// lint-rules crate lives in the same repo at the same tag/rev).
    pub lint_rules_dep: String,
}

impl Pin {
    /// Read the flat `mockspace_*` pin keys from a `mockspace.toml`. Only
    /// top-level (section-less) keys count, so a `mockspace_version` inside
    /// some later `[table]` is not mistaken for the pin. Hand-parsed rather
    /// than pulling a TOML parser into the published launcher.
    ///
    /// Precedence when several keys are present (only one is expected): an
    /// explicit `mockspace_rev` (immutable dev pin) beats `mockspace_tag`,
    /// which beats the released `mockspace_version`, which beats the fluid
    /// `mockspace_branch`.
    pub fn from_mockspace_toml(toml: &str) -> Option<Pin> {
        let mut in_top_level = true;
        let mut url: Option<String> = None;
        let mut version: Option<String> = None;
        let mut rev: Option<String> = None;
        let mut branch: Option<String> = None;
        let mut tag: Option<String> = None;
        for line in toml.lines() {
            let t = strip_comment(line).trim();
            if section_header(t).is_some() {
                in_top_level = false; // pin keys are top-level only
                continue;
            }
            if !in_top_level {
                continue;
            }
            if let Some((k, v)) = t.split_once('=') {
                let val = unquote(v.trim());
                match k.trim() {
                    "mockspace_git" => url = Some(val),
                    "mockspace_version" => version = Some(val),
                    "mockspace_rev" => rev = Some(val),
                    "mockspace_branch" => branch = Some(val),
                    "mockspace_tag" => tag = Some(val),
                    _ => {}
                }
            }
        }
        let url = url.unwrap_or_else(|| CANONICAL_URL.to_string());
        let nonempty = |o: Option<String>| o.filter(|s| !s.is_empty());
        let reference = if let Some(r) = nonempty(rev) {
            Reference::Rev(r)
        } else if let Some(t) = nonempty(tag) {
            Reference::Tag(t)
        } else if let Some(v) = nonempty(version) {
            Reference::Version(v)
        } else {
            // no explicit rev/tag/version: a branch is the last accepted form,
            // and its absence means there is no mockspace_* pin key at all.
            Reference::Branch(nonempty(branch)?)
        };
        Some(Pin { url, reference })
    }

    /// The legacy pin: the mockspace git rev recorded in the mock workspace's
    /// `Cargo.lock`, paired with the canonical URL. Lets the launcher run in a
    /// repo that has not been migrated to an explicit `[mockspace]` block yet.
    pub fn from_legacy_lock(lock: &str) -> Option<Pin> {
        let source = mockspace_source_in_lock(lock)?;
        // `git+<url>[?query]#<rev>`
        let locator = source.strip_prefix("git+").unwrap_or(&source);
        let (url_part, rev) = locator.rsplit_once('#')?;
        let url = url_part.split('?').next().unwrap_or(url_part).to_string();
        Some(Pin {
            url,
            reference: Reference::Rev(rev.to_string()),
        })
    }

    /// Resolve to concrete build attempts. A branch resolves to its current
    /// head via `git ls-remote`, cached with a TTL; a rev, tag, or version is
    /// already immutable.
    pub fn resolve(&self, cache_root: &Path) -> Result<Resolved, String> {
        let git = |sel: &[&str]| -> Vec<String> {
            let mut a = vec!["--git".to_string(), self.url.clone()];
            a.extend(sel.iter().map(|s| s.to_string()));
            a.push(ENGINE_CRATE.to_string());
            a
        };
        // the lint-rules dep, renamed to `mockspace`, pinned by the same git
        // ref (kind = "tag" | "rev") so a lint cdylib links identical types.
        let lint_dep = |kind: &str, val: &str| -> String {
            format!(
                "{{ package = \"mockspace-lint-rules\", git = \"{}\", {kind} = \"{val}\" }}",
                self.url
            )
        };
        match &self.reference {
            Reference::Version(v) => Ok(Resolved {
                key_rev: format!("v:{v}"),
                attempts: vec![
                    // crates.io release first ("maps to crates.io directly").
                    vec![ENGINE_CRATE.into(), "--version".into(), v.clone()],
                    // then the matching git tag, so it works before the engine
                    // is published (and for git-only consumers).
                    git(&["--tag", v]),
                ],
                lint_rules_dep: lint_dep("tag", v),
            }),
            Reference::Rev(r) => Ok(Resolved {
                key_rev: r.clone(),
                attempts: vec![git(&["--rev", r])],
                lint_rules_dep: lint_dep("rev", r),
            }),
            Reference::Tag(t) => Ok(Resolved {
                key_rev: format!("tag:{t}"),
                attempts: vec![git(&["--tag", t])],
                lint_rules_dep: lint_dep("tag", t),
            }),
            Reference::Branch(b) => {
                let sha = self.resolve_branch(b, cache_root)?;
                Ok(Resolved {
                    key_rev: sha.clone(),
                    attempts: vec![git(&["--rev", &sha])],
                    lint_rules_dep: lint_dep("rev", &sha),
                })
            }
        }
    }

    fn resolve_branch(&self, branch: &str, cache_root: &Path) -> Result<String, String> {
        let cache = branch_resolution_path(cache_root, &self.url, branch);
        if let Some(sha) = fresh_resolution(&cache) {
            return Ok(sha);
        }
        let sha = ls_remote_head(&self.url, branch)?;
        if let Some(parent) = cache.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let now = unix_now();
        let _ = std::fs::write(&cache, format!("{now}\n{sha}\n"));
        Ok(sha)
    }
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

fn ls_remote_head(url: &str, branch: &str) -> Result<String, String> {
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

/// The `source = "..."` of the `[[package]] name = "mockspace"` entry.
fn mockspace_source_in_lock(lock: &str) -> Option<String> {
    let mut in_mockspace = false;
    for line in lock.lines() {
        let t = line.trim();
        if t == "[[package]]" {
            in_mockspace = false;
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            let (k, v) = (k.trim(), unquote(v.trim()));
            match k {
                "name" => in_mockspace = v == "mockspace",
                "source" if in_mockspace => return Some(v),
                _ => {}
            }
        }
    }
    None
}

fn section_header(line: &str) -> Option<&str> {
    let line = line.strip_prefix('[')?;
    if line.starts_with('[') {
        return None;
    }
    let end = line.find(']')?;
    Some(line[..end].trim())
}

fn strip_comment(line: &str) -> &str {
    // good enough for our own controlled files: a `#` preceded by space and
    // not inside the quoted value. Values are simple version strings, URLs,
    // and shas without embedded `#`.
    match line.find(" #") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_pin_default_url() {
        let toml = "project_name = \"x\"\nmockspace_version = \"0.0.0-d05\"\n";
        let pin = Pin::from_mockspace_toml(toml).unwrap();
        assert_eq!(pin.url, CANONICAL_URL);
        assert_eq!(pin.reference, Reference::Version("0.0.0-d05".into()));
    }

    #[test]
    fn version_maps_to_cratesio_then_git_tag() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin::from_mockspace_toml("mockspace_version = \"0.0.0-d05\"\n").unwrap();
        let r = pin.resolve(dir.path()).unwrap();
        assert_eq!(r.key_rev, "v:0.0.0-d05");
        assert_eq!(r.attempts.len(), 2);
        // crates.io attempt first
        assert_eq!(r.attempts[0], vec!["mockspace", "--version", "0.0.0-d05"]);
        // git tag fallback
        assert_eq!(
            r.attempts[1],
            vec!["--git", CANONICAL_URL, "--tag", "0.0.0-d05", "mockspace"]
        );
    }

    #[test]
    fn parses_rev_pin() {
        let toml = "mockspace_git = \"ssh://git@example/x.git\"\nmockspace_rev = \"abc123\"\n";
        let pin = Pin::from_mockspace_toml(toml).unwrap();
        assert_eq!(pin.url, "ssh://git@example/x.git");
        assert_eq!(pin.reference, Reference::Rev("abc123".into()));
    }

    #[test]
    fn rev_overrides_version() {
        let toml = "mockspace_version = \"0.0.0-d05\"\nmockspace_rev = \"abc\"\n";
        let pin = Pin::from_mockspace_toml(toml).unwrap();
        assert_eq!(pin.reference, Reference::Rev("abc".into()));
    }

    #[test]
    fn parses_branch_pin() {
        let pin = Pin::from_mockspace_toml("mockspace_branch = \"dev\"\n").unwrap();
        assert_eq!(pin.reference, Reference::Branch("dev".into()));
    }

    #[test]
    fn no_pin_key_is_not_a_pin() {
        assert!(Pin::from_mockspace_toml("project_name = \"x\"\n").is_none());
        // a mockspace_version inside a later table is not the top-level pin
        assert!(Pin::from_mockspace_toml("[other]\nmockspace_version=\"1\"\n").is_none());
    }

    #[test]
    fn only_top_level_keys_count() {
        let toml = "mockspace_version = \"0.0.0-d05\"\n[table]\nmockspace_version = \"9\"\n";
        let pin = Pin::from_mockspace_toml(toml).unwrap();
        assert_eq!(pin.reference, Reference::Version("0.0.0-d05".into()));
    }

    #[test]
    fn legacy_lock_rev() {
        let lock = "\
[[package]]
name = \"other\"
version = \"1.0\"

[[package]]
name = \"mockspace\"
version = \"0.1.0\"
source = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#deadbeef1234\"
";
        let pin = Pin::from_legacy_lock(lock).unwrap();
        assert_eq!(pin.url, "ssh://git@github.com/hiisi-digital/mockspace.git");
        assert_eq!(pin.reference, Reference::Rev("deadbeef1234".into()));
    }

    #[test]
    fn rev_resolves_to_single_git_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin {
            url: "u".into(),
            reference: Reference::Rev("sha1".into()),
        };
        let r = pin.resolve(dir.path()).unwrap();
        assert_eq!(r.key_rev, "sha1");
        assert_eq!(
            r.attempts,
            vec![vec!["--git", "u", "--rev", "sha1", "mockspace"]]
        );
    }

    #[test]
    fn tag_resolves_to_git_tag_only() {
        let dir = tempfile::tempdir().unwrap();
        let pin = Pin {
            url: "u".into(),
            reference: Reference::Tag("nightly".into()),
        };
        let r = pin.resolve(dir.path()).unwrap();
        assert_eq!(r.key_rev, "tag:nightly");
        assert_eq!(
            r.attempts,
            vec![vec!["--git", "u", "--tag", "nightly", "mockspace"]]
        );
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
