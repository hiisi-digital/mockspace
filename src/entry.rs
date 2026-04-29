use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mockspace_lint_rules::{Lint, CrossCrateLint};

use crate::bench;
use crate::bootstrap;
use crate::config::Config;
use crate::design_round;
use crate::pdf;
use crate::dylib_check;
use crate::lint;
use crate::parse;
use crate::render;
use crate::render_agent;
use crate::render_design;
use crate::render_md;
use crate::LintMode;

pub fn run() -> ExitCode {
    run_inner(&[], &[])
}

pub fn run_with_custom_lints(
    custom_lints: Vec<Box<dyn Lint>>,
    custom_cross_lints: Vec<Box<dyn CrossCrateLint>>,
) -> ExitCode {
    run_inner(&custom_lints, &custom_cross_lints)
}

fn run_inner(
    custom_lints: &[Box<dyn Lint>],
    custom_cross_lints: &[Box<dyn CrossCrateLint>],
) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Determine mock directory:
    // 1. --dir <path> explicit override
    // 2. Search upward from cwd for mockspace.toml
    // 3. Fall back to cwd
    let mock_dir = if let Some(pos) = args.iter().position(|a| a == "--dir") {
        match args.get(pos + 1) {
            Some(p) => resolve_mock_dir(p),
            None => {
                eprintln!("error: --dir requires a path argument");
                return ExitCode::FAILURE;
            }
        }
    } else {
        match find_mockspace_root() {
            Some(dir) => dir,
            None => {
                let cwd = std::env::current_dir().unwrap();
                if cwd.join("crates").is_dir() {
                    cwd
                } else {
                    eprintln!("error: no mockspace.toml found. Run from a mockspace directory or use --dir <path>");
                    return ExitCode::FAILURE;
                }
            }
        }
    };

    let cfg = Config::from_dir(&mock_dir);

    // Subcommands: positional args that aren't flags or --dir values.
    let positional_args: Vec<&str> = {
        let mut result = Vec::new();
        let mut skip_next = false;
        for arg in args.iter().skip(1) {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "--dir" {
                skip_next = true; // skip the --dir value
                continue;
            }
            if arg.starts_with('-') {
                continue;
            }
            result.push(arg.as_str());
        }
        result
    };

    if let Some(&subcmd) = positional_args.first() {
        match subcmd {
            "activate" => {
                match bootstrap::activate(&cfg.repo_root, &cfg.mock_dir) {
                    Ok(()) => {
                        eprintln!("mockspace hooks activated (core.hooksPath set)");
                        eprintln!("  user hooks in .git/hooks/ will still run");
                        eprintln!("  deactivate with: cargo mock deactivate");
                        return ExitCode::SUCCESS;
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "deactivate" => {
                match bootstrap::deactivate(&cfg.repo_root) {
                    Ok(()) => {
                        eprintln!("mockspace hooks deactivated (core.hooksPath unset)");
                        eprintln!("  git will use .git/hooks/ directly");
                        return ExitCode::SUCCESS;
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            "status" => {
                if bootstrap::is_active(&cfg.repo_root) {
                    eprintln!("mockspace hooks: active");
                } else {
                    eprintln!("mockspace hooks: inactive");
                    eprintln!("  activate with: cargo mock activate");
                }
                return ExitCode::SUCCESS;
            }
            "check" => {
                return cmd_check(&cfg);
            }
            "pdf" => {
                // Forward all args that follow "pdf", dropping --dir <val>
                // (already consumed above to determine cfg).
                let mut extra: Vec<&str> = Vec::new();
                let mut found_pdf = false;
                let mut skip_next = false;
                for a in args.iter().skip(1) {
                    if skip_next { skip_next = false; continue; }
                    if a == "--dir" { skip_next = true; continue; }
                    if !found_pdf { if a == "pdf" { found_pdf = true; } continue; }
                    extra.push(a.as_str());
                }
                return pdf::cmd_pdf(&cfg.docs_dir, &cfg.repo_root, &extra);
            }
            "lock" | "deprecate" | "unlock" | "close" | "archive" | "migrate" => {
                let subcmd_opts = design_round::SubcmdOpts {
                    auto_commit: args.iter().any(|a| a == "--auto-commit"),
                };
                return match subcmd {
                    "lock" => design_round::cmd_lock(&cfg, &subcmd_opts),
                    "deprecate" => design_round::cmd_deprecate(&cfg, &subcmd_opts),
                    "unlock" => design_round::cmd_unlock(&cfg, &subcmd_opts),
                    "close" => design_round::cmd_close(&cfg, &subcmd_opts),
                    "archive" => design_round::cmd_archive(&cfg, &subcmd_opts),
                    "migrate" => design_round::cmd_migrate(&cfg, &subcmd_opts),
                    _ => unreachable!(),
                };
            }
            "bench" => {
                let bench_args: Vec<&str> = positional_args.iter().skip(1).copied().collect();
                return bench::cmd(&cfg, &bench_args);
            }
            _ => {} // Not a subcommand, continue to flags.
        }
    }

    // --nuke: wipe all mock crate source, leaving minimal lib.rs stubs.
    if args.iter().any(|a| a == "--nuke") {
        return nuke_mock_sources(&cfg);
    }

    let lint_only = args.iter().any(|a| a == "--lint-only");
    let doc_only = args.iter().any(|a| a == "--doc-only");
    let mode = if args.iter().any(|a| a == "--commit") {
        LintMode::Commit
    } else if args.iter().any(|a| a == "--strict") {
        LintMode::Push
    } else {
        LintMode::Build
    };

    // Health check: ensure alias and hooks are present and current.
    let mockspace_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bootstrap_actions = bootstrap::run(&cfg.repo_root, &cfg.mock_dir, &mockspace_dir);
    for action in &bootstrap_actions {
        eprintln!("--- bootstrap: {action} ---");
    }

    // --scope restricts linting to specific crates
    let scope_arg = args.iter()
        .position(|a| a == "--scope")
        .map(|i| {
            args.get(i + 1)
                .map(|s| s.as_str())
                .unwrap_or("")
        });

    let is_infra_only = scope_arg == Some("infra");

    // --- Detect nuked workspace ---
    let workspace_nuked = detect_nuked_workspace(&cfg);

    let doc_only = if workspace_nuked {
        eprintln!("--- nuked workspace detected: skipping source checks ---");
        true
    } else {
        doc_only
    };

    // Hash the proxy's Cargo.toml before cargo check. The build-script
    // bootstrap runs during cargo check and rewrites the proxy Cargo.toml
    // if the lockfile-resolved mockspace path differs from the one the
    // proxy currently references. If the hash changes, the running proxy
    // is stale (linked against an older mockspace) and re-executing via
    // `cargo mock` picks up the refreshed proxy (cargo rebuilds it on
    // the next `cargo run --manifest-path ...` invocation).
    let proxy_toml_path = cfg.repo_root.join("target/mockspace-proxy/Cargo.toml");
    let proxy_hash_before = fs::read_to_string(&proxy_toml_path)
        .map(|s| simple_hash(&s))
        .unwrap_or(0);

    if is_infra_only || workspace_nuked {
        eprintln!("--- cargo check skipped ({}) ---",
            if workspace_nuked { "nuked" } else { "infra-only" });
    } else {
        eprintln!("--- cargo check ---");
        // Strip inherited rustup env vars so the mock/ dir's own
        // rust-toolchain.toml wins. When cargo mock is launched from the
        // repo root, the outer cargo already resolved a toolchain (the
        // repo-root default, typically stable) and propagates
        // RUSTUP_TOOLCHAIN to children. That env var beats the file-based
        // override in mock/rust-toolchain.toml, so the inner check would
        // run with the outer toolchain. Removing these vars lets rustup
        // re-detect based on cwd (= mock/).
        let status = Command::new("cargo")
            .arg("check")
            .current_dir(&cfg.mock_dir)
            .env_remove("RUSTUP_TOOLCHAIN")
            .env_remove("RUSTC")
            .env_remove("RUSTDOC")
            .status()
            .expect("failed to run cargo check");

        if !status.success() {
            eprintln!("cargo check failed");
            return ExitCode::FAILURE;
        }
    }

    // If build-script bootstrap just regenerated the proxy Cargo.toml,
    // re-exec cargo mock so the updated mockspace actually runs. The
    // env-var guard prevents infinite loops if bootstrap is somehow
    // non-idempotent.
    let proxy_hash_after = fs::read_to_string(&proxy_toml_path)
        .map(|s| simple_hash(&s))
        .unwrap_or(0);
    if proxy_hash_before != proxy_hash_after
        && std::env::var("MOCKSPACE_REEXEC").is_err()
    {
        eprintln!(
            "--- proxy refreshed against updated mockspace; re-running cargo mock ---"
        );
        let forwarded: Vec<String> = std::env::args().skip(1).collect();
        let status = Command::new("cargo")
            .arg("mock")
            .args(&forwarded)
            .current_dir(&cfg.repo_root)
            .env("MOCKSPACE_REEXEC", "1")
            .status();
        return match status {
            Ok(s) if s.success() => ExitCode::SUCCESS,
            _ => ExitCode::FAILURE,
        };
    }

    eprintln!("--- parsing crates ---");
    let crates = parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);

    // --- Lints ---
    eprintln!("--- running lints ---");

    match scope_arg {
        Some("") => {
            eprintln!("error: --scope requires a value (use 'infra' for infrastructure-only commits)");
            return ExitCode::FAILURE;
        }
        Some("infra") => {
            eprintln!("  scope: infra (no crate lints)");
        }
        Some(crate_list) => {
            let names: Vec<String> = crate_list.split(',')
                .filter(|c| !c.is_empty())
                .map(String::from)
                .collect();
            if doc_only {
                eprintln!("  scoped to: {} (doc-only: source lints skipped)", names.join(", "));
            } else {
                eprintln!("  scoped to: {}", names.join(", "));
            }
            let violations = lint::run_lints(
                &crates, &cfg.crates_dir, mode, Some(&names), doc_only,
                &cfg.proc_macro_crates, cfg.lint_proc_macro_source, &cfg.crate_prefix,
                &cfg.lint_overrides, &cfg.primitive_introductions,
                custom_lints, custom_cross_lints,
            );
            if violations > 0 {
                eprintln!("lint check failed: {violations} violation(s)");
                return ExitCode::FAILURE;
            }
            eprintln!("  all lints passed");
        }
        None => {
            let violations = lint::run_lints(
                &crates, &cfg.crates_dir, mode, None, doc_only,
                &cfg.proc_macro_crates, cfg.lint_proc_macro_source, &cfg.crate_prefix,
                &cfg.lint_overrides, &cfg.primitive_introductions,
                custom_lints, custom_cross_lints,
            );
            if violations > 0 {
                eprintln!("lint check failed: {violations} violation(s)");
                return ExitCode::FAILURE;
            }
            eprintln!("  all lints passed");
        }
    }

    if lint_only {
        eprintln!("--- lint-only mode, skipping generation ---");
        return ExitCode::SUCCESS;
    }

    // --- Dylib module loading check ---
    if is_infra_only || workspace_nuked {
        eprintln!("--- dylib check skipped ({}) ---",
            if workspace_nuked { "nuked" } else { "infra-only" });
    } else if !cfg.module_crates.is_empty() {
        eprintln!("--- checking dylib modules ---");
        let build_status = Command::new("cargo")
            .args(["build", "--lib"])
            .current_dir(&cfg.mock_dir)
            .status()
            .expect("failed to run cargo build");

        if !build_status.success() {
            eprintln!("cargo build failed");
            return ExitCode::FAILURE;
        }

        let dylib_failures = dylib_check::check_module_dylibs(&cfg);
        if dylib_failures > 0 {
            eprintln!("dylib check failed: {dylib_failures} module(s) broken");
            return ExitCode::FAILURE;
        }
        eprintln!("  all dylib modules ok");
    }

    // --- Clean docs/ top-level files ---
    eprintln!("--- cleaning docs/ ---");
    if let Ok(entries) = fs::read_dir(&cfg.docs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let _ = fs::remove_file(&path);
            }
        }
    }
    eprintln!("  cleaned top-level files");

    // --- Dependency graph ---
    eprintln!("--- generating dependency graph ---");
    let dot_header = render_design::generation_header_dot(&cfg);
    let dot_body = render::generate_dot(&crates, &cfg);
    let dot = format!("{dot_header}{dot_body}");

    let dot_path = cfg.docs_dir.join("STRUCTURE.GRAPH.dot");
    fs::write(&dot_path, &dot).expect("failed to write dot file");
    eprintln!("  {}", dot_path.display());

    // Generate PNG and SVG from DOT
    for (ext, extra) in [("png", vec!["-Gdpi=150"]), ("svg", vec![])] {
        let out = cfg.docs_dir.join(format!("STRUCTURE.GRAPH.{ext}"));
        let mut cmd = Command::new("dot");
        cmd.arg(format!("-T{ext}"))
            .arg(&dot_path)
            .arg("-o")
            .arg(&out);
        for a in &extra {
            cmd.arg(a);
        }
        match cmd.status() {
            Ok(s) if s.success() => {
                eprintln!("  {}", out.display());
                if ext == "svg" {
                    if let Ok(svg_content) = fs::read_to_string(&out) {
                        let svg_header = render_design::generation_header_svg(&cfg);
                        let commented = format!("{svg_header}\n{svg_content}");
                        let _ = fs::write(&out, commented);
                    }
                }
            }
            Ok(_) => eprintln!("  dot failed for {ext} (is graphviz installed?)"),
            Err(e) => eprintln!("  dot not found for {ext}: {e}"),
        }
    }

    // Backward compat copies
    let _ = fs::copy(cfg.docs_dir.join("STRUCTURE.GRAPH.dot"), cfg.docs_dir.join("mock_crate_deps.dot"));
    let _ = fs::copy(cfg.docs_dir.join("STRUCTURE.GRAPH.png"), cfg.docs_dir.join("mock_crate_deps.png"));
    let _ = fs::copy(cfg.docs_dir.join("STRUCTURE.GRAPH.svg"), cfg.docs_dir.join("mock_crate_deps.svg"));

    // --- STRUCTURE.md ---
    eprintln!("--- generating STRUCTURE.md ---");
    let structure_header = render_design::generation_header_md(&cfg);
    let structure_body = render_md::generate_structure_md(&crates, &cfg);
    let structure_md = format!("{structure_header}\n{structure_body}");
    let structure_path = cfg.docs_dir.join("STRUCTURE.md");
    fs::write(&structure_path, &structure_md).expect("failed to write STRUCTURE.md");
    eprintln!("  {}", structure_path.display());

    // --- DESIGN.md ---
    eprintln!("--- generating DESIGN.md ---");
    match render_design::generate_design_md(&crates, &cfg) {
        Some(design_md) => {
            let design_path = cfg.docs_dir.join("DESIGN.md");
            fs::write(&design_path, &design_md).expect("failed to write DESIGN.md");
            eprintln!("  {}", design_path.display());
        }
        None => {
            eprintln!("  skipped (no DESIGN.md.tmpl found)");
        }
    }

    // --- DESIGN-DEEP-DIVES.md ---
    eprintln!("--- generating DESIGN-DEEP-DIVES.md ---");
    let deep_dives = render_design::generate_deep_dives_md(&cfg);
    if !deep_dives.is_empty() {
        let dd_path = cfg.docs_dir.join("DESIGN-DEEP-DIVES.md");
        fs::write(&dd_path, &deep_dives).expect("failed to write DESIGN-DEEP-DIVES.md");
        eprintln!("  {}", dd_path.display());
    }

    // --- Per-crate overview and deep dive files ---
    eprintln!("--- generating per-crate docs ---");
    render_design::generate_per_crate_docs(&cfg);

    // --- Passthrough templates ---
    eprintln!("--- copying passthrough templates ---");
    if let Ok(entries) = fs::read_dir(&cfg.mock_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".md.tmpl") && name != "DESIGN.md.tmpl" {
                let out_name = name.trim_end_matches(".tmpl");
                let header = render_design::generation_header_md(&cfg);
                let body = fs::read_to_string(&path).expect("failed to read template");
                let content = format!("{header}\n{body}");
                let out_path = cfg.docs_dir.join(out_name);
                fs::write(&out_path, &content).expect("failed to write passthrough doc");
                eprintln!("  {}", out_path.display());
            }
        }
    }

    // --- Agent rules and skills ---
    eprintln!("--- generating agent rules ---");
    let agent_count = render_agent::generate_agent_rules(&crates, &cfg);
    eprintln!("  generated {agent_count} agent files");

    ExitCode::SUCCESS
}


