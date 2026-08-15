//! Harness: one binary, two modes.
//!
//! Orchestrator (default): spawns itself as subprocesses per
//! (variant × cooldown × pass). Collects results, writes CSV + report.
//!
//! Worker (`--worker` flag): loads one dylib, runs one pass through
//! the workload, prints timing to stdout, exits. Clean process per
//! run.
//!
//! Worker output: one line per batch (not one averaged line per run).
//! This preserves batch-level distribution for bootstrap CIs.
//!
//! ## Architecture
//!
//! The consumer's bench binary is a single executable that contains
//! both the [`crate::RoutineSpec`] (built via
//! [`mockspace_bench_core::routine_bridge!`]) and a `main()` that
//! dispatches to [`run_orchestrator`] or [`run_worker`] based on the
//! `--worker` flag. The orchestrator spawns the same binary with
//! `--worker` and per-config args; the worker mode loads the variant
//! cdylib, runs one mode, prints per-batch lines to stdout, exits.
//!
//! Round 3 ships orchestrator + worker. The result type does NOT yet
//! carry [`crate::analysis::DataSet`] aggregation; that lands in
//! Round 5 as `BenchResult::dataset(mode)`.

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::config::BenchConfig;
use crate::core::counter::{self, Rng};
use crate::core::{AbiHashFn, BenchEntryFn, BenchNameFn, abi_hash};
use crate::env::{EnvMeta, collect_env_meta};
use crate::error::BenchError;
use crate::sample::{BenchResult, Sample};
use crate::spec::RoutineSpec;
use crate::workload::{Workload, mix};

// ── Environment metadata helpers ──

/// The environment variable the tool sets on the driver it spawns,
/// carrying the release-profile settings it actually passed on the
/// builds (`opt-level=..,lto=..,codegen-units=..`).
pub const BUILD_PROFILE_ENV: &str = "MOCKSPACE_BENCH_PROFILE";

/// Serialize [`EnvMeta`] to a JSON string (no serde_json dependency).
///
/// `build_profile` records the settings the tool passed on the
/// builds, read from [`BUILD_PROFILE_ENV`]. It was previously a
/// hardcoded constant, which became a lie the moment `[build]`
/// overrides existed: a tree overriding the profile got artifacts
/// recording a profile that was not used, and a comparison across
/// that change read as a performance shift with no visible cause.
/// When the variable is absent (the binary run by hand, outside the
/// tool), the field is omitted: no claim beats a wrong one.
fn env_meta_to_json(meta: &EnvMeta) -> String {
    env_meta_to_json_with(meta, std::env::var(BUILD_PROFILE_ENV).ok().as_deref())
}

fn env_meta_to_json_with(meta: &EnvMeta, build_profile: Option<&str>) -> String {
    let profile_field = match build_profile {
        Some(p) => format!(",\"build_profile\":\"{}\"", json_escape(p)),
        None => String::new(),
    };
    format!(
        "{{\"cpu\":\"{cpu}\",\"os\":\"{os}\",\"rustc\":\"{rustc}\",\
        \"git_commit\":\"{git}\",\"timestamp\":{ts},\
        \"counter_freq\":{freq},\"framework\":\"mockspace-bench-harness\"{profile_field}}}",
        cpu = json_escape(&meta.cpu),
        os = json_escape(&meta.os),
        rustc = json_escape(&meta.rustc),
        git = json_escape(&meta.git_commit),
        ts = meta.timestamp,
        freq = meta.counter_freq,
    )
}

/// Escape a string value for use in a JSON string literal.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out
}

/// Derive the `.meta.json` path from a CSV path: same stem, new extension.
fn meta_json_path(csv_path: &str) -> String {
    if let Some(stem) = csv_path.strip_suffix(".csv") {
        format!("{}.meta.json", stem)
    } else {
        format!("{}.meta.json", csv_path)
    }
}

// ── Worker mode ──

