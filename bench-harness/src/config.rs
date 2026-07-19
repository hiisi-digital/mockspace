//! Bench configuration shapes.
//!
//! Two tiers, mirroring the polka-dots split:
//!
//! - [`BenchManifest`]: TOML-loadable hierarchical shape that lives at
//!   `mock/benches/bench.toml`. Multi-bench, multi-size; the file the
//!   consumer authors.
//! - [`BenchConfig`]: flat per-run shape the orchestrator consumes.
//!   One [`BenchConfig`] per `(bench, size)` entry in the manifest.
//!
//! Round 1 ships both shapes and a [`BenchManifest::load`] that reads
//! TOML; the manifest-to-flat-config conversion (`for_size`) is
//! present but the orchestrator that consumes the result is stubbed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::BenchError;

// ── Tier 1: TOML-loadable manifest (`mock/benches/bench.toml`) ──

/// Hierarchical TOML shape lifted from polka-dots `framework/config.rs`.
///
/// Format example:
///
/// ```toml
/// [bench.content_hash]
/// title = "Content Hash"
/// workload = "hash"
/// master_seed = 0xDEADBEEFCAFEBABE
///
/// [[bench.content_hash.sizes]]
/// n = 64
/// variants = [
///     "variants/fnv1a/target/release/libfnv1a.dylib",
///     "variants/xxhash3/target/release/libxxhash3.dylib",
/// ]
///
/// [timing]
/// passes = 10
/// runs_per_pass = 50000
/// batch_size = 5000
/// harness_runs = 3
/// cooldowns_ms = [0, 100, 600]
/// ```
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct BenchManifest {
    /// Named bench entries. Key is the bench short name.
    #[serde(default)]
    pub bench:  HashMap<String, BenchSection>,
    /// Shared timing parameters applied to every bench unless a
    /// bench declares a `[bench.<name>.timing]` override.
    #[serde(default)]
    pub timing: TimingSection,
}

/// One named bench inside a [`BenchManifest`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BenchSection {
    /// Display title for the bench (used in findings.md).
    pub title:       String,
    /// Workload program identifier (matches a name registered with
    /// the harness's workload module in Round 2).
    pub workload:    String,
    /// Master seed for input generation. `0` means "use a fresh
    /// random seed every run"; any other value reproduces.
    ///
    /// Accepts either a TOML integer or a string. TOML 1.0 caps
    /// integer literals at `i64::MAX`; a string value (`"0xDEAD..."`
    /// hex with optional underscores, or decimal) carries the full
    /// `u64` range.
    #[serde(default, deserialize_with = "de_seed")]
    pub master_seed: u64,
    /// Bench-level variant list applied to every size that does not
    /// declare its own. Entries are either variant short names (no
    /// path separator; resolved to
    /// `variants/<name>/target/release/<platform dylib>`) or paths
    /// relative to `mock/benches/` (containing a separator; a bare
    /// stem gets the platform dylib prefix and extension).
    #[serde(default)]
    pub variants:    Vec<String>,
    /// The N values the bench runs at. Two TOML shapes are accepted:
    /// a plain integer array (`sizes = [64, 256, 1024]`, using the
    /// bench-level `variants`) or the array-of-tables form
    /// (`[[bench.<name>.sizes]]` with `n` and an optional per-size
    /// `variants` list overriding the bench-level one).
    #[serde(default, deserialize_with = "de_sizes")]
    pub sizes:       Vec<SizeSection>,
    /// Whether variants may produce different valid outputs for the
    /// same input. `false` (default): the harness cross-validates
    /// outputs byte-exact. `true`: variants are independent
    /// algorithms; only per-variant validation applies.
    #[serde(default)]
    pub may_differ:  bool,
    /// Whether a validation failure on this bench fails the whole
    /// run (process exit code). `false` (default): failures are
    /// recorded in findings and the run continues.
    #[serde(default)]
    pub required:    bool,
    /// Whether variants spawn their own threads inside the timed run
    /// block. `true` disables the worker's P-core self-pin (spawned
    /// threads never inherit the pin, and pinning only the
    /// coordinating thread skews a threaded workload). Timing stays
    /// wall-clock-correct either way; see the threading-contract
    /// notes in the framework docs for what is and is not measured.
    #[serde(default)]
    pub threaded:    bool,
    /// Per-bench timing override. Any field left out falls back to
    /// the global `[timing]` section.
    #[serde(default)]
    pub timing:      Option<TimingOverride>,
}

