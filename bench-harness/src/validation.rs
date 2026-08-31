//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Validation pass: run every variant across deterministic seeds and
//! compare outputs. Runs before any timing **on the [`crate::driver`] path**,
//! which is its only caller; [`crate::run`] and
//! [`crate::harness::run_orchestrator`] do not call it, so a consumer using
//! those times unvalidated dylibs. Each variant executes in
//! its own worker subprocess (`--mode validate`), so a variant's
//! cached per-process state (the setup-once pattern) lives and dies
//! with its worker; the orchestrator's memory stays bounded no matter
//! how many variants and sizes a run visits. The driver only dlopens
//! variants briefly for the ABI-hash and name checks, never calling
//! `bench_entry` in-process. Returns
//! [`BenchError::ValidationFailed`] on mismatch.
//!
//! Two independent checks, not three exclusive modes:
//!
//! - **Per-variant validity**, always. Each variant's output is passed to
//!   [`mockspace_bench_core::Routine::validate_output`], whose default returns
//!   `Ok(())`, so a routine that declared no validator pays a no-op.
//! - **Cross-variant agreement**, unless the routine declares
//!   [`mockspace_bench_core::Routine::outputs_may_differ`]. That flag is
//!   consent to variants disagreeing, and it is all it means (e.g. graph
//!   colouring may pick different but equally-valid colourings). When the
//!   routine declares
//!   [`mockspace_bench_core::Routine::max_relative_error`] as `Some(eps)`,
//!   outputs are compared element-wise as f64 slices with relative-error
//!   tolerance; otherwise byte-exact.
//!
//! A routine can want both, and the common case does: outputs that agree
//! byte-for-byte *and* each satisfy an invariant.

use std::collections::HashSet;
use std::num::NonZeroUsize;
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

/// How variants are compared against each other, when they are compared at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CrossVariant {
    /// Element-wise f64 comparison with the routine's relative-error tolerance.
    Approx(f64),
    /// Byte-exact equality against the baseline variant.
    ByteExact,
}

/// What one validation pass checks.
///
/// The `Routine` contract asks two independent questions and this type keeps
/// them independent. `per_variant` is whether each variant's own output is
/// checked for structural validity; `cross_variant` is whether variants must
/// agree with each other, and how. A routine can want both: outputs that agree
/// byte-for-byte *and* satisfy an invariant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ValidationPlan {
    /// Run `Routine::validate_output` on every variant's output.
    pub per_variant:   bool,
    /// Compare variants against each other. `None` when the routine consents
    /// to variants differing.
    pub cross_variant: Option<CrossVariant>,
}

/// Decide what a validation pass checks, from the two routine-bridge flags.
///
/// Separated from [`validate`] so the decision is testable without building
/// cdylibs, which is the only other way to reach it.
pub(crate) fn validation_plan(outputs_may_differ: bool, approx_eps: Option<f64>) -> ValidationPlan {
    ValidationPlan {
        // Always. The bridge cannot report whether the routine overrode
        // `validate_output`, and it does not need to: the trait's default
        // returns `Ok(())`, so a routine that declared nothing pays a no-op and
        // one that declared a validator gets it run. Gating this on
        // `outputs_may_differ` instead is what silently disabled every
        // validator written by a routine that also wanted byte-identical
        // output.
        per_variant:   true,
        cross_variant: if outputs_may_differ {
            None
        } else if let Some(eps) = approx_eps {
            Some(CrossVariant::Approx(eps))
        } else {
            Some(CrossVariant::ByteExact)
        },
    }
}

/// Run the routine's per-variant structural check over one seed's outputs.
///
/// `outputs` is one entry per variant, `None` for a variant that was skipped.
/// Returns `(variant index, refusal reason)` for each output the routine
/// refused, in variant order.
///
/// Separated from [`validate`] because the property that regressed is not which
/// comparison mode is chosen but **whether this check runs at all**. Holding the
/// decision here, and calling it unconditionally, means a test with a stub
/// validator can establish that it is reached. A gate reintroduced at the call
/// site would be visible as a second condition around a function that already
/// owns the decision.
fn check_each_variant(
    plan: ValidationPlan,
    validator: fn(&[u8], &[u8]) -> Result<(), String>,
    input_builder: fn(u64) -> Vec<u8>,
    seed: u64,
    outputs: &[Option<&[u8]>],
) -> Vec<(usize, String)> {
    if !plan.per_variant {
        return Vec::new();
    }
    let input = input_builder(seed);
    outputs
        .iter()
        .enumerate()
        .filter_map(|(i, output)| {
            let output = (*output)?;
            validator(&input, output).err().map(|reason| (i, reason))
        })
        .collect()
}

