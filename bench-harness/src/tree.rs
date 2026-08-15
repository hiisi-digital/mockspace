//! Nested bench-tree loading: one directory per bench, sweeps inside.
//!
//! A nested tree keeps everything a human authors about one bench in
//! that bench's own directory, and everything generated in one shared
//! output tree:
//!
//! ```text
//! mock/benches/
//!   bench.toml            globals: [timing] [dispatch] [workload.*]
//!   src/lib.rs            optional hooks crate
//!   support/<name>/       support crates used by several benches
//!   <bench>/
//!     bench.toml          [bench] meta + [sweep.<name>] sections
//!     arms/<arm>/         measured cdylibs (Cargo.toml optional)
//!     support/<name>/     this bench's support crates
//!   results/  history/  target/
//! ```
//!
//! A first-level subdirectory is a bench iff it contains a
//! `bench.toml`; the reserved names never are. A tree with no bench
//! subdirectories is a flat legacy tree and is loaded by
//! [`crate::config::BenchManifest::load`] unchanged.
//!
//! [`load_tree`] composes the per-bench files into one
//! [`BenchManifest`] whose keys are `<bench>` (a bench with one
//! default sweep) or `<bench>/<sweep>`, so the flat-config conversion
//! and the driver work identically on both layouts. Arm short names
//! are resolved at composition into explicit paths under the
//! tool-owned target tree (`target/mock-arms/<bench>/<arm>/release/`),
//! which is what makes "the directory name is the arm's name" true by
//! construction rather than by workspace accident.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::{
    BenchManifest, BenchSection, SizeSection, TimingOverride, de_seed_opt, de_sizes,
};
use crate::error::BenchError;

/// Directory names at the benches root (and inside a bench dir) that
/// are never benches, whatever they contain.
pub const RESERVED_DIRS: &[&str] = &[
    "support", "results", "history", "src", "target", "arms", "variants",
];

/// Whether `benches_dir` is a nested tree: at least one first-level,
/// non-reserved subdirectory carrying its own `bench.toml`.
pub fn is_nested_tree(benches_dir: &Path) -> bool {
    !bench_dirs(benches_dir).is_empty()
}

/// The bench directories of a nested tree, sorted by name.
pub fn bench_dirs(benches_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(benches_dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .and_then(|f| f.to_str())
                .is_some_and(|name| !RESERVED_DIRS.contains(&name) && !name.starts_with('.'))
        })
        .filter(|p| p.join("bench.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// One discovered arm: a directory under `<bench>/arms/`.
#[derive(Clone, Debug)]
pub struct ArmSource {
    /// The bench the arm belongs to (directory name).
    pub bench:        String,
    /// The arm name (directory name). In a nested tree this is also
    /// the crate and lib name, sanitized.
    pub arm:          String,
    /// The arm's source directory.
    pub dir:          PathBuf,
    /// Whether the consumer wrote a `Cargo.toml` (the escape hatch).
    /// Without one the tool generates the manifest.
    pub has_manifest: bool,
}

/// One discovered support crate: a directory under `support/` at the
/// root or inside a bench.
#[derive(Clone, Debug)]
pub struct SupportSource {
    /// Owning bench, or `None` for a root-level support crate.
    pub bench: Option<String>,
    /// The support crate's directory name.
    pub name:  String,
    /// The support crate's directory.
    pub dir:   PathBuf,
}

/// A composed nested tree: the manifest the driver consumes plus the
/// source inventory the tool builds from.
#[derive(Debug, Default)]
pub struct TreeManifest {
    /// The composed manifest. Keys are `<bench>` or `<bench>/<sweep>`;
    /// `nested` and `nested_mode` are populated so
    /// [`BenchManifest::for_size`] carries bench and sweep apart.
    pub manifest: BenchManifest,
    /// Every discovered arm, whether or not a sweep references it.
    pub arms:     Vec<ArmSource>,
    /// Every discovered support crate.
    pub support:  Vec<SupportSource>,
}

/// The `[bench]` table of a per-bench `bench.toml`.
///
/// The nested schema accepts only the canonical vocabulary: `arms`
/// and `points` have no legacy aliases here and the `normalise` table
/// does not exist, because no nested tree predates the vocabulary.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchMeta {
    title:       String,
    #[serde(default = "default_workload")]
    workload:    String,
    #[serde(default, deserialize_with = "de_seed_opt")]
    master_seed: Option<u64>,
    #[serde(default)]
    arms:        Vec<String>,
    #[serde(default, deserialize_with = "de_sizes")]
    points:      Vec<SizeSection>,
    #[serde(default)]
    may_differ:  bool,
    #[serde(default)]
    required:    bool,
    #[serde(default)]
    threaded:    bool,
    #[serde(default)]
    baseline:    Option<String>,
    #[serde(default)]
    floor:       Option<String>,
    #[serde(default)]
    delta:       Option<String>,
    #[serde(default)]
    timing:      Option<TimingOverride>,
}

fn default_workload() -> String {
    "default".to_string()
}

/// One `[sweep.<name>]` section: everything optional except the
/// points, inheriting the `[bench]` value where absent.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepSection {
    #[serde(default, deserialize_with = "de_sizes")]
    points:      Vec<SizeSection>,
    #[serde(default)]
    arms:        Vec<String>,
    #[serde(default)]
    title:       Option<String>,
    #[serde(default)]
    workload:    Option<String>,
    #[serde(default, deserialize_with = "de_seed_opt")]
    master_seed: Option<u64>,
    #[serde(default)]
    may_differ:  Option<bool>,
    #[serde(default)]
    required:    Option<bool>,
    #[serde(default)]
    threaded:    Option<bool>,
    #[serde(default)]
    baseline:    Option<String>,
    #[serde(default)]
    floor:       Option<String>,
    #[serde(default)]
    delta:       Option<String>,
    #[serde(default)]
    timing:      Option<TimingOverride>,
}

