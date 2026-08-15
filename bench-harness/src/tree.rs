//! Benchspace loading: declared members, never inferred ones.
//!
//! The root `bench.toml` may declare a benchspace, the way a cargo
//! workspace declares members:
//!
//! ```toml
//! [benchspace]
//! members = ["**"]        # the default when [benchspace] is absent
//! exclude = []
//! ```
//!
//! The rules, each preserving something real:
//!
//! - **No `[benchspace]` means the default glob applies.** Any
//!   subdirectory at any depth carrying its own `bench.toml` is a
//!   member. Membership is never inferred from a directory *looking*
//!   bench-shaped; the only signal is the file plus the pattern.
//! - **A member may be either shape.** Its own `[bench.<name>]`
//!   sections (the same syntax as a root file), or the composed form:
//!   top-level fields with `[sweep.<name>]` tables, no wrapper table,
//!   because the directory already carries the bench's name. The
//!   loader dispatches on what the member's file declares (a `bench`
//!   key means sections), not on where it sits.
//! - **The root may carry `[bench.*]` sections and members at once.**
//!   That is a real consumer's tree today.
//! - **An unlisted subdirectory is ignored without error**, so the
//!   convention is adoptable one bench at a time.
//! - **An explicitly listed member that is missing or has no
//!   `bench.toml` is an error** naming the member and the path:
//!   declared-and-absent is a typo, undeclared-and-present a choice.
//! - A member declaring its own `[benchspace]` is refused; nothing
//!   real nests, and lifting that later is additive.
//! - Member composition honours the member's own `[timing]`: a
//!   section's override wins, then the member's, then the root's.
//!
//! Member cells compose into the one manifest the driver already
//! consumes, keyed `<member>` (a composed member's single default
//! sweep) or `<member>/<sweep-or-section>`. A collision between a
//! root section and a member key is refused naming both locations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::{
    BenchManifest, BenchSection, BenchspaceSection, SizeSection, TimingOverride, de_seed_opt,
    de_sizes,
};
use crate::error::BenchError;

/// One discovered arm: a directory under `<member>/arms/` of a
/// composed-form member.
#[derive(Clone, Debug)]
pub struct ArmSource {
    /// The bench the arm belongs to (member path, `/`-separated).
    pub bench:        String,
    /// The arm name (directory name). Also the crate and lib name,
    /// sanitized.
    pub arm:          String,
    /// The arm's source directory.
    pub dir:          PathBuf,
    /// Whether the consumer wrote a `Cargo.toml` (the escape hatch).
    /// Without one the tool generates the manifest.
    pub has_manifest: bool,
}

/// One discovered support crate: a directory under `support/` at the
/// root or inside a composed-form member.
#[derive(Clone, Debug)]
pub struct SupportSource {
    /// Owning bench, or `None` for a root-level support crate.
    pub bench: Option<String>,
    /// The support crate's directory name.
    pub name:  String,
    /// The support crate's directory.
    pub dir:   PathBuf,
}

/// A loaded bench tree: the composed manifest plus the source
/// inventory the tool builds from.
#[derive(Debug, Default)]
pub struct TreeManifest {
    /// The composed manifest. `nested` maps member cell keys to
    /// `(bench, sweep)` so [`BenchManifest::for_size`] carries them
    /// apart; root sections are not in it and keep flat semantics.
    pub manifest:     BenchManifest,
    /// Every discovered arm of every composed-form member.
    pub arms:         Vec<ArmSource>,
    /// Every discovered support crate.
    pub support:      Vec<SupportSource>,
    /// Sections-form members (relative paths). Their variants build
    /// the legacy way, rooted at the member directory.
    pub flat_members: Vec<String>,
}

/// The composed member form: top-level fields, no wrapper table. The
/// directory carries the bench's name, so the file does not repeat
/// it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComposedBench {
    title:       String,
    #[serde(default = "default_workload")]
    workload:    String,
    #[serde(default, deserialize_with = "de_seed_opt")]
    master_seed: Option<u64>,
    #[serde(default, alias = "variants")]
    arms:        Vec<String>,
    #[serde(default, deserialize_with = "de_sizes", alias = "sizes")]
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
    #[serde(default)]
    sweep:       HashMap<String, SweepSection>,
}