/// One `(N, [variants])` pair inside a [`BenchSection`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SizeSection {
    /// Logical size parameter passed into `bench_entry(... , n: usize)`.
    pub n:        usize,
    /// Per-size variant entries (short names or paths, same grammar
    /// as the bench-level list). Empty means "use the bench-level
    /// `variants`".
    #[serde(default)]
    pub variants: Vec<String>,
}

/// Per-bench timing override: every knob optional, merged over the
/// global [`TimingSection`] by [`BenchManifest::for_size`].
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TimingOverride {
    pub passes:        Option<usize>,
    pub runs_per_pass: Option<usize>,
    pub batch_size:    Option<usize>,
    pub harness_runs:  Option<usize>,
    pub cooldowns_ms:  Option<Vec<u64>>,
}

/// Deserialize `master_seed` from a TOML integer or a string
/// (`"0x..."` hex or decimal, underscores allowed in either).
fn de_seed<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    use serde::de::Error;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Int(i64),
        Str(String),
    }
    match Raw::deserialize(d)? {
        Raw::Int(v) => Ok(v as u64),
        Raw::Str(s) => {
            let t = s.replace('_', "");
            let parsed = if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16)
            } else {
                t.parse::<u64>()
            };
            parsed.map_err(|e| D::Error::custom(format!("master_seed `{s}`: {e}")))
        },
    }
}

/// Deserialize `sizes` from either a plain integer array or the
/// array-of-tables form.
fn de_sizes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<SizeSection>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Bare(usize),
        Full(SizeSection),
    }
    let raw: Vec<Raw> = Vec::deserialize(d)?;
    Ok(raw
        .into_iter()
        .map(|r| {
            match r {
                Raw::Bare(n) => {
                    SizeSection {
                        n,
                        variants: Vec::new(),
                    }
                },
                Raw::Full(s) => s,
            }
        })
        .collect())
}

/// Resolve one manifest variant entry to a concrete dylib path.
///
/// A short name (no path separator) resolves to
/// `variants/<name>/target/release/<DLL_PREFIX><name><DLL_SUFFIX>`.
/// A path entry is joined to `mock_benches_dir`; when its file name
/// has no extension it is treated as a bare stem and receives the
/// platform dylib prefix and extension.
pub fn resolve_variant_path(entry: &str, mock_benches_dir: &Path) -> PathBuf {
    let is_short_name = !entry.contains('/') && !entry.contains(std::path::MAIN_SEPARATOR);
    let raw = if is_short_name {
        mock_benches_dir
            .join("variants")
            .join(entry)
            .join("target")
            .join("release")
            .join(entry)
    } else {
        mock_benches_dir.join(entry)
    };
    if raw.extension().is_some() {
        return raw;
    }
    let parent = raw.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = raw.file_name().and_then(|s| s.to_str()).unwrap_or("");
    parent.join(format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        stem,
        std::env::consts::DLL_SUFFIX
    ))
}

/// Shared timing knobs.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TimingSection {
    /// Outer pass count per harness run.
    #[serde(default = "default_passes")]
    pub passes:        usize,
    /// Inner runs per pass.
    #[serde(default = "default_runs")]
    pub runs_per_pass: usize,
    /// Calls per emitted [`crate::Sample`].
    #[serde(default = "default_batch")]
    pub batch_size:    usize,
    /// Outer harness runs (the whole pipeline repeated for stability).
    #[serde(default = "default_harness_runs")]
    pub harness_runs:  usize,
    /// Cooldown durations injected between cohorts, in milliseconds.
    /// Each cooldown becomes a separate cohort in the cache; analysis
    /// uses the spread to detect thermal drift.
    #[serde(default = "default_cooldowns")]
    pub cooldowns_ms:  Vec<u64>,
}