/// A per-bench `bench.toml`: the `[bench]` table plus sweeps.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchDirManifest {
    bench: BenchMeta,
    #[serde(default)]
    sweep: HashMap<String, SweepSection>,
}

/// Load and compose a nested tree.
///
/// The root `bench.toml` is optional and carries only globals; a root
/// file with `[bench.*]` sections beside bench directories is refused,
/// because two homes for one bench would leave which one runs to
/// directory-listing order.
pub fn load_tree(benches_dir: &Path) -> Result<TreeManifest, BenchError> {
    let dirs = bench_dirs(benches_dir);
    if dirs.is_empty() {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "{} has no bench directories (a bench is a subdirectory with its own \
                 bench.toml; {:?} are reserved and never benches)",
                benches_dir.display(),
                RESERVED_DIRS
            ),
        });
    }

    let root_path = benches_dir.join("bench.toml");
    let mut manifest = if root_path.is_file() {
        let root = BenchManifest::load(&root_path)?;
        if !root.bench.is_empty() {
            let names = root.bench.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(BenchError::InvalidConfig {
                reason: format!(
                    "{} declares [bench.*] sections ({names}) but this tree also has \
                     bench directories. In a nested tree the root bench.toml carries \
                     only globals ([timing], [dispatch], [workload.*]); move each \
                     section into <bench>/bench.toml.",
                    root_path.display()
                ),
            });
        }
        root
    } else {
        BenchManifest::default()
    };
    manifest.nested_mode = true;

    let mut tree = TreeManifest::default();
    tree.support.extend(support_in(benches_dir, None));

    for dir in dirs {
        let bench = dir
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or_default()
            .to_string();
        let text = std::fs::read_to_string(dir.join("bench.toml"))
            .map_err(|e| BenchError::io("reading bench.toml", e))?;
        let parsed: BenchDirManifest = toml::from_str(&text).map_err(|e| {
            BenchError::InvalidConfig {
                reason: format!("{}: {e}", dir.join("bench.toml").display()),
            }
        })?;

        let arms = discover_arms(&bench, &dir);
        tree.support.extend(support_in(&dir, Some(&bench)));

        if !parsed.bench.points.is_empty() && !parsed.sweep.is_empty() {
            return Err(BenchError::InvalidConfig {
                reason: format!(
                    "bench `{bench}` declares points on [bench] and [sweep.*] sections. \
                     With sweeps present, every point list lives in its sweep; move \
                     the [bench] points into one."
                ),
            });
        }
        if parsed.bench.points.is_empty() && parsed.sweep.is_empty() {
            return Err(BenchError::InvalidConfig {
                reason: format!(
                    "bench `{bench}` declares no points and no sweeps, so there is \
                     nothing to run. Add `points = [...]` to [bench] or a [sweep.<name>] \
                     section."
                ),
            });
        }

        let mut sweeps: Vec<(String, SweepSection)> = if parsed.sweep.is_empty() {
            // A single default sweep named after the bench, from the
            // [bench] table itself.
            vec![(bench.clone(), SweepSection {
                points:      parsed.bench.points.clone(),
                arms:        Vec::new(),
                title:       None,
                workload:    None,
                master_seed: None,
                may_differ:  None,
                required:    None,
                threaded:    None,
                baseline:    None,
                floor:       None,
                delta:       None,
                timing:      None,
            })]
        } else {
            parsed.sweep.clone().into_iter().collect()
        };
        sweeps.sort_by(|a, b| a.0.cmp(&b.0));

        for (sweep_name, sweep) in sweeps {
            if sweep.points.is_empty() {
                return Err(BenchError::InvalidConfig {
                    reason: format!(
                        "bench `{bench}` sweep `{sweep_name}` has no points; add \
                         `points = [...]`"
                    ),
                });
            }
            let entries = if sweep.arms.is_empty() { &parsed.bench.arms } else { &sweep.arms };
            if entries.is_empty() {
                return Err(BenchError::InvalidConfig {
                    reason: format!(
                        "bench `{bench}` sweep `{sweep_name}` lists no arms (neither \
                         the sweep nor the [bench] table names any)"
                    ),
                });
            }
            let resolved: Vec<String> = entries
                .iter()
                .map(|e| resolve_entry(&bench, e, &arms))
                .collect::<Result<_, _>>()?;
            let points = sweep
                .points
                .iter()
                .map(|p| {
                    Ok(SizeSection {
                        n:        p.n,
                        variants: p
                            .variants
                            .iter()
                            .map(|e| resolve_entry(&bench, e, &arms))
                            .collect::<Result<_, BenchError>>()?,
                    })
                })
                .collect::<Result<Vec<_>, BenchError>>()?;

            let key = if sweep_name == bench {
                bench.clone()
            } else {
                format!("{bench}/{sweep_name}")
            };
            let section = BenchSection {
                title:       sweep.title.clone().unwrap_or_else(|| parsed.bench.title.clone()),
                workload:    sweep
                    .workload
                    .clone()
                    .unwrap_or_else(|| parsed.bench.workload.clone()),
                master_seed: sweep
                    .master_seed
                    .or(parsed.bench.master_seed)
                    .unwrap_or_default(),
                variants:    resolved,
                sizes:       points,
                may_differ:  sweep.may_differ.unwrap_or(parsed.bench.may_differ),
                required:    sweep.required.unwrap_or(parsed.bench.required),
                threaded:    sweep.threaded.unwrap_or(parsed.bench.threaded),
                baseline:    sweep.baseline.clone().or_else(|| parsed.bench.baseline.clone()),
                floor:       sweep.floor.clone().or_else(|| parsed.bench.floor.clone()),
                delta:       sweep.delta.clone().or_else(|| parsed.bench.delta.clone()),
                timing:      sweep.timing.clone().or_else(|| parsed.bench.timing.clone()),
                normalise:   None,
            };
            manifest.bench.insert(key.clone(), section);
            manifest
                .nested
                .insert(key, (bench.clone(), sweep_name.clone()));
        }

        tree.arms.extend(arms);
    }

    manifest.validate_roles()?;
    tree.manifest = manifest;
    Ok(tree)
}

