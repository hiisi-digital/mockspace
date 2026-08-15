//! Bench framework command family.
//!
//! `mock bench init` scaffolds a config-only `mock/benches/` tree: a
//! root `bench.toml` of globals, a sample bench directory, and a
//! sample arm whose manifest the tool generates. No driver crate is
//! scaffolded; the driver binary is generated from `bench.toml`.
//!
//! `mock bench run` builds the arms into tool-owned target
//! directories with the pinned release profile, generates and builds
//! the driver, and spawns it. A consumer-owned `Cargo.toml` at the
//! bench root is the escape hatch: it takes the whole run over and
//! is built and located from cargo's own artifact records.
//!
//! `mock bench report` invokes the driver with `--report-only` to
//! regenerate the report files from the cached samples without
//! re-running the harness.

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
    eprintln!("  bench.toml            globals: [timing] [dispatch] [build] [workload.*]");
    eprintln!("  <bench>/bench.toml    the bench: [bench] meta + [sweep.<name>] sections");
    eprintln!("  <bench>/arms/<arm>/   one measured cdylib per arm (manifest generated)");
    eprintln!("  <bench>/support/      this bench's library crates");
    eprintln!("  support/              library crates several benches share");
    eprintln!("  src/lib.rs            optional hooks() library");
    eprintln!("  results/  history/    generated outputs (the driver is the only writer)");
    eprintln!();
    eprintln!("the driver binary is generated from bench.toml; a consumer-owned");
    eprintln!("Cargo.toml at the bench root takes the whole run over (escape hatch)");
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
            "warning: {ignored} variant entr{} outside variants/ were not rebuilt for this filtered run; their artifacts may be stale. Run without bench names to rebuild everything, or move them under variants/.",
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
    // An arm is a measured cdylib by definition; a consumer manifest
    // that builds anything else produces no dylib for the harness to
    // load, and the miss would otherwise surface as a preflight
    // failure naming a path instead of the rule.
    let is_cdylib = doc
        .get("lib")
        .and_then(|l| l.get("crate-type"))
        .and_then(|c| c.as_array())
        .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some("cdylib")));
    if !is_cdylib {
        return Err(format!(
            "arm {}/arms/{} must build a cdylib (the harness dlopens it), but its              Cargo.toml declares no `crate-type = [\"cdylib\"]`. A library crate that              arms link belongs under support/, not arms/.",
            arm.bench, arm.arm
        ));
    }
    match lib_name {
        Some(name) if name == expected => Ok(()),
        Some(name) => Err(format!(
            "arm {}/arms/{} declares lib name `{name}` but the directory name resolves to `{expected}`. In a nested tree the arm's directory name is its lib name; rename one to match the other.",
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
            "error: {} already exists; `mock bench init` scaffolds only a fresh tree.",
            bench_dir.display()
        );
        eprintln!(
            "To add a bench or an arm to the existing tree use `mock bench add`; to re-scaffold, move the directory aside first."
        );
        return ExitCode::FAILURE;
    }

    if let Err(e) = write_starter_files(&bench_dir) {
        eprintln!("error: scaffolding failed: {e}");
        return ExitCode::FAILURE;
    }

    eprintln!(
        "scaffolded {} with a sample bench (config-only: the driver binary is generated from bench.toml)",
        bench_dir.display()
    );
    eprintln!();
    eprintln!("next steps:");
    eprintln!("  1. edit sample/bench.toml: title, arms, points");
    eprintln!("  2. implement sample/arms/sample/src/lib.rs (the measured code)");
    eprintln!("  3. `mock bench add <bench> <arm>` scaffolds more of either");
    eprintln!("  4. `mock bench run` generates the driver, builds, measures");
    eprintln!("  5. `mock bench report` regenerates reports from cached samples");
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
    let bench_dir = cfg.mock_dir.join("benches");
    let legacy = bench_dir.join("variants").is_dir() && !bench_tree::is_nested_tree(&bench_dir);
    if legacy {
        return cmd_add_legacy(&bench_dir, args);
    }

    // Nested trees: `add <bench> <arm>` (or `<bench>/<arm>`). Adding
    // a bench that does not exist scaffolds it; adding an arm to an
    // existing bench scaffolds only the arm.
    let (bench, arm) = match args {
        [b, a] => (*b, Some(*a)),
        [one] if one.contains('/') => {
            let mut it = one.splitn(2, '/');
            (it.next().unwrap_or(""), it.next())
        },
        [b] => (*b, None),
        _ => {
            eprintln!("usage: mock bench add <bench> [<arm>]  (or <bench>/<arm>)");
            return ExitCode::FAILURE;
        },
    };
    let name_ok =
        |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !name_ok(bench) || arm.is_some_and(|a| !name_ok(a)) {
        eprintln!("error: bench and arm names are [a-zA-Z0-9_-] (the arm becomes a crate name)");
        return ExitCode::FAILURE;
    }

    let bdir = bench_dir.join(bench);
    if !bdir.join("bench.toml").exists() {
        let arms_list = format!("{:?}", arm.unwrap_or("sample"));
        let toml = STARTER_BENCH_DIR_TOML
            .replace("SAMPLE_TITLE", bench)
            .replace("\"SAMPLE_ARM\"", &arms_list);
        if let Err(e) = bench_gen::write_if_changed(&bdir.join("bench.toml"), &toml) {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
        eprintln!("scaffolded {bench}/bench.toml");
    }
    let arm = arm.unwrap_or("sample");
    let adir = bdir.join("arms").join(arm);
    if adir.exists() {
        eprintln!("error: {} already exists", adir.display());
        return ExitCode::FAILURE;
    }
    let lib = STARTER_ARM_LIB.replace("sample", &arm.replace('-', "_"));
    if let Err(e) = bench_gen::write_if_changed(&adir.join("src").join("lib.rs"), &lib) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("scaffolded {bench}/arms/{arm}/src/lib.rs (its manifest is generated). Next:");
    eprintln!("  1. implement the run block in {bench}/arms/{arm}/src/lib.rs");
    eprintln!("  2. reference \"{arm}\" from the bench's `arms` list if it is not there yet");
    ExitCode::SUCCESS
}

/// Legacy flat trees keep the old `add <variant>` behaviour, with
/// the `[workspace]` header the old scaffold was missing: without it
/// an outer workspace captures the crate (the target directory moves
/// and the profile is ignored) or refuses to build it outright.
fn cmd_add_legacy(bench_dir: &Path, args: &[&str]) -> ExitCode {
    let Some(name) = args.first() else {
        eprintln!("usage: mock bench add <variant-name>");
        return ExitCode::FAILURE;
    };
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        eprintln!("error: variant name must be [a-zA-Z0-9_] (it becomes a crate name)");
        return ExitCode::FAILURE;
    }
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
    let lib = STARTER_ARM_LIB.replace("sample", name);
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
    let write = |rel: &str, content: &str| -> std::io::Result<()> {
        let path = bench_dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, content)
    };
    write("bench.toml", STARTER_ROOT_BENCH_TOML)?;
    write(
        "sample/bench.toml",
        &STARTER_BENCH_DIR_TOML
            .replace("SAMPLE_TITLE", "Sample bench")
            .replace("SAMPLE_ARM", "sample"),
    )?;
    write("sample/arms/sample/src/lib.rs", STARTER_ARM_LIB)?;
    write("README.md", STARTER_README)?;
    Ok(())
}

