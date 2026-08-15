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
#[serde(deny_unknown_fields)]
pub struct BenchManifest {
    /// Named bench entries. Key is the bench short name.
    #[serde(default)]
    pub bench:  HashMap<String, BenchSection>,
    /// Shared timing parameters applied to every bench unless a
    /// bench declares a `[bench.<name>.timing]` override.
    #[serde(default)]
    pub timing: TimingSection,
    /// Opt-in generated documentation pass. Read by the docs
    /// regeneration path, which emits a `BENCHES.md` from the
    /// committed history when this is enabled.
    ///
    /// Typed here rather than left unknown because the section is a
    /// shipped feature: the scaffolded `bench.toml` documents it, and
    /// at least one consumer has it turned on. Refusing it as an
    /// unknown key would have turned off a working feature and told
    /// the consumer to delete the line.
    #[serde(default)]
    pub docgen: Option<DocgenSection>,
    /// Byte-dispatch declaration for the generated driver. `out` is
    /// the output width in bytes; `points` narrows the generated
    /// monomorphisation list, which otherwise defaults to the union
    /// of every bench's points.
    #[serde(default)]
    pub dispatch: Option<DispatchSection>,
    /// Declarative workload programs for the generated driver, by
    /// name. Two builtins exist without being declared: `default`
    /// and `realistic`.
    #[serde(default)]
    pub workload: HashMap<String, WorkloadSection>,
    /// Build settings for the tool-generated crates: the mockspace
    /// dependency spec they pin, and the release-profile values the
    /// tool passes on every build. Defaults are the framework's; a
    /// consumer overrides here, never in a manifest cargo may ignore.
    #[serde(default)]
    pub build: Option<BuildSection>,
    /// Nested-tree bookkeeping: manifest key to `(bench, sweep)`.
    /// Populated by [`crate::tree::load_tree`]; empty for a flat
    /// tree, where every section is its own single sweep.
    #[serde(skip)]
    pub nested: HashMap<String, (String, String)>,
    /// Whether this manifest was composed from a nested tree.
    #[serde(skip)]
    pub nested_mode: bool,
}

/// The `[build]` section: named defaults the tool applies to every
/// bench build, overridable here and nowhere else.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSection {
    /// The cargo dependency spec the generated crates use for the
    /// mockspace bench crates, verbatim, e.g.
    /// `{ git = "https://github.com/hiisi-digital/mockspace", rev = "..." }`.
    /// Default: the dev branch of the canonical repository.
    #[serde(default)]
    pub mockspace:     Option<String>,
    /// Release profile overrides. The tool passes the effective
    /// values on the command line (`--config`), where a manifest
    /// cannot silently drop them; these keys move the values, they
    /// do not relocate the mechanism.
    #[serde(default, rename = "opt-level")]
    pub opt_level:     Option<u32>,
    #[serde(default)]
    pub lto:           Option<String>,
    #[serde(default, rename = "codegen-units")]
    pub codegen_units: Option<u32>,
}

/// The `[dispatch]` section: what the generated driver declares to
/// `byte_routine_dispatch!`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchSection {
    /// Output width in bytes. The observed universal is 8.
    #[serde(default = "default_dispatch_out")]
    pub out:    usize,
    /// Explicit monomorphisation list, overriding the default union
    /// of every bench's points. Narrowing only; a manifest point
    /// outside the effective list is a targeted runtime error.
    #[serde(default, alias = "sizes")]
    pub points: Vec<usize>,
}

fn default_dispatch_out() -> usize {
    8
}

/// One `[workload.<name>]` section: an ordered stage list. Each
/// entry is a builtin stage constructor name, optionally with one
/// integer argument: `"algo_call"`, `"scalar_work 48"`,
/// `"graph_work 32"`, `"heavy_memory 384"`, `"branch_work 24"`,
/// `"light_scalar"`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkloadSection {
    /// The ordered stages of the program.
    pub stages: Vec<String>,
}

/// Opt-in for the generated `BENCHES.md` pass.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocgenSection {
    /// Whether the docs regeneration pass emits a generated
    /// `BENCHES.md` for this bench tree.
    #[serde(default)]
    pub enabled: bool,
}

