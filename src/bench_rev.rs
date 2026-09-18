//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! One mockspace for a whole bench run.
//!
//! A run links several crates that each reach the bench framework: the
//! generated driver, the support crates it takes by path, and one cdylib per
//! arm, loaded across FFI. Each resolves `mockspace-bench-core` out of its own
//! lockfile, and a `branch = "dev"` resolves once there and stays. So the driver
//! under `mock/target/` could sit at the commit of its first build while the
//! consumer's arms moved on, and a run died on a manifest key the older harness
//! had never heard of. The same skew with no new key reads timing fields across
//! a struct laid out two ways, which `bench_abi_hash` catches only when the
//! shape changed.
//!
//! The run is settled on one commit and every generated lockfile is moved to
//! it. Which commit is the consumer's to say, through the lockfiles it commits
//! beside its arms; arms that disagree are refused, since a run cannot link
//! both. Where no arm holds a lock, the branch's tip is taken, through the
//! same resolution the lint packs use. The manifests keep the branch as
//! written, so a support crate declaring the same branch resolves out of the
//! same source rather than out of a second one that only differs in spelling.

use std::path::Path;
use std::process::Command;
use std::time::SystemTime;

use crate::pack_pin::{self, Pinned};

/// The crate whose resolution a lockfile is read for. The framework's crates
/// come out of one repository, so one of them says where all of them are.
pub(crate) const BENCH_CORE: &str = "mockspace-bench-core";

/// The commit a lockfile holds bench-core at, when it came out of git.
pub(crate) fn locked_rev(lock: &str) -> Option<String> {
    let doc: toml_edit::DocumentMut = lock.parse().ok()?;
    let packages = doc.get("package")?.as_array_of_tables()?;
    packages
        .iter()
        .filter(|p| p.get("name").and_then(|n| n.as_str()) == Some(BENCH_CORE))
        .filter_map(|p| p.get("source")?.as_str())
        .filter_map(|s| s.strip_prefix("git+"))
        .filter_map(|s| s.rsplit_once('#'))
        .map(|(_, rev)| rev.to_string())
        .next()
}

/// The commit the lockfile beside `manifest_dir` holds, if it holds one.
pub(crate) fn locked_rev_at(manifest_dir: &Path) -> Option<String> {
    locked_rev(&std::fs::read_to_string(manifest_dir.join("Cargo.lock")).ok()?)
}

/// The commit a run is built at, or `None` where there is nothing to align:
/// the spec names a `rev`, a `tag` or a path, or the tip could not be read
/// and was never read before.
///
/// `arms` pairs each arm's name with what its committed lockfile holds.
pub(crate) fn run_rev(
    arms: &[(String, Option<String>)],
    dep: &str,
    cache: &Path,
    resolve: &dyn Fn(&str, &str) -> Result<String, String>,
) -> Result<Option<String>, String> {
    if pack_pin::branch_pin(dep).is_none() {
        return Ok(None);
    }
    let mut revs: Vec<&str> = arms.iter().filter_map(|(_, r)| r.as_deref()).collect();
    revs.sort_unstable();
    revs.dedup();
    match revs.as_slice() {
        [] => {},
        [one] => return Ok(Some((*one).to_string())),
        _ => {
            let each: Vec<String> = arms
                .iter()
                .filter_map(|(a, r)| Some(format!("{a} at {}", r.as_deref()?)))
                .collect();
            return Err(format!(
                "the arms lock `{BENCH_CORE}` at different commits, and one run links \
                 one: {}. Update the older locks, `cargo update -p {BENCH_CORE}` beside \
                 each arm",
                each.join(", ")
            ));
        },
    }
    let pinned = pack_pin::pin(dep, cache, SystemTime::now(), resolve);
    if let Some(note) = pack_pin::fallback_note("the bench crates' `mockspace`", &pinned) {
        eprintln!("{note}");
    }
    Ok(match pinned {
        Pinned::Current { rev } | Pinned::Stale { rev, .. } => Some(rev),
        Pinned::Untouched | Pinned::Unresolved { .. } => None,
    })
}

/// Move the lockfile beside `manifest_dir` to `rev`, unless it is there.
///
/// `update` does the moving; [`cargo_update`] is what a run passes, and it is
/// a parameter so the decision can be driven without cargo.
pub(crate) fn align(
    manifest_dir: &Path,
    rev: &str,
    update: &dyn Fn(&Path, &str) -> Result<(), String>,
) -> Result<(), String> {
    if locked_rev_at(manifest_dir).as_deref() == Some(rev) {
        return Ok(());
    }
    update(&manifest_dir.join("Cargo.toml"), rev)
}

/// `cargo update --precise` on bench-core, writing the lockfile first where
/// there is none, since cargo has nothing to update until there is.
pub(crate) fn cargo_update(manifest: &Path, rev: &str) -> Result<(), String> {
    let lock = manifest.with_file_name("Cargo.lock");
    if !lock.is_file() {
        cargo(&["generate-lockfile", "--manifest-path"], manifest, &[])?;
    }
    cargo(&["update", "--manifest-path"], manifest, &["-p", BENCH_CORE, "--precise", rev])
}

fn cargo(head: &[&str], manifest: &Path, tail: &[&str]) -> Result<(), String> {
    let out = Command::new("cargo")
        .args(head)
        .arg(manifest)
        .args(tail)
        .output()
        .map_err(|e| format!("spawning cargo for {}: {e}", manifest.display()))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "cargo {} failed for {}:\n{}",
        head[0],
        manifest.display(),
        String::from_utf8_lossy(&out.stderr).trim_end()
    ))
}

#[cfg(test)]
#[path = "bench_rev_tests.rs"]
mod tests;