const STARTER_ROOT_BENCH_TOML: &str = r#"# Bench tree globals. Each bench is a subdirectory with its own
# bench.toml; `mock bench list` prints what is registered,
# `mock bench run [names...]` runs it. The driver binary is generated
# from this configuration; no consumer Rust is needed for byte-shaped
# benches. Optional extension points:
#
#   src/lib.rs           a hooks() library (routine_for, after_cell,
#                        on_init, after_init)
#   <bench>/support/     library crates this bench's arms share
#   support/             library crates several benches share
#   [workload.<name>]    stages = ["algo_call", "scalar_work 48", ...]
#   [dispatch]           out = 8; points defaults to the union of
#                        every bench's points
#   [build]              mockspace dep spec + release profile values
#
# A consumer-owned Cargo.toml at this root takes the whole run over
# (the escape hatch for drivers the generator cannot express).

[timing]
passes = 4
runs_per_pass = 1000
batch_size = 100
harness_runs = 1
cooldowns_ms = [0]
"#;

const STARTER_BENCH_DIR_TOML: &str = r#"# One bench: one question, one set of competing arms. Sweeps are
# optional ([sweep.<name>] sections, each with its own points and
# overrides); without them the [bench] points make a single default
# sweep named after the bench. Declared roles: `baseline = "arm"`
# selects the arm every delta is computed against; `floor = "arm"`
# a null-cost arm subtracted from every arm first.

