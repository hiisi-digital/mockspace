//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Worker-mode dispatch for the library driver: parse the worker
//! args the orchestrator passed, rebuild the routine and workload
//! from the manifest, and run the worker loop.

use std::path::Path;
use std::process::ExitCode;

use super::{Cli, DriverSpec, resolve_routine};
use crate::config::BenchConfig;
use crate::harness;

/// Worker-mode dispatch: parse the worker args the orchestrator
/// passed, rebuild the routine and workload from the manifest, and
/// run the worker loop.
pub(super) fn drive_worker(spec: &DriverSpec, cli: &Cli) -> ExitCode {
    let args = &cli.raw;
    let get = |flag: &str| -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|pos| args.get(pos + 1).cloned())
    };
    let dylib_path = match get("--worker") {
        Some(p) => p,
        None => {
            eprintln!("error: --worker requires a dylib path");
            return ExitCode::FAILURE;
        },
    };
    let bench_name = get("--bench-name").unwrap_or_default();
    let seed: u64 = get("--seed").and_then(|s| s.parse().ok()).unwrap_or(0);
    let cooldown_ms: u64 = get("--cooldown").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mode = get("--mode").unwrap_or_else(|| "warm".into());
    let runs: usize = get("--runs").and_then(|s| s.parse().ok()).unwrap_or(0);
    let batch: usize = get("--batch").and_then(|s| s.parse().ok()).unwrap_or(1);
    let n: usize = get("--n").and_then(|s| s.parse().ok()).unwrap_or(64);
    let batch_k: usize = get("--batch-k").and_then(|s| s.parse().ok()).unwrap_or(1);
    let max_call_us: Option<u64> = get("--max-call-us")
        .and_then(|s| s.parse().ok())
        .filter(|&v| v != 0);
    let threaded = args.iter().any(|a| a == "--threaded");

    // Rebuild the routine + workload the same way the orchestrator
    // did: the worker inherits the orchestrator's cwd, so the
    // manifest is readable at the same relative path.
    let manifest = match super::load_manifest(Path::new(".")) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "error: worker could not load bench.toml (cwd must be \
                 mock/benches/): {e}"
            );
            return ExitCode::FAILURE;
        },
    };
    let Some((workload_name, may_differ)) = manifest
        .bench
        .get(&bench_name)
        .map(|s| (s.workload.clone(), s.may_differ))
    else {
        eprintln!("error: worker bench `{bench_name}` not found in bench.toml");
        return ExitCode::FAILURE;
    };
    let mut probe = BenchConfig::default();
    probe.bench_name = bench_name.clone();
    let (bench, sweep) = manifest
        .nested
        .get(&bench_name)
        .cloned()
        .unwrap_or_else(|| (bench_name.clone(), bench_name.clone()));
    probe.bench = bench;
    probe.sweep = sweep;
    probe.nested = manifest.nested.contains_key(&bench_name);
    probe.workload = workload_name.clone();
    probe.n = n;
    probe.may_differ = may_differ;
    let routine = match resolve_routine(spec, &probe) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: worker routine resolution: {e}");
            return ExitCode::FAILURE;
        },
    };
    let workload = (spec.build_workload)(&workload_name, n);

    if mode == "validate" {
        let seeds: Vec<u64> = get("--seeds")
            .map(|s| s.split(',').filter_map(|t| t.parse().ok()).collect())
            .unwrap_or_default();
        harness::run_worker_validate(&routine, &dylib_path, &seeds, n, threaded);
        return ExitCode::SUCCESS;
    }

    harness::run_worker(
        &routine,
        &workload,
        &dylib_path,
        seed,
        cooldown_ms,
        &mode,
        runs,
        batch,
        n,
        batch_k,
        max_call_us,
        threaded,
    );
    ExitCode::SUCCESS
}
