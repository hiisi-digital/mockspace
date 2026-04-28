//! Self-bench for mockspace. Compares two u64 hash-mixing functions
//! through the full bench harness pipeline (orchestrator, worker
//! subprocess, dlopen, CSV cache, findings.md). First real consumer
//! of mockspace-bench-harness.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mockspace_bench_core::{routine_bridge, Routine};
use mockspace_bench_harness::{
    self as harness, BenchManifest, RoutineSpec, Workload,
};

/// Routine: u64 hash-mixing. Variants differ in how they scramble the
/// input seed into a 64-bit digest.
pub struct HashMix;

impl Routine for HashMix {
    type Input = u64;
    type Output = u64;

    fn build_input(seed: u64) -> Self::Input {
        seed.wrapping_mul(0x9E3779B97F4A7C15)
    }

    fn ops_per_call(_input: &Self::Input) -> u64 {
        1
    }

    fn outputs_may_differ() -> bool {
        true
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--worker") {
        return run_worker(&args);
    }

    let report_only = args.iter().any(|a| a == "--report-only");

    let manifest_path = Path::new("bench.toml");
    let manifest = match BenchManifest::load(manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mock_benches_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let mut workload = Workload::new();
    workload.program("default", |b| {
        b.stage(vec![harness::algo_call(), harness::light_scalar()]);
    });

    for (bench_name, section) in &manifest.bench {
        for (size_idx, _size) in section.sizes.iter().enumerate() {
            let config = match manifest.for_size(bench_name, size_idx, &mock_benches_dir) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            };
            let routine = RoutineSpec {
                name: section.workload.clone(),
                bridge: routine_bridge!(HashMix),
            };

            let csv_path = format!("{}_n{}.csv", bench_name, config.n);
            let report_path = format!("{}_n{}_findings.md", bench_name, config.n);

            if report_only {
                let samples = match harness::load_samples_csv(Path::new(&csv_path)) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "error: report-only could not load `{csv_path}` for bench `{bench_name}` n={}: {e}",
                            config.n
                        );
                        eprintln!("hint: run the bench first to produce the csv");
                        return ExitCode::FAILURE;
                    }
                };
                if samples.is_empty() {
                    eprintln!(
                        "error: report-only: no samples in `{csv_path}` for bench `{bench_name}` n={}",
                        config.n
                    );
                    return ExitCode::FAILURE;
                }
                let result = mockspace_bench_harness::BenchResult {
                    title: section.title.clone(),
                    env: mockspace_bench_harness::EnvMeta::default(),
                    samples,
                    cache_path: csv_path.clone(),
                    report_path: report_path.clone(),
                };
                if let Err(e) =
                    harness::write_report_for_routine(&result, &routine, "warm", &report_path)
                {
                    eprintln!("error: writing report: {e}");
                    return ExitCode::FAILURE;
                }
                eprintln!("  regenerated {report_path}");
            } else {
                let result = match harness::run(&config, &routine, &workload) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("error: bench `{bench_name}` n={}: {e}", config.n);
                        return ExitCode::FAILURE;
                    }
                };
                if let Err(e) = harness::write_csv(&result, &csv_path) {
                    eprintln!("error: writing csv: {e}");
                    return ExitCode::FAILURE;
                }
                if let Err(e) =
                    harness::write_report_for_routine(&result, &routine, "warm", &report_path)
                {
                    eprintln!("error: writing report: {e}");
                    return ExitCode::FAILURE;
                }
                eprintln!("  wrote {csv_path} + {report_path}");
            }
        }
    }

    ExitCode::SUCCESS
}

fn run_worker(args: &[String]) -> ExitCode {
    let get = |flag: &str| -> Option<String> {
        let pos = args.iter().position(|a| a == flag)?;
        args.get(pos + 1).cloned()
    };

    let dylib_path = match get("--worker") {
        Some(p) => p,
        None => {
            eprintln!("worker: missing --worker <path>");
            return ExitCode::FAILURE;
        }
    };
    let bench_name = get("--bench-name").unwrap_or_default();
    let seed: u64 = get("--seed").and_then(|s| s.parse().ok()).unwrap_or(0);
    let cooldown_ms: u64 = get("--cooldown").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mode = get("--mode").unwrap_or_else(|| "warm".into());
    let runs: usize = get("--runs").and_then(|s| s.parse().ok()).unwrap_or(0);
    let batch: usize = get("--batch").and_then(|s| s.parse().ok()).unwrap_or(1);
    let n: usize = get("--n").and_then(|s| s.parse().ok()).unwrap_or(1);
    let batch_k: usize = get("--batch-k").and_then(|s| s.parse().ok()).unwrap_or(1);
    let max_call_us: Option<u64> = get("--max-call-us")
        .and_then(|s| s.parse().ok())
        .filter(|&v| v > 0);

    let routine = RoutineSpec {
        name: bench_name.clone(),
        bridge: routine_bridge!(HashMix),
    };

    let mut workload = Workload::new();
    workload.program("default", |b| {
        b.stage(vec![harness::algo_call(), harness::light_scalar()]);
    });

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
    );
    ExitCode::SUCCESS
}
