//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The source of each tracking arm, which its rows stand at.
//!
//! An arm's source is the digest of three things: every file it builds from, in
//! the bench tree and out of it, through every path package cargo resolved for
//! its build (`cargo metadata`), a crate a workspace hands down included, with
//! the root manifest it inherits from; the setup it runs under from the root
//! `bench.toml` (the lint pack's `bench_arm_code`); and the version each of its
//! other dependencies resolved to in the lockfile its build left. A commit that
//! changes nothing an arm builds from leaves its source where it was, so the
//! run adds to the same point; a change to any of it, a moved git branch or
//! a newer registry release included, starts a new point. Uncommitted edits
//! count, since the files are read as they are.
//!
//! `mock bench run` takes them once the arms are built and hands the driver a
//! file of `<path prefix>\t<source>` lines in
//! [`SOURCES_ENV`](mockspace_bench_harness::layout::SOURCES_ENV), the prefix
//! being where the arm's library is built, relative to the bench tree.

use std::path::{Path, PathBuf};

use mockspace_bench_harness::kinds::hex;
use mockspace_bench_harness::layout::source_id;
use mockspace_bench_harness::tree::{self as bench_tree, ArmSource};
use mockspace_lint_rules::{BenchArmCode, Reading, bench_arm_code, bench_crates_as_built};

use crate::bench_gen;

/// The versions a lockfile resolved every dependency that is not a path to:
/// `name version source` per package, in name order. The harness's own
/// crates are the instrument and are left out, and path packages carry no
/// source, their files being in the digest already.
#[must_use]
pub fn resolved(lock: &str) -> String {
    let Ok(doc) = lock.parse::<toml_edit::DocumentMut>() else {
        return String::new();
    };
    let mut lines: Vec<String> = doc
        .get("package")
        .and_then(toml_edit::Item::as_array_of_tables)
        .into_iter()
        .flat_map(|a| a.iter())
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?;
            let source = p.get("source")?.as_str()?;
            let version = p.get("version").and_then(|v| v.as_str()).unwrap_or("");
            (!name.starts_with("mockspace-bench-")).then(|| format!("{name} {version} {source}"))
        })
        .collect();
    lines.sort();
    lines.into_iter().map(|l| l + "\n").collect()
}

/// The crate directory of every path package cargo resolved for the arm built
/// from `manifest`, the arm's own left out, reached from the arm without going
/// through the harness's own crates, which are the instrument. A path package
/// is one with no `source`, which is how `cargo metadata` names a crate taken
/// by `path`, whether written out or inherited from a workspace.
pub fn path_crates(manifest: &Path) -> Result<Vec<PathBuf>, String> {
    let out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--offline", "--manifest-path"])
        .arg(manifest)
        .output()
        .map_err(|e| format!("spawning cargo metadata for {}: {e}", manifest.display()))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata for {} failed:\n{}",
            manifest.display(),
            String::from_utf8_lossy(&out.stderr).trim_end()
        ));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("reading cargo metadata for {}: {e}", manifest.display()))?;
    path_crates_of(&meta)
}