/// Check if every crate in the workspace has been nuked.
fn detect_nuked_workspace(cfg: &Config) -> bool {
    let entries = match fs::read_dir(&cfg.crates_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };

    let crate_dirs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();

    if crate_dirs.is_empty() {
        return false;
    }

    crate_dirs.iter().all(|entry| {
        let librs = entry.path().join("src/lib.rs");
        fs::read_to_string(&librs)
            .map(|s| s.contains(&cfg.nuke_marker))
            .unwrap_or(false)
    })
}

/// Wipe all mock crate source code, leaving minimal lib.rs stubs.
fn nuke_mock_sources(cfg: &Config) -> ExitCode {
    eprintln!("--- NUKE: wiping all mock crate source ---");
    eprintln!("    design docs and Cargo.toml files are preserved");
    eprintln!();

    let mut nuked_files = 0u32;
    let mut nuked_crates = 0u32;

    let mut entries: Vec<_> = fs::read_dir(&cfg.crates_dir)
        .expect("can't read crates dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let src_dir = entry.path().join("src");
        if !src_dir.exists() {
            continue;
        }

        let cargo_toml = entry.path().join("Cargo.toml");
        let is_proc_macro = fs::read_to_string(&cargo_toml)
            .map(|c| c.contains("proc-macro = true"))
            .unwrap_or(false);

        let deleted = delete_non_lib_rs(&src_dir);
        nuked_files += deleted;

        let lib_rs = src_dir.join("lib.rs");
        let stub = if is_proc_macro {
            format!(
                "//! {crate_name} — proc macro crate.\n\
                 //!\n\
                 //! {}. Rewrite from design docs (mechanical, no reinterpretation).\n\
                 \n\
                 extern crate proc_macro;\n",
                cfg.nuke_marker
            )
        } else {
            format!(
                "//! {crate_name} — nuked.\n\
                 //!\n\
                 //! {}. Rewrite from design docs (mechanical, no reinterpretation).\n",
                cfg.nuke_marker
            )
        };

        if lib_rs.exists() {
            nuked_files += 1;
        }
        fs::write(&lib_rs, &stub).expect("failed to write lib.rs stub");
        nuked_crates += 1;
        eprintln!("  nuked: {crate_name}");
    }

    eprintln!();
    eprintln!("--- NUKE complete: {nuked_files} files across {nuked_crates} crates ---");
    eprintln!("    cargo check will fail until source is rewritten from docs");
    ExitCode::SUCCESS
}