/// A variant count that has been established non-zero, and the only key to
/// [`validation_ok_line`].
///
/// What it enforces, exactly: **a plain `usize` will not substitute at the call
/// site**, which is what a bare `NonZeroUsize` failed to do, since both `Display`
/// and dropping the guard therefore still compiled and still printed "all 0
/// variants passed". Removing the guard now leaves nothing to hand
/// [`validation_ok_line`], and the crate stops building.
///
/// What it does not enforce: **anything against this module.** Rust privacy is
/// module-scoped and [`validate`] lives here, so code in this file can build a
/// `Survivors` directly and print whatever it likes. The guarantee is against
/// the rest of the crate, and against the accident of a refactor dropping the
/// call, and it stops there. An earlier version of this comment claimed the
/// summary could not be printed by any caller that had not gone through the
/// refusal, which is false and was demonstrated false in one appended line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Survivors(NonZeroUsize);

impl Survivors {
    pub(crate) fn get(self) -> usize {
        self.0.get()
    }
}

/// The line printed when validation passes, which may not be reached with a
/// count of zero because [`Survivors`] cannot hold one.
///
/// Deliberately does not say "valid". A routine that never overrode
/// `validate_output` gets the default `Ok(())`, and the bridge cannot report
/// which happened, so claiming structural validity here would assert exactly
/// what the harness is unable to know.
fn validation_ok_line(active: Survivors, plan: ValidationPlan) -> String {
    format!(
        "  Validation OK: all {} variants passed {}",
        active.get(),
        match plan.cross_variant {
            None => "the routine's own per-variant check",
            Some(CrossVariant::Approx(_)) =>
                "the routine's own per-variant check and agree within tolerance",
            Some(CrossVariant::ByteExact) =>
                "the routine's own per-variant check and produce identical output",
        }
    )
}

/// How many variants survived to be compared, or why none did.
///
/// The only constructor of [`Survivors`], which [`validation_ok_line`] requires,
/// so the pass summary cannot be printed without having come through here.
/// Dropping this call does not quietly restore the defect: it leaves nothing to
/// pass to that function.
///
/// The defect it forecloses: every variant having been skipped means no arm was
/// checked against any other, and the summary would report "Validation OK: all 0
/// variants passed" and return success. Zero mismatches out of zero comparisons
/// is not a pass.
///
/// `silent` counts variants whose worker exited cleanly and printed nothing.
/// When that is all of them the binary has no `--mode validate` arm at all,
/// which is a different problem with a different fix, so it gets its own text.
fn surviving_variants(total: usize, active: usize, silent: usize) -> Result<Survivors, String> {
    if let Some(n) = NonZeroUsize::new(active) {
        return Ok(Survivors(n));
    }
    if total == 0 {
        return Err("no variants were supplied to validate".to_string());
    }
    Err(if silent == total {
        format!(
            "all {total} variants' validation workers exited cleanly and printed no VOUT lines, \
             which means this binary has no `--mode validate` arm. Wire it to \
             `mockspace_bench_harness::run_worker_validate` in the same place the `--worker` mode \
             calls `run_worker`, or the harness cannot check that the timed arms compute the same \
             answer"
        )
    } else {
        format!(
            "all {total} variants were skipped before comparison ({silent} produced no output at \
             all), so no arm was validated against any other"
        )
    })
}

/// The first exported name that cannot survive the harness's own serialisation,
/// as `(index, the name, what is wrong with it)`.
///
/// A sample is written into a comma-separated CSV and a tab-separated history
/// ledger with the variant name in a field, and neither writer quotes and
/// neither reader unquotes. Every field on the read side falls back with
/// `unwrap_or`, so a name carrying a delimiter does not fail: it shifts every
/// column after it and the row comes back with plausible wrong numbers in the
/// wrong fields. A newline splits the row in two, and a leading `run,` makes a
/// row look like the header both readers skip.
///
/// The name comes from the dylib's `bench_name` C string rather than from the
/// manifest, so nothing upstream of here can catch it.
pub(crate) fn first_unsafe_name(names: &[String]) -> Option<(usize, &str, &'static str)> {
    for (i, name) in names.iter().enumerate() {
        let bad = if name.is_empty() {
            Some("is empty")
        } else if name.contains(',') {
            Some("contains a comma, which is the CSV field separator")
        } else if name.contains('\t') {
            Some("contains a tab, which is the history ledger's field separator")
        } else if name.contains('\n') || name.contains('\r') {
            Some("contains a line break, which ends a row in both formats")
        } else {
            None
        };
        if let Some(reason) = bad {
            return Some((i, name.as_str(), reason));
        }
    }
    None
}