/// Helper: load a variant cdylib and resolve the symbols the worker
/// needs. Returns `(name, entry)` on success or a human-readable
/// reason string on failure. Failures are not panics; the worker
/// reports them via stderr + a TIMEOUT line on stdout so the
/// orchestrator categorises them alongside other early-exit cases.
///
/// # Safety
///
/// The caller asserts that the cdylib at `dylib_path` was built
/// against a compatible `mockspace-bench-core` ABI; the function
/// double-checks via the `bench_abi_hash` symbol but the dlopen
/// itself runs initialisers and is unsafe by definition.
unsafe fn load_variant(dylib_path: &str) -> Result<(String, BenchEntryFn), String> {
    let lib = unsafe { libloading::Library::new(dylib_path) }
        .map_err(|e| format!("dlopen failed: {e}"))?;

    let hash_fn: libloading::Symbol<AbiHashFn> = unsafe { lib.get(b"bench_abi_hash") }
        .map_err(|e| format!("missing bench_abi_hash symbol: {e}"))?;
    let found = hash_fn();
    let expected = abi_hash();
    if found != expected {
        return Err(format!(
            "ABI hash mismatch: variant has {found:#x}, harness expects {expected:#x}. \
             Rebuild the variant against the current mockspace-bench-core."
        ));
    }

    let entry: libloading::Symbol<BenchEntryFn> = unsafe { lib.get(b"bench_entry") }
        .map_err(|e| format!("missing bench_entry symbol: {e}"))?;
    let entry_fn: BenchEntryFn = *entry;

    let name_fn: libloading::Symbol<BenchNameFn> =
        unsafe { lib.get(b"bench_name") }.map_err(|e| format!("missing bench_name symbol: {e}"))?;
    let name = unsafe { std::ffi::CStr::from_ptr(name_fn() as *const i8) }
        .to_string_lossy()
        .into_owned();

    // Leak the library so the function pointers stay valid for the
    // remainder of the worker's lifetime.
    std::mem::forget(lib);
    Ok((name, entry_fn))
}