fn delete_non_lib_rs(dir: &Path) -> u32 {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += delete_all_rs(&path);
                let _ = fs::remove_dir(&path);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                let name = path.file_name().unwrap().to_string_lossy();
                if name != "lib.rs" {
                    let _ = fs::remove_file(&path);
                    count += 1;
                }
            }
        }
    }
    count
}

fn delete_all_rs(dir: &Path) -> u32 {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += delete_all_rs(&path);
                let _ = fs::remove_dir(&path);
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                let _ = fs::remove_file(&path);
                count += 1;
            }
        }
    }
    count
}

/// Resolve a `--dir` argument to an absolute path containing `mockspace.toml`.
///
/// Tries in order:
/// 1. Absolute path as-is (already absolute from bootstrap alias)
/// 2. Relative path from CWD (user running from repo root)
/// 3. Relative path from git repo root (user running from a subdirectory
///    with a stale relative-path alias)
///
/// Falls back to the raw path if nothing matches, so downstream code
/// can produce a clear "no mockspace.toml found" error.
/// djb2 hash for detecting proxy Cargo.toml changes across cargo check runs.
fn simple_hash(s: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    h
}

fn resolve_mock_dir(raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);

    // Absolute and exists — use directly.
    if path.is_absolute() && path.join("mockspace.toml").exists() {
        return path;
    }

    // Relative from CWD.
    if let Ok(canonical) = fs::canonicalize(&path) {
        if canonical.join("mockspace.toml").exists() {
            return canonical;
        }
    }

    // Relative from repo root (handles CWD != repo root with relative alias).
    if path.is_relative() {
        if let Some(root) = find_repo_root_from_cwd() {
            let from_root = root.join(&path);
            if from_root.join("mockspace.toml").exists() {
                return from_root;
            }
        }
    }

    // Nothing matched — return canonicalized or raw for a clear downstream error.
    fs::canonicalize(&path).unwrap_or(path)
}

