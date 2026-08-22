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

/// Splits one subcommand's argv into positional names and flags to
/// forward, with one shared grammar rather than the two independent
/// ones `cmd_run` and `cmd_test` used to have (`starts_with("--")`
/// against `starts_with("-")`, so `-r` fell on opposite sides of the
/// two commands).
///
/// A flag is anything starting with `-` (`-r`, `--release`, `--seed`,
/// ...); everything else is positional. `value_flags` names the flags
/// in *this* argv position that take the following token as their
/// value (`--seed <n>` for the driver, `--profile <name>` for cargo):
/// that following token is forwarded as a flag argument too, rather
/// than falling through to positional and being read as a bench or
/// crate name. Before this existed, `mock bench run --seed 0xdead`
/// sent the driver `--only 0xdead --seed` (the value read as a
/// positional name, `--seed` left dangling with nothing after it),
/// and the driver's own printed `replay with --seed {:#x}` could not
/// be followed through the tool for exactly that reason.
///
/// Everything from a bare `--` onward is always a flag argument,
/// never positional and never itself treated as a value-flag lookup:
/// it is cargo's separator for arguments the *test binary* receives,
/// and `describe_cargo_profile` relies on that boundary to avoid
/// reading a libtest flag as a cargo one.
fn split_positional_and_flags<'a>(
    args: &[&'a str],
    value_flags: &[&str],
) -> (Vec<&'a str>, Vec<&'a str>) {
    let mut positional = Vec::new();
    let mut flags = Vec::new();
    let mut i = 0;
    let mut past_separator = false;
    while i < args.len() {
        let a = args[i];
        if past_separator {
            flags.push(a);
            i += 1;
            continue;
        }
        if a == "--" {
            past_separator = true;
            flags.push(a);
            i += 1;
            continue;
        }
        if value_flags.contains(&a) {
            flags.push(a);
            if let Some(v) = args.get(i + 1) {
                flags.push(v);
                i += 1;
            }
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            flags.push(a);
        } else {
            positional.push(a);
        }
        i += 1;
    }
    (positional, flags)
}

pub fn cmd(cfg: &Config, args: &[&str]) -> ExitCode {
    let sub = args.first().copied().unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).copied().collect();
    match sub {
        "init" => cmd_init(cfg),
        "run" => cmd_run(cfg, &rest),
        "report" => cmd_report(cfg, &rest),
        "test" => cmd_test(cfg, &rest),
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
    eprintln!("  test    run cargo test in every crate under the bench tree");
    eprintln!("  list    list benches, sizes, and variants from bench.toml");
    eprintln!("  add     scaffold a new variant crate: mock bench add <name>");
    eprintln!();
    eprintln!("`run` and `report` accept bench names to restrict the pass:");
    eprintln!("  mock bench run <name> [<name> ...]   run only the named benches");
    eprintln!("with no names, every bench in bench.toml runs");
    eprintln!();
    eprintln!("`mock/benches/` layout (created by `init`):");
    eprintln!("  bench.toml            globals: [timing] [dispatch] [build] [workload.*]");
    eprintln!("  <bench>/bench.toml    the bench: top-level fields + optional [sweep.<name>]");
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
    // (forwarded as --only) and the variant builds. `--seed` takes its
    // following token as its value, per split_positional_and_flags.
    let (names, extra) = split_positional_and_flags(args, &["--seed"]);

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
    let profile = consumer_tree_profile(&bench_dir);
    let bin_path = match build_variants_and_bin_filtered(&bench_dir, dirs.as_deref(), &profile) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    let mut cmd = Command::new(&bin_path);
    cmd.env(
        mockspace_bench_harness::harness::BUILD_PROFILE_ENV,
        profile_env_value(profile.iter().map(String::as_str)),
    );
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

    let profile = consumer_tree_profile(&bench_dir);
    let bin_path = match build_bin_only(&bench_dir, &profile) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        },
    };

    let mut cmd = Command::new(&bin_path);
    cmd.env(
        mockspace_bench_harness::harness::BUILD_PROFILE_ENV,
        profile_env_value(profile.iter().map(String::as_str)),
    );
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

/// How to describe the profile a `cargo test` run used, for the summary line.
///
/// A test count with no profile beside it is not interpretable: the same
/// thirty tests took 133.72s and 4.99s on one host under the two profiles.
/// Reported from the flags actually forwarded rather than from a constant, so
/// it cannot drift from what ran.
///
/// Only looks at flags before a bare `--`: everything after it is cargo's
/// own separator for arguments the test *binary* receives (libtest flags),
/// never cargo's own. `mock bench test -- --release` runs debug and passes
/// `--release` to libtest (which has no such flag and would reject it, but
/// that is cargo's business, not this function's); reporting "profile:
/// release" for that invocation would describe a build that did not happen.
/// Recognises `-r` as the short form of `--release`, which cargo itself
/// accepts and this function previously did not.
fn describe_cargo_profile(extra: &[&str]) -> String {
    let cargo_args: &[&str] =
        match extra.iter().position(|e| *e == "--") {
            Some(i) => &extra[..i],
            None => extra,
        };
    if cargo_args.iter().any(|e| *e == "--release" || *e == "-r") {
        return "cargo test --release, profile: release".to_string();
    }
    if let Some(p) = cargo_args.iter().position(|e| *e == "--profile") {
        if let Some(name) = cargo_args.get(p + 1) {
            return format!("cargo test --profile {name}, profile: {name}");
        }
    }
    if let Some(named) = cargo_args.iter().find_map(|e| e.strip_prefix("--profile=")) {
        return format!("cargo test --profile={named}, profile: {named}");
    }
    "cargo test, profile: debug (cargo's default)".to_string()
}

// ── test ──