fn default_passes() -> usize {
    10
}
fn default_runs() -> usize {
    50_000
}
fn default_batch() -> usize {
    5_000
}
fn default_harness_runs() -> usize {
    3
}
fn default_cooldowns() -> Vec<u64> {
    vec![0, 100, 600]
}

impl Default for TimingSection {
    fn default() -> Self {
        TimingSection {
            passes:        default_passes(),
            runs_per_pass: default_runs(),
            batch_size:    default_batch(),
            harness_runs:  default_harness_runs(),
            cooldowns_ms:  default_cooldowns(),
        }
    }
}

impl BenchManifest {
    /// Load a manifest from a TOML file. The file is read in full;
    /// missing keys fall back to [`Default`].
    pub fn load(path: &Path) -> Result<Self, BenchError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| BenchError::io("reading bench.toml", e))?;
        toml::from_str(&text).map_err(|e| {
            BenchError::InvalidConfig {
                reason: format!("bench.toml parse error: {e}"),
            }
        })
    }

    /// Build a flat [`BenchConfig`] for one `(bench, size_idx)` entry.
    /// Returns [`BenchError::InvalidConfig`] when either index is
    /// missing.
    ///
    /// Cdylib paths are resolved against `mock_benches_dir` so the
    /// orchestrator does not need to know how the manifest was
    /// loaded.
    pub fn for_size(
        &self,
        bench_name: &str,
        size_idx: usize,
        mock_benches_dir: &Path,
    ) -> Result<BenchConfig, BenchError> {
        let section = self.bench.get(bench_name).ok_or_else(|| {
            BenchError::InvalidConfig {
                reason: format!("bench `{bench_name}` not found in manifest"),
            }
        })?;
        let size = section.sizes.get(size_idx).ok_or_else(|| {
            BenchError::InvalidConfig {
                reason: format!(
                    "bench `{bench_name}` has no size at index {size_idx} (have {})",
                    section.sizes.len()
                ),
            }
        })?;
        let entries = if size.variants.is_empty() { &section.variants } else { &size.variants };
        if entries.is_empty() {
            return Err(BenchError::InvalidConfig {
                reason: format!(
                    "bench `{bench_name}` n={} lists no variants (neither the size \
                     entry nor the bench-level `variants` name any)",
                    size.n
                ),
            });
        }
        let variant_paths = entries
            .iter()
            .map(|p| resolve_variant_path(p, mock_benches_dir))
            .collect();
        let ov = section.timing.as_ref();
        Ok(BenchConfig {
            bench_name: bench_name.to_string(),
            title: section.title.clone(),
            workload: section.workload.clone(),
            master_seed: section.master_seed,
            n: size.n,
            variant_paths,
            passes: ov.and_then(|t| t.passes).unwrap_or(self.timing.passes),
            runs_per_pass: ov
                .and_then(|t| t.runs_per_pass)
                .unwrap_or(self.timing.runs_per_pass),
            batch_size: ov
                .and_then(|t| t.batch_size)
                .unwrap_or(self.timing.batch_size),
            harness_runs: ov
                .and_then(|t| t.harness_runs)
                .unwrap_or(self.timing.harness_runs),
            cooldowns_ms: ov
                .and_then(|t| t.cooldowns_ms.clone())
                .unwrap_or_else(|| self.timing.cooldowns_ms.clone()),
            may_differ: section.may_differ,
            required: section.required,
            threaded: section.threaded,
            batch_k: 1,
            max_call_us: None,
            tuning: HarnessTuning::default(),
        })
    }

    /// Bench names in deterministic (sorted) order.
    pub fn bench_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.bench.keys().cloned().collect();
        names.sort();
        names
    }
}

// ── Tier 2: flat per-run config the orchestrator consumes ──

