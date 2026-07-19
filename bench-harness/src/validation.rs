//! Validation pass: run every variant across deterministic seeds and
//! compare outputs. Runs before any timing. Each variant executes in
//! its own worker subprocess (`--mode validate`), so a variant's
//! cached per-process state (the setup-once pattern) lives and dies
//! with its worker; the orchestrator's memory stays bounded no matter
//! how many variants and sizes a run visits. The driver only dlopens
//! variants briefly for the ABI-hash and name checks, never calling
//! `bench_entry` in-process. Returns
//! [`BenchError::ValidationFailed`] on mismatch.
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
use crate::core::{AbiHashFn, BenchNameFn, abi_hash};
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
    let seeds: Vec<u64> = (0 .. validation_seeds).map(|_| rng.next()).collect();

    // ABI-hash and name checks only: the library is dropped right
    // after, and `bench_entry` is never called in this process.
    let mut names: Vec<String> = Vec::new();

    for path in variant_paths {
        let name = unsafe {
            let lib = libloading::Library::new(path).map_err(|e| {
                BenchError::DylibLoadFailed {
                    path:   path.into(),
                    reason: e.to_string(),
                }
            })?;

            let hash_fn: libloading::Symbol<AbiHashFn> =
                lib.get(b"bench_abi_hash").map_err(|e| {
                    BenchError::DylibLoadFailed {
                        path:   path.into(),
                        reason: format!("missing bench_abi_hash symbol: {e}"),
                    }
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

            let name_fn: libloading::Symbol<BenchNameFn> = lib.get(b"bench_name").map_err(|e| {
                BenchError::DylibLoadFailed {
                    path:   path.into(),
                    reason: format!("missing bench_name symbol: {e}"),
                }
            })?;
            let name = std::ffi::CStr::from_ptr(name_fn() as *const i8)
                .to_string_lossy()
                .into_owned();

            drop(lib);
            name
        };
        names.push(name);
    }

    // Pre-flight: probe each variant via a subprocess worker call.
    // Catches exponential-time variants before the full validation
    // loop. Variants that crash or time out are skipped, not aborted.
    let mut slow_variants: HashSet<usize> = HashSet::new();
    if let Some(limit_us) = max_call_us {
        let probe_timeout_s = ((limit_us as f64 * 10.0) / 1_000_000.0).max(2.0).ceil() as u64;
        let exe = std::env::current_exe().unwrap_or_default();
        for (vi, name) in names.iter().enumerate() {
            let variant_path = &variant_paths[vi];
            let mut child = Command::new(&exe)
                .args([
                    "--worker",
                    variant_path,
                    "--bench-name",
                    bench_name,
                    "--mode",
                    "warm",
                    "--runs",
                    "1",
                    "--batch",
                    "1",
                    "--n",
                    &n.to_string(),
                    "--max-call-us",
                    &limit_us.to_string(),
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
                    },
                    Ok(None) => {
                        if Instant::now() > deadline {
                            let _ = child.kill();
                            let _ = child.wait();
                            eprintln!("  SKIPPING {}: probe exceeded {}s", name, probe_timeout_s);
                            slow_variants.insert(vi);
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    },
                    Err(e) => {
                        return Err(BenchError::io("waiting on validation probe", e));
                    },
                }
            }
        }

        // Multi-seed probe to catch seed-dependent panics.
        let exe = std::env::current_exe().unwrap_or_default();
        let probe_seeds: Vec<u64> = seeds
            .iter()
            .step_by((validation_seeds / 20).max(1))
            .cloned()
            .collect();
        for (vi, name) in names.iter().enumerate() {
            if slow_variants.contains(&vi) {
                continue;
            }
            let variant_path = &variant_paths[vi];
            for &ps in &probe_seeds {
                let out = Command::new(&exe)
                    .args([
                        "--worker",
                        variant_path,
                        "--bench-name",
                        bench_name,
                        "--mode",
                        "warm",
                        "--runs",
                        "1",
                        "--batch",
                        "1",
                        "--n",
                        &n.to_string(),
                        "--seed",
                        &ps.to_string(),
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

    let active_count = names.len() - slow_variants.len();
    eprintln!(
        "  Validating {} variants × {} seeds...",
        active_count, validation_seeds
    );

    // One validate worker per variant runs ALL seeds: the expensive
    // per-process state builds once per variant, inside a process
    // that exits afterwards. Each VOUT line carries the seed's output
    // twice (the entry is called twice per seed), which doubles as
    // the determinism pair checked below.
    let exe = std::env::current_exe().unwrap_or_default();
    let seeds_arg = seeds
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut collected: Vec<Vec<(Vec<u8>, Vec<u8>)>> = Vec::new();
    for (vi, name) in names.iter().enumerate() {
        if slow_variants.contains(&vi) {
            collected.push(Vec::new());
            continue;
        }
        let out = Command::new(&exe)
            .args([
                "--worker",
                &variant_paths[vi],
                "--bench-name",
                bench_name,
                "--mode",
                "validate",
                "--n",
                &n.to_string(),
                "--seeds",
                &seeds_arg,
            ])
            .stderr(std::process::Stdio::inherit())
            .output()
            .map_err(|e| BenchError::io("spawning validation worker", e))?;
        if !out.status.success() {
            eprintln!(
                "  SKIPPING {}: validation worker crashed (exit {:?})",
                name,
                out.status.code()
            );
            slow_variants.insert(vi);
            collected.push(Vec::new());
            continue;
        }
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let mut parts = line.split('\t');
            if parts.next() != Some("VOUT") {
                continue;
            }
            let _seed = parts.next();
            let a = parts.next().and_then(from_hex);
            let b = parts.next().and_then(from_hex);
            if let (Some(a), Some(b)) = (a, b) {
                if a.len() == output_size && b.len() == output_size {
                    rows.push((a, b));
                }
            }
        }
        if rows.len() != seeds.len() {
            eprintln!(
                "  SKIPPING {}: validation worker returned {} of {} outputs",
                name,
                rows.len(),
                seeds.len()
            );
            slow_variants.insert(vi);
            collected.push(Vec::new());
            continue;
        }
        collected.push(rows);
    }

    // Recount after collection: worker crashes and truncated output
    // above grow `slow_variants`, and the OK summaries below must not
    // claim a skipped variant validated.
    let active_count = names.len() - slow_variants.len();

    let mut mismatches = 0usize;
    let mut first_mismatch_reason: Option<(String, String)> = None;
    // Baseline for cross-variant comparison: the first variant that
    // survived (previously index 0 unconditionally, which compared
    // zero-filled placeholder output when variant 0 had been skipped).
    let base_idx = (0 .. names.len()).find(|i| !slow_variants.contains(i));

    for (si, &seed) in seeds.iter().enumerate() {
        if let Some(validator) = &validator {
            let input = input_builder(seed);
            for i in 0 .. names.len() {
                if slow_variants.contains(&i) {
                    continue;
                }
                let output = &collected[i][si].0;
                if let Err(reason) = validator(&input, output) {
                    mismatches += 1;
                    if mismatches <= 3 {
                        eprintln!("  INVALID seed={} variant={}: {}", seed, names[i], reason);
                    }
                    first_mismatch_reason
                        .get_or_insert((names[i].clone(), format!("invalid output: {reason}")));
                }
            }
        } else if let Some(eps) = approx_eps {
            let Some(b0) = base_idx else { break };
            let baseline = &collected[b0][si].0;
            for i in (b0 + 1) .. names.len() {
                if slow_variants.contains(&i) {
                    continue;
                }
                if let Err(reason) = approx_comparator(baseline, &collected[i][si].0, eps) {
                    mismatches += 1;
                    if mismatches <= 3 {
                        eprintln!("  APPROX MISMATCH seed={} (#{}):", seed, si);
                        eprintln!("    {} vs {}: {}", names[b0], names[i], reason);
                    }
                    first_mismatch_reason.get_or_insert((
                        names[i].clone(),
                        format!("approx mismatch vs {}: {reason}", names[b0]),
                    ));
                }
            }
        } else {
            let Some(b0) = base_idx else { break };
            let baseline = &collected[b0][si].0;
            for i in (b0 + 1) .. names.len() {
                if slow_variants.contains(&i) {
                    continue;
                }
                if collected[i][si].0 != *baseline {
                    mismatches += 1;
                    if mismatches <= 3 {
                        eprintln!("  MISMATCH seed={} (#{}):", seed, si);
                        eprintln!("    {} vs {}", names[b0], names[i]);
                        for (j, (a, b)) in baseline.iter().zip(collected[i][si].0.iter()).enumerate() {
                            if a != b {
                                eprintln!("    first diff at byte {}: {} vs {}", j, a, b);
                                break;
                            }
                        }
                    }
                    first_mismatch_reason.get_or_insert((
                        names[i].clone(),
                        format!("byte mismatch vs {}", names[b0]),
                    ));
                }
            }
        }
    }

    if mismatches > 0 {
        let (variant, reason) = first_mismatch_reason.unwrap_or_else(|| {
            (
                "<unknown>".to_string(),
                "validation produced mismatches".to_string(),
            )
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
            active_count
        );
    } else {
        eprintln!(
            "  Validation OK: all {} variants produce identical output",
            active_count
        );
    }

    // Determinism check: call each variant twice with the same seed
    // and verify both outputs are identical.
    eprintln!(
        "  Determinism check: {} variants × {} seeds...",
        active_count, determinism_check_seeds
    );
    let mut det_mismatches = 0u32;
    let mut first_det_failure: Option<(String, String)> = None;
    for (si, &seed) in seeds.iter().take(determinism_check_seeds).enumerate() {
        for (vi, name) in names.iter().enumerate() {
            if slow_variants.contains(&vi) {
                continue;
            }
            let (out1, out2) = &collected[vi][si];
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
                first_det_failure
                    .get_or_insert((name.clone(), format!("non-deterministic on seed {seed}")));
            }
        }
    }
    if det_mismatches > 0 {
        let (variant, reason) = first_det_failure.unwrap_or_else(|| {
            (
                "<unknown>".to_string(),
                "determinism check failed".to_string(),
            )
        });
        return Err(BenchError::ValidationFailed {
            variant,
            reason: format!("{det_mismatches} non-deterministic outputs detected: {reason}"),
        });
    }
    eprintln!(
        "  Determinism OK: all {} variants are deterministic",
        active_count
    );

    // Subprocess sanity check: run one variant through the worker
    // path to verify the subprocess harness doesn't crash and
    // produces output.
    let sanity_target_idx = if !slow_variants.contains(&0) {
        Some(0)
    } else {
        (0 .. variant_paths.len()).find(|i| !slow_variants.contains(i))
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
        },
    };

    eprintln!("  Subprocess sanity: {} (1 run, warm)...", variant_path);
    let output = Command::new(&exe)
        .args([
            "--worker",
            variant_path,
            "--bench-name",
            bench_name,
            "--mode",
            "warm",
            "--runs",
            "1",
            "--batch",
            "1",
            "--n",
            &n.to_string(),
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
        },
        Err(e) => {
            eprintln!("  Subprocess sanity: spawn failed: {}", e);
        },
    }
}

/// Decode a lowercase hex string into bytes; `None` on malformed input.
fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0 .. s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i .. 2 * i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::from_hex;

    #[test]
    fn from_hex_round_trips() {
        assert_eq!(from_hex("00ff10"), Some(vec![0x00, 0xff, 0x10]));
        assert_eq!(from_hex(""), Some(Vec::new()));
    }

    #[test]
    fn from_hex_rejects_malformed() {
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }
}
