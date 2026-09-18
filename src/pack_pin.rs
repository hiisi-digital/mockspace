//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A `[lint-crates]` pack pinned by branch, built from the branch's tip.
//!
//! Cargo resolves `branch = "dev"` once, into the generated crate's lockfile,
//! and never again unless somebody runs `cargo update` against a manifest
//! nobody thinks of as theirs. So a repository that pinned a pack by branch
//! gated on whatever commit was the tip the first time it built, for as long
//! as that lockfile lived, while a fresh clone of the same repository resolved
//! today's tip and gated on something else. Nothing reported either.
//!
//! The spec handed to cargo is rewritten here instead: the branch is resolved
//! with `git ls-remote` and written as `rev`, so the manifest names the commit
//! and cargo re-resolves whenever that commit moves. A resolution is kept
//! beside the generated crate for [`TIP_TTL`], so a run does not pay a network
//! round trip per pack every time it gates a commit.
//!
//! Where the remote cannot be reached, the last resolution is used at any age
//! and the run says how old it is. Where there has never been one, the spec
//! goes to cargo unchanged, which is what happened before this existed, and
//! the run says that too.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use toml_edit::{DocumentMut, InlineTable, Value};

/// How long a resolved tip is taken as the branch's current one.
pub(crate) const TIP_TTL: Duration = Duration::from_secs(60 * 60);

/// A branch and the repository it lives in, read out of one pack's spec.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct BranchPin {
    pub(crate) git:    String,
    pub(crate) branch: String,
}

/// What a spec was pinned to, and how the run learned it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Pinned {
    /// Not a branch pin at all: a `rev`, a `tag`, a version or a path.
    Untouched,
    /// Resolved from the remote just now, or from a resolution younger than
    /// [`TIP_TTL`].
    Current { rev: String },
    /// The remote did not answer, so an older resolution stands in.
    Stale { rev: String, age: Duration, why: String },
    /// The remote did not answer and nothing was ever resolved, so cargo gets
    /// the branch and the lockfile decides.
    Unresolved { why: String },
}

/// The branch pin a spec carries, if it carries one and nothing fixed beside it.
///
/// A spec naming `rev` or `tag` as well is left alone, since cargo refuses the
/// combination and saying so is cargo's job.
pub(crate) fn branch_pin(spec: &str) -> Option<BranchPin> {
    let table = inline_table(spec)?;
    if table.contains_key("rev") || table.contains_key("tag") {
        return None;
    }
    Some(BranchPin {
        git:    table.get("git")?.as_str()?.to_string(),
        branch: table.get("branch")?.as_str()?.to_string(),
    })
}

/// The spec with its branch replaced by `rev`, every other key kept.
pub(crate) fn with_rev(spec: &str, rev: &str) -> Option<String> {
    let mut table = inline_table(spec)?;
    table.remove("branch")?;
    table.insert("rev", Value::from(rev));
    table.fmt();
    Some(table.to_string().trim().to_string())
}

/// Resolve one pack's pin, reading and writing the cache under `cache`.
///
/// `resolve` asks the remote for a branch's tip; it is a parameter so the
/// cache and the fallbacks can be driven without a network.
pub(crate) fn pin(
    spec: &str,
    cache: &Path,
    now: SystemTime,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Pinned {
    let Some(p) = branch_pin(spec) else {
        return Pinned::Untouched;
    };
    let file = cache_file(cache, &p);
    let held = read_resolution(&file);
    if let Some((at, rev)) = &held
        && age(*at, now) <= TIP_TTL
    {
        return Pinned::Current { rev: rev.clone() };
    }
    match resolve(&p.git, &p.branch) {
        Ok(rev) => {
            write_resolution(&file, now, &rev);
            Pinned::Current { rev }
        }
        Err(why) => match held {
            Some((at, rev)) => Pinned::Stale {
                rev,
                age: age(at, now),
                why,
            },
            None => Pinned::Unresolved { why },
        },
    }
}

/// The spec cargo is handed for one pack, saying on stderr where it had to
/// fall back.
pub(crate) fn spec_for_cargo(
    name: &str,
    spec: &str,
    cache: &Path,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> String {
    match pin(spec, cache, SystemTime::now(), resolve) {
        Pinned::Untouched => spec.to_string(),
        Pinned::Current { rev } => with_rev(spec, &rev).unwrap_or_else(|| spec.to_string()),
        Pinned::Stale { rev, age, why } => {
            eprintln!(
                "mock: lint pack `{name}`: the branch tip could not be read ({why}), so the \
                 pack is built at {rev}, resolved {} minutes ago",
                age.as_secs() / 60
            );
            with_rev(spec, &rev).unwrap_or_else(|| spec.to_string())
        }
        Pinned::Unresolved { why } => {
            eprintln!(
                "mock: lint pack `{name}`: the branch tip could not be read ({why}) and was \
                 never resolved here, so the pack is built at whatever the lockfile holds"
            );
            spec.to_string()
        }
    }
}

/// The tip of `branch` on `url`, by `git ls-remote`.
///
/// The full ref, since a bare name also matches a tag of the same name and
/// which one answers first is the remote's listing order.
pub(crate) fn ls_remote_head(url: &str, branch: &str) -> Result<String, String> {
    let refspec = format!("refs/heads/{branch}");
    let out = std::process::Command::new("git")
        .args(["ls-remote", url, &refspec])
        .output()
        .map_err(|e| format!("could not run git ls-remote: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-remote {url} {refspec} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let rev = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().next())
        .unwrap_or("");
    if rev.len() != 40 || !rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("{url} has no branch `{branch}`"));
    }
    Ok(rev.to_string())
}

fn inline_table(spec: &str) -> Option<InlineTable> {
    let doc: DocumentMut = format!("p = {spec}").parse().ok()?;
    doc.get("p")?.as_inline_table().cloned()
}

fn cache_file(cache: &Path, p: &BranchPin) -> PathBuf {
    // Two fields hashed as one string with a separator neither can carry, so
    // `a`+`bc` and `ab`+`c` are two files.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{}\n{}", p.git, p.branch).hash(&mut h);
    cache.join(format!("{:016x}", h.finish()))
}

fn read_resolution(file: &Path) -> Option<(u64, String)> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut lines = text.lines();
    let at = lines.next()?.trim().parse().ok()?;
    let rev = lines.next()?.trim();
    (rev.len() == 40).then(|| (at, rev.to_string()))
}

/// Best effort: a resolution that cannot be kept costs the next run a round
/// trip and nothing else.
fn write_resolution(file: &Path, now: SystemTime, rev: &str) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, format!("{}\n{rev}\n", unix(now)));
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn age(at: u64, now: SystemTime) -> Duration {
    Duration::from_secs(unix(now).saturating_sub(at))
}

#[cfg(test)]
#[path = "pack_pin_tests.rs"]
mod tests;