fn default_workload() -> String {
    "default".to_string()
}

/// One `[sweep.<name>]` section: everything optional except the
/// points, inheriting the top-level value where absent.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SweepSection {
    #[serde(default, deserialize_with = "de_sizes", alias = "sizes")]
    points:      Vec<SizeSection>,
    #[serde(default, alias = "variants")]
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

/// Load a bench tree: the root manifest plus every declared or
/// defaulted member, composed into one manifest.
pub fn load(benches_dir: &Path) -> Result<TreeManifest, BenchError> {
    let root_path = benches_dir.join("bench.toml");
    let mut manifest = BenchManifest::load(&root_path)?;
    let space = manifest.benchspace.clone().unwrap_or_default();

    let mut tree = TreeManifest::default();
    tree.support.extend(support_in(benches_dir, None));

    for member in resolve_members(benches_dir, &space)? {
        compose_member(benches_dir, &member, &mut manifest, &mut tree)?;
    }

    manifest.nested_mode = !manifest.nested.is_empty();
    manifest.validate_roles()?;
    tree.manifest = manifest;
    Ok(tree)
}

/// The member directories the `[benchspace]` declaration selects,
/// as sorted relative paths.
///
/// A literal entry (no `*`) is explicit: it must exist and carry a
/// `bench.toml`. A pattern entry matches discovered directories that
/// carry one, matching nothing without error. A matched member's own
/// subtree is not searched further: the member owns its interior.
fn resolve_members(
    benches_dir: &Path,
    space: &BenchspaceSection,
) -> Result<Vec<String>, BenchError> {
    let excluded =
        |rel: &str| -> bool { space.exclude.iter().any(|p| glob_match(p, rel)) };

    let mut members: Vec<String> = Vec::new();
    let mut push = |rel: String| {
        if !members.contains(&rel) {
            members.push(rel);
        }
    };

    for entry in &space.members {
        if entry.contains('*') {
            for rel in discover(benches_dir, entry) {
                if !excluded(&rel) {
                    push(rel);
                }
            }
        } else {
            let rel = entry.trim_matches('/').to_string();
            if excluded(&rel) {
                continue;
            }
            let dir = benches_dir.join(&rel);
            if !dir.join("bench.toml").is_file() {
                return Err(BenchError::InvalidConfig {
                    reason: format!(
                        "[benchspace] lists member `{rel}`, but {} does not exist. An \
                         explicitly listed member must be a directory with its own \
                         bench.toml; patterns (`*`) may match nothing.",
                        dir.join("bench.toml").display()
                    ),
                });
            }
            push(rel);
        }
    }
    members.sort();
    Ok(members)
}

/// Walk `benches_dir` for directories carrying a `bench.toml` whose
/// relative path matches `pattern`. A matched directory's subtree is
/// not descended into.
fn discover(benches_dir: &Path, pattern: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = vec![benches_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            // never walk into build output or hidden trees
            if name.starts_with('.') || name == "target" {
                continue;
            }
            let rel = path
                .strip_prefix(benches_dir)
                .ok()
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if path.join("bench.toml").is_file() && glob_match(pattern, &rel) {
                found.push(rel);
                continue; // the member owns its interior
            }
            stack.push(path);
        }
    }
    found.sort();
    found
}

/// Match a `/`-separated glob against a relative path. `*` matches
/// exactly one path component, `**` any number including zero;
/// anything else is literal per component.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    fn rec(pat: &[&str], path: &[&str]) -> bool {
        match pat.split_first() {
            None => path.is_empty(),
            Some((&"**", rest)) => {
                (0 ..= path.len()).any(|skip| rec(rest, &path[skip ..]))
            },
            Some((&"*", rest)) => !path.is_empty() && rec(rest, &path[1 ..]),
            Some((comp, rest)) => {
                path.first() == Some(comp) && rec(rest, &path[1 ..])
            },
        }
    }
    let pat: Vec<&str> = pattern.split('/').filter(|c| !c.is_empty()).collect();
    let path: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    rec(&pat, &path)
}

