#![allow(unused_imports)]
use super::*;

/// The value following `flag` in `args`, if present.
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

pub(crate) fn run_inner(
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

    // Launcher-era startup parity: keep the durable gate installed and active
    // with no user involvement, replacing what the build.rs bootstrap did.
    // Skip inside hook validation (`--lint-only`) and for the explicit
    // activate/deactivate commands, which manage the gate themselves.
    if !args.iter().any(|a| a == "--lint-only")
        && !args.iter().any(|a| a == "activate" || a == "deactivate")
    {
        bootstrap::ensure_gate(&cfg.repo_root, &cfg.mock_dir);
    }

    // Effective custom lints. The old proxy statically links them in and passes
    // them here; a launcher-run engine instead gets the pin-matched
    // `mockspace-lint-rules` dep via `--mockspace-lint-rules-dep` and loads this
    // repo's lints from a runtime cdylib. `loaded` holds the library so the
    // boxed lints' vtables outlive every use below; the shadowing binds the
    // effective slices for the rest of the function.
    let loaded = if custom_lints.is_empty() && custom_cross_lints.is_empty() {
        arg_value(&args, "--mockspace-lint-rules-dep").and_then(|dep| {
            match crate::custom_lints::load(&cfg, &cfg.config_path, &dep) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("mock: custom lints unavailable: {e}");
                    None
                }
            }
        })
    } else {
        None
    };
    let (custom_lints, custom_cross_lints) = match &loaded {
        Some(l) => (l.lints.as_slice(), l.cross.as_slice()),
        None => (custom_lints, custom_cross_lints),
    };

    // Subcommands: positional args that aren't flags or value-flag values.
    // `--dir` and `--scope` both take a value; their values must not be read
    // as a subcommand (a `--scope arvo` from a hook would otherwise look like
    // an unknown subcommand `arvo`).
    let positional_args: Vec<&str> = {
        let mut result = Vec::new();
        let mut skip_next = false;
        for arg in args.iter().skip(1) {
            if skip_next {
                skip_next = false;
                continue;
            }
            if arg == "--dir" || arg == "--scope" || arg == "--mockspace-lint-rules-dep" {
                skip_next = true; // skip the value that follows
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
            "query" => {
                // The argument after the subcommand, not a fixed position:
                // `--dir` and friends may precede it.
                let expr = args
                    .iter()
                    .skip_while(|a| a.as_str() != "query")
                    .nth(1)
                    .map(String::as_str)
                    .unwrap_or("");
                return registry::cmd_query(&cfg, expr);
            }
            "check" => {
                return cmd_check(&cfg);
            }
            "clean" => {
                return cmd_clean(&cfg);
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
            other => {
                // An unrecognised first positional is a mistyped subcommand,
                // not a reason to silently run the default full regeneration
                // (slow, and not what was asked). Report and exit non-zero.
                eprintln!("error: unknown subcommand `{other}`");
                if let Some(guess) = suggest_subcommand(other) {
                    eprintln!("  did you mean `{guess}`?");
                }
                eprintln!("\navailable subcommands:");
                for name in KNOWN_SUBCOMMANDS {
                    eprintln!("  {name}");
                }
                eprintln!(
                    "\n(run `cargo mock` with no subcommand to regenerate docs and agent rules)"
                );
                return ExitCode::from(2);
            }
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

    // Keep the locked mockspace current with its branch's remote head, but only
    // in an interactive `cargo mock` (not a git hook, where it must not mutate
    // Cargo.lock mid-commit, and not a build script). `--lint-only` marks the
    // hook invocations. Runs before the health check so a freshly-advanced lock
    // is what the proxy pin then tracks.
    let mut remote_actions = Vec::new();
    if !lint_only {
        bootstrap::ensure_mockspace_current(
            &cfg.repo_root,
            &cfg.mock_dir,
            true,
            &mut remote_actions,
        );
    }
    for action in &remote_actions {
        eprintln!("--- bootstrap: {action} ---");
    }

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

    if is_infra_only || workspace_nuked || lint_only {
        eprintln!(
            "--- cargo check skipped ({}) ---",
            if workspace_nuked {
                "nuked"
            } else if lint_only {
                "lint-only mode"
            } else {
                "infra-only"
            }
        );
        // Side effect of the --lint-only skip: the build script that
        // refreshes target/mockspace-proxy/Cargo.toml runs only when
        // cargo check actually executes. Skipping check here means
        // proxy refresh does NOT happen on --lint-only invocations.
        // For the pre-commit hook (the main consumer of --lint-only),
        // this is acceptable: developers run `cargo mock` without
        // --lint-only at least once per dev cycle (cargo build, cargo
        // run, full mock CI), and that invocation picks up any drift.
        // If a contributor's only mockspace invocation is the pre-
        // commit hook, they may run stale proxy logic until they
        // trigger a non-lint-only mockspace path. Acceptable today;
        // revisit if pre-commit-only-flow becomes a real complaint.
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

    // Several crates and not one edge between them is far more often a parse
    // failure than a real architecture. It reads as neither, because nothing
    // errors: layering, document ordering and the structure graph all keep
    // working and agree on the same wrong answer, that every crate sits at one
    // level. Saying so costs a line and turns a silent wrong answer into a
    // question.
    if crates.len() > 1 && crates.values().all(|c| c.deps.is_empty()) {
        eprintln!(
            "  note: {} crates, no dependency edges between any of them. If they do depend on \
             each other, their manifests are not being read: check that the dependency lines sit \
             under a [dependencies] table and start with the crate prefix `{}`.",
            crates.len(),
            cfg.crate_prefix
        );
    }

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
    // A repo generating for the first time has no docs/ yet, and every write
    // below targets it. Create it up front rather than panicking on the first
    // one.
    render_design::ensure_docs_dir(&cfg);

    // The placeholder vocabulary is identical for every template in a run and
    // some of it scans the mock tree, so compute it once here.
    let placeholders = render_design::Placeholders::compute(&crates, &cfg);
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

    let dot_path = cfg.docs_dir.join(render_design::ordered_doc_name("STRUCTURE.GRAPH.dot", &cfg));
    render_design::write_generated(&dot_path, &dot);
    eprintln!("  {}", dot_path.display());

    // Generate PNG and SVG from DOT
    for (ext, extra) in [("png", vec!["-Gdpi=150"]), ("svg", vec![])] {
        let out = cfg
            .docs_dir
            .join(render_design::ordered_doc_name(&format!("STRUCTURE.GRAPH.{ext}"), &cfg));
        // `dot -o` renders straight to the final path, so snapshot the previous
        // version before it is clobbered; otherwise the regeneration has nothing
        // to be compared against and every run rewrites the timestamp.
        let previous = fs::read_to_string(&out).ok();
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
                        render_design::write_generated_vs(&out, &commented, previous.as_deref());
                    }
                }
            }
            Ok(_) => eprintln!("  dot failed for {ext} (is graphviz installed?)"),
            Err(e) => eprintln!("  dot not found for {ext}: {e}"),
        }
    }


    // --- Bench results documentation (opt in via bench.toml [docgen]) ---
    crate::bench_docs::generate(&cfg);

    // STRUCTURE.md is markdown, so it goes through the same pipeline as every
    // other document rather than being written here. Computed now because it
    // reads the crate graph; rendered below with the rest.
    let structure_md = render_md::generate_structure_md(&crates, &cfg);

    // --- Combined deep-dive document (opt in) ---
    // Each deep dive already renders beside its crate's overview below, which
    // is where a reader looks for it. The combined file repeats that content
    // and grows without bound, so a project asks for it rather than gets it.
    let mut deep_dive_index: Option<String> = None;
    if cfg.deep_dive_index {
        eprintln!("--- generating DESIGN-DEEP-DIVES.md ---");
        // Planned like every other document rather than written here, so it
        // gets the placeholders, the references, and the naming the rest get.
        let deep_dives = render_design::generate_deep_dives_md(&cfg);
        if !deep_dives.is_empty() {
            deep_dive_index = Some(deep_dives);
        }
    }

    // --- Registry ---
    // Loaded before the passthrough templates because they resolve references
    // against it. A project declaring no namespaces gets an empty registry and
    // every step below is a no-op.
    let registry = registry::load_registry(&cfg.mock_dir, &cfg.registry_namespaces);

    // The index is built before ANY reference resolves, including the ones
    // held inside row data. Resolving data against an empty index was the
    // original bug wearing a different hat: a row referencing a crate or
    // another row got a link built by the fallbacks, which name `LAW.md` where
    // the file is `902_LAW.md`.
    //
    // Nothing here needs resolved data: planning reads filenames and namespace
    // keys, so the order has no cycle in it.
    let mut plan = document::plan(&cfg, &crates);
    plan.push(document::Planned::computed(
        document::DocId::root("STRUCTURE.md", &cfg),
        structure_md,
    ));
    if let Some(body) = deep_dive_index {
        plan.push(document::Planned::computed(
            document::DocId::root("DESIGN-DEEP-DIVES.md", &cfg),
            body,
        ));
    }
    document::plan_registry_pages(
        &mut plan,
        &cfg.registry_namespaces,
        &registry,
        &cfg.mock_dir,
        &cfg,
    );
    let mut cfg = cfg;
    cfg.doc_index = document::DocIndex::build(&plan, &cfg);
    let cfg = cfg;

    // Settle references held inside the data before any document renders, so
    // every consumer of a row sees final values rather than templates.
    let (registry, cycle_findings) = registry::resolve_data(
        &cfg.registry_namespaces,
        &registry,
        &cfg.registry_roots,
        &cfg.repo_root,
        &cfg.docs_dir,
        &cfg,
    );

    if !cfg.registry_namespaces.is_empty() {
        eprintln!("--- registry ---");
        let schemas = registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
        if schemas > 0 {
            eprintln!("  generated {schemas} schema files");
        }
        eprintln!("  {} rows across {} namespaces", registry.rows.len(), registry.by_namespace.len());
        for f in &cycle_findings {
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        for f in registry::validate_provenance(&cfg.repo_root, &cfg.registry_roots, &cfg.frozen_roots, &registry) {
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        for f in registry::namespace_root_collisions(&cfg.registry_namespaces, &cfg.registry_roots) {
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        for f in registry::validate(&cfg.registry_namespaces, &registry) {
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        match registry::check_schemas(&cfg.repo_root, &cfg.registry_namespaces) {
            registry::SchemaCheck::Ran { failures } if failures.is_empty() => {
                eprintln!("  schema check passed");
            }
            registry::SchemaCheck::Ran { failures } => {
                for f in failures {
                    eprintln!("  ERROR [schema]: {f}");
                }
            }
            registry::SchemaCheck::Unavailable => {
                eprintln!(
                    "  schema check SKIPPED: taplo is not installed. Row shape is unverified; install taplo to close this gap."
                );
            }
        }
    }

    // --- Every markdown document, through one pipeline ---
    //
    // Planned first, so the index of what will exist is complete before the
    // first reference resolves: a reference from a document written early to
    // one written later is ordinary rather than a special case. Then rendered,
    // in one loop, so a document cannot reach the output having skipped a step
    // because its path forgot one. Both were real, repeatedly.
    eprintln!("--- generating documents ---");
    let written = document::render_all(&plan, &placeholders, &registry, &cfg);
    eprintln!("  generated {} documents", written.len());

    // --- Unresolved references in what was just written ---
    // Scanned from the output rather than checked per path. Several paths
    // generate documents and each resolves references itself; two silently did
    // not, and the symptom was a literal reference in a finished document with
    // nothing saying so. Checking the result catches a path added later that
    // forgets the same thing.
    {
        let mut unresolved: Vec<(String, Vec<String>)> = Vec::new();
        if let Ok(entries) = fs::read_dir(&cfg.docs_dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .filter(|e| e.file_name().to_string_lossy().ends_with(".md"))
                .collect();
            files.sort_by_key(|e| e.file_name());
            for entry in files {
                let Ok(text) = fs::read_to_string(entry.path()) else {
                    continue;
                };
                let found = registry::unresolved_in_generated(&text);
                if !found.is_empty() {
                    unresolved.push((entry.file_name().to_string_lossy().to_string(), found));
                }
            }
        }
        if !unresolved.is_empty() {
            let total: usize = unresolved.iter().map(|(_, v)| v.len()).sum();
            eprintln!(
                "  warning: {total} unresolved reference(s) rendered literally in {} document(s):",
                unresolved.len()
            );
            for (file, found) in &unresolved {
                let mut shown: Vec<&str> = found.iter().map(String::as_str).collect();
                shown.sort_unstable();
                shown.dedup();
                shown.truncate(4);
                eprintln!("    {file}: {}", shown.join(", "));
            }
        }
    }

    // --- Agent rules and skills ---
    eprintln!("--- generating agent rules ---");
    let agent_count = render_agent::generate_agent_rules(&crates, &cfg, &registry);
    eprintln!("  generated {agent_count} agent files");

    ExitCode::SUCCESS
}