/// Walk up from CWD looking for a `.git` directory (repo root).
fn find_repo_root_from_cwd() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Walk up from the current directory looking for mockspace.toml.
fn find_mockspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join("mockspace.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// `cargo mock check` — readiness report.
//
// Non-mutating. Answers one question: "can I advance this round right
// now, or is something blocking?" Reports git cleanliness, remote
// sync, cargo-check status, lint status, and the phase-specific
// lock/close permission.
// ──────────────────────────────────────────────────────────────────────

/// Outcome of one readiness probe. Pass / warn / fail.
#[derive(Debug, Copy, Clone, PartialEq)]
enum CheckResult {
    Pass,
    Warn,
    Fail,
}

impl CheckResult {
    fn icon(self) -> &'static str {
        match self {
            CheckResult::Pass => "✓",
            CheckResult::Warn => "!",
            CheckResult::Fail => "✗",
        }
    }
}

fn print_row(section: &str, result: CheckResult, msg: &str) {
    eprintln!("  {} {:<10} {}", result.icon(), section, msg);
}

fn cmd_check(cfg: &Config) -> ExitCode {
    use mockspace_lint_rules::changelist_helpers;

    eprintln!("--- mockspace readiness check ---");

    let mut any_fail = false;

    // --- git: working tree cleanliness ---
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&cfg.repo_root)
        .output();
    match dirty {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            let n = s.lines().filter(|l| !l.is_empty()).count();
            if n == 0 {
                print_row("git", CheckResult::Pass, "working tree clean");
            } else {
                print_row(
                    "git",
                    CheckResult::Warn,
                    &format!("{n} uncommitted change(s)"),
                );
            }
        }
        _ => {
            print_row("git", CheckResult::Warn, "not a git repo (or git failed)");
        }
    }

    // --- git: current branch + remote sync ---
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&cfg.repo_root)
        .output()
        .ok()
        .and_then(|o| if o.status.success() { Some(o) } else { None })
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "(unknown)".into());

    // Try fetch-free: compare HEAD against @{upstream}. If no upstream,
    // that's a warn (can't push without setting one).
    let upstream = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "@{upstream}"])
        .current_dir(&cfg.repo_root)
        .output();
    match upstream {
        Ok(out) if out.status.success() => {
            let up = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let counts = Command::new("git")
                .args(["rev-list", "--left-right", "--count", &format!("HEAD...{up}")])
                .current_dir(&cfg.repo_root)
                .output();
            match counts {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout);
                    let mut parts = s.split_whitespace();
                    let ahead: u32 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let behind: u32 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let (result, msg) = match (ahead, behind) {
                        (0, 0) => (CheckResult::Pass, format!("{branch} in sync with {up}")),
                        (a, 0) => (CheckResult::Warn, format!("{branch} {a} ahead of {up} — push needed")),
                        (0, b) => (CheckResult::Warn, format!("{branch} {b} behind {up} — pull needed")),
                        (a, b) => (CheckResult::Warn, format!("{branch} diverged from {up} ({a} ahead, {b} behind)")),
                    };
                    print_row("remote", result, &msg);
                }
                _ => {
                    print_row("remote", CheckResult::Warn, &format!("{branch}: could not compare against upstream"));
                }
            }
        }
        _ => {
            print_row("remote", CheckResult::Warn, &format!("{branch} has no upstream — `git push -u` first"));
        }
    }

    // --- phase detection ---
    let design_rounds = cfg.mock_dir.join("design_rounds");
    let phase = changelist_helpers::current_phase(&design_rounds);
    print_row("phase", CheckResult::Pass, phase.label());

    // --- cargo check ---
    let check_status = Command::new("cargo")
        .arg("check")
        .current_dir(&cfg.mock_dir)
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("RUSTC")
        .env_remove("RUSTDOC")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match check_status {
        Ok(s) if s.success() => print_row("build", CheckResult::Pass, "cargo check green"),
        Ok(_) => {
            print_row("build", CheckResult::Fail, "cargo check failed — run `cargo check` in mock/ for details");
            any_fail = true;
        }
        Err(e) => {
            print_row("build", CheckResult::Fail, &format!("could not run cargo check: {e}"));
            any_fail = true;
        }
    }

    // --- cargo test ---
    // Tests in this repo exercise the designed API surface. If the CL
    // promises functionality and tests assert it, missing impl fails here.
    let test_status = Command::new("cargo")
        .arg("test")
        .current_dir(&cfg.mock_dir)
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("RUSTC")
        .env_remove("RUSTDOC")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match test_status {
        Ok(s) if s.success() => print_row("tests", CheckResult::Pass, "cargo test green"),
        Ok(_) => {
            print_row("tests", CheckResult::Fail, "cargo test failed — run `cargo test` in mock/ for details");
            any_fail = true;
        }
        Err(e) => {
            print_row("tests", CheckResult::Fail, &format!("could not run cargo test: {e}"));
            any_fail = true;
        }
    }

    // --- mockspace lint pipeline (strict) ---
    // Delegates to `cargo mock --lint-only --strict` so the exact same
    // lint set that will run on push fires here. Strict mode is the
    // pre-push tier: any HARD_ERROR lint fails the check.
    let lint_status = Command::new("cargo")
        .args(["mock", "--lint-only", "--strict"])
        .current_dir(&cfg.repo_root)
        .env("MOCKSPACE_REEXEC", "1") // suppress proxy re-exec inside the child
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match lint_status {
        Ok(s) if s.success() => print_row("lints", CheckResult::Pass, "mockspace lint pipeline green (strict)"),
        Ok(_) => {
            print_row(
                "lints",
                CheckResult::Fail,
                "mockspace lints failed — run `cargo mock --lint-only --strict` for details",
            );
            any_fail = true;
        }
        Err(e) => {
            print_row("lints", CheckResult::Fail, &format!("could not run cargo mock lints: {e}"));
            any_fail = true;
        }
    }

    // --- phase-specific lock readiness ---
    use mockspace_lint_rules::changelist_helpers::Phase;
    match phase {
        Phase::Topic => {
            print_row(
                "advance",
                CheckResult::Pass,
                "author a topic + doc changelist to start DOC phase",
            );
        }
        Phase::Doc => {
            print_row(
                "advance",
                CheckResult::Pass,
                "`cargo mock lock` when doc edits done (DOC → SRC-PLAN)",
            );
        }
        Phase::SrcPlan => {
            print_row(
                "advance",
                CheckResult::Pass,
                "author a src changelist to enter SRC phase",
            );
        }
        Phase::Src => {
            let msg = if any_fail {
                "SRC impl in progress — build failing; fix before `cargo mock lock`"
            } else {
                "SRC impl ready for `cargo mock lock` (SRC → DONE) once CL is fulfilled"
            };
            print_row(
                "advance",
                if any_fail { CheckResult::Fail } else { CheckResult::Pass },
                msg,
            );
        }
        Phase::Done => {
            print_row(
                "advance",
                CheckResult::Pass,
                "round complete — `cargo mock close` to archive",
            );
        }
    }

    eprintln!();
    if any_fail {
        eprintln!("  verdict: NOT READY");
        eprintln!("  resolve the ✗ rows above before locking or closing.");
        ExitCode::FAILURE
    } else {
        eprintln!("  verdict: ready to proceed (see `advance` row for next step)");
        ExitCode::SUCCESS
    }
}