/// Flat per-run config. One [`BenchConfig`] feeds one [`crate::run`]
/// invocation.
///
/// Construct manually for ad-hoc runs, or via
/// [`BenchManifest::for_size`] for manifest-driven runs.
#[derive(Clone, Debug)]
pub struct BenchConfig {
    /// Manifest key for this bench (e.g. `"content_hash"`).
    pub bench_name:    String,
    /// Display title (for findings.md).
    pub title:         String,
    /// Workload program identifier.
    pub workload:      String,
    /// Master seed (`0` = fresh random).
    pub master_seed:   u64,
    /// Logical size N (passed into `bench_entry(... n)` and
    /// `Routine::max_call_us(n)`).
    pub n:             usize,
    /// Resolved cdylib paths (one per variant).
    pub variant_paths: Vec<PathBuf>,
    /// Outer pass count.
    pub passes:        usize,
    /// Inner runs per pass.
    pub runs_per_pass: usize,
    /// Calls per emitted sample.
    pub batch_size:    usize,
    /// Outer harness runs.
    pub harness_runs:  usize,
    /// Cooldown cohorts (milliseconds).
    pub cooldowns_ms:  Vec<u64>,
    /// Cross-variant outputs may differ (from the manifest).
    pub may_differ:    bool,
    /// Validation failure fails the whole run (from the manifest).
    pub required:      bool,
    /// Variants spawn threads; the worker skips its P-core self-pin.
    pub threaded:      bool,
    /// Batch-amortised mode. `1` = normal (one timed call per batch
    /// entry). `>1` = K calls between one outer counter pair, then
    /// per-call time = total / K. Useful when bridge overhead
    /// dominates measured time at small N.
    pub batch_k:       usize,
    /// Per-call timeout in microseconds. If a worker's batch mean
    /// exceeds this, the worker aborts and reports
    /// [`BenchError::WorkerFailed`]. `None` = no timeout.
    pub max_call_us:   Option<u64>,
    /// Tunable iteration counts and on-disk roots; see
    /// [`HarnessTuning`] for individual knobs and defaults.
    pub tuning:        HarnessTuning,
}

/// Tunable iteration counts. Defaults match the polka-dots
/// constants. Override on a [`BenchConfig`] to tighten dev
/// iteration speed (lower seed counts) or to plug in different
/// statistical budgets.
///
/// Cache and history roots are NOT configured here. Use
/// [`crate::cache::Cache::load_in`], [`crate::history::append_in`],
/// and [`crate::history::load_in`] directly when the harness needs
/// to live outside the consumer's cwd; the cwd-relative defaults
/// ([`crate::DEFAULT_CACHE_ROOT`] / [`crate::DEFAULT_HISTORY_DIR`])
/// remain the implicit fallback.
#[derive(Clone, Debug)]
pub struct HarnessTuning {
    /// Number of seeds used in [`crate::validate`]. Default 100.
    pub validation_seeds:        usize,
    /// Subset of `validation_seeds` used for the determinism check.
    /// Default 10.
    pub determinism_check_seeds: usize,
    /// Number of seeds used in [`crate::measure_quality`]. Default
    /// 1000.
    pub quality_seeds:           usize,
    /// Bootstrap iterations for CI estimates in
    /// [`crate::analysis::bootstrap_ci_median`] /
    /// [`crate::analysis::bootstrap_ci_diff`]. Default 10000.
    ///
    /// Currently informational: the analysis module reads from a
    /// const for the v2 launch. Wiring the override end-to-end is
    /// part of the v3 polish (#281, item 1).
    pub bootstrap_iterations:    usize,
}

impl Default for HarnessTuning {
    fn default() -> Self {
        HarnessTuning {
            validation_seeds:        100,
            determinism_check_seeds: 10,
            quality_seeds:           1000,
            bootstrap_iterations:    10_000,
        }
    }
}