/// One named bench inside a [`BenchManifest`].
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    /// The canonical spelling is `arms`; `variants` is the accepted
    /// legacy spelling. Writing both is a duplicate-field error.
    #[serde(default, alias = "arms")]
    pub variants:    Vec<String>,
    /// The N values the bench runs at. Two TOML shapes are accepted:
    /// a plain integer array (`sizes = [64, 256, 1024]`, using the
    /// bench-level `variants`) or the array-of-tables form
    /// (`[[bench.<name>.sizes]]` with `n` and an optional per-size
    /// `variants` list overriding the bench-level one).
    /// The canonical spelling is `points`; `sizes` is the accepted
    /// legacy spelling. A point is the integer parameter of one cell
    /// and is not necessarily a size: several trees pack encoded case
    /// keys into it.
    #[serde(default, deserialize_with = "de_sizes", alias = "points")]
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
    /// Declared role: the arm every delta is computed against. The
    /// flattened spelling of `[bench.<name>.normalise] baseline`;
    /// writing both forms is refused at load.
    #[serde(default)]
    pub baseline:    Option<String>,
    /// Declared role: the null-cost arm subtracted from every arm
    /// (including the baseline) before ratios. Flattened spelling of
    /// `normalise.floor`; requires `baseline`.
    #[serde(default)]
    pub floor:       Option<String>,
    /// Which normalised column to add: `"subtract"` (default),
    /// `"ratio"`, `"percent"`, or `"none"`. Flattened spelling of
    /// `normalise.mode`; requires `baseline`.
    #[serde(default)]
    pub delta:       Option<String>,
    /// Per-bench timing override. Any field left out falls back to
    /// the global `[timing]` section.
    #[serde(default)]
    pub timing:      Option<TimingOverride>,
    /// Optional differential-analysis config. When present, the
    /// findings report is normalised against a named baseline variant
    /// so the shared common cost (interpreter overhead, fixed sibling
    /// work) cancels and each variant's attributable delta is
    /// surfaced. See [`NormaliseSection`].
    #[serde(default)]
    pub normalise:   Option<NormaliseSection>,
}

/// Differential-analysis config: declare a baseline VARIANT to
/// normalise the bench against.
///
/// When present, report generation selects `baseline` as the analysis
/// baseline (instead of the default first variant), so every variant
/// is measured against it via the harness's paired-sample comparison
/// (same seed/input, subtract, then the delta distribution + CI).
/// This is the statistically-correct way to remove a shared common
/// cost that would otherwise swamp the variant-to-variant signal.
///
/// The raw per-variant columns are kept; `mode` selects which
/// normalised column(s) to add alongside them.
///
/// TODO(post_process symbol): the declarative baseline here covers the
/// common "normalise against a named variant" case. For arbitrary
/// derived metrics (composite scores, cross-N regressions, custom
/// transforms), add an optional `post_process` code symbol to the
/// `DriverRegistry`, symmetric with `build_workload` / `dispatch`,
/// receiving the collected `BenchResult` and returning extra findings.
/// It must NOT live in the `timed!` macro: normalisation is a
/// cross-variant, post-collection operation and the timing macro has
/// no visibility across variants.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormaliseSection {
    /// Variant short name used as the analysis baseline.
    pub baseline: String,
    /// Which normalised column(s) to add to the report table:
    /// `"subtract"` (paired `variant - baseline` in ns, the default
    /// when the block is present), `"ratio"` (`variant / baseline`),
    /// or `"percent"` (the relative % already shown). `"none"` keeps
    /// the declared baseline for the existing columns without adding a
    /// new one (some benches want the baseline choice but raw framing).
    #[serde(default)]
    pub mode:     Option<String>,
    /// Null-floor differencing target: a variant short name whose per-call time
    /// is subtracted from every variant (including the baseline) before ratios,
    /// isolating pure dispatch cost above the null-dispatch floor. `None` = raw.
    #[serde(default)]
    pub floor:    Option<String>,
}

/// One `(N, [variants])` pair inside a [`BenchSection`].
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SizeSection {
    /// Logical size parameter passed into `bench_entry(... , n: usize)`.
    pub n:        usize,
    /// Per-size variant entries (short names or paths, same grammar
    /// as the bench-level list). Empty means "use the bench-level
    /// `variants`". Accepts the canonical `arms` spelling too.
    #[serde(default, alias = "arms")]
    pub variants: Vec<String>,
}

/// Per-bench timing override: every knob optional, merged over the
/// global [`TimingSection`] by [`BenchManifest::for_size`].
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

/// Deserialize an optional `master_seed` with the same integer-or-
/// string grammar as [`de_seed`]. Used by the nested per-bench schema
/// where absence means "inherit".
pub(crate) fn de_seed_opt<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<u64>, D::Error> {
    de_seed(d).map(Some)
}

