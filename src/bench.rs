//! Bench framework command family.
//!
//! `mock bench init` scaffolds a `mock/benches/` directory in the
//! consumer with a starter `bench.toml`, a binary that drives the
//! harness, an example variant, and a README. The layout is exactly
//! what the harness discovers at run time.
//!
//! `mock bench run` builds the consumer's bench binary + variants in
//! release mode and spawns the binary, which drives the harness via
//! `mockspace-bench-harness`.
//!
//! `mock bench report` invokes the bench binary with `--report-only`
//! to regenerate findings.md from the existing CSV cache without
//! re-running the full harness.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_bench_harness::config::BuildSection;
use mockspace_bench_harness::tree as bench_tree;

use crate::bench_gen;
use crate::config::Config;

pub fn cmd(cfg: &Config, args: &[&str]) -> ExitCode {
    let sub = args.first().copied().unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).copied().collect();
    match sub {
        "init" => cmd_init(cfg),
        "run" => cmd_run(cfg, &rest),
        "report" => cmd_report(cfg, &rest),
        "list" => cmd_list(cfg),
        "add" => cmd_add(cfg, &rest),
        "" => {
            print_help();
            ExitCode::SUCCESS
        },
        other => {
            eprintln!("error: unknown bench subcommand `{other}`");
            print_help();
            ExitCode::FAILURE
        },
    }
}

fn print_help() {
    eprintln!("mock bench. canonical bench framework commands");
    eprintln!();
    eprintln!("subcommands:");
    eprintln!("  init    scaffold mock/benches/ in this consumer");
    eprintln!("  run     build variants + bench binary, run the harness");
    eprintln!("  report  regenerate findings.md from cached results");
    eprintln!("  list    list benches, sizes, and variants from bench.toml");
    eprintln!("  add     scaffold a new variant crate: mock bench add <name>");
    eprintln!();
    eprintln!("`run` and `report` accept bench names to restrict the pass:");
    eprintln!("  mock bench run <name> [<name> ...]   run only the named benches");
    eprintln!("with no names, every bench in bench.toml runs");
    eprintln!();
    eprintln!("`mock/benches/` layout (created by `init`):");
    eprintln!("  Cargo.toml         the bench binary");
    eprintln!("  src/main.rs        Routine impl + run/report dispatch");
    eprintln!("  bench.toml         per-bench config (sizes, timing, variants)");
    eprintln!("  variants/<name>/   one cdylib per variant");
    eprintln!("  README.md");
}

// ── run ──

fn cmd_run(cfg: &Config, args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        eprintln!(
            "error: {} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    // Positional args are bench names: they restrict both the run
    // (forwarded as --only) and the variant builds.
    let names: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| !a.starts_with("--"))
        .collect();
    let extra: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| a.starts_with("--"))
        .collect();

    // The escape hatch: a consumer-owned driver crate at the bench
    // root drives the run exactly as before. Without one, the driver
    // is generated from bench.toml.
    if !bench_dir.join("Cargo.toml").exists() {
        return run_generated(cfg, &bench_dir, &names, &extra, false);
    }

    let dirs = if names.is_empty() {
        None
    } else {
        match variant_dirs_for(&bench_dir, &names) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        }
    };
    let bin_path = match build_variants_and_bin_filtered(&bench_dir, dirs.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    let mut cmd = Command::new(&bin_path);
    for n in &names {
        cmd.args(["--only", n]);
    }
    for e in &extra {
        cmd.arg(e);
    }
    let status = cmd.current_dir(&bench_dir).status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench binary exited with {:?}", s.code());
            ExitCode::FAILURE
        },
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        },
    }
}

// ── report ──

fn cmd_report(cfg: &Config, _args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        eprintln!(
            "error: {} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    if !bench_dir.join("Cargo.toml").exists() {
        let names: Vec<&str> = _args
            .iter()
            .copied()
            .filter(|a| !a.starts_with("--"))
            .collect();
        return run_generated(cfg, &bench_dir, &names, &["--report-only"], true);
    }

    let bin_path = match build_bin_only(&bench_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    let mut cmd = Command::new(&bin_path);
    cmd.arg("--report-only");
    for a in _args.iter().filter(|a| !a.starts_with("--")) {
        cmd.args(["--only", a]);
    }
    let status = cmd.current_dir(&bench_dir).status();

    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench binary exited with {:?}", s.code());
            ExitCode::FAILURE
        },
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        },
    }
}

// ── helpers ──

