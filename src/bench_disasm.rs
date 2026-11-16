//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock bench disasm <bench> <arm> <symbol>`: build one arm as a run builds
//! it and write the disassembly of one of its functions as a results artifact,
//! `results/<bench>/disasm/<machine class>/<run>/<arm>.<symbol>.txt`, to commit
//! beside the runs. The build is removed again afterwards, as a run's is.

use std::path::Path;
use std::process::ExitCode;

use mockspace_bench_harness::disasm::{ArtifactPlace, write_symbol_artifact};
use mockspace_bench_harness::{env, history, layout};

use crate::bench::{aligned, build_arm, profile_args_for, run_rev_of};
use crate::config::Config;
use crate::{bench_builds, bench_gen, bench_rev};

const USAGE: &str = "usage: mock bench disasm <bench> <arm> <symbol>";

/// The three names the command takes, or the usage line.
fn parse<'a>(args: &[&'a str]) -> Result<(&'a str, &'a str, &'a str), String> {
    match args {
        [bench, arm, symbol]
            if !bench.starts_with('-') && !arm.starts_with('-') && !symbol.starts_with('-') =>
        {
            Ok((bench, arm, symbol))
        },
        _ => Err(USAGE.to_string()),
    }
}

pub(crate) fn cmd(cfg: &Config, args: &[&str]) -> ExitCode {
    match run(cfg, args) {
        Ok(path) => {
            println!("{}", path.display());
            ExitCode::SUCCESS
        },
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        },
    }
}

fn run(cfg: &Config, args: &[&str]) -> Result<std::path::PathBuf, String> {
    let (bench, arm_name, symbol) = parse(args)?;
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        return Err(format!(
            "{} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        ));
    }
    let _tree_lock = bench_gen::lock_tree(&bench_gen::driver_gen_dir(&cfg.mock_dir))
        .map_err(|e| format!("taking the bench tree's run lock: {e}"))?;
    let plan = bench_gen::plan(&bench_dir)?;
    let arm = find_arm(&plan.arms, bench, arm_name)?;
    let dep = bench_gen::mockspace_dep(&plan.manifest);
    let profile = profile_args_for(plan.manifest.build.as_ref());
    let inherited = crate::build_dir::build_target_dir(&cfg.mock_dir);
    let target = bench_builds::shared_target(&cfg.mock_dir, inherited.as_deref());
    let mut builds = bench_builds::Builds::new(&bench_dir, false);
    let wanted = bench_rev::wanted(&[bench]);
    let run_rev = run_rev_of(cfg, &plan, &dep, wanted.as_deref())?;
    let align = |dir: &Path| aligned(dir, run_rev.as_deref());
    let (manifest, built) = build_arm(cfg, &plan, &dep, arm, &align, &profile, target.as_deref())?;
    let library = builds
        .record_cdylib(&manifest, &built)
        .ok_or_else(|| format!("arm {bench}/arms/{arm_name} built no cdylib"))?;
    let machine = env::collect_run_env().machine_class;
    let run = layout::run_id(history::timestamp(), &env::run_commit());
    let results_root = bench_dir.join(layout::RESULTS_DIR);
    let place = ArtifactPlace {
        results_root: &results_root,
        bench,
        arm: arm_name,
        machine: &machine,
        run: &run,
    };
    write_symbol_artifact(&place, symbol, &library)
}

/// The arm `arm` of `bench`, or what the bench has instead.
fn find_arm<'a>(
    arms: &'a [mockspace_bench_harness::tree::ArmSource],
    bench: &str,
    arm: &str,
) -> Result<&'a mockspace_bench_harness::tree::ArmSource, String> {
    if let Some(found) = arms.iter().find(|a| a.bench == bench && a.arm == arm) {
        return Ok(found);
    }
    let of_bench: Vec<&str> = arms
        .iter()
        .filter(|a| a.bench == bench)
        .map(|a| a.arm.as_str())
        .collect();
    if of_bench.is_empty() {
        let mut benches: Vec<&str> = arms.iter().map(|a| a.bench.as_str()).collect();
        benches.dedup();
        return Err(format!(
            "no bench `{bench}` with arms under <bench>/arms/. Benches with arms: {}",
            if benches.is_empty() { "none".to_string() } else { benches.join(", ") }
        ));
    }
    Err(format!(
        "bench `{bench}` has no arm `{arm}`. Its arms: {}",
        of_bench.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mockspace_bench_harness::tree::ArmSource;

    use super::*;

    fn arm(bench: &str, arm: &str) -> ArmSource {
        ArmSource {
            bench:        bench.into(),
            arm:          arm.into(),
            dir:          PathBuf::new(),
            has_manifest: false,
        }
    }

    #[test]
    fn exactly_three_names_are_taken() {
        assert_eq!(
            parse(&["hash", "scalar", "bench_entry"]),
            Ok(("hash", "scalar", "bench_entry"))
        );
        for bad in [
            &["hash", "scalar"][..],
            &["hash", "scalar", "f", "g"],
            &["hash", "--keep", "f"],
            &[],
        ] {
            assert_eq!(parse(bad), Err(USAGE.to_string()), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_arm_or_bench_names_what_there_is() {
        let arms = [arm("hash", "scalar"), arm("hash", "unrolled"), arm("sort", "radix")];
        assert_eq!(find_arm(&arms, "hash", "unrolled").unwrap().arm, "unrolled");
        let err = find_arm(&arms, "hash", "simd").unwrap_err();
        assert!(
            err.contains("has no arm `simd`") && err.contains("scalar, unrolled"),
            "{err}"
        );
        let err = find_arm(&arms, "zip", "x").unwrap_err();
        assert!(
            err.contains("no bench `zip`") && err.contains("hash, sort"),
            "{err}"
        );
    }
}