/// First pair of variants sharing an exported name, as
/// `(earlier index, later index, the name)`.
///
/// Separated from [`validate`] so the detection is testable without building
/// two cdylibs that deliberately collide, which is the only other way to
/// reach it.
fn first_duplicate_name(names: &[String]) -> Option<(usize, usize, &str)> {
    for (j, name) in names.iter().enumerate() {
        if let Some(i) = names[.. j].iter().position(|earlier| earlier == name) {
            return Some((i, j, name.as_str()));
        }
    }
    None
}

/// Validate all variant cdylibs against the given [`RoutineSpec`].
///
/// Returns the subset of `variant_paths` that survived the probes
/// (variants that crashed or timed out at the probe stage are
/// excluded so the orchestrator can still proceed without them).
///
/// What gets checked is [`validation_plan`]'s decision, over two independent
/// questions: whether each variant's own output is checked for structural
/// validity, and whether variants must agree with each other.
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
    let validator = routine.bridge.validator;
    let plan = validation_plan(routine.bridge.outputs_may_differ, approx_eps);

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
            let name = crate::harness::variant_name(*name_fn);

            drop(lib);
            name
        };
        names.push(name);
    }

    // A sample is labelled by the name the dylib exports, and every grouping
    // downstream keys on that label. Two variants exporting one name merge
    // into a single arm carrying samples from both, and the median that comes
    // out is internally consistent while describing neither, which is the
    // worst shape a wrong number can take. The manifest cannot catch it,
    // because the name lives in the dylib rather than in the path, so two
    // distinct crates with distinct dylib file names can still collide.
    if let Some((i, name, reason)) = first_unsafe_name(&names) {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{bench_name}` n={n}: variant {} exports the name `{name}`, which \
                 {reason}. Every sample is written into a CSV and a tab-separated \
                 history ledger with this name in a field, unquoted, and both readers \
                 fall back silently on a short row, so the numbers would come back in \
                 the wrong columns rather than failing. Give the variant a plain name.",
                variant_paths[i],
            ),
        });
    }

    if let Some((i, j, name)) = first_duplicate_name(&names) {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{bench_name}` n={n}: variants {} and {} both export the name \
                 `{name}`; their samples would merge into one arm whose median \
                 describes neither. Give each variant its own name.",
                variant_paths[i], variant_paths[j],
            ),
        });
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
    // Variants whose worker exited cleanly and printed nothing. That is the
    // signature of a binary with no `--mode validate` arm: it takes the flags,
    // does not recognise them, exits zero, and every variant looks skippable.
    // Counted separately from a crash so the error below can name the cause.
    let mut silent_workers = 0usize;
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
            if rows.is_empty() {
                silent_workers += 1;
            }
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

    // Recount after collection: worker crashes and truncated output above grow
    // `slow_variants`, and the OK summaries below must not claim a skipped
    // variant validated. Every variant skipped means nothing was compared, and
    // the summary would print "Validation OK: all 0 variants passed" and return
    // success. Zero mismatches out of zero comparisons is not a pass, so refuse
    // here. The count is non-zero by type from here down, which is what stops
    // this being a check somebody can quietly drop.
    let active_count = match surviving_variants(
        names.len(),
        names.len() - slow_variants.len(),
        silent_workers,
    ) {
        Ok(n) => n,
        Err(reason) => {
            return Err(BenchError::ValidationFailed {
                variant: bench_name.to_string(),
                reason,
            });
        },
    };

    let mut mismatches = 0usize;
    let mut first_mismatch_reason: Option<(String, String)> = None;
    // Baseline for cross-variant comparison: the first variant that
    // survived (previously index 0 unconditionally, which compared
    // zero-filled placeholder output when variant 0 had been skipped).
    let base_idx = (0 .. names.len()).find(|i| !slow_variants.contains(i));

    for (si, &seed) in seeds.iter().enumerate() {
        // Per-variant structural validity. Called unconditionally: whether
        // variants agree with each other says nothing about whether each one is
        // individually valid, and the decision lives in `check_each_variant`.
        let per_seed_outputs: Vec<Option<&[u8]>> = (0 .. names.len())
            .map(|i| {
                if slow_variants.contains(&i) {
                    None
                } else {
                    Some(collected[i][si].0.as_slice())
                }
            })
            .collect();
        for (i, reason) in
            check_each_variant(plan, validator, input_builder, seed, &per_seed_outputs)
        {
            mismatches += 1;
            if mismatches <= 3 {
                eprintln!("  INVALID seed={} variant={}: {}", seed, names[i], reason);
            }
            first_mismatch_reason
                .get_or_insert((names[i].clone(), format!("invalid output: {reason}")));
        }

        // Cross-variant agreement, skipped when the routine consents to
        // variants differing.
        match plan.cross_variant {
            None => {},
            Some(CrossVariant::Approx(eps)) => {
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
            },
            Some(CrossVariant::ByteExact) => {
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
                            for (j, (a, b)) in
                                baseline.iter().zip(collected[i][si].0.iter()).enumerate()
                            {
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
            },
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

    eprintln!("{}", validation_ok_line(active_count, plan));

    // Determinism check: call each variant twice with the same seed
    // and verify both outputs are identical.
    eprintln!(
        "  Determinism check: {} variants × {} seeds...",
        active_count.get(),
        determinism_check_seeds
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
        active_count.get()
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
    use std::cell::Cell;

    use super::{
        CrossVariant,
        check_each_variant,
        first_duplicate_name,
        first_unsafe_name,
        from_hex,
        surviving_variants,
        validation_ok_line,
        validation_plan,
    };

    // The defect these pin, in the words of the run that had it: a consumer
    // binary with no `--mode validate` arm takes the worker flags, does not
    // recognise them, exits zero and prints nothing. Every variant then returns
    // zero of N outputs, every variant is skipped, and the summary reports
    // "Validation OK: all 0 variants passed" before returning success. A whole
    // committed bench corpus was produced under exactly that, and nothing in
    // the output said the arms had never been compared.

    // Every reachable `(total, active, silent)` up to a bound, rather than the
    // handful of points a sampled test would pick. `active + silent <= total`
    // holds by construction at the call site: both count disjoint subsets of
    // the variants. Choosing which triples to assert over would be choosing
    // which region not to find out about.
    fn every_reachable_triple(bound: usize) -> impl Iterator<Item = (usize, usize, usize)> {
        (0 ..= bound).flat_map(move |total| {
            (0 ..= total).flat_map(move |active| {
                (0 ..= total - active).map(move |silent| (total, active, silent))
            })
        })
    }

    #[test]
    fn the_verdict_is_exactly_whether_anything_survived() {
        // The law over the whole matrix, both directions in one pass: every
        // triple with a survivor is accepted and every triple without one is
        // refused. A guard that always refused, or always accepted, fails half
        // of this, which is what makes it a law rather than two lists.
        let mut accepted = 0usize;
        let mut refused = 0usize;
        for (total, active, silent) in every_reachable_triple(6) {
            match surviving_variants(total, active, silent) {
                Ok(n) => {
                    accepted += 1;
                    assert_eq!(n.get(), active, "({total},{active},{silent}) miscounted");
                    assert!(
                        active > 0,
                        "({total},{active},{silent}) accepted with no survivor"
                    );
                },
                Err(reason) => {
                    refused += 1;
                    assert_eq!(active, 0, "({total},{active},{silent}) refused a survivor");
                    assert!(
                        !reason.is_empty(),
                        "({total},{active},{silent}) refused mutely"
                    );
                },
            }
        }
        // Both arms were actually reached. Without this the loop could have
        // asserted nothing at all and still passed, which is the shape of a
        // test that measures its own iteration count.
        assert!(
            accepted > 0 && refused > 0,
            "{accepted} accepted, {refused} refused"
        );
    }

    #[test]
    fn an_unimplemented_worker_mode_is_named_apart_from_a_crash() {
        // The two causes want different fixes, so they may not share a message.
        // Wiring the worker fixes the first and says nothing about the second.
        let silent = surviving_variants(3, 0, 3).expect_err("must refuse");
        assert!(silent.contains("run_worker_validate"), "{silent}");
        assert!(silent.contains("--mode validate"), "{silent}");

        let crashed = surviving_variants(3, 0, 0).expect_err("must refuse");
        assert!(
            !crashed.contains("run_worker_validate"),
            "a crash is not a missing worker arm: {crashed}"
        );
        assert!(crashed.contains("skipped before comparison"), "{crashed}");

        // Partial silence is the crash wording, not the worker-mode wording: a
        // binary that answered for some variants plainly has the arm.
        let mixed = surviving_variants(3, 0, 2).expect_err("must refuse");
        assert!(!mixed.contains("run_worker_validate"), "{mixed}");
    }

    #[test]
    fn the_pass_summary_can_only_be_built_from_a_survivor_count() {
        // What this pins is that `validation_ok_line` takes `Survivors` and
        // nothing else. `Survivors` has a private field and one constructor, so
        // a caller holding a raw count cannot reach the summary at all.
        //
        // The first attempt at this guard returned a bare `NonZeroUsize` and the
        // doc comment claimed dropping the guard would stop the crate compiling.
        // It did not: both types implement `Display`, so replacing the call with
        // the raw subtraction compiled cleanly and printed "all 0 variants
        // passed" exactly as before. Measured, not assumed, which is why the
        // newtype exists.
        let one = surviving_variants(3, 1, 2).expect("one survivor");
        let line = validation_ok_line(one, validation_plan(false, None));
        assert!(line.contains("all 1 variants passed"), "{line}");
        assert!(line.contains("identical output"), "{line}");

        // The count that reaches the line is the survivor count, not the total.
        let two = surviving_variants(9, 2, 7).expect("two survivors");
        assert!(
            validation_ok_line(two, validation_plan(true, None)).contains("all 2 variants"),
            "the summary must report what was compared, not what was supplied"
        );
    }

    #[test]
    fn no_variants_at_all_is_its_own_refusal() {
        // Reachable through `validate`'s `>= 2` guard only if that guard moves,
        // so this pins the arm rather than describing today's call site.
        let none = surviving_variants(0, 0, 0).expect_err("must refuse");
        assert!(none.contains("no variants were supplied"), "{none}");
    }

    // The contract these pin is `Routine`'s own documentation in bench-core:
    // `validate_output`'s default is "no structural check; the harness STILL
    // does cross-variant byte comparison unless `outputs_may_differ` is true",
    // and `outputs_may_differ = false` means the harness "ALSO does
    // cross-variant byte comparison". Both sentences describe the two checks as
    // independent. A routine may want its outputs to agree byte-for-byte AND to
    // each satisfy an invariant.

    #[test]
    fn per_variant_validation_runs_regardless_of_cross_variant_agreement() {
        // The regression this names: gating the per-variant validator on
        // `outputs_may_differ` means a routine that declares a validator and
        // also expects byte-identical outputs never has that validator called.
        assert!(validation_plan(false, None).per_variant);
        assert!(validation_plan(false, Some(1e-9)).per_variant);
        assert!(validation_plan(true, None).per_variant);
        assert!(validation_plan(true, Some(1e-9)).per_variant);
    }

    // Call counter for the stub validator. A `fn` pointer cannot capture, and
    // the bridge stores `fn` pointers, so the count cannot live in a closure.
    // Thread-local rather than a `static`: the test harness runs each test on
    // its own thread, and a shared counter made these two tests read each
    // other's calls when they ran concurrently.
    thread_local! {
        static VALIDATOR_CALLS: Cell<usize> = const { Cell::new(0) };
    }

    fn validator_calls() -> usize {
        VALIDATOR_CALLS.with(Cell::get)
    }

    fn counting_validator(_input: &[u8], output: &[u8]) -> Result<(), String> {
        VALIDATOR_CALLS.with(|c| c.set(c.get() + 1));
        // Refuse exactly one recognisable output so the reporting path is
        // exercised too, rather than only the call count.
        if output == [0xBA, 0xD1] {
            return Err("stub refusal".to_string());
        }
        Ok(())
    }

    fn stub_input(_seed: u64) -> Vec<u8> {
        vec![0x11]
    }

    /// The check the plan test cannot make: that the validator is actually
    /// reached. `validation_plan` hardcodes `per_variant`, so asserting on it
    /// alone asserts a literal, and a gate reintroduced around the call site
    /// would leave that assertion green.
    #[test]
    fn the_validator_is_called_for_every_live_variant_when_outputs_must_agree() {
        let good: &[u8] = &[0x01];
        let bad: &[u8] = &[0xBA, 0xD1];
        // `outputs_may_differ = false` with no tolerance: byte-exact
        // cross-variant comparison, which is the configuration whose validator
        // was silently dropped.
        let plan = validation_plan(false, None);
        let outputs = [Some(good), Some(bad), None, Some(good)];

        let refused = check_each_variant(plan, counting_validator, stub_input, 7, &outputs);

        // Three live variants, one skipped: three calls, not four and not zero.
        assert_eq!(validator_calls(), 3);
        // The refusal is reported against the right variant index, with the
        // skipped variant not shifting the numbering.
        assert_eq!(refused, vec![(1, "stub refusal".to_string())]);
    }

    #[test]
    fn a_skipped_variant_is_not_validated_and_a_clean_run_reports_nothing() {
        let good: &[u8] = &[0x01];
        let plan = validation_plan(true, None);
        let outputs = [None, Some(good)];

        let refused = check_each_variant(plan, counting_validator, stub_input, 7, &outputs);

        assert_eq!(validator_calls(), 1);
        assert!(refused.is_empty());
    }

    #[test]
    fn cross_variant_comparison_is_skipped_only_by_consent() {
        // `outputs_may_differ` is consent to variants disagreeing, and that is
        // the only thing it controls.
        assert_eq!(validation_plan(true, None).cross_variant, None);
        assert_eq!(validation_plan(true, Some(1e-9)).cross_variant, None);
    }

    #[test]
    fn tolerance_selects_approximate_comparison_over_byte_exact() {
        assert_eq!(
            validation_plan(false, None).cross_variant,
            Some(CrossVariant::ByteExact)
        );
        assert_eq!(
            validation_plan(false, Some(1e-9)).cross_variant,
            Some(CrossVariant::Approx(1e-9))
        );
    }

    #[test]
    fn from_hex_round_trips() {
        assert_eq!(from_hex("00ff10"), Some(vec![0x00, 0xFF, 0x10]));
        assert_eq!(from_hex(""), Some(Vec::new()));
    }

    #[test]
    fn from_hex_rejects_malformed() {
        assert_eq!(from_hex("0"), None);
        assert_eq!(from_hex("zz"), None);
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn a_name_that_would_corrupt_the_csv_or_the_ledger_is_refused() {
        // The comma is the CSV separator and the tab is the ledger's. Neither
        // writer quotes and neither reader unquotes, and every field on the
        // read side has a silent `unwrap_or` default, so a delimiter in a name
        // shifts the columns after it and the row parses into wrong numbers
        // instead of failing.
        for (bad, what) in [
            ("has,comma", "comma"),
            ("has\ttab", "tab"),
            ("has\nnewline", "newline"),
            ("has\rcarriage", "carriage return"),
            ("", "empty name"),
        ] {
            let n = names(&["fine", bad, "also_fine"]);
            let found = first_unsafe_name(&n);
            assert!(
                found.is_some(),
                "`{}` ({what}) was accepted",
                bad.escape_debug()
            );
            assert_eq!(found.unwrap().0, 1, "the offending index is reported");
        }
    }

    #[test]
    fn ordinary_names_are_not_refused() {
        // Underscores, digits, dashes and dots are all ordinary in a crate or
        // symbol name and none of them is a delimiter in either format.
        assert_eq!(
            first_unsafe_name(&names(&["fnv1a", "xx_hash-64", "v1.2", "a"])),
            None
        );
        assert_eq!(first_unsafe_name(&names(&[])), None);
        // A space is awkward to read and does not corrupt anything, so it is
        // not this check's business.
        assert_eq!(first_unsafe_name(&names(&["two words"])), None);
    }
    #[test]
    fn duplicate_exported_names_are_found() {
        // Adjacent, apart, and three-way, because a check that only sees the
        // adjacent case passes on the shape a real manifest hits least often.
        assert_eq!(first_duplicate_name(&names(&["a", "a"])), Some((0, 1, "a")));
        assert_eq!(
            first_duplicate_name(&names(&["a", "b", "c", "b"])),
            Some((1, 3, "b"))
        );
        // The earliest colliding pair is reported, not the last one found.
        assert_eq!(
            first_duplicate_name(&names(&["a", "b", "a", "b"])),
            Some((0, 2, "a"))
        );
    }

    #[test]
    fn distinct_exported_names_pass() {
        assert_eq!(first_duplicate_name(&names(&[])), None);
        assert_eq!(first_duplicate_name(&names(&["only"])), None);
        assert_eq!(first_duplicate_name(&names(&["a", "b", "c"])), None);
        // A name that is a prefix of another is not a collision; the label is
        // compared whole, and a substring check here would refuse a legitimate
        // pair like `sum` beside `sum_windowed`.
        assert_eq!(first_duplicate_name(&names(&["sum", "sum_windowed"])), None);
    }
}
