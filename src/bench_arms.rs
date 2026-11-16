//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Building one arm of a composed bench tree as a run builds it, shared by
//! `mock bench run` and `mock bench disasm`: the framework commit the run
//! links, the alignment of an arm's lockfile to it, and the build itself.

use std::path::{Path, PathBuf};

use mockspace_bench_harness::tree as bench_tree;

use crate::bench::{cargo_build_at, check_arm_lib_name};
use crate::config::Config;
use crate::{bench_gen, bench_rev};

/// One commit of the framework for every crate a run links, taken from the
/// locks of the arms it builds (`wanted`, every arm where `None`). `bench_rev`
/// says why.
pub(crate) fn run_rev_of(
    cfg: &Config,
    plan: &bench_gen::GenPlan,
    dep: &str,
    wanted: Option<&[String]>,
) -> Result<Option<String>, String> {
    let arm_locks = bench_rev::arm_locks(&plan.arms, wanted);
    let pins = bench_gen::driver_gen_dir(&cfg.mock_dir).join(".pack-pins");
    bench_rev::run_rev(&arm_locks, dep, &pins, &crate::pack_pin::ls_remote_head)
}

/// Align the crate at `dir` to the run's framework commit, where it has one.
pub(crate) fn aligned(dir: &Path, run_rev: Option<&str>) -> Result<(), String> {
    match run_rev {
        Some(rev) => bench_rev::align(dir, rev, &bench_rev::cargo_update),
        None => Ok(()),
    }
}

/// Build one arm of a composed tree as a run builds it: its own manifest
/// (checked) or the generated one (written), aligned to the run's framework
/// commit by `align`, with the bench's profile into `target_dir`. Returns the
/// manifest built and cargo's artifact records.
pub(crate) fn build_arm(
    cfg: &Config,
    plan: &bench_gen::GenPlan,
    dep: &str,
    arm: &bench_tree::ArmSource,
    align: &impl Fn(&Path) -> Result<(), String>,
    profile: &[String],
    target_dir: Option<&Path>,
) -> Result<(PathBuf, Vec<u8>), String> {
    let what = format!("arm {}/arms/{}", arm.bench, arm.arm);
    let manifest_path = if arm.has_manifest {
        check_arm_lib_name(arm)?;
        arm.dir.join("Cargo.toml")
    } else {
        let gen_dir = bench_gen::arm_gen_dir(&cfg.mock_dir, arm);
        let toml = bench_gen::arm_cargo_toml(arm, dep, &plan.support)?;
        bench_gen::write_if_changed(&gen_dir.join("Cargo.toml"), &toml)?;
        gen_dir.join("Cargo.toml")
    };
    align(manifest_path.parent().unwrap_or(&arm.dir)).map_err(|e| format!("{what}: {e}"))?;
    eprintln!("  building {what}...");
    let built = cargo_build_at(&manifest_path, &what, profile, target_dir)?;
    Ok((manifest_path, built))
}