/// Compose one member into the manifest, dispatching on the shape
/// its own file declares.
fn compose_member(
    benches_dir: &Path,
    member: &str,
    manifest: &mut BenchManifest,
    tree: &mut TreeManifest,
) -> Result<(), BenchError> {
    let dir = benches_dir.join(member);
    let file = dir.join("bench.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| BenchError::io("reading bench.toml", e))?;
    let raw: toml::Value = text.parse().map_err(|e| {
        BenchError::InvalidConfig {
            reason: format!("{}: {e}", file.display()),
        }
    })?;
    if raw.get("benchspace").is_some() {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "member `{member}` declares its own [benchspace]. Benchspaces do not \
                 nest; move the inner members up to {}.",
                benches_dir.join("bench.toml").display()
            ),
        });
    }

    if raw.get("bench").is_some() {
        compose_sections_member(member, &file, &text, &raw, manifest, tree)
    } else {
        compose_composed_member(member, &dir, &file, &text, manifest, tree)
    }
}

/// A sections-form member: the same `[bench.<name>]` syntax as a
/// root file, composed under `<member>/<section>` keys. Its own
/// `[timing]`, when declared, overrides the root's for its sections.
fn compose_sections_member(
    member: &str,
    file: &Path,
    text: &str,
    raw: &toml::Value,
    manifest: &mut BenchManifest,
    tree: &mut TreeManifest,
) -> Result<(), BenchError> {
    let parsed: BenchManifest = toml::from_str(text).map_err(|e| {
        BenchError::InvalidConfig {
            reason: format!("{}: {e}", file.display()),
        }
    })?;
    parsed.validate_roles()?;
    // "Declared" cannot be read off TimingSection (its fields carry
    // defaults), so presence in the raw document is the signal.
    let member_timing = raw.get("timing").map(|_| {
        TimingOverride {
            passes:        Some(parsed.timing.passes),
            runs_per_pass: Some(parsed.timing.runs_per_pass),
            batch_size:    Some(parsed.timing.batch_size),
            harness_runs:  Some(parsed.timing.harness_runs),
            cooldowns_ms:  Some(parsed.timing.cooldowns_ms.clone()),
        }
    });

    let mut names: Vec<&String> = parsed.bench.keys().collect();
    names.sort();
    for name in names {
        let section = &parsed.bench[name];
        let key = format!("{member}/{name}");
        let mut composed = section.clone();
        composed.variants = section
            .variants
            .iter()
            .map(|e| member_relative_entry(member, e))
            .collect();
        composed.sizes = section
            .sizes
            .iter()
            .map(|sz| {
                SizeSection {
                    n:        sz.n,
                    variants: sz
                        .variants
                        .iter()
                        .map(|e| member_relative_entry(member, e))
                        .collect(),
                }
            })
            .collect();
        composed.timing = merge_timing(section.timing.as_ref(), member_timing.as_ref());
        insert_composed(manifest, key, composed, member, name)?;
    }
    tree.flat_members.push(member.to_string());
    Ok(())
}