[bench]
title = "SAMPLE_TITLE"
workload = "default"
arms = ["SAMPLE_ARM"]
points = [64, 256]
"#;

const STARTER_ARM_LIB: &str = r#"//! One arm: the measured implementation, compiled to a cdylib the
//! harness loads in an isolated subprocess. The manifest is
//! generated (write a Cargo.toml here to take it over); the exports
//! below are the ABI the harness looks up after dlopen.

use mockspace_bench_core::{abi_hash, timed, FfiBenchCall};

/// The actual algorithm under test for this arm. Replace with your
/// real implementation.
fn sample_impl(input: &u64, output: &mut u64) {
    *output = input.wrapping_add(1);
}

#[unsafe(no_mangle)]
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

#[unsafe(no_mangle)]
pub extern "C" fn bench_name() -> *const u8 {
    b"sample\0".as_ptr()
}

#[unsafe(no_mangle)]
pub extern "C" fn bench_abi_hash() -> u64 {
    abi_hash()
}
"#;

/// The legacy flat-tree variant manifest, for `add` in trees that
/// predate the nested layout. The `[workspace]` header is not
/// optional: without it an outer workspace captures the crate, which
/// has broken a whole bench tree once and the lint cdylib once.
const STARTER_VARIANT_CARGO_TOML: &str = r#"[workspace]

[package]
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

const STARTER_README: &str = r#"# benches

Canonical mockspace bench tree. Scaffolded by `mock bench init`; the
driver binary is generated from `bench.toml` on every run.

## Layout

- `bench.toml`: globals ([timing], [dispatch], [workload.*], [build]).
- `<bench>/bench.toml`: the bench: [bench] meta plus optional
  [sweep.<name>] sections.
- `<bench>/arms/<arm>/src/lib.rs`: one measured cdylib per arm. The
  manifest is generated; writing a Cargo.toml in the arm directory
  takes it over.
- `<bench>/support/`, `support/`: ordinary library crates the arms
  and the optional hooks library link.
- `src/lib.rs` (optional): `pub fn hooks() -> Hooks` with any of
  `routine_for`, `after_cell`, `on_init`, `after_init`.
- `results/<bench>/`: samples (CSV), meta, and reports per cell,
  written transactionally. Safe to delete; a run regenerates it.
- `history/<bench>/`: the append-only per-cell ledger (median, CI,
  commit) that feeds regression detection. Tracked; never deleted.

## Workflow

1. `mock bench add <bench> <arm>` scaffolds a bench or an arm.
2. Implement the arm's run block.
3. Point lists, arm lists, roles (baseline/floor), and timing live in
   the bench's own bench.toml.
4. `mock bench run [names...]` generates the driver, builds arms into
   tool-owned target directories with the pinned release profile, and
   measures. A bench name selects all its sweeps.
5. `mock bench report` regenerates reports from cached samples.

## Escape hatch

