//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Transactional result staging: crash-borne outputs void themselves.
//!
//! A run writes every per-stage output (CSV, meta, findings) under
//! `results/.inflight/<runid>/<bench>/` and promotes the whole tree
//! into `results/<bench>/` only when the run loop completes in an
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
pub(super) const INFLIGHT_DIR: &str = ".inflight";
/// Subdirectory of the results root holding quarantined crash-borne
/// output trees.
pub(super) const VOID_DIR: &str = "void";

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
/// Promotion is all-or-nothing toward canonical: if any move fails
/// partway, every file already promoted is rolled back into staging
/// so canonical never shows a partial run, and the intact staged
/// tree is voided by the next start. A rollback failure is reported
/// loudly per file; only then can canonical hold a partial state.
pub(super) fn promote(results_root: &Path, stage_root: &Path) -> std::io::Result<()> {
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> std::io::Result<()> {
        for bench_entry in std::fs::read_dir(stage_root)? {
            let bench_dir = bench_entry?.path();
            if !bench_dir.is_dir() {
                continue;
            }
            let Some(bench_name) = bench_dir.file_name().map(|n| n.to_owned()) else {
                continue;
            };
            let final_dir = results_root.join(&bench_name);
            std::fs::create_dir_all(&final_dir)?;
            for file_entry in std::fs::read_dir(&bench_dir)? {
                let staged = file_entry?.path();
                let Some(file_name) = staged.file_name().map(|n| n.to_owned()) else {
                    continue;
                };
                let target = final_dir.join(&file_name);
                std::fs::rename(&staged, &target)?;
                moved.push((target, staged));
            }
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
        let root = std::env::temp_dir().join(format!(
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
}
