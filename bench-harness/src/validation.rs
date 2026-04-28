//! Validation pass: load all variant dylibs in-process, run with
//! deterministic seeds, compare outputs. Runs before any timing.
//! Returns [`BenchError::ValidationFailed`] on mismatch.
//!
//! Three modes:
//!
//! - **Per-variant validity check** (when the routine implements
//!   [`mockspace_bench_core::Routine::validate_output`]): each
//!   variant's output is checked individually. Outputs may differ
//!   across variants as long as each is valid (e.g. graph coloring
//!   may pick different but equally-valid colourings).
//! - **Approximate cross-variant comparison** (when the routine
//!   declares [`mockspace_bench_core::Routine::max_relative_error`]
//!   as `Some(eps)`): outputs are compared element-wise as f64 slices
//!   with relative-error tolerance.
//! - **Byte-exact cross-variant comparison** (default): all variants
//!   must produce identical bytes.

use std::collections::HashSet;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::config::HarnessTuning;
use crate::core::counter::Rng;
use crate::core::{abi_hash, AbiHashFn, BenchEntryFn, BenchNameFn};
use crate::error::BenchError;
use crate::spec::RoutineSpec;

/// Default seed count for [`validate`] when callers do not supply a
/// [`HarnessTuning`].
pub const DEFAULT_VALIDATION_SEEDS: usize = 100;
const VALIDATION_ROOT_SEED: u64 = 0xCAFE_BABE_DEAD_BEEF;
/// Default seed count for the determinism check, a subset of
/// [`DEFAULT_VALIDATION_SEEDS`].
pub const DEFAULT_DETERMINISM_CHECK_SEEDS: usize = 10;