A consumer-owned `Cargo.toml` (+ `src/main.rs`) at this root replaces
the generated driver entirely; `mock bench run` builds and runs it
instead. The library driver (`mockspace_bench_harness::driver`) keeps
the same manifest semantics either way.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_mock(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mockspace-bench-cmd-test-{}-{name}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn success() -> String {
        format!("{:?}", ExitCode::SUCCESS)
    }

    /// The old scaffold shipped a main.rs that no longer compiled
    /// against its own harness (stage constructors grew arguments).
    /// The guard is structural now: whatever init writes must load
    /// through the same planner the run uses, and the driver source
    /// must generate from it.
    #[test]
    fn init_scaffolds_a_tree_the_planner_accepts_and_the_generator_serves() {
        let mock = temp_mock("init");
        let cfg = Config::from_dir(&mock);
        let code = cmd_init(&cfg);
        assert_eq!(format!("{code:?}"), success());

        let bench_dir = mock.join("benches");
        let plan = bench_gen::plan(&bench_dir).expect("the scaffold loads");
        assert!(plan.manifest.nested_mode, "init scaffolds the nested layout");
        assert_eq!(plan.manifest.bench_names(), vec!["sample"]);
        assert_eq!(plan.arms.len(), 1);
        assert!(!plan.arms[0].has_manifest, "the arm manifest is generated");
        bench_gen::driver_main_source(&plan.manifest, None).expect("the driver generates");

        // init refuses an existing tree and points at `add`
        let code = cmd_init(&cfg);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::FAILURE));
        std::fs::remove_dir_all(&mock).ok();
    }

    #[test]
    fn an_arm_manifest_that_is_not_a_cdylib_is_refused_naming_the_rule() {
        let dir = temp_mock("arm-crate-type");
        let arm_dir = dir.join("warm/arms/kernel");
        std::fs::create_dir_all(arm_dir.join("src")).unwrap();
        std::fs::write(
            arm_dir.join("Cargo.toml"),
            "[package]\nname = \"kernel\"\n[lib]\nname = \"kernel\"\n",
        )
        .unwrap();
        let arm = bench_tree::ArmSource {
            bench:        "warm".into(),
            arm:          "kernel".into(),
            dir:          arm_dir.clone(),
            has_manifest: true,
        };
        let err = check_arm_lib_name(&arm).unwrap_err();
        assert!(err.contains("belongs under support/"), "{err}");
        // the same manifest as a proper cdylib passes both checks
        std::fs::write(
            arm_dir.join("Cargo.toml"),
            "[package]\nname = \"kernel\"\n[lib]\nname = \"kernel\"\ncrate-type = [\"cdylib\"]\n",
        )
        .unwrap();
        check_arm_lib_name(&arm).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn add_scaffolds_a_bench_and_an_arm_in_a_nested_tree_and_refuses_duplicates() {
        let mock = temp_mock("add");
        let cfg = Config::from_dir(&mock);
        assert_eq!(format!("{:?}", cmd_init(&cfg)), success());
        let bench_dir = mock.join("benches");

        // a new bench with a named arm
        let code = cmd_add(&cfg, &["warm-container", "kernel"]);
        assert_eq!(format!("{code:?}"), success());
        assert!(bench_dir.join("warm-container/bench.toml").is_file());
        assert!(bench_dir.join("warm-container/arms/kernel/src/lib.rs").is_file());
        assert!(
            !bench_dir.join("warm-container/arms/kernel/Cargo.toml").exists(),
            "no manifest is scaffolded; the tool generates it"
        );
        // the scaffolded bench names the arm it was created with
        let toml = std::fs::read_to_string(bench_dir.join("warm-container/bench.toml")).unwrap();
        assert!(toml.contains("\"kernel\""), "{toml}");

        // the slash form adds an arm to the existing bench
        let code = cmd_add(&cfg, &["warm-container/native"]);
        assert_eq!(format!("{code:?}"), success());
        assert!(bench_dir.join("warm-container/arms/native/src/lib.rs").is_file());

        // a duplicate arm is refused
        let code = cmd_add(&cfg, &["warm-container", "kernel"]);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::FAILURE));

        // the whole result still loads
        let plan = bench_gen::plan(&bench_dir).expect("still loads");
        assert!(plan.manifest.bench_names().contains(&"warm-container".to_string()));
        std::fs::remove_dir_all(&mock).ok();
    }

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