/// The composed member form: top-level fields plus `[sweep.*]`.
fn compose_composed_member(
    member: &str,
    dir: &Path,
    file: &Path,
    text: &str,
    manifest: &mut BenchManifest,
    tree: &mut TreeManifest,
) -> Result<(), BenchError> {
    let parsed: ComposedBench = toml::from_str(text).map_err(|e| {
        BenchError::InvalidConfig {
            reason: format!("{}: {e}", file.display()),
        }
    })?;
    let arms = discover_arms(member, dir);
    tree.support.extend(support_in(dir, Some(member)));

    if !parsed.points.is_empty() && !parsed.sweep.is_empty() {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{member}` declares top-level points and [sweep.*] sections. \
                 With sweeps present, every point list lives in its sweep; move the \
                 top-level points into one."
            ),
        });
    }
    if parsed.points.is_empty() && parsed.sweep.is_empty() {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{member}` declares no points and no sweeps, so there is \
                 nothing to run. Add `points = [...]` or a [sweep.<name>] section."
            ),
        });
    }

    let mut sweeps: Vec<(String, SweepSection)> = if parsed.sweep.is_empty() {
        vec![(member.to_string(), SweepSection {
            points:      parsed.points.clone(),
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
                    "bench `{member}` sweep `{sweep_name}` has no points; add \
                     `points = [...]`"
                ),
            });
        }
        let entries = if sweep.arms.is_empty() { &parsed.arms } else { &sweep.arms };
        if entries.is_empty() {
            return Err(BenchError::InvalidConfig {
                reason: format!(
                    "bench `{member}` sweep `{sweep_name}` lists no arms (neither the \
                     sweep nor the top-level `arms` name any)"
                ),
            });
        }
        let resolved: Vec<String> = entries
            .iter()
            .map(|e| resolve_arm_entry(member, e, &arms))
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
                        .map(|e| resolve_arm_entry(member, e, &arms))
                        .collect::<Result<_, BenchError>>()?,
                })
            })
            .collect::<Result<Vec<_>, BenchError>>()?;

        let key = if sweep_name == member {
            member.to_string()
        } else {
            format!("{member}/{sweep_name}")
        };
        let section = BenchSection {
            title:       sweep.title.clone().unwrap_or_else(|| parsed.title.clone()),
            workload:    sweep
                .workload
                .clone()
                .unwrap_or_else(|| parsed.workload.clone()),
            master_seed: sweep
                .master_seed
                .or(parsed.master_seed)
                .unwrap_or_default(),
            variants:    resolved,
            sizes:       points,
            may_differ:  sweep.may_differ.unwrap_or(parsed.may_differ),
            required:    sweep.required.unwrap_or(parsed.required),
            threaded:    sweep.threaded.unwrap_or(parsed.threaded),
            baseline:    sweep.baseline.clone().or_else(|| parsed.baseline.clone()),
            floor:       sweep.floor.clone().or_else(|| parsed.floor.clone()),
            delta:       sweep.delta.clone().or_else(|| parsed.delta.clone()),
            timing:      merge_timing(sweep.timing.as_ref(), parsed.timing.as_ref()),
            normalise:   None,
        };
        insert_composed(manifest, key, section, member, &sweep_name)?;
    }

    tree.arms.extend(arms);
    Ok(())
}

/// Insert one composed cell key, refusing a collision with anything
/// already present (a root section, in practice) by naming both
/// locations.
fn insert_composed(
    manifest: &mut BenchManifest,
    key: String,
    section: BenchSection,
    member: &str,
    sweep: &str,
) -> Result<(), BenchError> {
    if manifest.bench.contains_key(&key) {
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench key `{key}` is declared twice: as a [bench.{key}] section in \
                 the root bench.toml and by member directory `{member}/`. Rename one; \
                 which of two same-named benches runs must never depend on load order."
            ),
        });
    }
    manifest
        .nested
        .insert(key.clone(), (member.to_string(), sweep.to_string()));
    manifest.bench.insert(key, section);
    Ok(())
}

/// Field-wise merge: `first` wins where declared, else `second`.
fn merge_timing(
    first: Option<&TimingOverride>,
    second: Option<&TimingOverride>,
) -> Option<TimingOverride> {
    match (first, second) {
        (None, None) => None,
        (Some(t), None) | (None, Some(t)) => Some(t.clone()),
        (Some(a), Some(b)) => {
            Some(TimingOverride {
                passes:        a.passes.or(b.passes),
                runs_per_pass: a.runs_per_pass.or(b.runs_per_pass),
                batch_size:    a.batch_size.or(b.batch_size),
                harness_runs:  a.harness_runs.or(b.harness_runs),
                cooldowns_ms:  a.cooldowns_ms.clone().or_else(|| b.cooldowns_ms.clone()),
            })
        },
    }
}