/// The tool-owned build location of one arm's release artifacts,
/// relative to the benches root. Per-arm target directories cannot
/// collide however the arms are named across benches.
pub fn arm_target_dir(bench: &str, arm: &str) -> PathBuf {
    PathBuf::from("target")
        .join("mock-arms")
        .join(bench)
        .join(arm)
}

/// The dylib file name an arm builds to. The crate name is the arm
/// directory name with dashes as underscores, so the file name
/// follows from it.
pub fn arm_dylib_name(arm: &str) -> String {
    format!(
        "{}{}{}",
        std::env::consts::DLL_PREFIX,
        arm.replace('-', "_"),
        std::env::consts::DLL_SUFFIX
    )
}

/// Resolve one sweep entry. A short name must be a discovered arm of
/// this bench and resolves into the tool-owned target tree; a path
/// entry is made relative to the benches root by prefixing the bench
/// directory.
fn resolve_entry(bench: &str, entry: &str, arms: &[ArmSource]) -> Result<String, BenchError> {
    let is_short = !entry.contains('/') && !entry.contains(std::path::MAIN_SEPARATOR);
    if !is_short {
        return Ok(format!("{bench}/{entry}"));
    }
    if !arms.iter().any(|a| a.arm == entry) {
        let available: Vec<&str> = arms.iter().map(|a| a.arm.as_str()).collect();
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{bench}` names arm `{entry}`, but {bench}/arms/{entry}/ does \
                 not exist. Discovered arms: [{}]. An arm outside arms/ is referenced \
                 by path.",
                available.join(", ")
            ),
        });
    }
    Ok(arm_target_dir(bench, entry)
        .join("release")
        .join(arm_dylib_name(entry))
        .to_string_lossy()
        .into_owned())
}

fn discover_arms(bench: &str, bench_dir: &Path) -> Vec<ArmSource> {
    let arms_dir = bench_dir.join("arms");
    let Ok(entries) = std::fs::read_dir(&arms_dir) else {
        return Vec::new();
    };
    let mut arms: Vec<ArmSource> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let arm = p.file_name()?.to_str()?.to_string();
            Some(ArmSource {
                bench:        bench.to_string(),
                arm,
                has_manifest: p.join("Cargo.toml").is_file(),
                dir:          p,
            })
        })
        .collect();
    arms.sort_by(|a, b| a.arm.cmp(&b.arm));
    arms
}

fn support_in(dir: &Path, bench: Option<&str>) -> Vec<SupportSource> {
    let support_dir = dir.join("support");
    let Ok(entries) = std::fs::read_dir(&support_dir) else {
        return Vec::new();
    };
    let mut found: Vec<SupportSource> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|p| {
            let name = p.file_name()?.to_str()?.to_string();
            Some(SupportSource {
                bench: bench.map(str::to_string),
                name,
                dir: p,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree {
        root: PathBuf,
    }

    impl Tree {
        fn new(name: &str) -> Tree {
            let root = std::env::temp_dir()
                .join(format!("mockspace-tree-test-{}-{name}", std::process::id()));
            std::fs::remove_dir_all(&root).ok();
            std::fs::create_dir_all(&root).unwrap();
            Tree { root }
        }

        fn write(&self, rel: &str, contents: &str) -> &Self {
            let p = self.root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, contents).unwrap();
            self
        }

        fn mkdir(&self, rel: &str) -> &Self {
            std::fs::create_dir_all(self.root.join(rel)).unwrap();
            self
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.root).ok();
        }
    }

    const HASH_BENCH: &str = r#"
        [bench]
        title = "Hash mixers"
        workload = "realistic"
        arms = ["fnv", "xx"]
        points = [64, 256]
    "#;

    #[test]
    fn a_flat_tree_is_not_nested_and_a_bench_dir_makes_it_nested() {
        let t = Tree::new("detect");
        t.write("bench.toml", "[bench.b]\ntitle='B'\nworkload='w'\n");
        assert!(!is_nested_tree(&t.root));
        t.write("hash/bench.toml", HASH_BENCH);
        assert!(is_nested_tree(&t.root));
    }

    #[test]
    fn reserved_directories_are_never_benches() {
        let t = Tree::new("reserved");
        for d in RESERVED_DIRS {
            t.write(&format!("{d}/bench.toml"), HASH_BENCH);
        }
        assert!(!is_nested_tree(&t.root), "reserved dirs must not read as benches");
    }

    #[test]
    fn a_single_sweep_bench_composes_under_its_own_name() {
        let t = Tree::new("single");
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        let tree = load_tree(&t.root).unwrap();
        assert!(tree.manifest.nested_mode);
        assert_eq!(tree.manifest.bench_names(), vec!["hash"]);
        let c = tree.manifest.for_size("hash", 1, &t.root).unwrap();
        assert_eq!(c.bench, "hash");
        assert_eq!(c.sweep, "hash");
        assert_eq!(c.n, 256);
        // short names resolved into the tool-owned target tree
        let p = c.variant_paths[0].display().to_string();
        assert!(
            p.contains("target/mock-arms/hash/fnv/release/"),
            "resolved into the tool target tree: {p}"
        );
        assert!(p.ends_with(std::env::consts::DLL_SUFFIX));
    }

    #[test]
    fn named_sweeps_compose_as_bench_slash_sweep_and_inherit_the_bench_meta() {
        let t = Tree::new("sweeps");
        t.write(
            "warm/bench.toml",
            r#"
            [bench]
            title = "Warm container"
            workload = "realistic"
            master_seed = 7
            arms = ["kernel", "native"]
            baseline = "native"

            [sweep.width-l1]
            points = [80003, 130003]

            [sweep.density-w13]
            points = [130001]
            arms = ["kernel"]
            required = true
        "#,
        );
        t.mkdir("warm/arms/kernel/src").mkdir("warm/arms/native/src");
        let tree = load_tree(&t.root).unwrap();
        assert_eq!(
            tree.manifest.bench_names(),
            vec!["warm/density-w13", "warm/width-l1"]
        );
        let c = tree.manifest.for_size("warm/width-l1", 0, &t.root).unwrap();
        assert_eq!((c.bench.as_str(), c.sweep.as_str()), ("warm", "width-l1"));
        assert_eq!(c.title, "Warm container");
        assert_eq!(c.workload, "realistic");
        assert_eq!(c.master_seed, 7);
        assert_eq!(c.variant_paths.len(), 2);
        assert_eq!(c.normalise_baseline.as_deref(), Some("native"));
        // the sweep-level overrides win where declared
        let d = tree.manifest.for_size("warm/density-w13", 0, &t.root).unwrap();
        assert_eq!(d.variant_paths.len(), 1);
        assert!(d.required);
        assert!(!c.required);
    }

    #[test]
    fn a_root_manifest_with_bench_sections_beside_bench_dirs_is_refused() {
        let t = Tree::new("both");
        t.write("bench.toml", "[bench.old]\ntitle='O'\nworkload='w'\n");
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        let err = load_tree(&t.root).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("only globals"), "got: {msg}");
        assert!(msg.contains("old"), "names the stray section: {msg}");
    }

    #[test]
    fn naming_an_arm_that_does_not_exist_is_refused_listing_what_does() {
        let t = Tree::new("missing-arm");
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src");
        let err = load_tree(&t.root).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("`xx`"), "names the missing arm: {msg}");
        assert!(msg.contains("fnv"), "lists the discovered arms: {msg}");
    }

    #[test]
    fn bench_level_points_and_sweeps_together_are_refused() {
        let t = Tree::new("ambiguous");
        t.write(
            "hash/bench.toml",
            r#"
            [bench]
            title = "H"
            arms = ["fnv"]
            points = [64]
            [sweep.wide]
            points = [128]
        "#,
        );
        t.mkdir("hash/arms/fnv/src");
        let err = load_tree(&t.root).unwrap_err();
        assert!(format!("{err}").contains("move"), "got: {err}");
    }

    #[test]
    fn a_sweep_without_points_is_refused_by_name() {
        let t = Tree::new("no-points");
        t.write(
            "hash/bench.toml",
            r#"
            [bench]
            title = "H"
            arms = ["fnv"]
            [sweep.wide]
            arms = ["fnv"]
        "#,
        );
        t.mkdir("hash/arms/fnv/src");
        let err = load_tree(&t.root).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("`wide`"), "names the sweep: {msg}");
    }

    #[test]
    fn the_legacy_normalise_table_does_not_exist_in_the_nested_schema() {
        let t = Tree::new("no-normalise");
        t.write(
            "hash/bench.toml",
            r#"
            [bench]
            title = "H"
            arms = ["fnv"]
            points = [64]
            [bench.normalise]
            baseline = "fnv"
        "#,
        );
        t.mkdir("hash/arms/fnv/src");
        let err = load_tree(&t.root).unwrap_err();
        assert!(
            format!("{err}").contains("normalise"),
            "the unknown key is named: {err}"
        );
    }

    #[test]
    fn arms_and_support_crates_are_discovered_and_told_apart_by_location() {
        let t = Tree::new("inventory");
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        t.write("hash/arms/fnv/Cargo.toml", "[package]\nname='fnv'\n");
        t.mkdir("hash/support/hash-kit/src");
        t.mkdir("support/carrier/src");
        let tree = load_tree(&t.root).unwrap();
        let arm_names: Vec<&str> = tree.arms.iter().map(|a| a.arm.as_str()).collect();
        assert_eq!(arm_names, vec!["fnv", "xx"]);
        assert!(tree.arms[0].has_manifest);
        assert!(!tree.arms[1].has_manifest);
        let support: Vec<(Option<&str>, &str)> = tree
            .support
            .iter()
            .map(|s| (s.bench.as_deref(), s.name.as_str()))
            .collect();
        assert_eq!(support, vec![(None, "carrier"), (Some("hash"), "hash-kit")]);
    }

    #[test]
    fn a_path_entry_is_made_relative_to_the_benches_root() {
        let t = Tree::new("path-entry");
        t.write(
            "hash/bench.toml",
            r#"
            [bench]
            title = "H"
            arms = ["../shared-arm/out/libx.dylib"]
            points = [64]
        "#,
        );
        let tree = load_tree(&t.root).unwrap();
        let c = tree.manifest.for_size("hash", 0, &t.root).unwrap();
        let p = c.variant_paths[0].display().to_string();
        assert!(
            p.contains("hash/../shared-arm/out/libx.dylib"),
            "joined through the bench dir: {p}"
        );
    }
}