/// [`path_crates`] over a `cargo metadata` answer.
pub fn path_crates_of(meta: &serde_json::Value) -> Result<Vec<PathBuf>, String> {
    let packages: std::collections::HashMap<&str, &serde_json::Value> = meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let deps: std::collections::HashMap<&str, Vec<&str>> = meta["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| {
            let id = n["id"].as_str()?;
            let to = n["deps"].as_array().into_iter().flatten().filter_map(|d| d["pkg"].as_str()).collect();
            Some((id, to))
        })
        .collect();
    let root = meta["resolve"]["root"]
        .as_str()
        .ok_or("cargo metadata named no root package for the arm")?;
    let mut seen = std::collections::BTreeSet::from([root]);
    let mut queue = vec![root];
    let mut out = Vec::new();
    while let Some(id) = queue.pop() {
        for &to in deps.get(id).into_iter().flatten() {
            let Some(p) = packages.get(to) else { continue };
            let name = p["name"].as_str().unwrap_or_default();
            if name.starts_with("mockspace-bench-") || !seen.insert(to) {
                continue;
            }
            if p["source"].is_null() {
                if let Some(dir) = p["manifest_path"].as_str().and_then(|m| Path::new(m).parent()) {
                    out.push(dir.to_path_buf());
                }
            }
            queue.push(to);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The workspace root manifest the crate at `dir` inherits from, where its
/// manifest inherits anything (a `workspace = true` key, `[lints] workspace`):
/// the one its `package.workspace` names, or else the nearest manifest above
/// it declaring `[workspace]`, as cargo finds it. Its `[workspace.*]` tables
/// are part of what the crate builds from.
#[must_use]
pub fn inherited_root(dir: &Path) -> Option<PathBuf> {
    let doc: toml_edit::DocumentMut = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?.parse().ok()?;
    if !inherits(doc.as_item()) {
        return None;
    }
    if let Some(named) = doc.get("package").and_then(|p| p.get("workspace")).and_then(|w| w.as_str()) {
        return Some(dir.join(named).join("Cargo.toml"));
    }
    dir.ancestors().skip(1).map(|a| a.join("Cargo.toml")).find(|m| {
        std::fs::read_to_string(m)
            .ok()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok())
            .is_some_and(|d| d.contains_key("workspace"))
    })
}

/// Whether any table under `item` holds `workspace = true`.
fn inherits(item: &toml_edit::Item) -> bool {
    let table_like = item.as_table_like();
    table_like.is_some_and(|t| {
        t.iter().any(|(k, v)| (k == "workspace" && v.as_bool() == Some(true)) || inherits(v))
    })
}

/// The code text of a tracking arm built from `manifest`: what the lint pack
/// reads of it ([`BenchArmCode::text`]), with every path package cargo
/// resolved for it read as built and the workspace root manifest each one
/// inherits from.
pub fn arm_text(
    view: &mockspace_lint_rules::View<'_>,
    bench_dir: &Path,
    code: &BenchArmCode,
    manifest: &Path,
) -> Result<String, String> {
    let crates = path_crates(manifest)?;
    let mut more = bench_crates_as_built(view, bench_dir, &crates);
    more.extend(crates.iter().filter_map(|d| inherited_root(d)));
    Ok(code.text(view, bench_dir, &more))
}

/// The source of an arm whose files read `files` and whose lockfile, where
/// its build left one, reads `lock`.
#[must_use]
pub fn arm_source(files: &str, lock: Option<&str>) -> String {
    source_id(&hex(format!(
        "{files}\n# resolved\n{}",
        lock.map(resolved).unwrap_or_default()
    )
    .as_bytes()))
}

/// Take the source of every tracking arm the run built and write them to
/// `<gen_dir>/sources.tsv`, returning its path.
pub fn write_sources(
    repo_root: &Path,
    mock_dir: &Path,
    bench_dir: &Path,
    gen_dir: &Path,
    arms: &[ArmSource],
) -> Result<PathBuf, String> {
    let reading = Reading::new(repo_root);
    let view = reading.view();
    let mut lines = String::new();
    for code in bench_arm_code(&view, bench_dir) {
        let (prefix, lock_dir) = match &code.section {
            // A composed member's arm builds into the tool's target, and a
            // generated manifest's lockfile sits beside that manifest.
            None => {
                let arm = code
                    .crate_dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let lock_dir = match arms.iter().find(|a| a.bench == code.bench && a.arm == arm) {
                    Some(a) if !a.has_manifest => bench_gen::arm_gen_dir(mock_dir, a),
                    _ => bench_dir.join(&code.crate_dir),
                };
                (bench_tree::arm_target_dir(&code.bench, &arm), lock_dir)
            },
            // A section's variant builds in its own crate directory.
            Some(_) => (code.crate_dir.clone(), bench_dir.join(&code.crate_dir)),
        };
        // An arm this run left out has no manifest generated yet, and is not
        // run: the driver refuses a tracking arm it was handed no source for.
        let manifest = lock_dir.join("Cargo.toml");
        if !manifest.is_file() {
            continue;
        }
        let text = arm_text(&view, bench_dir, &code, &manifest)?;
        let lock = std::fs::read_to_string(lock_dir.join("Cargo.lock")).ok();
        lines.push_str(&format!(
            "{}\t{}\n",
            prefix.display(),
            arm_source(&text, lock.as_deref())
        ));
    }
    let path = gen_dir.join("sources.tsv");
    bench_gen::write_if_changed(&path, &lines)?;
    Ok(path)
}

#[cfg(test)]
#[path = "bench_sources_tests.rs"]
mod tests;