/// A sections-form member's entries resolve against the member
/// directory: a short name becomes the member's own `variants/`
/// path, a path entry is prefixed with the member directory.
fn member_relative_entry(member: &str, entry: &str) -> String {
    let is_short = !entry.contains('/') && !entry.contains(std::path::MAIN_SEPARATOR);
    if is_short {
        format!(
            "{member}/variants/{entry}/target/release/{}{entry}{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        )
    } else {
        format!("{member}/{entry}")
    }
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

/// Resolve one composed-member sweep entry. A short name must be a
/// discovered arm of this bench and resolves into the tool-owned
/// target tree; a path entry is made relative to the benches root by
/// prefixing the member directory.
fn resolve_arm_entry(
    member: &str,
    entry: &str,
    arms: &[ArmSource],
) -> Result<String, BenchError> {
    let is_short = !entry.contains('/') && !entry.contains(std::path::MAIN_SEPARATOR);
    if !is_short {
        return Ok(format!("{member}/{entry}"));
    }
    if !arms.iter().any(|a| a.arm == entry) {
        let available: Vec<&str> = arms.iter().map(|a| a.arm.as_str()).collect();
        return Err(BenchError::InvalidConfig {
            reason: format!(
                "bench `{member}` names arm `{entry}`, but {member}/arms/{entry}/ does \
                 not exist. Discovered arms: [{}]. An arm outside arms/ is referenced \
                 by path.",
                available.join(", ")
            ),
        });
    }
    Ok(arm_target_dir(member, entry)
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
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "mockspace-benchspace-test-{}-{}-{name}",
                std::process::id(),
                SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            ));
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

    /// The composed form: top-level fields, no wrapper table.
    const HASH_BENCH: &str = r#"
        title = "Hash mixers"
        workload = "realistic"
        arms = ["fnv", "xx"]
        points = [64, 256]
    "#;

    const EMPTY_ROOT: &str = "[timing]\npasses = 4\n";

    #[test]
    fn the_default_glob_makes_any_subdir_with_a_bench_toml_a_member() {
        let t = Tree::new("default-glob");
        t.write("bench.toml", EMPTY_ROOT);
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        let tree = load(&t.root).unwrap();
        assert_eq!(tree.manifest.bench_names(), vec!["hash"]);
        let c = tree.manifest.for_size("hash", 1, &t.root).unwrap();
        assert_eq!((c.bench.as_str(), c.sweep.as_str(), c.n), ("hash", "hash", 256));
        assert!(c.nested);
        let p = c.variant_paths[0].display().to_string();
        assert!(p.contains("target/mock-arms/hash/fnv/release/"), "{p}");
    }

    #[test]
    fn the_regression_shape_an_undeclared_subdir_is_ignored_without_error() {
        // The exact shape the heuristic broke on: a self-contained
        // bench directory with its own bench.toml, NOT listed by an
        // explicit member list. It must be ignored, and the root's
        // own sections must load untouched.
        let t = Tree::new("regression");
        t.write(
            "bench.toml",
            r#"
            [benchspace]
            members = ["declared"]

            [bench.rootbench]
            title = "Root"
            workload = "default"
            variants = ["variants/v/target/release/libv.dylib"]
            sizes = [64]
        "#,
        );
        t.write("declared/bench.toml", HASH_BENCH);
        t.mkdir("declared/arms/fnv/src").mkdir("declared/arms/xx/src");
        // the self-contained stranger, complete with its own driver crate
        t.write(
            "resource_storage/bench.toml",
            "[timing]\npasses = 5\n[bench.rsb_clean]\ntitle = 'R'\nworkload = 'w'\nvariants = ['../target/release/v0']\nsizes = [16]\n",
        );
        t.write("resource_storage/Cargo.toml", "[package]\nname = \"rs\"\n");
        let tree = load(&t.root).expect("the stranger must not break the load");
        assert_eq!(tree.manifest.bench_names(), vec!["declared", "rootbench"]);
        assert!(
            !tree.manifest.bench.keys().any(|k| k.contains("rsb")),
            "the undeclared directory contributes nothing"
        );
    }

    #[test]
    fn a_sections_member_composes_with_prefixed_keys_and_its_own_timing() {
        // resource_storage's real shape: [timing] + [bench.*], own
        // deliberately trimmed budget, entries relative to itself.
        let t = Tree::new("sections-member");
        t.write("bench.toml", "[timing]\npasses = 10\nruns_per_pass = 50000\n");
        t.write(
            "storage/bench.toml",
            r#"
            [timing]
            passes = 5
            runs_per_pass = 2000

            [bench.rsb_clean]
            title = "Clean"
            workload = "rsb_clean"
            variants = ["rsb_v0", "../target/release/rsb_v1"]
            sizes = [16]

            [bench.rsb_heavy]
            title = "Heavy"
            workload = "rsb_heavy"
            variants = ["rsb_v0"]
            sizes = [16]

            [bench.rsb_heavy.timing]
            passes = 2
        "#,
        );
        let tree = load(&t.root).unwrap();
        assert_eq!(
            tree.manifest.bench_names(),
            vec!["storage/rsb_clean", "storage/rsb_heavy"]
        );
        assert_eq!(tree.flat_members, vec!["storage"]);

        let clean = tree.manifest.for_size("storage/rsb_clean", 0, &t.root).unwrap();
        assert_eq!((clean.bench.as_str(), clean.sweep.as_str()), ("storage", "rsb_clean"));
        assert!(clean.nested);
        // member timing wins over root where the section has none
        assert_eq!(clean.passes, 5);
        assert_eq!(clean.runs_per_pass, 2000);
        // a short name resolves against the member's own variants/
        let short = clean.variant_paths[0].display().to_string();
        assert!(short.contains("storage/variants/rsb_v0/target/release/"), "{short}");
        // a path entry is joined through the member directory; the
        // bare stem gets the platform affixes exactly as it does in
        // the member's own flat tree today
        let path = clean.variant_paths[1].display().to_string();
        assert!(path.contains("storage/../target/release/"), "{path}");
        assert!(
            path.ends_with(&format!(
                "{}rsb_v1{}",
                std::env::consts::DLL_PREFIX,
                std::env::consts::DLL_SUFFIX
            )),
            "{path}"
        );

        // section override wins over member timing
        let heavy = tree.manifest.for_size("storage/rsb_heavy", 0, &t.root).unwrap();
        assert_eq!(heavy.passes, 2);
        assert_eq!(heavy.runs_per_pass, 2000, "unset fields still inherit the member");
    }

    #[test]
    fn root_sections_and_members_coexist_and_root_cells_stay_flat() {
        // hilavitkutin's shape: 29 root sections plus one member.
        let t = Tree::new("mixed");
        t.write(
            "bench.toml",
            r#"
            [bench.ema_axis_h]
            title = "EMA"
            workload = "realistic"
            variants = ["variants/neon/target/release/libneon.dylib"]
            sizes = [64]
        "#,
        );
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        let tree = load(&t.root).unwrap();
        assert_eq!(tree.manifest.bench_names(), vec!["ema_axis_h", "hash"]);
        let root_cell = tree.manifest.for_size("ema_axis_h", 0, &t.root).unwrap();
        assert!(!root_cell.nested, "root sections keep flat output naming");
        let member_cell = tree.manifest.for_size("hash", 0, &t.root).unwrap();
        assert!(member_cell.nested);
    }

    #[test]
    fn an_explicitly_listed_member_that_is_missing_is_an_error_naming_it() {
        let t = Tree::new("missing-member");
        t.write("bench.toml", "[benchspace]\nmembers = [\"ghost\"]\n");
        let err = load(&t.root).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("`ghost`"), "{msg}");
        assert!(msg.contains("ghost/bench.toml"), "names the path: {msg}");
    }

    #[test]
    fn exclude_removes_a_directory_the_glob_would_take() {
        let t = Tree::new("exclude");
        t.write("bench.toml", "[benchspace]\nmembers = [\"**\"]\nexclude = [\"scratch\"]\n");
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        t.write("scratch/bench.toml", HASH_BENCH);
        let tree = load(&t.root).unwrap();
        assert_eq!(tree.manifest.bench_names(), vec!["hash"]);
    }

    #[test]
    fn a_member_declaring_its_own_benchspace_is_refused() {
        let t = Tree::new("nesting");
        t.write("bench.toml", EMPTY_ROOT);
        t.write(
            "outer/bench.toml",
            &format!("[benchspace]\nmembers = [\"inner\"]\n{HASH_BENCH}"),
        );
        let err = load(&t.root).unwrap_err();
        assert!(format!("{err}").contains("do not nest"), "{err}");
    }

    #[test]
    fn a_root_section_colliding_with_a_member_name_is_refused_naming_both() {
        let t = Tree::new("collision");
        t.write(
            "bench.toml",
            r#"
            [bench.hash]
            title = "Root hash"
            workload = "default"
            variants = ["variants/v/target/release/libv.dylib"]
            sizes = [64]
        "#,
        );
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        let err = load(&t.root).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("declared twice"), "{msg}");
        assert!(msg.contains("[bench.hash]"), "{msg}");
        assert!(msg.contains("hash/"), "{msg}");
    }

    #[test]
    fn named_sweeps_compose_and_inherit_the_top_level_fields() {
        let t = Tree::new("sweeps");
        t.write("bench.toml", EMPTY_ROOT);
        t.write(
            "warm/bench.toml",
            r#"
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
        let tree = load(&t.root).unwrap();
        assert_eq!(
            tree.manifest.bench_names(),
            vec!["warm/density-w13", "warm/width-l1"]
        );
        let c = tree.manifest.for_size("warm/width-l1", 0, &t.root).unwrap();
        assert_eq!((c.bench.as_str(), c.sweep.as_str()), ("warm", "width-l1"));
        assert_eq!(c.master_seed, 7);
        assert_eq!(c.normalise_baseline.as_deref(), Some("native"));
        let d = tree.manifest.for_size("warm/density-w13", 0, &t.root).unwrap();
        assert_eq!(d.variant_paths.len(), 1);
        assert!(d.required && !c.required);
    }

    #[test]
    fn naming_an_arm_that_does_not_exist_is_refused_listing_what_does() {
        let t = Tree::new("missing-arm");
        t.write("bench.toml", EMPTY_ROOT);
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src");
        let err = load(&t.root).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("`xx`"), "{msg}");
        assert!(msg.contains("fnv"), "{msg}");
    }

    #[test]
    fn a_wrapper_bench_table_in_a_member_is_not_the_composed_form() {
        // the rejected wrapper spelling must not silently parse as a
        // bench of that name; it reads as the sections form and its
        // scalar values fail loudly as malformed sections
        let t = Tree::new("wrapper");
        t.write("bench.toml", EMPTY_ROOT);
        t.write(
            "hash/bench.toml",
            "[bench]\ntitle = \"Hash\"\narms = [\"fnv\"]\npoints = [64]\n",
        );
        let err = load(&t.root).unwrap_err();
        assert!(format!("{err}").contains("hash/bench.toml"), "{err}");
    }

    #[test]
    fn arms_and_support_are_discovered_and_glob_match_holds_its_grammar() {
        let t = Tree::new("inventory");
        t.write("bench.toml", EMPTY_ROOT);
        t.write("hash/bench.toml", HASH_BENCH);
        t.mkdir("hash/arms/fnv/src").mkdir("hash/arms/xx/src");
        t.write("hash/arms/fnv/Cargo.toml", "[package]\nname='fnv'\n");
        t.mkdir("hash/support/hash-kit/src");
        t.mkdir("support/carrier/src");
        let tree = load(&t.root).unwrap();
        let arm_names: Vec<&str> = tree.arms.iter().map(|a| a.arm.as_str()).collect();
        assert_eq!(arm_names, vec!["fnv", "xx"]);
        assert!(tree.arms[0].has_manifest && !tree.arms[1].has_manifest);
        let support: Vec<(Option<&str>, &str)> = tree
            .support
            .iter()
            .map(|s| (s.bench.as_deref(), s.name.as_str()))
            .collect();
        assert_eq!(support, vec![(None, "carrier"), (Some("hash"), "hash-kit")]);

        // the glob grammar, at its edges
        assert!(glob_match("**", "a"));
        assert!(glob_match("**", "a/b/c"));
        assert!(glob_match("*", "a"));
        assert!(!glob_match("*", "a/b"));
        assert!(glob_match("a/*", "a/b"));
        assert!(!glob_match("a/*", "a/b/c"));
        assert!(glob_match("a/**", "a/b/c"));
        assert!(!glob_match("a/**", "b/c"));
        assert!(glob_match("a", "a"));
        assert!(!glob_match("a", "b"));
    }
}