/// Run as a worker subprocess. Loads one dylib, runs ONE mode, prints
/// timing. Emits one line per batch to preserve distributional
/// information.
///
/// Output format on stdout, one line per batch:
///
/// ```text
/// <name>\t<mode>\t<batch_idx>\t<e2e_ns>\t<algo_ns>\t<bridge_ns>\t<batch_count>\t<instructions>\t<cycles>\t<setup_ns>\t<first_ns>\t<digest>[\t<score>]
/// ```
///
/// `setup_ns`/`first_ns`/`digest` are always present (zero when a variant
/// does not measure them, e.g. plain `timed!` variants); `score` is the
/// optional trailing column.
///
/// Or `TIMEOUT\t<name>\t<mode>\t<value>` on early abort.
#[allow(clippy::too_many_arguments)]
pub fn run_worker(
    routine: &RoutineSpec,
    workload: &Workload,
    dylib_path: &str,
    seed: u64,
    cooldown_ms: u64,
    mode: &str,
    runs: usize,
    batch_size: usize,
    n: usize,
    batch_k: usize,
    max_call_us: Option<u64>,
    threaded: bool,
) {
    // A threaded bench's spawned workers never inherit the pin, and
    // pinning only the coordinating thread skews the workload; the
    // manifest's `threaded = true` opts out of the self-pin entirely.
    if !threaded {
        counter::pin_to_perf_cores();
    }

    // Optional hardware perf counters: arm on this worker thread when the env
    // opts in (set by running the bench with `--perf-counters`, which requires
    // sudo; the env inherits to each serially-spawned worker). The RAII guard
    // releases the PMU on every exit path, including early returns and panics,
    // so the exclusive claim is never left for the next worker.
    struct PerfGuard;
    impl Drop for PerfGuard {
        fn drop(&mut self) {
            crate::perf::teardown();
        }
    }
    if std::env::var("MOCKSPACE_BENCH_PERF").is_ok() {
        crate::perf::setup();
    }
    let _perf_guard = PerfGuard;

    // One-shot validation diagnostic: with MOCKSPACE_BENCH_PERF_DIAG set (and the
    // PMU armed), run a known ~1e6-iteration loop and dump the raw per-slot
    // counter deltas to stderr. The slot whose delta is ~a few million is the
    // instructions counter; the larger one is cycles. This is the single command
    // that confirms (or corrects) the FIXED_*_SUBINDEX guesses in perf.rs.
    if std::env::var("MOCKSPACE_BENCH_PERF_DIAG").is_ok() && crate::perf::available() {
        let before = crate::perf::read_all_raw();
        let mut acc = 0u64;
        for i in 0 .. 1_000_000u64 {
            acc = acc.wrapping_add(std::hint::black_box(i));
        }
        std::hint::black_box(acc);
        let after = crate::perf::read_all_raw();
        let deltas: Vec<i128> = after
            .iter()
            .zip(before.iter())
            .map(|(a, b)| *a as i128 - *b as i128)
            .collect();
        eprintln!("PERF_DIAG raw before: {:?}", before);
        eprintln!("PERF_DIAG raw after:  {:?}", after);
        eprintln!(
            "PERF_DIAG deltas (1e6-iter loop): {:?} -- the slot ~a few million is instructions, the larger is cycles",
            deltas
        );
        let snap = crate::perf::read();
        eprintln!(
            "PERF_DIAG current perf.rs mapping reads: instructions={} cycles={} ipc={:.3}",
            snap.instructions, snap.cycles, snap.ipc()
        );
    }

    // Worker mode emits structured failure on stderr + a TIMEOUT-shaped
    // line on stdout so the orchestrator can categorise the failure
    // alongside other early-exit cases. Returning `None` from the
    // closure short-circuits the rest of the worker body.
    let (name, entry) = unsafe {
        match load_variant(dylib_path) {
            Ok(pair) => pair,
            Err(reason) => {
                eprintln!("  WORKER LOAD FAIL: {} :: {}", dylib_path, reason);
                println!("TIMEOUT\t<load-fail>\t{}\t0", mode);
                return;
            },
        }
    };

    let input_builder = routine.bridge.input_builder;
    let output_size = routine.bridge.output_size;
    // Use the routine's scorer iff the routine declared a score label
    // (presence of label is the consent signal that scoring is meaningful).
    let output_scorer: Option<fn(&[u8], &[u8]) -> Option<f64>> =
        routine.bridge.score_label.map(|_| routine.bridge.scorer);

    // At least one warmup call always runs, so lazy per-process state
    // (a variant caching an expensive structure across calls) is built
    // outside the preflight probe and the timed batches.
    let warmup = (runs / 5).max(1);
    let mut rng = Rng::new(seed);
    let sub_seeds = rng.seeds(warmup + runs);
    let sleep_dur = Duration::from_millis(cooldown_ms);
    let freq = counter::counter_frequency();
    let ticks_to_ns = 1_000_000_000.0 / freq as f64;

    // Persistent input for warm mode: built once, reused across calls.
    let warm_input = input_builder(seed);
    let mut warm_output = vec![0u8; output_size];

    // Warmup
    for i in 0 .. warmup {
        let s = sub_seeds[i];
        if mode == "warm" {
            workload.run_program(s, &mut |_| unsafe {
                entry(warm_input.as_ptr(), warm_output.as_mut_ptr(), n)
            });
        } else {
            let input = input_builder(s);
            let mut output = vec![0u8; output_size];
            workload.run_program(s, &mut |_| unsafe {
                entry(input.as_ptr(), output.as_mut_ptr(), n)
            });
        }
    }

    // Pre-flight probe: run one call and check wall-clock time.
    // In worker mode, a panic just kills this subprocess (safe).
    if let Some(limit_us) = max_call_us {
        let probe_start = Instant::now();
        unsafe {
            entry(warm_input.as_ptr(), warm_output.as_mut_ptr(), n);
        }
        let probe_us = probe_start.elapsed().as_micros() as u64;
        if probe_us > limit_us * 10 {
            eprintln!(
                "  PREFLIGHT: {} ({}) single call {}us > {}us limit*10, skipping",
                name, mode, probe_us, limit_us
            );
            println!("TIMEOUT\t{}\t{}\t{}", name, mode, probe_us);
            return;
        }
    }

    // Timed batches: emit one line per batch (preserves distribution).
    let batches = runs / batch_size;
    // Cold mode: keep last input/output alive for scoring without leaking.
    let mut last_cold_input: Option<Vec<u8>> = None;
    let mut last_cold_output: Option<Vec<u8>> = None;

    for b in 0 .. batches {
        let base = warmup + b * batch_size;
        let mut batch_e2e_ticks = 0u64;
        let mut batch_algo_ticks = 0u64;
        let mut batch_bridge_ticks = 0u64;
        // host-side PMU deltas over the measured region, accumulated per batch.
        // Zero when perf counters are unavailable / off (the reads return zeros).
        let mut batch_instructions = 0u64;
        let mut batch_cycles = 0u64;
        // matrix-scaffold data carried on the FfiBenchCall: the one-time setup
        // cost S and the cold first-touch pass (both summed per program like the
        // algo ticks, then meaned over the batch), plus the reps-invariant
        // fidelity digest (a witness, captured not summed). All zero for plain
        // timed! variants and in the amortised-batch path, which does not capture
        // the call result.
        let mut batch_setup_ticks = 0u64;
        let mut batch_first_ticks = 0u64;
        let mut batch_digest = 0u64;

        if batch_k > 1 {
            for i in 0 .. batch_size {
                let pf_start = crate::perf::read();
                let fw_start = counter::read_counter();
                if mode == "warm" {
                    for _ in 0 .. batch_k {
                        unsafe {
                            entry(warm_input.as_ptr(), warm_output.as_mut_ptr(), n);
                        }
                    }
                } else {
                    let s = sub_seeds[base + i];
                    let input = input_builder(s);
                    let mut output = vec![0u8; output_size];
                    for _ in 0 .. batch_k {
                        unsafe {
                            entry(input.as_ptr(), output.as_mut_ptr(), n);
                        }
                    }
                    last_cold_input = Some(input);
                    last_cold_output = Some(output);
                }
                let fw_end = counter::read_counter();
                // bracket PMU symmetrically with the timer and divide by batch_k
                // for a per-call count, so amortized-batch samples carry real
                // counters too (not zeros that would dilute the per-variant mean).
                let pd = crate::perf::read().delta(&pf_start);
                let bk = batch_k as u64;
                batch_instructions += pd.instructions / bk;
                batch_cycles += pd.cycles / bk;
                let per_call = (fw_end - fw_start) / bk;
                batch_algo_ticks += per_call;
                batch_e2e_ticks += per_call;
            }
        } else {
            for i in 0 .. batch_size {
                let s = sub_seeds[base + i];
                let pf_start = crate::perf::read();
                let fw_start = counter::read_counter();
                let wall_start = Instant::now();
                let mut algo_accum = 0u64;
                // outer timer around entry()
                let mut call_accum = 0u64;
                // matrix-scaffold fields off the FfiBenchCall, per program.
                let mut setup_accum = 0u64;
                let mut first_accum = 0u64;
                let mut digest_last = 0u64;

                if mode == "warm" {
                    workload.run_program(s, &mut |_| {
                        let call_start = counter::read_counter();
                        let result =
                            unsafe { entry(warm_input.as_ptr(), warm_output.as_mut_ptr(), n) };
                        let call_end = counter::read_counter();
                        algo_accum += result.run_ticks;
                        setup_accum += result.setup_ticks;
                        first_accum += result.first_ticks;
                        digest_last = result.digest;
                        call_accum += call_end - call_start;
                        result
                    });
                } else {
                    let input = input_builder(s);
                    let mut output = vec![0u8; output_size];
                    workload.run_program(s, &mut |_| {
                        let call_start = counter::read_counter();
                        let result = unsafe { entry(input.as_ptr(), output.as_mut_ptr(), n) };
                        let call_end = counter::read_counter();
                        algo_accum += result.run_ticks;
                        setup_accum += result.setup_ticks;
                        first_accum += result.first_ticks;
                        digest_last = result.digest;
                        call_accum += call_end - call_start;
                        result
                    });
                    last_cold_input = Some(input);
                    last_cold_output = Some(output);
                }

                // Per-call wall-clock timeout: abort mid-batch if a single
                // program invocation exceeded the budget.
                if let Some(limit_us) = max_call_us {
                    let wall_us = wall_start.elapsed().as_micros() as u64;
                    if wall_us > limit_us {
                        eprintln!(
                            "  TIMEOUT: {} ({}) call {}us > limit {}us at batch {}/call {}",
                            name, mode, wall_us, limit_us, b, i
                        );
                        println!("TIMEOUT\t{}\t{}\t{}", name, mode, wall_us);
                        return;
                    }
                }

                let fw_end = counter::read_counter();
                let pd = crate::perf::read().delta(&pf_start);
                batch_instructions += pd.instructions;
                batch_cycles += pd.cycles;
                batch_algo_ticks += algo_accum;
                batch_setup_ticks += setup_accum;
                batch_first_ticks += first_accum;
                // digest is a fidelity witness, reps-invariant and identical
                // across programs; the last one seen represents the batch.
                batch_digest = digest_last;
                // bridge = conversion overhead = (outer algo - inner algo)
                let bridge = call_accum.saturating_sub(algo_accum);
                // e2e = program time minus conversion overhead
                batch_e2e_ticks += (fw_end - fw_start).saturating_sub(bridge);
                batch_bridge_ticks += bridge;
            }
        }

        // Per-batch mean
        let count = batch_size as f64;
        let e2e_ns = (batch_e2e_ticks as f64 / count) * ticks_to_ns;
        let algo_ns = (batch_algo_ticks as f64 / count) * ticks_to_ns;
        let bridge_ns = (batch_bridge_ticks as f64 / count) * ticks_to_ns;
        // matrix-scaffold means (0 for plain timed! variants / amortised path).
        let setup_ns = (batch_setup_ticks as f64 / count) * ticks_to_ns;
        let first_ns = (batch_first_ticks as f64 / count) * ticks_to_ns;

        // Score at multiple evenly-spaced points within each batch in
        // normal (non-batch-amortised) warm mode, average the scores.
        // In cold mode and batch-amortised mode: score last call only.
        let batch_score = output_scorer.and_then(|scorer| {
            if mode == "warm" && batch_k == 1 {
                let mut scores: Vec<f64> = Vec::new();
                let positions = [0usize, batch_size / 2, batch_size.saturating_sub(1)];
                for &_pos in &positions {
                    // warm_input is stable; re-call entry to get a fresh output.
                    let mut tmp_out = vec![0u8; output_size];
                    unsafe {
                        entry(warm_input.as_ptr(), tmp_out.as_mut_ptr(), n);
                    }
                    if let Some(s) = scorer(&warm_input, &tmp_out) {
                        scores.push(s);
                    }
                }
                if scores.is_empty() {
                    None
                } else {
                    Some(scores.iter().sum::<f64>() / scores.len() as f64)
                }
            } else if mode == "warm" {
                // batch_k > 1: score last state only
                scorer(&warm_input, &warm_output)
            } else {
                match (&last_cold_input, &last_cold_output) {
                    (Some(inp), Some(out)) => scorer(inp, out),
                    _ => None,
                }
            }
        });

        // per-call PMU means for this batch (0 when counters are off).
        let bs = batch_size.max(1) as u64;
        let instr_per_call = batch_instructions / bs;
        let cycles_per_call = batch_cycles / bs;

        // One line per batch. instructions/cycles sit at fixed columns 7,8; the
        // always-present matrix columns setup_ns/first_ns/digest at 9,10,11; and
        // the optional score at 12. The parser reads them positionally.
        if let Some(s) = batch_score {
            println!(
                "{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{}\t{:.2}",
                name, mode, b, e2e_ns, algo_ns, bridge_ns, batch_size,
                instr_per_call, cycles_per_call, setup_ns, first_ns, batch_digest, s
            );
        } else {
            println!(
                "{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{}",
                name, mode, b, e2e_ns, algo_ns, bridge_ns, batch_size,
                instr_per_call, cycles_per_call, setup_ns, first_ns, batch_digest
            );
        }

        // Timeout check: if batch mean exceeds max_call_us, abort early.
        if let Some(limit_us) = max_call_us {
            let algo_us = algo_ns / 1000.0;
            if algo_us > limit_us as f64 {
                eprintln!(
                    "  TIMEOUT: {} ({}) batch {} mean {:.0}us > limit {}us, aborting",
                    name, mode, b, algo_us, limit_us
                );
                println!("TIMEOUT\t{}\t{}\t{:.0}", name, mode, algo_us);
                return;
            }
        }

        if cooldown_ms > 0 {
            sleep(sleep_dur);
        }
    }
}

