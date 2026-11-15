//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Transactional result staging: crash-borne outputs void themselves.
//!
//! A run writes every cell's run directory under
//! `results/.inflight/<stage>/` and promotes the whole tree into the same
//! relative paths under `results/` only when the run loop completes in an
//! orderly way. A run that dies mid-way (panic, abort, kill, OOM)
//! leaves its staging directory behind, and the next orchestrator
//! start quarantines it into `results/void/<runid>/` with a notice.
//!
//! The whole-run granularity is deliberate: a crash voids even the
//! stages that "completed" before it, because the conditions that
//! produced the crash (memory pressure, a degrading host) were also
//! present while those stages timed. A completed-but-failing run
//! (validation drops, `required` failures) still promotes; those
//! outcomes are visible and reported, not silently contaminated.
//!
//! Voided trees are kept, not deleted: they are the audit trail of
//! the crashed run, clearly out of the canonical results location so
//! nothing (history, docgen, readers) can mistake them for valid.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Subdirectory of the results root holding in-flight run staging.
pub(super) const INFLIGHT_DIR: &str = mockspace_bench_layout::INFLIGHT_DIR;
/// Subdirectory of the results root holding quarantined crash-borne
/// output trees.
pub(super) const VOID_DIR: &str = mockspace_bench_layout::VOID_DIR;

/// Upper age for an in-flight tree to be considered possibly live.
/// A recorded pid can be reused by an unrelated process after a
/// crash, so liveness alone cannot protect a tree forever; past this
/// age the tree is voided regardless of the pid probe. No real bench
/// run approaches this bound.
const MAX_INFLIGHT_AGE_SECS: u64 = 48 * 60 * 60;

/// Quarantine any stale staging tree left by a crashed run: move
/// `results/.inflight/<runid>` to `results/void/<runid>` for every
/// runid whose recorded pid is no longer alive, or whose tree is
/// older than [`MAX_INFLIGHT_AGE_SECS`] (a live pid is only evidence
/// of a concurrent run while the tree is recent; pids get reused).
pub(super) fn quarantine_stale(results_root: &Path) {
    let inflight = results_root.join(INFLIGHT_DIR);
    let Ok(entries) = std::fs::read_dir(&inflight) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let recent = ts_from_runid(&name)
            .map(|ts| now_secs().saturating_sub(ts) < MAX_INFLIGHT_AGE_SECS)
            .unwrap_or(false);
        if recent {
            if let Some(pid) = pid_from_runid(&name) {
                if pid_alive(pid) {
                    eprintln!(
                        "  note: in-flight results of live run {name} left alone \
                         (concurrent bench run?)"
                    );
                    continue;
                }
            }
        }
        let void_dir = results_root.join(VOID_DIR);
        if let Err(e) = std::fs::create_dir_all(&void_dir) {
            eprintln!("  warning: creating {}: {e}", void_dir.display());
            continue;
        }
        let target = void_dir.join(&name);
        match std::fs::rename(&path, &target) {
            Ok(()) => {
                eprintln!(
                    "  VOIDED crash-borne results of run {name}: the run did not \
                 complete, so every stage it wrote is untrusted (the crash \
                 conditions were present while they timed). Quarantined at {}",
                    target.display()
                )
            },
            Err(e) => eprintln!("  warning: quarantining {}: {e}", path.display()),
        }
    }
}

/// Create and return this run's staging root,
/// `results/.inflight/<timestamp>-<pid>`.
pub(super) fn create_stage_root(results_root: &Path) -> std::io::Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let root = results_root
        .join(INFLIGHT_DIR)
        .join(format!("{}-{}", ts, std::process::id()));
    std::fs::create_dir_all(&root)?;
    Ok(root)
}

/// Promote every staged file into the canonical results tree and
/// remove the staging root. Called only when the run loop completed
/// in an orderly way; a crash never reaches this point, which is the
/// entire mechanism.
///
/// Each staged file moves to the same relative path under the results root,
/// and a file already there is refused, never replaced: a run directory is
/// written once, so a target that exists is a collision to report and not a
/// file to overwrite. The previous form moved files into `results/<bench>/`
/// by name and replaced whatever an earlier run had left there.
///
/// Promotion is all-or-nothing toward canonical: if any move fails
/// partway, every file already promoted is rolled back into staging
/// so canonical never shows a partial run, and the intact staged
/// tree is voided by the next start. A rollback failure is reported
/// loudly per file; only then can canonical hold a partial state.
pub(super) fn promote(results_root: &Path, stage_root: &Path) -> std::io::Result<()> {
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> std::io::Result<()> {
        for staged in staged_files(stage_root)? {
            let Ok(rel) = staged.strip_prefix(stage_root) else {
                continue;
            };
            let target = results_root.join(rel);
            if target.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!(
                        "{} already exists; a run directory is written once and \
                         this run would have replaced it",
                        target.display()
                    ),
                ));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&staged, &target)?;
            moved.push((target, staged));
        }
        Ok(())
    })();
    if let Err(e) = result {
        for (target, staged) in moved.iter().rev() {
            if let Err(back) = std::fs::rename(target, staged) {
                eprintln!(
                    "  warning: rollback of {} failed ({back}); canonical may \
                     hold a partial run until manually reconciled",
                    target.display()
                );
            }
        }
        return Err(e);
    }
    std::fs::remove_dir_all(stage_root)?;
    // Drop the .inflight parent when this was the only run in it.
    let inflight = results_root.join(INFLIGHT_DIR);
    let _ = std::fs::remove_dir(&inflight);
    Ok(())
}

