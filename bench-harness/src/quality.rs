//! Quality scoring: run each variant on deterministic seeds, score
//! outputs via `Routine::score_output`, report the distribution.
//!
//! Generic across routines: works for any routine that defines a
//! score metric. For routines without a quality metric the
//! [`measure`] helper returns an empty vector.

use crate::config::HarnessTuning;
use crate::core::counter::Rng;
use crate::core::{abi_hash, AbiHashFn, BenchEntryFn, BenchNameFn};
use crate::error::BenchError;
use crate::spec::RoutineSpec;

/// Default seed count for [`measure`] when callers do not supply a
/// [`HarnessTuning`].
pub const DEFAULT_QUALITY_SEEDS: usize = 1000;
const QUALITY_ROOT_SEED: u64 = 0xC0DE_0A11_1700_BEEF;

/// One row of the quality table per variant.
pub struct VariantQuality {
    pub name: String,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub median: f64,
}

/// Run quality analysis across all variant cdylibs.
///
/// Uses [`RoutineSpec::bridge`] for the input builder, output size,
/// and scorer. Returns an empty vector when the routine has no
/// quality metric (i.e. the routine's `score_output` always returns
/// `None`). The `label` is the routine's quality-metric label
/// (e.g. `"colors"`, `"bandwidth"`); used only in the eprint header.
pub fn measure(
    routine: &RoutineSpec,
    variant_paths: &[String],
    n: usize,
    label: &str,
    tuning: Option<&HarnessTuning>,
) -> Result<Vec<VariantQuality>, BenchError> {
    let input_builder = routine.bridge.input_builder;
    let output_size = routine.bridge.output_size;
    let scorer = routine.bridge.scorer;

    let quality_seeds = tuning
        .map(|t| t.quality_seeds)
        .unwrap_or(DEFAULT_QUALITY_SEEDS);

    let mut rng = Rng::new(QUALITY_ROOT_SEED);
    let seeds: Vec<u64> = (0..quality_seeds).map(|_| rng.next()).collect();

    let mut results: Vec<VariantQuality> = Vec::new();
    let mut _libs: Vec<libloading::Library> = Vec::new();

    for path in variant_paths {
        let (variant_name, entry) = unsafe {
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

        let mut scores: Vec<f64> = Vec::new();

        for &seed in &seeds {
            let input = input_builder(seed);
            let mut output = vec![0u8; output_size];
            unsafe {
                entry(input.as_ptr(), output.as_mut_ptr(), n);
            }
            if let Some(s) = scorer(&input, &output) {
                scores.push(s);
            }
        }

        if scores.is_empty() {
            continue;
        }

        scores.sort_by(|a, b| a.total_cmp(b));
        let count = scores.len();
        let mean = scores.iter().sum::<f64>() / count as f64;

        results.push(VariantQuality {
            name: variant_name,
            mean,
            min: scores[0],
            max: scores[count - 1],
            median: scores[count / 2],
        });
    }

    if !results.is_empty() {
        eprintln!("  Quality. {} ({} seeds):", label, quality_seeds);
        eprintln!(
            "  {:20} {:>8} {:>6} {:>6} {:>8}",
            "Variant", "mean", "min", "max", "median"
        );
        for r in &results {
            eprintln!(
                "  {:20} {:8.2} {:6.0} {:6.0} {:8.2}",
                r.name, r.mean, r.min, r.max, r.median
            );
        }
    }

    Ok(results)
}
