mod bootstrap;
mod config;
mod design_round;
mod model;
mod parse;
mod graph;
mod lint;
mod dylib_check;
mod render;
mod render_md;
mod render_design;
mod render_agent;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use config::Config;
use mockspace_lint_rules::LintMode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();

    // Determine mock directory:
    // 1. --dir <path> explicit override
    // 2. Search upward from cwd for mockspace.toml
    // 3. Fall back to cwd
    let mock_dir = if let Some(pos) = args.iter().position(|a| a == "--dir") {
        args.get(pos + 1)
            .map(|p| resolve_mock_dir(p))
            .unwrap_or_else(|| {
                eprintln!("error: --dir requires a path argument");
                std::process::exit(1);
            })
    } else {
        find_mockspace_root().unwrap_or_else(|| {
            let cwd = std::env::current_dir().unwrap();
            if cwd.join("crates").is_dir() {
                cwd
            } else {
                eprintln!("error: no mockspace.toml found. Run from a mockspace directory or use --dir <path>");
                std::process::exit(1);
            }
        })
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
            "lock" | "deprecate" | "unlock" | "close" | "migrate" => {
                let subcmd_opts = design_round::SubcmdOpts {
                    auto_commit: args.iter().any(|a| a == "--auto-commit"),
                };
                return match subcmd {
                    "lock" => design_round::cmd_lock(&cfg, &subcmd_opts),
                    "deprecate" => design_round::cmd_deprecate(&cfg, &subcmd_opts),
                    "unlock" => design_round::cmd_unlock(&cfg, &subcmd_opts),
                    "close" => design_round::cmd_close(&cfg, &subcmd_opts),
                    "migrate" => design_round::cmd_migrate(&cfg, &subcmd_opts),
                    _ => unreachable!(),
                };
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

    if is_infra_only || workspace_nuked {
        eprintln!("--- cargo check skipped ({}) ---",
            if workspace_nuked { "nuked" } else { "infra-only" });
    } else {
        eprintln!("--- cargo check ---");
        let status = Command::new("cargo")
            .arg("check")
            .current_dir(&cfg.mock_dir)
            .status()
            .expect("failed to run cargo check");

        if !status.success() {
            eprintln!("cargo check failed");
            return ExitCode::FAILURE;
        }
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
            let violations = lint::run_lints(&crates, &cfg.crates_dir, mode, Some(&names), doc_only, &cfg.proc_macro_crates, &cfg.crate_prefix);
            if violations > 0 {
                eprintln!("lint check failed: {violations} violation(s)");
                return ExitCode::FAILURE;
            }
            eprintln!("  all lints passed");
        }
        None => {
            let violations = lint::run_lints(&crates, &cfg.crates_dir, mode, None, doc_only, &cfg.proc_macro_crates, &cfg.crate_prefix);
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
                 //! {}. Regenerate from design docs.\n\
                 \n\
                 extern crate proc_macro;\n",
                cfg.nuke_marker
            )
        } else {
            format!(
                "//! {crate_name} — nuked.\n\
                 //!\n\
                 //! {}. Regenerate from design docs.\n",
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
    eprintln!("    cargo check will fail until source is regenerated from docs");
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