/// A bench tree is built from many small crates (arms, support
/// libraries, an optional hooks lib), most of them path dependencies
/// of the generated or consumer-owned driver rather than members of
/// any cargo workspace. `cargo test` run at the tree root therefore
/// tests only the driver crate itself and reports its zero tests as
/// a pass; the crates that actually carry assertions, typically the
/// `*-shared` support libraries whose arms depend on them, are never
/// reached. This is the same "reads as a pass, measured nothing"
/// shape `tree::load` already refuses for a bench tree resolving to
/// zero benches (see its own comment); this command is the
/// equivalent refusal for a bench tree whose crates report zero
/// tests between them.
///
/// So `mock bench test` does not delegate to a single `cargo test`
/// invocation. It walks the tree for every crate manifest (skipping
/// `target/` and dot directories, the same rule `tree::discover`
/// uses), runs `cargo test` inside each crate's own directory, and
/// aggregates the result. A crate reporting zero tests is normal (an
/// arm cdylib usually carries none) and is not itself a failure; the
/// tree as a whole reporting zero tests across every crate is
/// refused, because that is indistinguishable from the driver-only
/// invocation this command exists to replace.
fn cmd_test(cfg: &Config, args: &[&str]) -> ExitCode {
    let bench_dir = cfg.mock_dir.join("benches");
    if !bench_dir.exists() {
        eprintln!(
            "error: {} does not exist. Run `mock bench init` first.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    // `--profile <name>` takes its following token as its value, per
    // split_positional_and_flags: without this, `mock bench test --profile
    // bench` sent "bench" to the crate-name filter (matching nothing, or
    // matching something the caller never named) instead of leaving it
    // attached to `--profile`, the same class of bug `cmd_run` had with
    // `--seed`.
    let (filters, extra) = split_positional_and_flags(args, &["--profile"]);
    let mut manifests = find_crate_manifests(&bench_dir);

    // The generated-driver shape has no committed manifest for an arm
    // that is only a `src/lib.rs`: `mock bench run` generates one on
    // demand, under `mock/target/`, outside `bench_dir`, which is why
    // the walk above cannot find it. A freshly `mock bench init`'d tree
    // therefore has zero Cargo.toml anywhere until something generates
    // one, and the walk alone reports "nothing to run" on a tree that
    // plainly has an arm in it. Write the same manifest `mock bench run`
    // would (never build it; `cargo test` builds what it needs), and
    // fold it into the set to walk. Skipped entirely on the escape-hatch
    // shape (a consumer-owned root `Cargo.toml`), since `bench_gen::plan`
    // does not describe that tree and the walk above already found
    // everything real in it.
    if !bench_dir.join("Cargo.toml").is_file() {
        if let Ok(plan) = bench_gen::plan(&bench_dir) {
            let dep = bench_gen::mockspace_dep(&plan.manifest);
            for arm in &plan.arms {
                if arm.has_manifest {
                    continue; // already on disk; the walk above found it
                }
                let toml = match bench_gen::arm_cargo_toml(arm, &dep, &plan.support) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!(
                            "error: generating a manifest for {}/arms/{}: {e}",
                            arm.bench, arm.arm
                        );
                        return ExitCode::FAILURE;
                    },
                };
                let gen_dir = bench_gen::arm_gen_dir(&cfg.mock_dir, arm);
                if let Err(e) = bench_gen::write_if_changed(&gen_dir.join("Cargo.toml"), &toml) {
                    eprintln!(
                        "error: generating a manifest for {}/arms/{}: {e}",
                        arm.bench, arm.arm
                    );
                    return ExitCode::FAILURE;
                }
                manifests.push(gen_dir.join("Cargo.toml"));
            }
        }
        // A `plan` error (no bench declared anywhere, an invalid
        // manifest) is not this command's to report a second time: the
        // walk above already found whatever is really on disk, and if
        // that is nothing, the refusal immediately below says so.
    }
    manifests.sort();
    manifests.dedup();

    if manifests.is_empty() {
        eprintln!(
            "error: no Cargo.toml found under {}. There is nothing for `mock bench test` to run.",
            bench_dir.display()
        );
        return ExitCode::FAILURE;
    }

    // A name that selects nothing is refused rather than ignored. The first
    // version dropped every non-flag argument silently, so `mock bench test
    // <typo>` ran the whole tree and reported a clean pass over crates the
    // caller had not asked for. `cmd_run` already refuses an unknown bench
    // name by listing the ones that exist; this is the same refusal.
    let manifests: Vec<PathBuf> = if filters.is_empty() {
        manifests
    } else {
        let selected: Vec<PathBuf> = manifests
            .iter()
            .filter(|m| {
                let rel = m.strip_prefix(&bench_dir).unwrap_or(m).to_string_lossy().to_string();
                filters.iter().any(|f| rel.contains(f))
            })
            .cloned()
            .collect();
        if selected.is_empty() {
            let available: Vec<String> = manifests
                .iter()
                .filter_map(|m| m.parent())
                .filter_map(|d| d.strip_prefix(&bench_dir).ok())
                .map(|p| p.display().to_string())
                .filter(|p| !p.is_empty())
                .collect();
            eprintln!(
                "error: no crate under {} matches {:?}. Available: {}",
                bench_dir.display(),
                filters,
                available.join(", ")
            );
            return ExitCode::FAILURE;
        }
        selected
    };

    let mut total_passed = 0u64;
    let mut total_failed = 0u64;
    let mut total_ignored = 0u64;
    let mut crates_with_tests = 0usize;
    let mut crates_failed: Vec<PathBuf> = Vec::new();

    // One shared target directory for every crate this command builds,
    // rather than each crate's own default `target/` next to its manifest.
    // Without this, running the tree's real shape (arvo's is ninety-five
    // crates) builds ninety-five separate target directories, none of them
    // sharing a compiled copy of a common dependency (mockspace-bench-core,
    // mockspace-bench-harness, ...), which is real disk and real duplicated
    // compile time on every run.
    let target_dir =
        crate::build_dir::ensure_under_target(&cfg.mock_dir, &["mockspace-bench-test"]);

    for manifest in &manifests {
        let dir = manifest.parent().unwrap_or(manifest.as_path());
        let rel = manifest_display(manifest, &bench_dir, &cfg.mock_dir);
        // Printed before the build starts, and flushed, so a crate that
        // hangs (this tree has one known case, handled separately) shows
        // exactly where it is stuck rather than producing total silence
        // until it is killed. `.output()` below blocks until the child
        // exits and captures everything at once; nothing between "running"
        // and the eventual ok/FAIL line is visible without this.
        // A full line on its own, on stderr, rather than a continuation
        // meant to share a line with the eventual ok/FAIL (stdout): the two
        // streams interleave unpredictably in a terminal, and a hung crate
        // must leave an unambiguous "this is where it is stuck" line behind
        // regardless of buffering.
        eprintln!("running {rel}...");

        let mut cmd = Command::new("cargo");
        cmd.arg("test").arg("--manifest-path").arg(manifest);
        cmd.arg("--target-dir").arg(&target_dir);
        for e in &extra {
            cmd.arg(e);
        }
        let output = match cmd.current_dir(dir).output() {
            Ok(o) => o,
            Err(e) => {
                eprintln!("error: failed to run cargo test in {}: {e}", dir.display());
                return ExitCode::FAILURE;
            },
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let (passed, failed, ignored) = parse_test_result_lines(&stdout);
        total_passed += passed;
        total_failed += failed;
        total_ignored += ignored;
        if passed > 0 || failed > 0 || ignored > 0 {
            crates_with_tests += 1;
        }
        if output.status.success() {
            println!("ok    {rel}  {passed} passed, {failed} failed, {ignored} ignored");
        } else {
            println!("FAIL  {rel}  {passed} passed, {failed} failed, {ignored} ignored");
            crates_failed.push(manifest.clone());
            if !stdout.trim().is_empty() {
                eprintln!("{stdout}");
            }
            if !stderr.trim().is_empty() {
                eprintln!("{stderr}");
            }
        }
    }

    // The profile is named because a bare count is not interpretable without
    // it. A consumer tree measured 133.72s under the default profile and
    // 4.99s under release for the identical thirty tests; a downstream reader
    // handed only the number cannot tell which was run, and a panel in this
    // workspace retired a true figure on exactly that ambiguity.
    println!(
        "\n{} crates, {crates_with_tests} carrying tests, {total_passed} passed, {total_failed} failed, {total_ignored} ignored  [{}]",
        manifests.len(),
        describe_cargo_profile(&extra)
    );

    if !crates_failed.is_empty() {
        eprintln!(
            "error: {} of {} crates failed their tests",
            crates_failed.len(),
            manifests.len()
        );
        return ExitCode::FAILURE;
    }

    if crates_with_tests == 0 {
        eprintln!(
            "error: {} crates were found and built, and none of them carries a single test. Either the tree genuinely has no test coverage, in which case this is the honest answer `cargo test` at the tree root was hiding, or the discovery above missed something; either way this is not a state to report as a pass.",
            manifests.len()
        );
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

/// A readable label for a manifest path in `mock bench test`'s output:
/// relative to `bench_dir` where it lives under the tree the consumer
/// authors, relative to `mock_dir` where it is a generated arm
/// manifest (which lives under `mock_dir/target/`, outside
/// `bench_dir`), or the raw path if neither applies. Without this a
/// generated arm's line printed its full absolute path, which is
/// correct and unreadable, and gave no sign that the crate being
/// tested was one `mock bench test` generated rather than one the
/// consumer wrote.
fn manifest_display(manifest: &Path, bench_dir: &Path, mock_dir: &Path) -> String {
    if let Ok(rel) = manifest.strip_prefix(bench_dir) {
        return rel.display().to_string();
    }
    if let Ok(rel) = manifest.strip_prefix(mock_dir) {
        return format!("(generated) {}", rel.display());
    }
    manifest.display().to_string()
}

/// Every crate manifest under `dir`, found by walking every
/// directory (skipping `target/` and dot directories, the same rule
/// `tree::discover` uses to skip build output and hidden trees) and
/// recording a `Cargo.toml` wherever one exists.
///
/// **This does not stop descending at a found manifest.** A bench
/// tree's own root carries a `Cargo.toml` (the driver bin crate, or
/// the consumer-owned escape-hatch crate), and every arm and support
/// crate lives in a subdirectory of that same root, so stopping at
/// the first manifest found finds only the driver and nothing it was
/// meant to reach: arvo's tree has one root manifest and ninety-four
/// more beneath it, and the earlier form of this function returned
/// exactly the one. The corrected form treats "this directory is a
/// crate" and "keep looking for more crates underneath it" as
/// independent facts, because in this tree shape they always are.
fn find_crate_manifests(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let manifest = d.join("Cargo.toml");
        if manifest.is_file() {
            found.push(manifest);
        }
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "target" {
                continue;
            }
            stack.push(path);
        }
    }
    found.sort();
    found
}

/// Sums every `test result: ok|FAILED. N passed; M failed; K ignored; ...`
/// line `cargo test`'s human-readable output carries, one per test binary
/// (unit tests, each integration test file, doctests). Parsed from stdout
/// text rather than `--format json` because the unstable json output would
/// add a nightly-only dependency this command does not otherwise need, and
/// the line format is `cargo`'s own stable, documented summary line.
fn parse_test_result_lines(stdout: &str) -> (u64, u64, u64) {
    let mut passed = 0u64;
    let mut failed = 0u64;
    let mut ignored = 0u64;
    for line in stdout.lines() {
        let Some(rest) = line.trim_start().strip_prefix("test result:") else {
            continue;
        };
        // Each field is "<verdict>. <n> passed" for the first field and
        // "<n> <word>" for the rest, so the number is always the token
        // immediately before the field's trailing word rather than the
        // whole field: stripping the suffix and parsing the remainder
        // fails on the first field ("ok. 15 passed" is not a bare number
        // once " passed" is stripped), which the accompanying unit test
        // pins so this cannot regress silently.
        for field in rest.split(';') {
            let words: Vec<&str> = field.split_whitespace().collect();
            for pair in words.windows(2) {
                let (n, tag) = (pair[0], pair[1]);
                let Ok(n) = n.parse::<u64>() else {
                    continue;
                };
                match tag {
                    "passed" => passed += n,
                    "failed" => failed += n,
                    "ignored" => ignored += n,
                    _ => {},
                }
            }
        }
    }
    (passed, failed, ignored)
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


/// The effective release profile for a **consumer-owned-driver** tree.
///
/// `[build]` reached only the generated path: `profile_args_for` had one
/// production caller, inside `run_generated`, while every build on this path
/// went through `build_argv`, which took no config and passed the framework
/// constant. So a tree with its own driver could declare `opt-level = 0`,
/// see no error, and be built at 3, with the run's own metadata then
/// truthfully recording the 3 that was used and nothing recording that the
/// declaration had been dropped. `BuildSection`'s doc promises the opposite:
/// the values travel on the command line "where a manifest cannot silently
/// drop them".
///
/// A tree with no `bench.toml` gets the framework defaults, which is the
/// documented behaviour rather than a fallback. A `bench.toml` that exists
/// and does not parse is reported, because defaulting silently on an
/// unreadable declaration is the failure this function exists to remove.
fn consumer_tree_profile(bench_dir: &Path) -> Vec<String> {
    let path = bench_dir.join("bench.toml");
    if !path.exists() {
        return profile_args_for(None);
    }
    match mockspace_bench_harness::config::BenchManifest::load(&path) {
        Ok(m) => profile_args_for(m.build.as_ref()),
        Err(e) => {
            eprintln!(
                "warning: {} could not be read ({e}), so [build] could not be applied and the framework's default release profile is in use.",
                path.display()
            );
            profile_args_for(None)
        },
    }
}

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
fn build_argv(manifest: &Path, profile: &[String]) -> Vec<std::ffi::OsString> {
    let mut argv: Vec<std::ffi::OsString> = ["build", "--release", "--message-format=json-render-diagnostics"]
        .iter()
        .map(Into::into)
        .collect();
    argv.extend(profile.iter().map(Into::into));
    argv.push("--manifest-path".into());
    argv.push(manifest.as_os_str().to_owned());
    argv
}

/// Run one cargo build and return its stdout for artifact parsing.
fn cargo_build_json(manifest: &Path, what: &str, profile: &[String]) -> Result<Vec<u8>, String> {
    let out = Command::new("cargo")
        .args(build_argv(manifest, profile))
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
    profile: &[String],
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
                cargo_build_json(&manifest, &format!("variant {}", path.display()), profile)?;
            }
        }
    }

    build_bin_only(bench_dir, profile)
}