/// Every file under `dir`, at any depth, sorted.
fn staged_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}
/// Parse the pid component out of a `<timestamp>-<pid>` runid.
fn pid_from_runid(runid: &str) -> Option<u32> {
    runid.rsplit('-').next()?.parse().ok()
}

/// Parse the timestamp component out of a `<timestamp>-<pid>` runid.
fn ts_from_runid(runid: &str) -> Option<u64> {
    runid.split('-').next()?.parse().ok()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether a pid names a live process. Signal 0 probes existence
/// without touching the process; on platforms without a `kill`
/// binary the probe fails and the tree is treated as stale.
fn pid_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_results_root(tag: &str) -> PathBuf {
        let root = mockspace_manifest::scratch::root().join(format!(
            "mockspace-staging-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn promote_moves_staged_files_and_clears_inflight() {
        let root = temp_results_root("promote");
        let stage = create_stage_root(&root).unwrap();
        let bench = stage.join("some_bench");
        std::fs::create_dir_all(&bench).unwrap();
        std::fs::write(bench.join("some_bench_n64.csv"), "a,b\n").unwrap();
        promote(&root, &stage).unwrap();
        assert!(root.join("some_bench/some_bench_n64.csv").exists());
        assert!(!root.join(INFLIGHT_DIR).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quarantine_moves_dead_run_tree_to_void() {
        let root = temp_results_root("void");
        // Fabricate a staging tree whose pid can no longer be alive.
        let stale = root.join(INFLIGHT_DIR).join("123-4294967295");
        std::fs::create_dir_all(stale.join("some_bench")).unwrap();
        std::fs::write(stale.join("some_bench/some_bench_n64.csv"), "a,b\n").unwrap();
        quarantine_stale(&root);
        assert!(!stale.exists());
        assert!(
            root.join(VOID_DIR)
                .join("123-4294967295/some_bench/some_bench_n64.csv")
                .exists()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quarantine_leaves_live_recent_run_tree_alone() {
        let root = temp_results_root("live");
        // This test process's own pid is alive by construction and
        // the timestamp is now, so the tree reads as a live run.
        let live = root
            .join(INFLIGHT_DIR)
            .join(format!("{}-{}", now_secs(), std::process::id()));
        std::fs::create_dir_all(&live).unwrap();
        quarantine_stale(&root);
        assert!(live.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn quarantine_voids_ancient_tree_despite_live_pid() {
        let root = temp_results_root("ancient");
        // Timestamp 1 is far past the age bound; a live (possibly
        // reused) pid must not shield the tree forever.
        let stale = root
            .join(INFLIGHT_DIR)
            .join(format!("1-{}", std::process::id()));
        std::fs::create_dir_all(&stale).unwrap();
        quarantine_stale(&root);
        assert!(!stale.exists());
        assert!(
            root.join(VOID_DIR)
                .join(format!("1-{}", std::process::id()))
                .exists()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_promote_rolls_canonical_back() {
        let root = temp_results_root("rollback");
        let stage = create_stage_root(&root).unwrap();
        // First bench promotes fine; the second collides with a
        // regular FILE at its canonical dir path, failing mid-way.
        for b in ["bench_a", "bench_b"] {
            let d = stage.join(b);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(format!("{b}_n64.csv")), "a,b\n").unwrap();
        }
        std::fs::write(root.join("bench_b"), "blocking file").unwrap();
        assert!(promote(&root, &stage).is_err());
        // Canonical gained nothing from bench_a; staging is intact.
        assert!(!root.join("bench_a").join("bench_a_n64.csv").exists());
        assert!(stage.join("bench_a/bench_a_n64.csv").exists());
        assert!(stage.join("bench_b/bench_b_n64.csv").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A run directory promotes at its full depth, beside an earlier run of
    /// the same cell, and the earlier run is untouched.
    #[test]
    fn a_run_promotes_beside_an_earlier_one() {
        let root = temp_results_root("beside");
        let cell = "hash/hash_n64/linux-x86_64-c4-bare-amd";
        std::fs::create_dir_all(root.join(cell).join("20261114T000000Z-a")).unwrap();
        std::fs::write(root.join(cell).join("20261114T000000Z-a/samples.csv"), "old\n").unwrap();
        let stage = create_stage_root(&root).unwrap();
        std::fs::create_dir_all(stage.join(cell).join("20261115T000000Z-b")).unwrap();
        std::fs::write(stage.join(cell).join("20261115T000000Z-b/samples.csv"), "new\n").unwrap();
        promote(&root, &stage).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(cell).join("20261114T000000Z-a/samples.csv")).unwrap(), "old\n");
        assert_eq!(std::fs::read_to_string(root.join(cell).join("20261115T000000Z-b/samples.csv")).unwrap(), "new\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The defect this exists for: a re-run replaced the earlier run's files in
    /// place. A staged file whose target exists is refused, the existing bytes
    /// stand, and the staged run stays whole for the next start to void.
    #[test]
    fn promote_never_replaces_an_existing_file() {
        let root = temp_results_root("never-replace");
        std::fs::create_dir_all(root.join("hash")).unwrap();
        std::fs::write(root.join("hash/hash_n64.csv"), "earlier run\n").unwrap();
        let stage = create_stage_root(&root).unwrap();
        std::fs::create_dir_all(stage.join("hash")).unwrap();
        std::fs::write(stage.join("hash/hash_n64.csv"), "this run\n").unwrap();
        let err = promote(&root, &stage).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(std::fs::read_to_string(root.join("hash/hash_n64.csv")).unwrap(), "earlier run\n");
        assert!(stage.join("hash/hash_n64.csv").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