/// Validate all variant cdylibs against the given [`RoutineSpec`].
///
/// Returns the subset of `variant_paths` that survived the probes
/// (variants that crashed or timed out at the probe stage are
/// excluded so the orchestrator can still proceed without them).
///
/// The validation strategy is selected from the routine bridge:
///
/// - If the routine has a custom validator (set via the
///   `Routine::validate_output` default override), per-variant
///   validity is checked.
/// - Else if the routine declares `max_relative_error = Some(eps)`,
///   cross-variant comparison uses the routine's `compare_outputs_approx`
///   with that tolerance.
/// - Else byte-exact cross-variant comparison.
///
/// `outputs_may_differ = true` on the routine bridge skips
/// cross-variant byte comparison (the per-variant validator alone is
/// authoritative).
pub fn validate(
    routine: &RoutineSpec,
    variant_paths: &[String],
    n: usize,
    bench_name: &str,
    max_call_us: Option<u64>,
    tuning: Option<&HarnessTuning>,
) -> Result<Vec<String>, BenchError> {
    if variant_paths.len() < 2 {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "validation needs at least 2 variants, got {}",
                variant_paths.len()
            ),
        });
    }

    let validation_seeds = tuning
        .map(|t| t.validation_seeds)
        .unwrap_or(DEFAULT_VALIDATION_SEEDS);
    let determinism_check_seeds = tuning
        .map(|t| t.determinism_check_seeds)
        .unwrap_or(DEFAULT_DETERMINISM_CHECK_SEEDS);

    let input_builder = routine.bridge.input_builder;
    let output_size = routine.bridge.output_size;
    let approx_eps = routine.bridge.max_relative_error;
    let approx_comparator = routine.bridge.approx_comparator;
    // The validator is only meaningful when the Routine actually
    // declared one; we cannot tell from the bridge alone, so use
    // outputs_may_differ as the consent signal.
    let validator: Option<fn(&[u8], &[u8]) -> Result<(), String>> =
        if routine.bridge.outputs_may_differ {
            Some(routine.bridge.validator)
        } else {
            None
        };

    let mut rng = Rng::new(VALIDATION_ROOT_SEED);
    let seeds: Vec<u64> = (0..validation_seeds).map(|_| rng.next()).collect();

    let mut variants: Vec<(String, BenchEntryFn)> = Vec::new();
    let mut _libs: Vec<libloading::Library> = Vec::new();

    for path in variant_paths {
        let (name, entry) = unsafe {
            let lib = libloading::Library::new(path).map_err(|e| {
                BenchError::DylibLoadFailed {
                    path: path.into(),
                    reason: e.to_string(),
                }
            })?;

            let hash_fn: libloading::Symbol<AbiHashFn> = lib
                .get(b"bench_abi_hash")
                .map_err(|e| BenchError::DylibLoadFailed {
                    path: path.into(),
                    reason: format!("missing bench_abi_hash symbol: {e}"),
                })?;
            let found = hash_fn();
            let expected = abi_hash();
            if found != expected {
                return Err(BenchError::AbiMismatch {
                    path: path.into(),
                    expected,
                    found,
                });
            }

            let entry: libloading::Symbol<BenchEntryFn> = lib
                .get(b"bench_entry")
                .map_err(|e| BenchError::DylibLoadFailed {
                    path: path.into(),
                    reason: format!("missing bench_entry symbol: {e}"),
                })?;
            let entry_fn: BenchEntryFn = *entry;

            let name_fn: libloading::Symbol<BenchNameFn> = lib
                .get(b"bench_name")
                .map_err(|e| BenchError::DylibLoadFailed {
                    path: path.into(),
                    reason: format!("missing bench_name symbol: {e}"),
                })?;
            let name = std::ffi::CStr::from_ptr(name_fn() as *const i8)
                .to_string_lossy()
                .into_owned();

            _libs.push(lib);
            (name, entry_fn)
        };
        variants.push((name, entry));
    }

    let names: Vec<String> = variants.iter().map(|(n, _)| n.clone()).collect();

    // Pre-flight: probe each variant via a subprocess worker call.
    // Catches exponential-time variants before the full validation
    // loop. Variants that crash or time out are skipped, not aborted.
    let mut slow_variants: HashSet<usize> = HashSet::new();
    if let Some(limit_us) = max_call_us {
        let probe_timeout_s = ((limit_us as f64 * 10.0) / 1_000_000.0).max(2.0).ceil() as u64;
        let exe = std::env::current_exe().unwrap_or_default();
        for (vi, (name, _entry)) in variants.iter().enumerate() {
            let variant_path = &variant_paths[vi];
            let mut child = Command::new(&exe)
                .args([
                    "--worker", variant_path,
                    "--bench-name", bench_name,
                    "--mode", "warm",
                    "--runs", "1",
                    "--batch", "1",
                    "--n", &n.to_string(),
                    "--max-call-us", &limit_us.to_string(),
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| BenchError::io("spawning validation probe", e))?;

            let deadline = Instant::now() + Duration::from_secs(probe_timeout_s);
            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            eprintln!(
                                "  SKIPPING {}: probe crashed (exit {:?})",
                                name,
                                status.code()
                            );
                            slow_variants.insert(vi);
                        }
                        break;
                    }
                    Ok(None) => {
                        if Instant::now() > deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            eprintln!(
                                "  SKIPPING {}: probe exceeded {}s",
                                name, probe_timeout_s
                            );
                            slow_variants.insert(vi);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => {
                        return Err(BenchError::io("waiting on validation probe", e));
                    }
                }
            }
        }

        // Multi-seed probe to catch seed-dependent panics.
        let exe = std::env::current_exe().unwrap_or_default();
        let probe_seeds: Vec<u64> = seeds
            .iter()
            .step_by(validation_seeds / 20)
            .cloned()
            .collect();
        for (vi, (name, _entry)) in variants.iter().enumerate() {
            if slow_variants.contains(&vi) {
                continue;
            }
            let variant_path = &variant_paths[vi];
            for &ps in &probe_seeds {
                let out = Command::new(&exe)
                    .args([
                        "--worker", variant_path,
                        "--bench-name", bench_name,
                        "--mode", "warm",
                        "--runs", "1", "--batch", "1",
                        "--n", &n.to_string(),
                        "--seed", &ps.to_string(),
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .output();
                if let Ok(o) = out {
                    if !o.status.success() {
                        eprintln!("  SKIPPING {}: crashed on seed {}", name, ps);
                        slow_variants.insert(vi);
                        break;
                    }
                }
            }
        }
    }

    let active_count = variants.len() - slow_variants.len();
    eprintln!(
        "  Validating {} variants × {} seeds...",
        active_count, validation_seeds
    );

    let mut mismatches = 0usize;
    let mut first_mismatch_reason: Option<(String, String)> = None;

    for (si, &seed) in seeds.iter().enumerate() {
        let input = input_builder(seed);

        let mut outputs: Vec<Vec<u8>> = Vec::new();

        for (vi, (_name, entry)) in variants.iter().enumerate() {
            if slow_variants.contains(&vi) {
                outputs.push(vec![0u8; output_size]);
                continue;
            }
            let mut output = vec![0u8; output_size];
            unsafe {
                entry(input.as_ptr(), output.as_mut_ptr(), n);
            }
            outputs.push(output);
        }

        if let Some(validator) = &validator {
            for (i, output) in outputs.iter().enumerate() {
                if slow_variants.contains(&i) {
                    continue;
                }
                if let Err(reason) = validator(&input, output) {
                    mismatches += 1;
                    if mismatches <= 3 {
                        eprintln!(
                            "  INVALID seed={} variant={}: {}",
                            seed, names[i], reason
                        );
                    }
                    first_mismatch_reason
                        .get_or_insert((names[i].clone(), format!("invalid output: {reason}")));
                }
            }
        } else if let Some(eps) = approx_eps {
            let baseline = &outputs[0];
            for i in 1..outputs.len() {
                if let Err(reason) = approx_comparator(baseline, &outputs[i], eps) {
                    mismatches += 1;
                    if mismatches <= 3 {
                        eprintln!("  APPROX MISMATCH seed={} (#{}):", seed, si);
                        eprintln!("    {} vs {}: {}", names[0], names[i], reason);
                    }
                    first_mismatch_reason.get_or_insert((
                        names[i].clone(),
                        format!("approx mismatch vs {}: {reason}", names[0]),
                    ));
                }
            }
        } else {
            let baseline = &outputs[0];
            for i in 1..outputs.len() {
                if outputs[i] != *baseline {
                    mismatches += 1;
                    if mismatches <= 3 {
                        eprintln!("  MISMATCH seed={} (#{}):", seed, si);
                        eprintln!("    {} vs {}", names[0], names[i]);
                        for (j, (a, b)) in baseline.iter().zip(outputs[i].iter()).enumerate() {
                            if a != b {
                                eprintln!("    first diff at byte {}: {} vs {}", j, a, b);
                                break;
                            }
                        }
                    }
                    first_mismatch_reason.get_or_insert((
                        names[i].clone(),
                        format!("byte mismatch vs {}", names[0]),
                    ));
                }
            }
        }
    }

    if mismatches > 0 {
        let (variant, reason) = first_mismatch_reason.unwrap_or_else(|| {
            ("<unknown>".to_string(), "validation produced mismatches".to_string())
        });
        return Err(BenchError::ValidationFailed {
            variant,
            reason: format!(
                "{mismatches} mismatches across {validation_seeds} seeds; first: {reason}"
            ),
        });
    }

    if validator.is_some() {
        eprintln!(
            "  Validation OK: all {} variants produce valid output",
            variants.len()
        );
    } else {
        eprintln!(
            "  Validation OK: all {} variants produce identical output",
            variants.len()
        );
    }

    // Determinism check: call each variant twice with the same seed
    // and verify both outputs are identical.
    eprintln!(
        "  Determinism check: {} variants × {} seeds...",
        variants.len(),
        determinism_check_seeds
    );
    let mut det_mismatches = 0u32;
    let mut first_det_failure: Option<(String, String)> = None;
    for &seed in seeds.iter().take(determinism_check_seeds) {
        let input = input_builder(seed);
        for (vi, (name, entry)) in variants.iter().enumerate() {
            if slow_variants.contains(&vi) {
                continue;
            }
            let mut out1 = vec![0u8; output_size];
            let mut out2 = vec![0u8; output_size];
            unsafe {
                entry(input.as_ptr(), out1.as_mut_ptr(), n);
                entry(input.as_ptr(), out2.as_mut_ptr(), n);
            }
            if out1 != out2 {
                det_mismatches += 1;
                if det_mismatches <= 3 {
                    eprintln!(
                        "  NON-DETERMINISTIC seed={} variant={}: outputs differ on identical input",
                        seed, name
                    );
                    for (j, (a, b)) in out1.iter().zip(out2.iter()).enumerate() {
                        if a != b {
                            eprintln!("    first diff at byte {}: {} vs {}", j, a, b);
                            break;
                        }
                    }
                }
                first_det_failure.get_or_insert((
                    name.clone(),
                    format!("non-deterministic on seed {seed}"),
                ));
            }
        }
    }
    if det_mismatches > 0 {
        let (variant, reason) = first_det_failure.unwrap_or_else(|| {
            ("<unknown>".to_string(), "determinism check failed".to_string())
        });
        return Err(BenchError::ValidationFailed {
            variant,
            reason: format!("{det_mismatches} non-deterministic outputs detected: {reason}"),
        });
    }
    eprintln!(
        "  Determinism OK: all {} variants are deterministic",
        variants.len()
    );

    // Subprocess sanity check: run one variant through the worker
    // path to verify the subprocess harness doesn't crash and
    // produces output.
    let sanity_target_idx = if !slow_variants.contains(&0) {
        Some(0)
    } else {
        (0..variant_paths.len()).find(|i| !slow_variants.contains(i))
    };
    if let Some(idx) = sanity_target_idx {
        subprocess_sanity_check(&variant_paths[idx], n, bench_name);
    }

    let safe_paths: Vec<String> = variant_paths
        .iter()
        .enumerate()
        .filter(|(i, _)| !slow_variants.contains(i))
        .map(|(_, p)| p.clone())
        .collect();
    if safe_paths.len() < variant_paths.len() {
        eprintln!(
            "  {} variants excluded ({} safe)",
            variant_paths.len() - safe_paths.len(),
            safe_paths.len()
        );
    }
    Ok(safe_paths)
}

/// Run ONE variant through the worker subprocess path as a sanity
/// check. Uses `--worker --mode warm --runs 1 --batch 1`. Verifies
/// non-crash + non-empty output.
fn subprocess_sanity_check(variant_path: &str, n: usize, bench_name: &str) {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("  Subprocess sanity: could not locate harness binary, skipping");
            return;
        }
    };

    eprintln!("  Subprocess sanity: {} (1 run, warm)...", variant_path);
    let output = Command::new(&exe)
        .args([
            "--worker", variant_path,
            "--bench-name", bench_name,
            "--mode", "warm",
            "--runs", "1",
            "--batch", "1",
            "--n", &n.to_string(),
        ])
        .output();

    match output {
        Ok(out) => {
            if !out.status.success() {
                eprintln!(
                    "  Subprocess sanity FAILED: worker exited with {:?}\n  stderr: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            } else if out.stdout.is_empty() {
                eprintln!("  Subprocess sanity FAILED: worker produced no output");
            } else {
                eprintln!("  Subprocess sanity OK");
            }
        }
        Err(e) => {
            eprintln!("  Subprocess sanity: spawn failed: {}", e);
        }
    }
}