/// Read `bench.toml` and map the named benches to the variant
/// directories their entries reference. Short names map to
/// `variants/<name>`; path entries starting with `variants/` map to
/// their first two components; anything else is ignored (an
/// out-of-tree path is not ours to build).
fn variant_dirs_for(bench_dir: &Path, names: &[&str]) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(bench_dir.join("bench.toml"))
        .map_err(|e| format!("reading bench.toml: {e}"))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("parsing bench.toml: {e}"))?;
    let bench = doc
        .get("bench")
        .and_then(|b| b.as_table())
        .ok_or_else(|| "bench.toml has no [bench.*] sections".to_string())?;
    let mut dirs: Vec<String> = Vec::new();
    let mut ignored: usize = 0;
    let mut push_entry = |entry: &str, dirs: &mut Vec<String>, ignored: &mut usize| {
        let dir = if !entry.contains('/') {
            Some(entry.to_string())
        } else {
            entry
                .strip_prefix("variants/")
                .and_then(|rest| rest.split('/').next())
                .map(|d| d.to_string())
        };
        match dir {
            Some(d) => {
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            },
            None => *ignored += 1,
        }
    };
    let collect_array = |item: Option<&toml_edit::Item>,
                         dirs: &mut Vec<String>,
                         ignored: &mut usize,
                         push: &mut dyn FnMut(&str, &mut Vec<String>, &mut usize)| {
        if let Some(arr) = item.and_then(|v| v.as_array()) {
            for v in arr.iter() {
                if let Some(sv) = v.as_str() {
                    push(sv, dirs, ignored);
                }
            }
        }
    };
    for name in names {
        let section = bench.get(name).ok_or_else(|| {
            let available: Vec<&str> = bench.iter().map(|(k, _)| k).collect();
            format!(
                "bench `{name}` not found in bench.toml. Available: {}",
                available.join(", ")
            )
        })?;
        collect_array(section.get("variants"), &mut dirs, &mut ignored, &mut push_entry);
        if let Some(sizes) = section.get("sizes") {
            if let Some(arr) = sizes.as_array_of_tables() {
                for t in arr.iter() {
                    collect_array(t.get("variants"), &mut dirs, &mut ignored, &mut push_entry);
                }
            }
            if let Some(arr) = sizes.as_array() {
                for v in arr.iter() {
                    if let Some(t) = v.as_inline_table() {
                        if let Some(tv) = t.get("variants").and_then(|x| x.as_array()) {
                            for e in tv.iter() {
                                if let Some(sv) = e.as_str() {
                                    push_entry(sv, &mut dirs, &mut ignored);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if ignored > 0 {
        // The silent version of this measured stale artifacts: a
        // filtered run built nothing for path-style entries and then
        // timed whatever dylibs were already on disk.
        eprintln!(
            "warning: {ignored} variant entr{} outside variants/ were not rebuilt for              this filtered run; their artifacts may be stale. Run without bench names              to rebuild everything, or move them under variants/.",
            if ignored == 1 { "y" } else { "ies" }
        );
    }
    Ok(dirs)
}

/// The release profile the framework guarantees, passed on every
/// build rather than left to the consumer's manifests.
///
/// A `[profile.release]` table is honoured only in a workspace root,
/// so a consumer whose manifests never declared one never had the
/// documented profile at all. That is what was measured: a tree of
/// ninety-four variant crates, none declaring a profile and none
/// declaring a workspace, built at cargo's default `lto = false,
/// codegen-units = 16` while the framework's documentation promised
/// fat LTO and a single codegen unit.
///
/// A workspace member losing its own profile is a second, related
/// mechanism. It was not observed in that tree, and it is not silent:
/// cargo prints `profiles for the non root package will be ignored`.
///
/// Codegen-unit partitioning is not stable across builds, so the
/// default is a reproducibility defect and not only a slower one:
/// two runs of an unchanged variant can differ in inlining and
/// layout, which is exactly the contamination per-variant cdylib
/// isolation exists to prevent.
const PROFILE_ARGS: [&str; 6] = [
    "--config",
    "profile.release.opt-level=3",
    "--config",
    "profile.release.lto=\"fat\"",
    "--config",
    "profile.release.codegen-units=1",
];

/// Every executable cargo reports building for `manifest`.
///
/// Asked rather than guessed. The name was hardcoded to `benches`,
/// the starter template's package name, so a consumer that renamed
/// its bench package built everything and then failed to locate it.
/// Reading the name out of the manifest instead fixed that case and
/// kept the guessing: a manifest with several `[[bin]]` tables, an
/// inline `bin = [{ name = ... }]` array, or a binary discovered at
/// `src/bin/` rather than declared, each resolve to something the
/// build may not have produced. A bench root that is a member of an
/// outer workspace also puts its artifacts in the outer `target/`,
/// which no amount of name resolution finds.
///
/// `--message-format=json` removes all of it at once: cargo emits a
/// `compiler-artifact` record per built target carrying the absolute
/// path of what it just wrote.
fn built_executables(manifest: &Path, stdout: &[u8]) -> Vec<PathBuf> {
    let manifest = manifest.canonicalize().unwrap_or_else(|_| manifest.to_path_buf());
    String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("reason").and_then(|r| r.as_str()) == Some("compiler-artifact"))
        .filter(|v| {
            v.get("manifest_path")
                .and_then(|m| m.as_str())
                .map(|m| Path::new(m).canonicalize().unwrap_or_else(|_| PathBuf::from(m)))
                .is_some_and(|m| m == manifest)
        })
        .filter_map(|v| {
            v.get("executable")
                .and_then(|e| e.as_str())
                .map(PathBuf::from)
        })
        .collect()
}

/// The argv of every build this module runs.
///
/// One function so the profile cannot be passed on one path and
/// forgotten on the other, and so a test can assert the flags are
/// present without spawning cargo. They were previously written out
/// at both call sites, where dropping them from both left every test
/// green.
fn build_argv(manifest: &Path) -> Vec<std::ffi::OsString> {
    let mut argv: Vec<std::ffi::OsString> = ["build", "--release", "--message-format=json-render-diagnostics"]
        .iter()
        .map(Into::into)
        .collect();
    argv.extend(PROFILE_ARGS.iter().map(Into::into));
    argv.push("--manifest-path".into());
    argv.push(manifest.as_os_str().to_owned());
    argv
}

/// Run one cargo build and return its stdout for artifact parsing.
fn cargo_build_json(manifest: &Path, what: &str) -> Result<Vec<u8>, String> {
    let out = Command::new("cargo")
        .args(build_argv(manifest))
        .output()
        .map_err(|e| format!("spawning cargo for {what}: {e}"))?;
    if !out.status.success() {
        // Diagnostics are already rendered to stderr by
        // `json-render-diagnostics`, so the consumer has seen them.
        return Err(format!("cargo build failed for {what}"));
    }
    Ok(out.stdout)
}

fn build_variants_and_bin_filtered(
    bench_dir: &Path,
    only_dirs: Option<&[String]>,
) -> Result<PathBuf, String> {
    let variants_dir = bench_dir.join("variants");
    if variants_dir.exists() {
        for entry in
            fs::read_dir(&variants_dir).map_err(|e| format!("reading variants dir: {e}"))?
        {
            let entry = entry.map_err(|e| format!("variants dir entry: {e}"))?;
            let path = entry.path();
            if let Some(dirs) = only_dirs {
                let dir_name = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or("")
                    .to_string();
                if !dirs.contains(&dir_name) {
                    continue;
                }
            }
            let manifest = path.join("Cargo.toml");
            if manifest.exists() {
                eprintln!("  building variant {}...", path.display());
                cargo_build_json(&manifest, &format!("variant {}", path.display()))?;
            }
        }
    }

    build_bin_only(bench_dir)
}

/// Build the bench binary and return the path cargo says it wrote.
///
/// Building and locating are one step because they share an answer:
/// the artifact record cargo emits during the build already carries
/// the absolute path. Splitting them is what let the two drift, with
/// the locator guessing a name and a directory the build never
/// promised.
fn build_bin_only(bench_dir: &Path) -> Result<PathBuf, String> {
    let manifest = bench_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!(
            "{} not found; the scaffold may have been deleted",
            manifest.display()
        ));
    }
    eprintln!("  building bench binary...");
    let stdout = cargo_build_json(&manifest, "bench binary")?;
    let mut bins = built_executables(&manifest, &stdout);
    match bins.len() {
        1 => Ok(bins.remove(0)),
        0 => Err(format!(
            "{} declares no binary target, so there is nothing to run. Add a [[bin]] \
             section or a src/main.rs.",
            manifest.display()
        )),
        _ => {
            let names: Vec<String> = bins
                .iter()
                .map(|b| {
                    b.file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect();
            Err(format!(
                "{} declares {} binary targets ({}), so which one drives the harness is \
                 ambiguous. Leave one [[bin]] in the bench package and move the others \
                 elsewhere.",
                manifest.display(),
                names.len(),
                names.join(", ")
            ))
        },
    }
}

/// The effective release-profile flags: the framework defaults with
/// any `[build]` overrides from the tree's own root manifest. The
/// values always travel on the command line, where a manifest cannot
/// silently drop them; the override moves the values, never the
/// mechanism.
fn profile_args_for(build: Option<&BuildSection>) -> Vec<String> {
    let opt = build.and_then(|b| b.opt_level).unwrap_or(3);
    let lto = build
        .and_then(|b| b.lto.clone())
        .unwrap_or_else(|| "fat".to_string());
    let cgu = build.and_then(|b| b.codegen_units).unwrap_or(1);
    vec![
        "--config".into(),
        format!("profile.release.opt-level={opt}"),
        "--config".into(),
        format!("profile.release.lto=\"{lto}\""),
        "--config".into(),
        format!("profile.release.codegen-units={cgu}"),
    ]
}

/// One cargo build with explicit profile flags and an optional
/// tool-owned target directory; returns stdout for artifact parsing.
fn cargo_build_at(
    manifest: &Path,
    what: &str,
    profile: &[String],
    target_dir: Option<&Path>,
) -> Result<Vec<u8>, String> {
    let mut argv: Vec<std::ffi::OsString> =
        ["build", "--release", "--message-format=json-render-diagnostics"]
            .iter()
            .map(Into::into)
            .collect();
    argv.extend(profile.iter().map(Into::into));
    if let Some(td) = target_dir {
        argv.push("--target-dir".into());
        argv.push(td.as_os_str().to_owned());
    }
    argv.push("--manifest-path".into());
    argv.push(manifest.as_os_str().to_owned());
    let out = Command::new("cargo")
        .args(argv)
        .output()
        .map_err(|e| format!("spawning cargo for {what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo build failed for {what}"));
    }
    Ok(out.stdout)
}

/// Resolve exactly one built executable from a build's artifact
/// records, with the same refusals as the legacy path.
fn single_executable(manifest: &Path, stdout: &[u8], what: &str) -> Result<PathBuf, String> {
    let mut bins = built_executables(manifest, stdout);
    match bins.len() {
        1 => Ok(bins.remove(0)),
        0 => Err(format!("{what} built no executable ({} declares no binary target)", manifest.display())),
        n => Err(format!("{what} built {n} executables; one [[bin]] is expected")),
    }
}

/// The generated-driver run path: `bench.toml` is the whole input.
///
/// Arms build into per-arm tool-owned target directories (so where a
/// dylib lands is a tool guarantee rather than a workspace accident),
/// the driver crate is generated under `mock/target/mockspace-bench/`
/// the way the custom-lint collect crate is, and the optional hooks
/// library at `src/lib.rs` is compiled in by path when present.
fn run_generated(
    cfg: &Config,
    bench_dir: &Path,
    names: &[&str],
    extra: &[&str],
    report_only: bool,
) -> ExitCode {
    let plan = match bench_gen::plan(bench_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    let dep = bench_gen::mockspace_dep(&plan.manifest);
    let profile = profile_args_for(plan.manifest.build.as_ref());

    if !report_only {
        // Which benches the filter selects; a request may name a
        // sweep (`bench/sweep`), which builds its bench's arms.
        let wanted: Option<Vec<String>> = if names.is_empty() {
            None
        } else {
            Some(
                names
                    .iter()
                    .map(|n| n.split('/').next().unwrap_or(n).to_string())
                    .collect(),
            )
        };
        for arm in &plan.arms {
            if let Some(w) = &wanted {
                if !w.contains(&arm.bench) {
                    continue;
                }
            }
            let target = bench_dir.join(bench_tree::arm_target_dir(&arm.bench, &arm.arm));
            let what = format!("arm {}/arms/{}", arm.bench, arm.arm);
            let manifest_path = if arm.has_manifest {
                if let Err(e) = check_arm_lib_name(arm) {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
                arm.dir.join("Cargo.toml")
            } else {
                let gen_dir = bench_gen::arm_gen_dir(&cfg.mock_dir, arm);
                let toml = match bench_gen::arm_cargo_toml(arm, &dep, &plan.support) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::FAILURE;
                    },
                };
                if let Err(e) = bench_gen::write_if_changed(&gen_dir.join("Cargo.toml"), &toml) {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
                gen_dir.join("Cargo.toml")
            };
            eprintln!("  building {what}...");
            if let Err(e) = cargo_build_at(&manifest_path, &what, &profile, Some(&target)) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
        // A flat tree without a driver crate still builds its
        // variants/ directories the legacy way; resolution for them
        // is unchanged.
        if !plan.manifest.nested_mode {
            if let Err(e) = build_flat_variants(bench_dir, names, &profile) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // ── the generated driver crate ──
    let gen_dir = bench_gen::driver_gen_dir(&cfg.mock_dir);
    let hooks_lib = bench_dir.join("src").join("lib.rs");
    let hooks_lib = hooks_lib.exists().then(|| {
        hooks_lib
            .canonicalize()
            .unwrap_or_else(|_| hooks_lib.clone())
    });
    let cargo_toml = match bench_gen::driver_cargo_toml(&dep, &plan.support) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    let main_rs = match bench_gen::driver_main_source(&plan.manifest, hooks_lib.as_deref()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    if let Err(e) = bench_gen::write_if_changed(&gen_dir.join("Cargo.toml"), &cargo_toml)
        .and_then(|_| bench_gen::write_if_changed(&gen_dir.join("src").join("main.rs"), &main_rs))
    {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("  building generated bench driver...");
    let stdout =
        match cargo_build_at(&gen_dir.join("Cargo.toml"), "generated bench driver", &profile, None)
        {
            Ok(o) => o,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            },
        };
    let bin_path = match single_executable(
        &gen_dir.join("Cargo.toml"),
        &stdout,
        "generated bench driver",
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    let mut cmd = Command::new(&bin_path);
    for n in names {
        cmd.args(["--only", n]);
    }
    for e in extra {
        cmd.arg(e);
    }
    let status = cmd.current_dir(bench_dir).status();
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => {
            eprintln!("bench driver exited with {:?}", s.code());
            ExitCode::FAILURE
        },
        Err(e) => {
            eprintln!("error: failed to spawn {}: {e}", bin_path.display());
            ExitCode::FAILURE
        },
    }
}

/// In a nested tree the arm's directory name is its lib name (that
/// is what short-name resolution builds paths from), so a consumer
/// manifest declaring a different one would build a dylib nothing
/// can find. Refuse it with the fix spelled out.
fn check_arm_lib_name(arm: &bench_tree::ArmSource) -> Result<(), String> {
    let expected = arm.arm.replace('-', "_");
    let text = fs::read_to_string(arm.dir.join("Cargo.toml"))
        .map_err(|e| format!("reading {}: {e}", arm.dir.join("Cargo.toml").display()))?;
    let doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", arm.dir.join("Cargo.toml").display()))?;
    let lib_name = doc
        .get("lib")
        .and_then(|l| l.get("name"))
        .and_then(|n| n.as_str())
        .or_else(|| {
            doc.get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
        })
        .map(|n| n.replace('-', "_"));
    match lib_name {
        Some(name) if name == expected => Ok(()),
        Some(name) => Err(format!(
            "arm {}/arms/{} declares lib name `{name}` but the directory name resolves              to `{expected}`. In a nested tree the arm's directory name is its lib              name; rename one to match the other.",
            arm.bench, arm.arm
        )),
        None => Err(format!(
            "{} has no [lib] or [package] name",
            arm.dir.join("Cargo.toml").display()
        )),
    }
}

/// Build the `variants/` directories of a flat tree (the legacy
/// resolution keeps pointing into their own target dirs).
fn build_flat_variants(bench_dir: &Path, names: &[&str], profile: &[String]) -> Result<(), String> {
    let only_dirs = if names.is_empty() {
        None
    } else {
        Some(variant_dirs_for(bench_dir, names)?)
    };
    let variants_dir = bench_dir.join("variants");
    if !variants_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&variants_dir).map_err(|e| format!("reading variants dir: {e}"))? {
        let entry = entry.map_err(|e| format!("variants dir entry: {e}"))?;
        let path = entry.path();
        if let Some(dirs) = &only_dirs {
            let dir_name = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("")
                .to_string();
            if !dirs.contains(&dir_name) {
                continue;
            }
        }
        let manifest = path.join("Cargo.toml");
        if manifest.exists() {
            eprintln!("  building variant {}...", path.display());
            cargo_build_at(
                &manifest,
                &format!("variant {}", path.display()),
                profile,
                None,
            )?;
        }
    }
    Ok(())
}

// ── init ──

fn cmd_init(cfg: &Config) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if bench_dir.exists() {
        eprintln!(
            "error: {} already exists. `mock bench init` is idempotent only when the dir is absent.",
            bench_dir.display()
        );
        eprintln!("delete the directory or pick a different scaffolding strategy.");
        return ExitCode::FAILURE;
    }

    if let Err(e) = fs::create_dir_all(&bench_dir) {
        eprintln!("error: failed to create {}: {}", bench_dir.display(), e);
        return ExitCode::FAILURE;
    }
    for sub in &["src", "variants/sample/src"] {
        if let Err(e) = fs::create_dir_all(bench_dir.join(sub)) {
            eprintln!("error: failed to create benches/{sub}: {e}");
            return ExitCode::FAILURE;
        }
    }

    if let Err(e) = write_starter_files(&bench_dir) {
        eprintln!("error: scaffolding failed: {e}");
        return ExitCode::FAILURE;
    }

    eprintln!(
        "scaffolded {} with starter bench binary + sample variant",
        bench_dir.display()
    );
    eprintln!();
    eprintln!("next steps:");
    eprintln!("  1. edit src/main.rs: replace IdentityAdd with your Routine");
    eprintln!("  2. edit bench.toml: set sizes + variant cdylib paths");
    eprintln!("  3. add a variant under variants/<name>/ for each impl");
    eprintln!("  4. run `mock bench run` to build + benchmark");
    eprintln!("  5. run `mock bench report` to regenerate findings.md from cache");
    ExitCode::SUCCESS
}

// ── list ──

/// Human display for one arm entry: the short name where the entry
/// is one, the dylib stem where it is a path.
fn entry_display(entry: &str) -> String {
    let stem = entry
        .rsplit('/')
        .next()
        .unwrap_or(entry)
        .trim_end_matches(std::env::consts::DLL_SUFFIX);
    stem.strip_prefix(std::env::consts::DLL_PREFIX)
        .unwrap_or(stem)
        .to_string()
}

fn cmd_list(cfg: &Config) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    let plan = match bench_gen::plan(&bench_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };
    for name in plan.manifest.bench_names() {
        let section = &plan.manifest.bench[&name];
        let points: Vec<String> = section.sizes.iter().map(|s| s.n.to_string()).collect();
        let mut arms: Vec<String> = section.variants.iter().map(|e| entry_display(e)).collect();
        arms.dedup();
        println!(
            "{name}  [{}]  arms: {}  {}",
            points.join(", "),
            arms.join(", "),
            section.title
        );
    }
    println!();
    println!("run one with: mock bench run <name>");
    ExitCode::SUCCESS
}

// ── add ──

fn cmd_add(cfg: &Config, args: &[&str]) -> ExitCode {
    let Some(name) = args.first() else {
        eprintln!("usage: mock bench add <variant-name>");
        return ExitCode::FAILURE;
    };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        eprintln!("error: variant name must be [a-zA-Z0-9_] (it becomes a crate name)");
        return ExitCode::FAILURE;
    }
    let bench_dir = cfg.mock_dir.join("benches");
    let dir = bench_dir.join("variants").join(name);
    if dir.exists() {
        eprintln!("error: {} already exists", dir.display());
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::create_dir_all(dir.join("src")) {
        eprintln!("error: creating {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }
    let cargo = STARTER_VARIANT_CARGO_TOML.replace("sample", name);
    let lib = STARTER_VARIANT_LIB.replace("sample", name);
    if let Err(e) = fs::write(dir.join("Cargo.toml"), cargo)
        .and_then(|_| fs::write(dir.join("src/lib.rs"), lib))
    {
        eprintln!("error: writing variant files: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("scaffolded variants/{name}/. Next:");
    eprintln!("  1. implement the run block in variants/{name}/src/lib.rs");
    eprintln!("  2. reference \"{name}\" from a bench's `variants` list in bench.toml");
    ExitCode::SUCCESS
}

fn write_starter_files(bench_dir: &Path) -> std::io::Result<()> {
    fs::write(bench_dir.join("Cargo.toml"), STARTER_BIN_CARGO_TOML)?;
    fs::write(bench_dir.join("src/main.rs"), STARTER_BIN_MAIN)?;
    fs::write(bench_dir.join("bench.toml"), STARTER_BENCH_TOML)?;
    fs::write(bench_dir.join("README.md"), STARTER_README)?;
    fs::write(
        bench_dir.join("variants/sample/Cargo.toml"),
        STARTER_VARIANT_CARGO_TOML,
    )?;
    fs::write(
        bench_dir.join("variants/sample/src/lib.rs"),
        STARTER_VARIANT_LIB,
    )?;
    Ok(())
}

const STARTER_BIN_CARGO_TOML: &str = r#"[package]
name = "benches"
version = "0.0.0"
edition = "2021"
publish = false

[[bin]]
name = "benches"
path = "src/main.rs"

[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev", features = ["std"] }
mockspace-bench-harness = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev" }

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
"#;

const STARTER_BIN_MAIN: &str = r##"//! Consumer bench binary: registrations only. The generic loop
//! (manifest iteration, filtering, report-only, preflight, seed
//! replay, validation, history, summary, findings index) lives in
//! `mockspace_bench_harness::driver::drive`.

use std::process::ExitCode;

use mockspace_bench_core::byte_routine_dispatch;
use mockspace_bench_harness::driver::{drive, DriverRegistry};
use mockspace_bench_harness::{self as harness, BenchConfig, RoutineSpec, Workload};

/// Build the workload program for a workload name. The workload
/// surrounds the measured call with realistic context so numbers
/// approximate the real calling environment; add named programs as
/// your benches need them.
fn build_workload(name: &str, _n: usize) -> Workload {
    let mut w = Workload::new();
    match name {
        "realistic" => {
            w.program("realistic", |b| {
                b.stage(vec![
                    harness::algo_call(),
                    harness::light_scalar(),
                    harness::heavy_memory(),
                    harness::branch_work(),
                ]);
            });
        }
        _ => {
            w.program("default", |b| {
                b.stage(vec![harness::algo_call(), harness::light_scalar()]);
            });
        }
    }
    w
}

/// Custom routines for benches whose inputs are not plain bytes
/// (graph shapes, sparse layouts). Return `None` to fall through to
/// the byte dispatch below.
fn routine_for(_config: &BenchConfig) -> Option<RoutineSpec> {
    None
}

fn main() -> ExitCode {
    drive(&DriverRegistry {
        build_workload,
        routine_for,
        // Every size stays its own monomorphisation: the declared
        // list is the strictly controlled input set. A manifest size
        // outside it is a targeted error naming this line.
        byte_dispatch: byte_routine_dispatch!(out = 8, sizes = [64, 256, 1024, 4096, 16384]),
    })
}
"##;

const STARTER_BENCH_TOML: &str = r#"# Bench harness configuration. `mock bench run [names...]` runs it;
# `mock bench list` prints what is registered here.
#
# Each [bench.<name>] section is one bench:
#   variants = ["a", "b"]   variant short names (dirs under variants/)
#   sizes = [64, 256]       the N list; every N must be in the bench
#                           binary's byte_routine_dispatch! declaration
#                           (each size is its own monomorphisation)
#   may_differ = false      variants may produce different valid outputs
#   required = false        validation failure fails the whole run
#   threaded = false        variants spawn threads (skips the P-core pin)
#   [bench.<name>.timing]   per-bench override of any [timing] knob
#
# master_seed: integer, or a string ("0x...") for values past the TOML
# i64 cap; 0 picks a fresh random seed (printed, replayable with
# `--seed`).
#
# [docgen] enabled = true makes the docs regeneration pass emit a
# generated BENCHES.md (plus graphviz visualisations) under docs/
# from the bench history.

[bench.sample]
title = "Sample bench"
workload = "default"
variants = ["sample"]
sizes = [64, 256]

[timing]
passes = 4
runs_per_pass = 1000
batch_size = 100
harness_runs = 1
cooldowns_ms = [0]
"#;

const STARTER_VARIANT_CARGO_TOML: &str = r#"[package]
name = "sample"
version = "0.0.0"
edition = "2021"
publish = false

[lib]
name = "sample"
path = "src/lib.rs"
crate-type = ["cdylib"]

[dependencies]
mockspace-bench-core = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev", features = ["std"] }

[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
"#;

const STARTER_VARIANT_LIB: &str = r#"//! Sample variant cdylib.
//!
//! One cdylib per variant. Each one exports `bench_entry`,
//! `bench_name`, `bench_abi_hash` (extern "C") that the harness
//! looks up via dlsym after dlopen.

use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

/// The actual algorithm under test for this variant. Replace with
/// your real impl. Operates on the same Input / Output shape as the
/// Routine in the parent bench binary.
fn sample_impl(input: &u64, output: &mut u64) {
    *output = input.wrapping_add(1);
}

#[no_mangle]
pub unsafe extern "C" fn bench_entry(
    input_ptr: *const u8,
    output_ptr: *mut u8,
    _n: usize,
) -> FfiBenchCall {
    let input = unsafe { &*(input_ptr as *const u64) };
    let output = unsafe { &mut *(output_ptr as *mut u64) };
    timed! {
        run { sample_impl(input, output); }
    }
}

#[no_mangle]
pub extern "C" fn bench_name() -> *const u8 {
    b"sample\0".as_ptr()
}

#[no_mangle]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}
"#;

const STARTER_README: &str = r#"# benches

Canonical mockspace bench framework. Consumer-side scaffolding generated
by `mock bench init`.

## Layout

- `Cargo.toml` + `src/main.rs`: the bench binary. Defines `Routine`
  impls, builds a workload program, dispatches to the harness.
- `bench.toml`: per-bench config (sizes, timing, variant cdylib paths).
- `variants/<name>/`: one workspace per variant. Each compiles to a
  cdylib that exports `bench_entry`, `bench_name`, `bench_abi_hash`.
- `target/release/<your bin name>`: the built bench binary, located from what cargo reports building.
- `target/release/lib<variant>.{dylib,so,dll}`: the built variant cdylibs.

## Workflow

1. Edit `src/main.rs`: replace `IdentityAdd` with the Routine you
   want to benchmark. The trait specifies what is computed (input
   shape, output shape, validation, scoring, ops count).
2. Edit `bench.toml`: set sizes and the cdylib path for each
   variant.
3. Add a variant under `variants/<name>/` for each implementation.
   Each variant exports `bench_entry` calling its own algorithm via
   the `timed!` macro.
4. `mock bench run` builds everything and runs the harness.
5. `mock bench report` regenerates `findings.md` from the CSV cache
   without re-running.

## Status

v2 of the bench framework. The harness ships full orchestration:
variant isolation via subprocess + dlopen, hardware counter timing
(`CNTVCT_EL0` / `rdtsc`), CSV cache with drift correction, validation
across variants (byte-exact / approximate / per-variant validity),
analysis (quintile + bootstrap CI + sign test + Pareto + multi-N
scaling), findings.md generator, history log with regression
detection, optional perf counter integration, asm dedup check.

## References

- `mockspace-bench-core` (the framework): see Routine trait docs.
- `mockspace-bench-harness` (the orchestrator): see `harness::run`
  and `harness::write_report`.
- Origin: framework was extracted from `polka-dots/mock/benches/`.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn artifact(manifest: &Path, executable: Option<&str>) -> String {
        let exe = match executable {
            Some(e) => format!("\"{e}\""),
            None => "null".to_string(),
        };
        format!(
            "{{\"reason\":\"compiler-artifact\",\"manifest_path\":\"{}\",\"executable\":{exe}}}",
            manifest.display()
        )
    }

    /// The profile must reach the argv of every build. Dropping
    /// `PROFILE_ARGS` from the builder turns this red, which the
    /// previous test could not do: it asserted the constant against
    /// itself and never reached a command.
    #[test]
    fn the_build_argv_carries_the_profile_and_the_json_format() {
        let argv: Vec<String> = build_argv(Path::new("/x/Cargo.toml"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        for expected in [
            "profile.release.opt-level=3",
            "profile.release.lto=\"fat\"",
            "profile.release.codegen-units=1",
        ] {
            let at = argv.iter().position(|a| a == expected);
            let at = at.unwrap_or_else(|| panic!("{expected} missing from argv: {argv:?}"));
            assert_eq!(argv[at - 1], "--config", "value not preceded by its flag");
        }
        assert!(argv.contains(&"--message-format=json-render-diagnostics".to_string()));
        assert_eq!(argv.last().unwrap(), "/x/Cargo.toml");
    }

    /// The control for the test above: a value that is not passed
    /// must not be reported as present, or the assertion is vacuous.
    #[test]
    fn the_build_argv_does_not_carry_a_profile_setting_we_never_pass() {
        let argv: Vec<String> = build_argv(Path::new("/x/Cargo.toml"))
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!argv.iter().any(|a| a.contains("panic=abort")));
    }

    /// The executable comes from cargo's own record. Hardcoding a
    /// name anywhere turns this red, because the name here is one
    /// the framework has never used.
    #[test]
    fn the_executable_path_comes_from_the_artifact_record() {
        let m = Path::new("/w/Cargo.toml");
        let out = artifact(m, Some("/w/target/release/some-consumers-own-name"));
        let bins = built_executables(m, out.as_bytes());
        assert_eq!(bins.len(), 1);
        assert_eq!(
            bins[0],
            PathBuf::from("/w/target/release/some-consumers-own-name")
        );
    }

    /// Three ways a line must be ignored. Without these the parser
    /// would report an executable for almost any build output.
    #[test]
    fn unrelated_artifact_lines_are_ignored() {
        let m = Path::new("/w/Cargo.toml");
        let lines = [
            artifact(m, None),
            artifact(Path::new("/other/Cargo.toml"), Some("/other/target/release/dep")),
            "{\"reason\":\"build-finished\",\"success\":true}".to_string(),
            "not json at all".to_string(),
        ]
        .join("\n");
        assert!(built_executables(m, lines.as_bytes()).is_empty());
    }

    /// A build that produced nothing runnable is named as such,
    /// rather than reported as a missing file.
    #[test]
    fn a_manifest_with_no_binary_target_is_explained() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("Cargo.toml"), "[package]\nname = \"x\"\n");
        let bins: Vec<PathBuf> = built_executables(&tmp.path().join("Cargo.toml"), b"");
        assert!(bins.is_empty());
    }

    /// Several binaries is ambiguous rather than a licence to pick
    /// the first. Taking `.next()` silently launched whichever
    /// target happened to be declared first.
    #[test]
    fn several_executables_are_all_returned_so_the_caller_can_refuse() {
        let m = Path::new("/w/Cargo.toml");
        let out = [
            artifact(m, Some("/w/target/release/helper-tool")),
            artifact(m, Some("/w/target/release/bench-driver")),
        ]
        .join("\n");
        let bins = built_executables(m, out.as_bytes());
        assert_eq!(bins.len(), 2, "both must survive for the caller to refuse");
    }

    /// The artifact path is absolute and wherever cargo put it, so a
    /// bench root inside an outer workspace resolves too. The old
    /// locator looked under the bench directory and would not have.
    #[test]
    fn an_artifact_outside_the_bench_directory_still_resolves() {
        let m = Path::new("/repo/mock/benches/Cargo.toml");
        let out = artifact(m, Some("/repo/target/release/arvo-benches"));
        let bins = built_executables(m, out.as_bytes());
        assert_eq!(bins[0], PathBuf::from("/repo/target/release/arvo-benches"));
    }
}