/// Deserialize `sizes` from either a plain integer array or the
/// array-of-tables form.
pub(crate) fn de_sizes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<SizeSection>, D::Error> {
    // Not `#[serde(untagged)]`. An untagged enum swallows the
    // struct-level `deny_unknown_fields` inside a failed variant and
    // reports "data did not match any variant", naming neither the
    // offending key nor the accepted ones, with the span on the table
    // header. That is the shape most manifests use for per-size
    // variants, so the deny would have been useless exactly where it
    // is needed. `deserialize_any` dispatches on the TOML shape and
    // forwards a map straight through, so the inner error survives.
    enum Raw {
        Bare(usize),
        Full(SizeSection),
    }
    impl<'de> Deserialize<'de> for Raw {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> serde::de::Visitor<'de> for V {
                type Value = Raw;

                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("an integer size or a table with `n` and optional `variants`")
                }

                fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Raw, E> {
                    Ok(Raw::Bare(v as usize))
                }

                fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Raw, E> {
                    Ok(Raw::Bare(v as usize))
                }

                fn visit_map<A: serde::de::MapAccess<'de>>(self, m: A) -> Result<Raw, A::Error> {
                    SizeSection::deserialize(serde::de::value::MapAccessDeserializer::new(m))
                        .map(Raw::Full)
                }
            }
            d.deserialize_any(V)
        }
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
#[serde(deny_unknown_fields)]
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
    ///
    /// An unrecognised key is an error rather than a silent default.
    /// Every section carries `deny_unknown_fields`, because a
    /// measurement tool that ignores a key its author wrote is the
    /// worst shape available: the run succeeds, the report looks
    /// ordinary, and the setting was never applied. A typoed
    /// `threaded` leaves the harness pinning a threaded workload to
    /// one core and skewing every timing. A typoed `master_seed`
    /// trades reproducibility for a fresh random seed. Neither says
    /// anything at the time, and both are invisible in the artifact.
    ///
    /// The error carries the path because a consumer with several
    /// bench trees otherwise has to guess which one refused.
    pub fn load(path: &Path) -> Result<Self, BenchError> {
        let text =
            std::fs::read_to_string(path).map_err(|e| BenchError::io("reading bench.toml", e))?;
        let manifest: BenchManifest = toml::from_str(&text).map_err(|e| {
            let shown = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
            BenchError::InvalidConfig {
                reason: format!("{}: {e}", shown.display()),
            }
        })?;
        manifest.validate_roles()?;
        Ok(manifest)
    }

    /// Refuse contradictory or incomplete role declarations.
    ///
    /// The flattened `baseline` / `floor` / `delta` keys and the
    /// `[bench.<name>.normalise]` table express the same thing; a
    /// section carrying both would leave which one wins to reading
    /// the source, so it is refused. `floor` and `delta` qualify a
    /// baseline and are refused without one, matching the table form
    /// where `baseline` is required.
    pub fn validate_roles(&self) -> Result<(), BenchError> {
        for (name, section) in &self.bench {
            let flattened =
                section.baseline.is_some() || section.floor.is_some() || section.delta.is_some();
            if flattened && section.normalise.is_some() {
                return Err(BenchError::InvalidConfig {
                    reason: format!(
                        "bench `{name}` declares roles twice: flattened                          `baseline`/`floor`/`delta` keys and a `normalise` table.                          Keep one form; the flattened keys are the canonical one."
                    ),
                });
            }
            if section.baseline.is_none() && (section.floor.is_some() || section.delta.is_some()) {
                return Err(BenchError::InvalidConfig {
                    reason: format!(
                        "bench `{name}` declares `floor` or `delta` without a                          `baseline`. Both qualify a baseline arm; name one."
                    ),
                });
            }
        }
        Ok(())
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
            let available = self.bench_names().join(", ");
            BenchError::InvalidConfig {
                reason: format!(
                    "bench `{bench_name}` not found in manifest. Available: {available}"
                ),
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
        // Effective roles: the normalise table and the flattened keys
        // are mutually exclusive (validate_roles), so whichever is
        // present wins without a precedence question.
        let (nz_baseline, nz_mode, nz_floor) = if let Some(nz) = &section.normalise {
            (
                Some(nz.baseline.clone()),
                Some(nz.mode.clone().unwrap_or_else(|| "subtract".to_string())),
                nz.floor.clone(),
            )
        } else if let Some(b) = &section.baseline {
            (
                Some(b.clone()),
                Some(section.delta.clone().unwrap_or_else(|| "subtract".to_string())),
                section.floor.clone(),
            )
        } else {
            (None, None, None)
        };
        let (bench, sweep) = self
            .nested
            .get(bench_name)
            .cloned()
            .unwrap_or_else(|| (bench_name.to_string(), bench_name.to_string()));
        Ok(BenchConfig {
            bench_name: bench_name.to_string(),
            bench,
            sweep,
            nested: self.nested_mode,
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
            normalise_baseline: nz_baseline,
            normalise_mode: nz_mode,
            normalise_floor: nz_floor,
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
    /// Manifest key for this bench (e.g. `"content_hash"`). In a
    /// nested tree this is the unique `<bench>/<sweep>` composite;
    /// in a flat tree it is the section name.
    pub bench_name:    String,
    /// The bench this cell belongs to: the directory name in a
    /// nested tree, the section name in a flat one. This is what
    /// [`crate::routine_table!`] and the `routine_for` hook key on.
    pub bench:         String,
    /// The sweep within the bench. Equal to the bench name in a flat
    /// tree, where every section is its own single sweep.
    pub sweep:         String,
    /// Whether this cell came from a nested tree. Decides the output
    /// naming (`<bench>/<sweep>_n<point>_report.md` against the flat
    /// `<bench>/<bench>_n<n>_findings.md`) and the history root.
    pub nested:        bool,
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
    /// Optional analysis-baseline variant name (from the manifest's
    /// `[bench.<name>.normalise]`). When set, report generation
    /// normalises against this variant. `None` = default baseline
    /// (first variant).
    pub normalise_baseline: Option<String>,
    /// Which normalised column(s) to add (from `normalise.mode`);
    /// `None` = the default (`subtract`) when a baseline is set.
    pub normalise_mode:     Option<String>,
    /// Null-floor differencing target variant (from `normalise.floor`); when set,
    /// the report subtracts this variant's per-call time from every variant before
    /// ratios, isolating pure dispatch cost. `None` = no floor differencing.
    pub normalise_floor:    Option<String>,
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
            bench:         String::new(),
            sweep:         String::new(),
            nested:        false,
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
            normalise_baseline: None,
            normalise_mode:     None,
            normalise_floor:    None,
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
    fn normalise_section_parses_and_flows_to_config() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["alpha", "beta", "gamma"]
            sizes = [64]
            [bench.b.normalise]
            baseline = "beta"
            mode = "subtract"
        "#,
        );
        let nz = m.bench["b"].normalise.as_ref().expect("normalise parsed");
        assert_eq!(nz.baseline, "beta");
        assert_eq!(nz.mode.as_deref(), Some("subtract"));
        let cfg = m.for_size("b", 0, Path::new(".")).expect("for_size");
        assert_eq!(cfg.normalise_baseline.as_deref(), Some("beta"));
        assert_eq!(cfg.normalise_mode.as_deref(), Some("subtract"));
    }

    #[test]
    fn normalise_defaults_to_subtract_and_absent_is_none() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["alpha", "beta"]
            sizes = [64]
            [bench.b.normalise]
            baseline = "alpha"
        "#,
        );
        let cfg = m.for_size("b", 0, Path::new(".")).expect("for_size");
        assert_eq!(cfg.normalise_mode.as_deref(), Some("subtract"));
        // a bench with no normalise block => None (default baseline behaviour)
        let plain = manifest(BASE).for_size("b", 0, Path::new(".")).expect("for_size");
        assert!(plain.normalise_baseline.is_none());
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
    fn the_arms_spelling_parses_identically_to_variants() {
        let canonical = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            arms = ["alpha", "beta"]
            points = [64, 1024]
        "#,
        );
        let legacy = manifest(BASE);
        let c = canonical.for_size("b", 1, Path::new("/root")).unwrap();
        let l = legacy.for_size("b", 1, Path::new("/root")).unwrap();
        assert_eq!(c.variant_paths, l.variant_paths);
        assert_eq!(c.n, l.n);
    }

    #[test]
    fn the_arms_spelling_works_per_point_too() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"

            [[bench.b.points]]
            n = 64
            arms = ["gamma"]
        "#,
        );
        let c = m.for_size("b", 0, Path::new("/root")).unwrap();
        assert!(c.variant_paths[0].display().to_string().contains("gamma"));
    }

    #[test]
    fn writing_both_spellings_of_one_field_is_refused() {
        // serde reports the duplicate under the canonical field name,
        // which is the field both spellings map to.
        let err = toml::from_str::<BenchManifest>(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            variants = ["a"]
            arms = ["a"]
            sizes = [64]
        "#,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("duplicate"), "got: {err}");
    }

    #[test]
    fn flattened_roles_flow_to_the_config() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            arms = ["alpha", "beta", "nullarm"]
            points = [64]
            baseline = "beta"
            floor = "nullarm"
            delta = "ratio"
        "#,
        );
        m.validate_roles().expect("roles are consistent");
        let c = m.for_size("b", 0, Path::new(".")).unwrap();
        assert_eq!(c.normalise_baseline.as_deref(), Some("beta"));
        assert_eq!(c.normalise_floor.as_deref(), Some("nullarm"));
        assert_eq!(c.normalise_mode.as_deref(), Some("ratio"));
    }

    #[test]
    fn flattened_baseline_defaults_delta_to_subtract() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            arms = ["alpha", "beta"]
            points = [64]
            baseline = "alpha"
        "#,
        );
        let c = m.for_size("b", 0, Path::new(".")).unwrap();
        assert_eq!(c.normalise_mode.as_deref(), Some("subtract"));
    }

    #[test]
    fn load_itself_refuses_double_roles_not_only_validate_roles() {
        // Guards the call site: `validate_roles` existing is not the
        // fix, `load` running it is. Deleting the call turns this red
        // while the direct `validate_roles` tests stay green.
        let dir = std::env::temp_dir().join(format!(
            "mockspace-bench-config-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bench.toml");
        std::fs::write(
            &path,
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            arms = ["alpha"]
            points = [64]
            baseline = "alpha"
            [bench.b.normalise]
            baseline = "alpha"
        "#,
        )
        .unwrap();
        let err = BenchManifest::load(&path).unwrap_err();
        assert!(format!("{err}").contains("declares roles twice"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn declaring_roles_in_both_forms_is_refused() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            arms = ["alpha", "beta"]
            points = [64]
            baseline = "alpha"
            [bench.b.normalise]
            baseline = "beta"
        "#,
        );
        let err = m.validate_roles().unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("declares roles twice"), "got: {msg}");
        assert!(msg.contains("`b`"), "names the bench: {msg}");
    }

    #[test]
    fn floor_or_delta_without_baseline_is_refused() {
        let m = manifest(
            r#"
            [bench.b]
            title = "B"
            workload = "default"
            arms = ["alpha", "nullarm"]
            points = [64]
            floor = "nullarm"
        "#,
        );
        let err = m.validate_roles().unwrap_err();
        assert!(format!("{err}").contains("without a"), "got: {err}");
    }

    #[test]
    fn a_flat_tree_config_carries_bench_and_sweep_equal_to_the_section_name() {
        let c = manifest(BASE).for_size("b", 0, Path::new("/root")).unwrap();
        assert_eq!(c.bench, "b");
        assert_eq!(c.sweep, "b");
        assert_eq!(c.bench_name, "b");
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

    /// A key nobody reads must not be accepted. The failure this
    /// prevents is silent: the run succeeds, the report looks
    /// ordinary, and the setting the author wrote was discarded.
    #[test]
    fn an_unknown_top_level_section_is_refused_and_named() {
        let err = toml::from_str::<BenchManifest>(
            "[telemetry]\nenabled = true\n\n[bench.b]\ntitle = \"t\"\nworkload = \"w\"\n",
        )
        .expect_err("an unread section must not parse");
        let msg = format!("{err}");
        assert!(msg.contains("telemetry"), "does not name the offending key: {msg}");
    }

    /// `[docgen]` is a shipped feature, not an unknown key. An
    /// earlier revision of this change refused it and shipped a test
    /// asserting the refusal, which would have turned off a working
    /// docs pass and left a test standing as the reason not to
    /// restore it.
    #[test]
    fn the_docgen_section_is_a_feature_and_still_parses() {
        let m = toml::from_str::<BenchManifest>("[docgen]\nenabled = true\n")
            .expect("docgen is a real section");
        assert!(m.docgen.expect("present").enabled);
    }

    /// And it is typed rather than merely tolerated, so a typo
    /// inside it is still caught.
    #[test]
    fn a_typo_inside_docgen_is_still_refused() {
        let err = toml::from_str::<BenchManifest>("[docgen]\nenabledd = true\n")
            .expect_err("a typo inside a known section must not parse");
        assert!(format!("{err}").contains("enabledd"));
    }

    /// A typo in a bench key falls back to a default that is not
    /// what the author asked for. `threaded` is the sharpest case:
    /// left false, the harness pins a threaded workload to one core
    /// and every timing in the run is skewed.
    #[test]
    fn a_typoed_bench_key_is_refused_rather_than_defaulted() {
        let src = "[bench.b]\ntitle = \"t\"\nworkload = \"w\"\nthreadedd = true\n";
        let err = toml::from_str::<BenchManifest>(src).expect_err("a typo must not parse");
        assert!(format!("{err}").contains("threadedd"));
    }

    /// The array-of-tables `sizes` form is the shape most manifests
    /// use, and an untagged wrapper used to swallow the deny there:
    /// the error named no key and pointed at the table header. This
    /// asserts the offending key survives to the message.
    #[test]
    fn a_typo_inside_the_array_of_tables_sizes_form_names_the_key() {
        let src = "[bench.b]\ntitle = \"t\"\nworkload = \"w\"\n\n\
                   [[bench.b.sizes]]\nn = 64\nvariantss = [\"a\"]\n";
        let err = toml::from_str::<BenchManifest>(src).expect_err("a typo must not parse");
        let msg = format!("{err}");
        assert!(msg.contains("variantss"), "the key is not named: {msg}");
    }

    /// Both `sizes` shapes must still parse, since the hand-written
    /// deserializer that fixed the error message is where a shape
    /// would be lost.
    #[test]
    fn both_sizes_shapes_survive_the_hand_written_deserializer() {
        let bare = manifest(
            "[bench.b]\ntitle = \"t\"\nworkload = \"w\"\nvariants = [\"a\"]\nsizes = [16, 32]\n",
        );
        assert_eq!(bare.bench["b"].sizes.len(), 2);
        assert_eq!(bare.bench["b"].sizes[0].n, 16);
        let tabled = manifest(
            "[bench.b]\ntitle = \"t\"\nworkload = \"w\"\n\n\
             [[bench.b.sizes]]\nn = 64\nvariants = [\"a\"]\n",
        );
        assert_eq!(tabled.bench["b"].sizes[0].n, 64);
        assert_eq!(tabled.bench["b"].sizes[0].variants, vec!["a".to_string()]);
    }

    /// The control, and it covers every field of every struct that
    /// carries the deny. Named for what it does: a sampled version
    /// of this would not notice the deny starting to refuse a valid
    /// manifest in the structs it skipped.
    #[test]
    fn every_field_of_every_denied_struct_still_parses() {
        let src = "\n[timing]\npasses = 2\nruns_per_pass = 3\nbatch_size = 4\n\
                   harness_runs = 5\ncooldowns_ms = [1, 2]\n\n\
                   [docgen]\nenabled = true\n\n\
                   [bench.b]\ntitle = \"t\"\nworkload = \"w\"\nmaster_seed = 7\n\
                   variants = [\"a\"]\nmay_differ = true\nrequired = true\n\
                   threaded = true\n\n\
                   [bench.b.timing]\npasses = 9\nruns_per_pass = 8\nbatch_size = 7\n\
                   harness_runs = 6\ncooldowns_ms = [3]\n\n\
                   [bench.b.normalise]\nbaseline = \"a\"\nmode = \"ratio\"\nfloor = \"a\"\n\n\
                   [[bench.b.sizes]]\nn = 64\nvariants = [\"a\"]\n";
        let m = toml::from_str::<BenchManifest>(src).expect("every documented key must parse");
        let b = m.bench.get("b").expect("bench present");
        assert!(b.may_differ && b.required && b.threaded);
        assert_eq!(b.master_seed, 7);
        assert_eq!(m.timing.cooldowns_ms, vec![1, 2]);
        assert!(m.docgen.expect("docgen present").enabled);
        let t = b.timing.as_ref().expect("override present");
        assert_eq!(t.cooldowns_ms, Some(vec![3]));
        let n = b.normalise.as_ref().expect("normalise present");
        assert_eq!(n.baseline, "a");
        assert_eq!(n.mode.as_deref(), Some("ratio"));
        assert_eq!(n.floor.as_deref(), Some("a"));
        assert_eq!(b.sizes[0].n, 64);
    }
}