// ── Orchestrator mode ──

/// Run the full benchmark by spawning worker subprocesses, one per
/// `(variant × cooldown × pass × mode)` combination.
///
/// The orchestrator re-execs the current binary (`std::env::current_exe()`)
/// with `--worker` plus per-call args. The worker dispatches to
/// [`run_worker`].
pub fn run_orchestrator(
    config: &BenchConfig,
    routine: &RoutineSpec,
    workload: &Workload,
) -> Result<BenchResult, BenchError> {
    if config.variant_paths.is_empty() {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{}` has no variant_paths; nothing to run",
                config.bench_name
            ),
        });
    }

    let nc = config.cooldowns_ms.len();
    let nv = config.variant_paths.len();
    let mut all_samples = Vec::new();
    let total_start = Instant::now();

    let exe = std::env::current_exe()
        .map_err(|e| BenchError::io("locating own binary for worker spawn", e))?;

    let variant_path_strings: Vec<String> = config
        .variant_paths
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    // Throwaway warmup spawn to warm the process-spawn path before
    // real benchmarking.
    if let Some(first_variant) = variant_path_strings.first() {
        let _ = Command::new(&exe)
            .args([
                "--worker",
                first_variant,
                "--bench-name",
                &config.bench_name,
                "--seed",
                "0",
                "--cooldown",
                "0",
                "--mode",
                "warm",
                "--runs",
                "1",
                "--batch",
                "1",
                "--n",
                &config.n.to_string(),
                "--batch-k",
                "1",
            ])
            .output();
        eprintln!("  Warmup spawn complete.");
    }

    let _ = (routine, workload);
    let input_tagger = routine.bridge.input_tagger;

    for hr in 0 .. config.harness_runs {
        eprintln!("  ── Harness run {}/{} ──", hr + 1, config.harness_runs);

        let seed_source = if config.master_seed != 0 {
            config.master_seed
        } else {
            // Wall-clock entropy; the prior `Instant::now().elapsed()`
            // form measured a freshly created Instant and was near-zero
            // (and thus near-constant) every run.
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x9E37_79B9)
        };
        let mut rng = Rng::new(seed_source.wrapping_add(hr as u64 * 0xDEADBEEF));

        let total_passes = config.passes * nc;
        let seeds: Vec<u64> = (0 .. total_passes).map(|_| rng.next()).collect();

        for pass_idx in 0 .. total_passes {
            let ci = pass_idx % nc;
            let cd = config.cooldowns_ms[ci];
            let seed = seeds[pass_idx];
            let pass_num = pass_idx / nc + 1;

            // Randomise variant order
            let mut variant_order: Vec<usize> = (0 .. nv).collect();
            let mut h = mix(seed);
            for i in (1 .. nv).rev() {
                h = mix(h);
                let j = (h as usize) % (i + 1);
                variant_order.swap(i, j);
            }

            eprint!("  pass {}/{} cd={}ms...", pass_num, config.passes, cd);

            // Each mode gets its own subprocess for clean cache state
            for mode in &["warm", "cold"] {
                for &vi in &variant_order {
                    let variant_path = &variant_path_strings[vi];

                    // Subprocess timeout: max_call_us * runs * 3, clamped
                    // to 5s minimum. Prevents a hung worker from stalling
                    // the entire harness.
                    let subprocess_timeout_s = config
                        .max_call_us
                        .map(|us| {
                            ((us as f64 * config.runs_per_pass as f64 * 3.0) / 1_000_000.0)
                                .max(5.0)
                                .ceil() as u64
                        })
                        .unwrap_or(300);

                    let mut cmd = Command::new(&exe);
                    if config.threaded {
                        cmd.arg("--threaded");
                    }
                    let mut child = cmd
                        .args([
                            "--worker",
                            variant_path,
                            "--bench-name",
                            &config.bench_name,
                            "--seed",
                            &seed.to_string(),
                            "--cooldown",
                            &cd.to_string(),
                            "--mode",
                            mode,
                            "--runs",
                            &config.runs_per_pass.to_string(),
                            "--batch",
                            &config.batch_size.to_string(),
                            "--n",
                            &config.n.to_string(),
                            "--batch-k",
                            &config.batch_k.to_string(),
                            "--max-call-us",
                            &config
                                .max_call_us
                                .map(|v| v.to_string())
                                .unwrap_or_else(|| "0".into()),
                        ])
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn()
                        .map_err(|e| BenchError::io("spawning worker subprocess", e))?;

                    let deadline = Instant::now() + Duration::from_secs(subprocess_timeout_s);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => break,
                            Ok(None) => {
                                if Instant::now() > deadline {
                                    let _ = child.kill();
                                    let _ = child.wait();
                                    eprintln!(
                                        " {} ({}) subprocess killed after {}s",
                                        variant_path, mode, subprocess_timeout_s
                                    );
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(10));
                            },
                            Err(e) => {
                                return Err(BenchError::io("waiting on worker subprocess", e));
                            },
                        }
                    }

                    let output = child
                        .wait_with_output()
                        .map_err(|e| BenchError::io("collecting worker output", e))?;

                    if !output.status.success() {
                        eprintln!(
                            " {} ({}) FAILED: {}",
                            variant_path,
                            mode,
                            String::from_utf8_lossy(&output.stderr).trim()
                        );
                        continue;
                    }

                    // Parse per-batch lines from worker stdout
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.trim().lines() {
                        if line.starts_with("TIMEOUT") {
                            eprintln!("  {}", line);
                            continue;
                        }
                        let parts: Vec<&str> = line.split('\t').collect();
                        if parts.len() >= 9 {
                            all_samples.push(Sample {
                                run:         hr + 1,
                                pass:        pass_num,
                                cooldown_ms: cd,
                                mode:        parts[1].to_string(),
                                variant:     parts[0].to_string(),
                                batch_idx:   parts[2].parse().unwrap_or(0),
                                e2e_ns:      parts[3].parse().unwrap_or(0.0),
                                algo_ns:     parts[4].parse().unwrap_or(0.0),
                                bridge_ns:   parts[5].parse().unwrap_or(0.0),
                                batch_count: parts[6].parse().unwrap_or(0),
                                // instructions/cycles at fixed 7,8; matrix
                                // setup_ns/first_ns/digest at 9,10,11; optional
                                // score at 12.
                                instructions: parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0),
                                cycles:       parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
                                setup_ns:    parts.get(9).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                                first_ns:    parts.get(10).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                                digest:      parts.get(11).and_then(|s| s.parse().ok()).unwrap_or(0),
                                score:       parts.get(12).and_then(|s| s.parse().ok()),
                                input_tag:   input_tagger.map(|f| f(seed).1),
                            });
                        }
                    }
                }
            }
            let done_passes = hr * total_passes + pass_idx + 1;
            let all_passes = config.harness_runs * total_passes;
            let el = total_start.elapsed().as_secs_f64();
            let eta = el / done_passes as f64 * (all_passes - done_passes) as f64;
            eprintln!(" done ({el:.0}s elapsed, ~{eta:.0}s left)");
        }
    }

    let total_secs = total_start.elapsed().as_secs_f64();
    eprintln!("  Total: {:.1}s", total_secs);

    Ok(BenchResult {
        title:       config.title.clone(),
        env:         collect_env_meta(),
        samples:     all_samples,
        cache_path:  String::new(),
        report_path: String::new(),
    })
}

// ── CSV + meta.json writer ──

/// Write the result samples to a CSV at `path` plus a sidecar
/// `<path>.meta.json` carrying [`EnvMeta`].
pub fn write_csv(result: &BenchResult, path: &str) -> Result<(), BenchError> {
    let mut csv = String::from(
        "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag,instructions,cycles,setup_ns,first_ns,digest\n",
    );
    for s in &result.samples {
        let score_str = s.score.map(|v| format!("{:.2}", v)).unwrap_or_default();
        let tag_str = s.input_tag.map(|v| v.to_string()).unwrap_or_default();
        csv.push_str(&format!(
            "{},{},{},{},{},{},{:.1},{:.1},{:.1},{},{},{},{},{},{:.1},{:.1},{}\n",
            s.run,
            s.pass,
            s.cooldown_ms,
            s.mode,
            s.variant,
            s.batch_idx,
            s.e2e_ns,
            s.algo_ns,
            s.bridge_ns,
            s.batch_count,
            score_str,
            tag_str,
            s.instructions,
            s.cycles,
            s.setup_ns,
            s.first_ns,
            s.digest
        ));
    }
    std::fs::write(path, &csv).map_err(|e| BenchError::io("writing csv", e))?;
    eprintln!("  CSV: {} ({} rows)", path, result.samples.len());

    let json = env_meta_to_json(&result.env);
    let meta_path = meta_json_path(path);
    std::fs::write(&meta_path, &json).map_err(|e| BenchError::io("writing meta.json", e))?;
    eprintln!("  Meta: {}", meta_path);
    Ok(())
}

/// Validation worker: run the variant once per seed (twice, for the
/// determinism pair) in THIS subprocess and emit outputs on stdout as
/// `VOUT\t<seed>\t<hex_first>\t<hex_second>` lines. The variant's
/// cached per-process state (the setup-once pattern) lives and dies
/// with this process, so orchestrator memory stays bounded no matter
/// how many variants or sizes a run visits.
pub fn run_worker_validate(
    routine: &RoutineSpec,
    dylib_path: &str,
    seeds: &[u64],
    n: usize,
    threaded: bool,
) {
    if !threaded {
        counter::pin_to_perf_cores();
    }
    let (_name, entry) = unsafe {
        match load_variant(dylib_path) {
            Ok(pair) => pair,
            Err(reason) => {
                // Zero VOUT lines already read as a skip in the
                // orchestrator; no protocol token needed.
                eprintln!("  WORKER LOAD FAIL: {} :: {}", dylib_path, reason);
                return;
            },
        }
    };
    let input_builder = routine.bridge.input_builder;
    let output_size = routine.bridge.output_size;
    for &seed in seeds {
        let input = input_builder(seed);
        let mut out_first = vec![0u8; output_size];
        let mut out_second = vec![0u8; output_size];
        unsafe {
            entry(input.as_ptr(), out_first.as_mut_ptr(), n);
            entry(input.as_ptr(), out_second.as_mut_ptr(), n);
        }
        println!(
            "VOUT\t{}\t{}\t{}",
            seed,
            to_hex(&out_first),
            to_hex(&out_second)
        );
    }
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod env_meta_json_tests {
    use super::*;

    #[test]
    fn the_profile_field_carries_the_passed_value_and_is_absent_when_unknown() {
        let meta = crate::env::EnvMeta::default();
        let with = env_meta_to_json_with(&meta, Some("opt-level=0,lto=\"off\",codegen-units=16"));
        assert!(
            with.contains("\"build_profile\":\"opt-level=0,lto=\\\"off\\\",codegen-units=16\""),
            "{with}"
        );
        let without = env_meta_to_json_with(&meta, None);
        assert!(
            !without.contains("build_profile"),
            "no claim beats a wrong one: {without}"
        );
        assert!(
            !without.contains("opt-level=3"),
            "the old hardcoded literal must be gone: {without}"
        );
    }
}
