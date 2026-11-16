//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The source of each tracking arm, which its rows stand at.
//!
//! An arm's source is the digest of two things: every file it builds from, in
//! the bench tree and out of it, followed through the crates it takes by path
//! (the lint pack's `bench_arm_code`), and the version each of its other
//! dependencies resolved to in the lockfile its build left. A commit that
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
use mockspace_lint_rules::{Reading, bench_arm_code};

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
    let mut lines = String::new();
    for code in bench_arm_code(&reading.view(), bench_dir) {
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
        let lock = std::fs::read_to_string(lock_dir.join("Cargo.lock")).ok();
        lines.push_str(&format!(
            "{}\t{}\n",
            prefix.display(),
            arm_source(&code.files, lock.as_deref())
        ));
    }
    let path = gen_dir.join("sources.tsv");
    bench_gen::write_if_changed(&path, &lines)?;
    Ok(path)
}

#[cfg(test)]
#[path = "bench_sources_tests.rs"]
mod tests;
