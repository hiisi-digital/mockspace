//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Worker-mode dispatch for the library driver: parse the worker
//! args the orchestrator passed, rebuild the routine and workload
//! from the manifest, and run the worker loop.

use std::path::Path;
use std::process::ExitCode;

use super::worker_args::{WorkerArgs, worker_args};
use super::{Cli, DriverSpec, resolve_routine};
use crate::config::BenchConfig;
use crate::harness;

/// Worker-mode dispatch: parse the worker args the orchestrator
/// passed, rebuild the routine and workload from the manifest, and
/// run the worker loop.
pub(super) fn drive_worker(spec: &DriverSpec, cli: &Cli) -> ExitCode {
    let WorkerArgs {
        dylib_path,
        bench_name,
        seed,
        cooldown_ms,
        mode,
        runs,
        batch,
        n,
        batch_k,
        max_call_us,
        threaded,
        seeds,
    } = match worker_args(&cli.raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: worker: {e}");
            return ExitCode::FAILURE;
        },
    };

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

    if let Some(seeds) = seeds {
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