/// Build the bench binary and return the path cargo says it wrote.
///
/// Building and locating are one step because they share an answer:
/// the artifact record cargo emits during the build already carries
/// the absolute path. Splitting them is what let the two drift, with
/// the locator guessing a name and a directory the build never
/// promised.
fn build_bin_only(bench_dir: &Path, profile: &[String]) -> Result<PathBuf, String> {
    let manifest = bench_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!(
            "{} not found; the scaffold may have been deleted",
            manifest.display()
        ));
    }
    eprintln!("  building bench binary...");
    let stdout = cargo_build_json(&manifest, "bench binary", profile)?;
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
///
/// This is the single source of the profile. It was two: a
/// `PROFILE_ARGS` constant that every build on the consumer-owned
/// driver path used, and this function, which only the generated path
/// called. The two agreed on the defaults, so nothing was wrong until
/// a tree declared `[build]`, at which point one path honoured it and
/// the other did not.
///
/// Why the values travel on the command line at all: a
/// `[profile.release]` table is honoured only in a workspace root, so
/// a consumer whose manifests never declared one never had the
/// documented profile. That is what was measured: a tree of
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

/// The value the spawned driver's `MOCKSPACE_BENCH_PROFILE` carries:
/// the `profile.release.*` settings from the exact flags passed to
/// the builds, so the run's metadata records what was used rather
/// than a constant that stops being true the moment `[build]`
/// overrides anything.
fn profile_env_value<'a>(flags: impl IntoIterator<Item = &'a str>) -> String {
    flags
        .into_iter()
        .filter_map(|f| f.strip_prefix("profile.release."))
        .collect::<Vec<_>>()
        .join(",")
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
        // Root sections and sections-form members build their
        // variants/ directories the legacy way; resolution for them
        // is unchanged. A filtered run that selects only members
        // still builds the root set, which is cheap and never wrong.
        let root_names: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| plan.manifest.nested.get(*n).is_none() && !n.contains('/'))
            .collect();
        if names.is_empty() || !root_names.is_empty() {
            if let Err(e) = build_flat_variants(bench_dir, &root_names, &profile) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
        for member in &plan.flat_members {
            if let Some(w) = &wanted {
                if !w.iter().any(|n| n == member) {
                    continue;
                }
            }
            if let Err(e) = build_flat_variants(&bench_dir.join(member), &[], &profile) {
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
    cmd.env(
        mockspace_bench_harness::harness::BUILD_PROFILE_ENV,
        profile_env_value(profile.iter().map(String::as_str)),
    );
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
            "arm {}/arms/{} must build a cdylib (the harness dlopens it), but its \
             Cargo.toml declares no `crate-type = [\"cdylib\"]`. A library crate that \
             arms link belongs under support/, not arms/.",
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
    // `add` recognises no flags at all, so anything shaped like one is
    // refused rather than handed to the positional matcher below. Without
    // this, `mock bench add newb --force` matched the two-argument arm
    // (`[b, a] => (*b, Some(*a))`) and `name_ok` permits `-`, needed for
    // real hyphenated names, so `--force` passed validation as an arm name
    // and the command scaffolded `newb/arms/--force/` and wrote
    // `arms = ["--force"]` into `newb/bench.toml`, reporting a clean exit
    // 0 over a tree that now cannot run. There is nothing this subcommand
    // does with a flag today; a caller reaching for one (a reasonable
    // guess, since `init` and other tools take `--force`) gets told so
    // rather than getting silent garbage.
    if let Some(flag) = args.iter().find(|a| a.starts_with('-')) {
        eprintln!(
            "error: `mock bench add` takes no flags; `{flag}` would otherwise be read as a bench or arm name. usage: mock bench add <bench> [<arm>]  (or <bench>/<arm>)"
        );
        return ExitCode::FAILURE;
    }

    let bench_dir = cfg.mock_dir.join("benches");
    // A tree whose root carries a variants/ directory is the flat
    // layout; `add` keeps its legacy shape there. Everything else
    // scaffolds a member, which the default benchspace glob picks up
    // without a declaration.
    if bench_dir.join("variants").is_dir() {
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
        warn_if_not_a_member(&bench_dir, bench);
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

/// A freshly scaffolded bench is a member through the default glob;
/// a root that declares an explicit member list has opted out of
/// discovery, so say so instead of leaving a bench that never runs.
fn warn_if_not_a_member(bench_dir: &Path, bench: &str) {
    let Ok(manifest) =
        mockspace_bench_harness::config::BenchManifest::load(&bench_dir.join("bench.toml"))
    else {
        return;
    };
    let Some(space) = manifest.benchspace else {
        return; // default ["**"] matches everything
    };
    let matched = space.members.iter().any(|p| bench_tree::glob_match(p, bench))
        && !space.exclude.iter().any(|p| bench_tree::glob_match(p, bench));
    if !matched {
        eprintln!(
            "warning: [benchspace] members = {:?} does not match `{bench}`; add it to \
             the list or it will never run.",
            space.members
        );
    }
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
# Membership is declared, cargo-workspace style, and defaults to
# every subdirectory carrying its own bench.toml:
#
#   [benchspace]
#   members = ["**"]      # the default when the section is absent
#   exclude = []
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

const STARTER_BENCH_DIR_TOML: &str = r#"# One bench: one question, one set of competing arms. The directory
# name is the bench's name, so the fields sit at the top level with
# no wrapper table. Sweeps are optional ([sweep.<name>] sections,
# each with its own points and overrides); without them the points
# here make a single default sweep named after the bench. Declared
# roles: `baseline = "arm"` selects the arm every delta is computed
# against; `floor = "arm"` a null-cost arm subtracted from every arm
# first.

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
- `<bench>/bench.toml`: the bench. Its fields sit at the top level
  with no wrapper table, because the directory name is the bench's
  name. Optional [sweep.<name>] sections carry per-sweep points and
  overrides.
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

    /// A real `cargo test` line, captured verbatim from `warm-container-shared`
    /// (arvo's bench tree) rather than typed from memory of the format.
    const REAL_OK_LINE: &str =
        "test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; \
         finished in 0.18s";

    /// The case that must fail: the first field is "ok. 15 passed", not a bare
    /// number, so a parser that strips " passed" and parses the remainder as a
    /// whole gets "ok. 15" and silently reports zero. Written against the
    /// buggy first draft of `parse_test_result_lines` before it was fixed, kept
    /// here so the fix cannot regress unnoticed.
    #[test]
    fn parse_test_result_lines_reads_the_ok_verdict_prefix_correctly() {
        assert_eq!(parse_test_result_lines(REAL_OK_LINE), (15, 0, 0));
    }

    #[test]
    fn parse_test_result_lines_reads_failures_and_ignores() {
        let line = "test result: FAILED. 3 passed; 2 failed; 1 ignored; 0 measured; \
                     0 filtered out; finished in 0.02s";
        assert_eq!(parse_test_result_lines(line), (3, 2, 1));
    }

    /// Multiple test binaries (unit tests plus an integration test file)
    /// each print their own `test result:` line; the parser sums across all
    /// of them rather than reading only the first.
    #[test]
    fn parse_test_result_lines_sums_across_multiple_binaries() {
        let stdout = format!("{REAL_OK_LINE}\n\n{REAL_OK_LINE}\n");
        assert_eq!(parse_test_result_lines(&stdout), (30, 0, 0));
    }

    /// Negative control: stdout carrying no `test result:` line at all
    /// (a build failure, or output from something else entirely) must not
    /// be misread as zero tests passing; it is zero tests *found*, which is
    /// the same value and a different meaning, and `cmd_test` is the layer
    /// that tells them apart via the crate-count, not this function.
    #[test]
    fn parse_test_result_lines_on_unrelated_output_finds_nothing() {
        assert_eq!(
            parse_test_result_lines("error[E0308]: mismatched types\nsome other text"),
            (0, 0, 0)
        );
    }

    #[test]
    fn find_crate_manifests_finds_nested_crates_and_skips_target() {
        let root = temp_mock("find-manifests");
        // The root itself carries a manifest, exactly as every real bench
        // tree does (the generated or consumer-owned driver bin crate).
        // Omitting this is what let the "stop descending at a found
        // manifest" defect through: every fixture in the first version of
        // this test lacked the one file that triggers it. See
        // find_crate_manifests_descends_past_a_root_manifest_to_find_what_is_beneath_it
        // for the regression this fixture alone would not have caught.
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"driver\"").unwrap();
        std::fs::create_dir_all(root.join("arms/a/src")).unwrap();
        std::fs::write(root.join("arms/a/Cargo.toml"), "[package]\nname=\"a\"").unwrap();
        std::fs::create_dir_all(root.join("support/shared/src")).unwrap();
        std::fs::write(root.join("support/shared/Cargo.toml"), "[package]\nname=\"shared\"")
            .unwrap();
        // A target/ directory containing a vendored crate's manifest must
        // never be walked into: this is the exact class the tool's own
        // `tree::discover` refuses to walk for the identical reason.
        std::fs::create_dir_all(root.join("target/debug/build/decoy/out")).unwrap();
        std::fs::write(
            root.join("target/debug/build/decoy/out/Cargo.toml"),
            "[package]\nname=\"decoy\"",
        )
        .unwrap();
        let found = find_crate_manifests(&root);
        let rels: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&root).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(rels, vec!["Cargo.toml", "arms/a/Cargo.toml", "support/shared/Cargo.toml"]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// The regression itself, isolated to the one line that caused it: a
    /// root manifest, alone with nothing else, must not be the only thing
    /// found once a nested crate exists. The prior version of this
    /// function returned exactly the root and nothing beneath it whenever
    /// the root itself had a `Cargo.toml`, which is arvo's tree's actual
    /// shape (a driver bin crate at the root, ninety-four crates beneath
    /// it) and is why `mock bench test` found one manifest of ninety-five
    /// on the tree it was built for.
    #[test]
    fn find_crate_manifests_descends_past_a_root_manifest_to_find_what_is_beneath_it() {
        let root = temp_mock("find-manifests-root-and-nested");
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"driver\"").unwrap();
        std::fs::create_dir_all(root.join("variants/only-one/src")).unwrap();
        std::fs::write(root.join("variants/only-one/Cargo.toml"), "[package]\nname=\"only-one\"")
            .unwrap();
        let found = find_crate_manifests(&root);
        assert_eq!(found.len(), 2, "expected the root manifest and the one nested crate, \
            found {found:?}; a walk that stops at the root would report only the root");
    }

    /// Negative control on the walker: an empty tree (a bench dir that
    /// exists but holds nothing yet) must return an empty list rather than
    /// panicking or fabricating an entry, which is the shape
    /// `cmd_test` relies on to produce its own "nothing to run" refusal.
    /// The second broken tree shape the review named: `mock bench init`
    /// scaffolds a config-only tree with no `Cargo.toml` anywhere (the
    /// sample arm is a bare `src/lib.rs`; its manifest is generated on
    /// demand, the same way `mock bench run` generates one, under
    /// `mock_dir/target/`, outside `bench_dir`). Before the fix,
    /// `cmd_test`'s walk found nothing and refused with "no Cargo.toml
    /// found", on the exact tree `mock bench init` itself produces. This
    /// checks that the generated arm's manifest lands on disk where
    /// `mock bench test` looks for it, which is what distinguishes
    /// "found the crate, it has no tests" (correct, and what this test
    /// expects) from "found nothing at all" (the bug).
    #[test]
    fn cmd_test_generates_a_manifest_for_the_freshly_scaffolded_sample_arm() {
        let root = temp_mock("cmd-test-after-init");
        let cfg = Config::from_dir(&root);
        assert_eq!(format!("{:?}", cmd_init(&cfg)), success());

        let bench_dir = root.join("benches");
        assert!(
            !bench_dir.join("Cargo.toml").exists(),
            "a freshly init'd tree must not have a root Cargo.toml; if it does, this test is no longer exercising the generated-driver shape"
        );

        let code = cmd_test(&cfg, &[]);
        // The starter arm has no #[test] in it, so the honest result is
        // the "carries a single test" refusal, not success; what this
        // test is actually checking is that the manifest was generated
        // and reached, which the exit code alone cannot distinguish from
        // the pre-fix "found nothing" refusal, so it checks the file.
        let _ = code;

        let plan = bench_gen::plan(&bench_dir).expect("the scaffold loads");
        assert_eq!(plan.arms.len(), 1, "expected exactly the one scaffolded sample arm");
        let arm = &plan.arms[0];
        assert!(!arm.has_manifest, "the scaffolded arm should still have no manifest of its own");
        let generated = bench_gen::arm_gen_dir(&cfg.mock_dir, arm).join("Cargo.toml");
        assert!(
            generated.is_file(),
            "expected {} to exist after `mock bench test`; the generated-driver shape's \
             manifests are outside bench_dir and cmd_test must write them before walking, \
             the same way `mock bench run` does",
            generated.display()
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// The `crates_with_tests == 0` refusal, exercised end to end rather
    /// than only by shell probes: a tree whose one real, buildable crate
    /// carries no `#[test]` at all must not report success. Before this
    /// existed, `mock bench test` on such a tree reported "ok" over a
    /// crate count that included zero tests, which is exactly the
    /// `cargo test` at the bench root reads-as-a-pass shape this command
    /// exists to eliminate, reintroduced one level down.
    #[test]
    fn cmd_test_refuses_when_every_discovered_crate_carries_zero_tests() {
        let root = temp_mock("cmd-test-zero-tests");
        let bench_dir = root.join("benches");
        std::fs::create_dir_all(&bench_dir).unwrap();
        std::fs::write(bench_dir.join("bench.toml"), "").unwrap();
        write_minimal_driver_crate(&bench_dir);
        std::fs::create_dir_all(bench_dir.join("support/no-tests-here/src")).unwrap();
        std::fs::write(
            bench_dir.join("support/no-tests-here/Cargo.toml"),
            "[package]\nname=\"no-tests-here\"\nedition=\"2021\"",
        )
        .unwrap();
        std::fs::write(
            bench_dir.join("support/no-tests-here/src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )
        .unwrap();

        let cfg = Config::from_dir(&root);
        let code = cmd_test(&cfg, &[]);
        assert_eq!(
            format!("{code:?}"),
            format!("{:?}", ExitCode::FAILURE),
            "a real, compiling, test-free crate must not make `mock bench test` report success"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    /// The unmatched-filter refusal, exercised directly. Filtering happens
    /// before any crate is built, so this fixture does not need a real
    /// buildable crate, only a discoverable manifest.
    #[test]
    fn cmd_test_refuses_a_filter_that_matches_nothing() {
        let root = temp_mock("cmd-test-unmatched-filter");
        let bench_dir = root.join("benches");
        std::fs::create_dir_all(&bench_dir).unwrap();
        std::fs::write(bench_dir.join("bench.toml"), "").unwrap();
        write_minimal_driver_crate(&bench_dir);
        std::fs::create_dir_all(bench_dir.join("support/real-crate")).unwrap();
        std::fs::write(
            bench_dir.join("support/real-crate/Cargo.toml"),
            "[package]\nname=\"real-crate\"",
        )
        .unwrap();

        let cfg = Config::from_dir(&root);
        let code = cmd_test(&cfg, &["this-name-matches-nothing"]);
        assert_eq!(
            format!("{code:?}"),
            format!("{:?}", ExitCode::FAILURE),
            "a filter matching no discovered crate must be refused rather than silently \
             running the whole tree"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn find_crate_manifests_on_an_empty_tree_finds_nothing() {
        let root = temp_mock("find-manifests-empty");
        assert!(find_crate_manifests(&root).is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn split_positional_and_flags_separates_names_from_flags() {
        let (pos, flags) = split_positional_and_flags(&["warm-container", "--release"], &[]);
        assert_eq!(pos, vec!["warm-container"]);
        assert_eq!(flags, vec!["--release"]);
    }

    /// `-r` and other single-dash flags must land as flags, not names:
    /// `cmd_run`'s old `starts_with("--")` split let a single-dash flag
    /// fall through as a positional bench name.
    #[test]
    fn split_positional_and_flags_treats_single_dash_as_a_flag_too() {
        let (pos, flags) = split_positional_and_flags(&["-r", "warm-container"], &[]);
        assert_eq!(pos, vec!["warm-container"]);
        assert_eq!(flags, vec!["-r"]);
    }

    /// The regression: a value-taking flag's value must travel with it,
    /// never fall through to positional. Before this, `mock bench run
    /// --seed 0xdead` sent the driver `--only 0xdead --seed`, reading the
    /// seed value as a bench-name filter and leaving `--seed` with
    /// nothing after it, so `seed_override` stayed `None` and the
    /// driver's own `replay with --seed {:#x}` instruction could not be
    /// followed through the tool.
    #[test]
    fn split_positional_and_flags_keeps_a_value_flags_value_attached() {
        let (pos, flags) = split_positional_and_flags(&["--seed", "0xdead"], &["--seed"]);
        assert!(pos.is_empty(), "the seed value must not be read as a positional name: {pos:?}");
        assert_eq!(flags, vec!["--seed", "0xdead"]);

        // A real bench name alongside it is unaffected.
        let (pos, flags) =
            split_positional_and_flags(&["warm-container", "--seed", "0xdead"], &["--seed"]);
        assert_eq!(pos, vec!["warm-container"]);
        assert_eq!(flags, vec!["--seed", "0xdead"]);
    }

    /// A value flag at the very end of argv, with nothing after it, must
    /// not panic and must not consume a token that does not exist.
    #[test]
    fn split_positional_and_flags_handles_a_dangling_value_flag() {
        let (pos, flags) = split_positional_and_flags(&["--seed"], &["--seed"]);
        assert!(pos.is_empty());
        assert_eq!(flags, vec!["--seed"]);
    }

    /// Everything after a bare `--` is a flag argument (libtest's own
    /// arguments), never positional, and never itself looked up in
    /// `value_flags`: a libtest argument that happened to share a value
    /// flag's name must not eat the token after it.
    #[test]
    fn split_positional_and_flags_treats_everything_after_the_separator_as_flags() {
        let (pos, flags) =
            split_positional_and_flags(&["warm-container", "--", "--seed", "extra"], &["--seed"]);
        assert_eq!(pos, vec!["warm-container"]);
        assert_eq!(flags, vec!["--", "--seed", "extra"]);
    }

    /// Runs `cmd_test` against a real consumer's bench tree rather than a
    /// synthetic fixture, per the same discipline `tests/real_trees.rs`
    /// states: a fixture built by the same hands as the code under test
    /// tends to avoid exactly the shape that breaks it. `MOCKSPACE_REAL_TREES`
    /// names a workspace root (e.g. `~/Dev/clause-dev`); without it this
    /// PANICS rather than silently passing, for the reason `real_trees.rs`
    /// gives: a skip that reads as a pass is how a gate stops being one.
    ///
    /// Scoped to one support crate rather than a whole tree, because a real
    /// tree the size of arvo's (94 arm crates plus 13 support crates, one of
    /// which is a multi-minute concurrency stress suite) is not what this
    /// test exists to time; `cmd_test`'s own walk-and-aggregate logic is
    /// exercised identically at any scale, and this is the smallest real
    /// input that exercises it against a support crate `mock bench test`
    /// was specifically built to reach and a bare `cargo test` at the bench
    /// root cannot.
    ///
    /// Run with:
    ///   MOCKSPACE_REAL_TREES=~/Dev/clause-dev cargo test --lib \
    ///     bench::tests::cmd_test_finds_and_runs_a_real_arvo_support_crate -- --ignored
    #[test]
    #[ignore]
    fn cmd_test_finds_and_runs_a_real_arvo_support_crate() {
        let workspace = std::env::var("MOCKSPACE_REAL_TREES")
            .expect("set MOCKSPACE_REAL_TREES=<path to the clause-dev workspace root>");
        let source = PathBuf::from(&workspace)
            .join("arvo/mock/benches/variants/warm-container-shared");
        assert!(
            source.join("src/lib.rs").is_file(),
            "expected {} to exist; is MOCKSPACE_REAL_TREES pointed at the right root?",
            source.display()
        );

        let root = temp_mock("cmd-test-real-support-crate");
        let bench_dir = root.join("benches");
        std::fs::create_dir_all(bench_dir.join("support/warm-container-shared")).unwrap();
        std::fs::write(bench_dir.join("bench.toml"), "").unwrap();
        // The root driver manifest, present in every real tree (the
        // generated driver or the consumer-owned escape hatch). Its
        // absence here previously hid the fact that the walk stopped
        // descending the moment it found a root manifest, since a fixture
        // with none never exercised that branch. Needs a real target
        // (write_minimal_driver_crate), not a name-only placeholder: a
        // manifest cargo cannot build breaks every sibling crate's build
        // too, since cargo walks up looking for a workspace root.
        write_minimal_driver_crate(&bench_dir);
        for entry in ["Cargo.toml", "src"] {
            copy_recursive(&source.join(entry), &bench_dir.join("support/warm-container-shared").join(entry));
        }

        let cfg = Config::from_dir(&root);
        let code = cmd_test(&cfg, &[]);
        assert_eq!(
            format!("{code:?}"),
            success(),
            "a real, previously-verified-passing crate's tests should make `mock bench test` succeed"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    fn copy_recursive(from: &Path, to: &Path) {
        if from.is_dir() {
            std::fs::create_dir_all(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap().flatten() {
                let name = entry.file_name();
                if name == "target" {
                    continue;
                }
                copy_recursive(&entry.path(), &to.join(name));
            }
        } else {
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(from, to).unwrap();
        }
    }

    fn temp_mock(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "mockspace-bench-cmd-test-{}-{name}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A root driver `Cargo.toml` that actually builds, for fixtures whose
    /// `cmd_test` call really invokes `cargo test`. A `[package]`-only
    /// manifest with no `[lib]`/`[[bin]]` and no source file parses fine on
    /// its own but makes `cargo` refuse it outright the moment anything
    /// tries to build it ("no targets specified in the manifest"), and
    /// because cargo walks up looking for a workspace root when it builds
    /// a *sibling* crate too, that refusal is not contained to the root:
    /// every crate under `bench_dir` fails to build until the root has a
    /// real target. No `[workspace]` header, matching arvo's actual root
    /// manifest exactly (`arvo/mock/benches/Cargo.toml` has none); adding
    /// one here would make this directory try to claim its own
    /// subdirectories as workspace members and refuse them for not being
    /// declared, which is the "[workspace] header trap" the ergonomics
    /// survey names and precisely the opposite failure from the one this
    /// helper exists to avoid.
    fn write_minimal_driver_crate(bench_dir: &Path) {
        std::fs::create_dir_all(bench_dir.join("src")).unwrap();
        let manifest = "[package]\nname = \"driver\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[[bin]]\nname = \"driver\"\npath = \"src/main.rs\"\n";
        std::fs::write(bench_dir.join("Cargo.toml"), manifest).unwrap();
        std::fs::write(bench_dir.join("src/main.rs"), "fn main() {}\n").unwrap();
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

    /// A flag reaching `cmd_add` must be refused, not read as a name.
    /// `name_ok` permits `-`, which real arm names need, and the
    /// two-argument positional match cannot otherwise tell a flag from a
    /// name: before this was refused, `mock bench add newb --force`
    /// scaffolded `newb/arms/--force/` and wrote `arms = ["--force"]`
    /// into `newb/bench.toml`, exit 0, and `mock bench run newb`
    /// subsequently ran an arm literally named `--force`.
    #[test]
    fn add_refuses_a_flag_instead_of_scaffolding_it_as_a_name() {
        let mock = temp_mock("add-flag-refused");
        let cfg = Config::from_dir(&mock);
        assert_eq!(format!("{:?}", cmd_init(&cfg)), success());
        let bench_dir = mock.join("benches");

        let code = cmd_add(&cfg, &["newb", "--force"]);
        assert_eq!(
            format!("{code:?}"),
            format!("{:?}", ExitCode::FAILURE),
            "a flag must be refused rather than accepted as an arm name"
        );
        assert!(
            !bench_dir.join("newb").exists(),
            "nothing should have been scaffolded once the flag was refused"
        );

        // The same refusal fires wherever the flag sits, not only last.
        let code = cmd_add(&cfg, &["--force", "newb", "kernel"]);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::FAILURE));
        assert!(!bench_dir.join("newb").exists());

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

    /// The env value is derived from the flags actually passed, so
    /// an override that changes the flags changes the record too.
    #[test]
    fn the_profile_env_value_mirrors_the_flags() {
        let flags = profile_args_for(None);
        assert_eq!(
            profile_env_value(flags.iter().map(String::as_str)),
            "opt-level=3,lto=\"fat\",codegen-units=1"
        );
        let overridden = profile_args_for(Some(&BuildSection {
            mockspace:     None,
            opt_level:     Some(0),
            lto:           Some("off".into()),
            codegen_units: Some(16),
        }));
        assert_eq!(
            profile_env_value(overridden.iter().map(String::as_str)),
            "opt-level=0,lto=\"off\",codegen-units=16"
        );
    }

    /// The profile must reach the argv of every build. Dropping the
    /// profile from the builder turns this red, which an earlier
    /// version could not do: it asserted the constant against itself
    /// and never reached a command.
    #[test]
    fn the_build_argv_carries_the_profile_and_the_json_format() {
        let argv: Vec<String> = build_argv(Path::new("/x/Cargo.toml"), &profile_args_for(None))
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

    /// The summary names the profile it ran under, because a bare test count
    /// is not interpretable without one: the same thirty tests take 133.72s
    /// and 4.99s on one host under the two profiles, and a panel in this
    /// workspace retired a true timing on exactly that ambiguity.
    #[test]
    fn the_summary_names_the_profile_it_ran_under() {
        assert!(describe_cargo_profile(&[]).contains("debug"));
        assert!(describe_cargo_profile(&["--release"]).contains("release"));
        assert!(describe_cargo_profile(&["--profile", "bench"]).contains("bench"));
        assert!(describe_cargo_profile(&["--profile=bench"]).contains("bench"));
        // `-r` is cargo's own short form of `--release` and was missing
        // from the earlier version of this function; `mock bench test -r`
        // ran release and reported "profile: debug (cargo's default)".
        assert!(describe_cargo_profile(&["-r"]).contains("release"));
    }

    /// The control for the test above. A description that says "debug" for
    /// everything would pass every assertion in it, so the default must NOT
    /// claim release and the release form must NOT claim debug.
    #[test]
    fn the_profile_description_does_not_name_the_profile_that_was_not_used() {
        assert!(!describe_cargo_profile(&[]).contains("release"));
        let rel = describe_cargo_profile(&["--release"]);
        assert!(
            !rel.contains("debug"),
            "the release description must not mention debug: {rel}"
        );
    }

    /// A flag after a bare `--` belongs to the test binary (libtest), never
    /// to cargo, and must not be read as a cargo profile flag. Before this
    /// was fixed, `mock bench test -- --release` (release meant for
    /// libtest, which has no such flag) reported "profile: release" for a
    /// run that was actually debug, because the description scanned the
    /// whole argv rather than stopping at the separator.
    #[test]
    fn a_flag_after_the_separator_does_not_change_the_reported_profile() {
        let desc = describe_cargo_profile(&["--", "--release"]);
        assert!(!desc.contains("release"), "the separator must gate --release: {desc}");
        assert!(desc.contains("debug"), "no real cargo profile flag was given: {desc}");

        // A real cargo flag before the separator still takes effect, and a
        // libtest flag after it that happens to share `--release`'s name
        // does not double-count or otherwise confuse the description.
        let desc = describe_cargo_profile(&["--release", "--", "--release"]);
        assert!(desc.contains("release"));
        assert!(!desc.contains("debug"), "{desc}");
    }

    /// A `[build]` override in a consumer-owned-driver tree reaches the
    /// build argv.
    ///
    /// The regression: `build_argv` took no config and passed a constant,
    /// and `profile_args_for` had one caller, in `run_generated`. So the
    /// generated path honoured `[build]` and the consumer-owned path
    /// silently ignored it. `BuildSection`'s own doc promises the values
    /// travel "where a manifest cannot silently drop them"; on one of the
    /// two paths they were dropped before they got there.
    #[test]
    fn a_build_override_reaches_the_argv_on_the_consumer_driver_path() {
        let dir = std::env::temp_dir().join(format!("ms-build-ovr-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(
            dir.join("bench.toml"),
            "[build]\nopt-level = 0\nlto = \"off\"\ncodegen-units = 16\n",
        )
        .expect("writing bench.toml");
        let profile = consumer_tree_profile(&dir);
        let argv: Vec<String> = build_argv(Path::new("/x/Cargo.toml"), &profile)
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            argv.contains(&"profile.release.opt-level=0".to_string()),
            "the declared override must reach the build: {argv:?}"
        );
        assert!(
            !argv.contains(&"profile.release.opt-level=3".to_string()),
            "the framework default must not survive alongside it: {argv:?}"
        );
        // And the record the driver receives must agree with what was built,
        // or the artifact names a profile that was not used.
        assert_eq!(
            profile_env_value(profile.iter().map(String::as_str)),
            "opt-level=0,lto=\"off\",codegen-units=16"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The control: with no `bench.toml` the framework defaults apply, so
    /// the test above is detecting the override rather than detecting that
    /// `consumer_tree_profile` returns something.
    #[test]
    fn a_tree_with_no_bench_toml_gets_the_framework_defaults() {
        let dir = std::env::temp_dir().join(format!("ms-build-def-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::create_dir_all(&dir);
        let profile = consumer_tree_profile(&dir);
        assert_eq!(
            profile_env_value(profile.iter().map(String::as_str)),
            "opt-level=3,lto=\"fat\",codegen-units=1"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The control for the test above: a value that is not passed
    /// must not be reported as present, or the assertion is vacuous.
    #[test]
    fn the_build_argv_does_not_carry_a_profile_setting_we_never_pass() {
        let argv: Vec<String> = build_argv(Path::new("/x/Cargo.toml"), &profile_args_for(None))
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