impl Default for BenchConfig {
    fn default() -> Self {
        BenchConfig {
            bench_name:    String::new(),
            title:         "Benchmark".into(),
            workload:      String::new(),
            master_seed:   0,
            n:             64,
            variant_paths: Vec::new(),
            passes:        default_passes(),
            runs_per_pass: default_runs(),
            batch_size:    default_batch(),
            harness_runs:  default_harness_runs(),
            cooldowns_ms:  default_cooldowns(),
            may_differ:    false,
            required:      false,
            threaded:      false,
            batch_k:       1,
            max_call_us:   None,
            tuning:        HarnessTuning::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(toml: &str) -> BenchManifest {
        toml::from_str(toml).expect("manifest parses")
    }

    const BASE: &str = r#"
        [bench.b]
        title = "B"
        workload = "default"
        variants = ["alpha", "beta"]
        sizes = [64, 1024]
    "#;

    #[test]
    fn plain_sizes_array_and_bench_level_variants() {
        let m = manifest(BASE);
        let c = m.for_size("b", 1, Path::new("/root")).unwrap();
        assert_eq!(c.n, 1024);
        assert_eq!(c.variant_paths.len(), 2);
        let p = c.variant_paths[0].display().to_string();
        assert!(p.starts_with("/root/variants/alpha/target/release/"));
        assert!(p.contains("alpha"));
    }

    #[test]
    fn per_size_variants_override_bench_level() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["alpha"]

            [[bench.b.sizes]]
            n = 64
            variants = ["gamma"]
        "#,
        );
        let c = m.for_size("b", 0, Path::new("/root")).unwrap();
        assert_eq!(c.variant_paths.len(), 1);
        assert!(c.variant_paths[0].display().to_string().contains("gamma"));
    }

    #[test]
    fn string_master_seed_carries_full_u64_range() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            master_seed = "0xFFFF_FFFF_FFFF_FFFF"
            variants = ["a"]
            sizes = [64]
        "#,
        );
        assert_eq!(m.bench["b"].master_seed, u64::MAX);
        let dec = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            master_seed = "12345"
            variants = ["a"]
            sizes = [64]
        "#,
        );
        assert_eq!(dec.bench["b"].master_seed, 12345);
    }

    #[test]
    fn integer_master_seed_still_parses() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            master_seed = 0x1234_5678
            variants = ["a"]
            sizes = [64]
        "#,
        );
        assert_eq!(m.bench["b"].master_seed, 0x1234_5678);
    }

    #[test]
    fn timing_override_merges_over_global() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["a"]
            sizes = [64]

            [bench.b.timing]
            runs_per_pass = 7
            harness_runs = 1

            [timing]
            passes = 3
            runs_per_pass = 50000
        "#,
        );
        let c = m.for_size("b", 0, Path::new("/root")).unwrap();
        assert_eq!(c.runs_per_pass, 7, "override wins");
        assert_eq!(c.harness_runs, 1, "override wins");
        assert_eq!(c.passes, 3, "global fills the gap");
    }

    #[test]
    fn flags_default_off_and_parse() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["a"]
            sizes = [64]
            may_differ = true
            required = true
            threaded = true
        "#,
        );
        let c = m.for_size("b", 0, Path::new("/root")).unwrap();
        assert!(c.may_differ && c.required && c.threaded);
        let base = manifest(BASE).for_size("b", 0, Path::new("/root")).unwrap();
        assert!(!base.may_differ && !base.required && !base.threaded);
    }

    #[test]
    fn resolve_variant_path_shapes() {
        let root = Path::new("/root");
        let short = resolve_variant_path("alpha", root).display().to_string();
        assert!(short.starts_with("/root/variants/alpha/target/release/"));
        assert!(short.ends_with(std::env::consts::DLL_SUFFIX));
        let bare_stem = resolve_variant_path("variants/x/target/release/x", root)
            .display()
            .to_string();
        assert!(bare_stem.ends_with(std::env::consts::DLL_SUFFIX));
        let explicit = resolve_variant_path("variants/x/target/release/libx.dylib", root)
            .display()
            .to_string();
        assert!(
            explicit.ends_with("libx.dylib"),
            "explicit extensions pass through"
        );
    }

    #[test]
    fn empty_variant_lists_error_by_name() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            sizes = [64]
        "#,
        );
        let err = m.for_size("b", 0, Path::new("/root")).unwrap_err();
        assert!(format!("{err}").contains("lists no variants"));
    }
}
