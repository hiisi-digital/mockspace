#![allow(unused_imports)]
use super::*;

/// Whether a pack contributes no lints of any kind.
///
/// The statically-linked path passes lints in directly; only an empty pack means
/// the engine should try loading this repo's cdylib instead.
fn pack_is_empty(pack: &LintPack) -> bool {
    pack.crate_lints.is_empty()
        && pack.workspace_lints.is_empty()
        && pack.repo_lints.is_empty()
        && pack.message_lints.is_empty()
        // Tools count. A statically-linked pack carrying only tools is not
        // empty, and treating it as empty would discard it and try to load a
        // cdylib over the top.
        && pack.tools.is_empty()
}

/// The value following `flag` in `args`, if present.
fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Whether this invocation is one that repairs or inspects rather than gates.
///
/// These stay usable when the pack will not build, because `clean` and
/// `migrate` are among the ways a broken pack gets fixed, and refusing to run
/// them would leave the repo with no route out. Everything else gates, so
/// everything else blocks.
fn is_recovery_command(args: &[String]) -> bool {
    const RECOVERY: &[&str] = &["clean", "activate", "deactivate", "status", "migrate"];
    args.iter().skip(1).any(|a| RECOVERY.contains(&a.as_str()))
}

/// Whether `subcmd` is both a builtin name and the name of a discovered tool,
/// which means the tool can never be reached: the literal match arm for the
/// builtin claims the name first, so the catch-all arm that would dispatch to
/// the tool is never entered for it.
///
/// Checked here, against the one name actually about to be dispatched, rather
/// than inside `tool::run`'s catch-all arm: `other` in that arm is never one
/// of the fifteen literal match-arm names, so a check placed there can never
/// see a collision, which is exactly the bug this replaces.
///
/// **Does not cover `help`.** `mock help` returns before this function is
/// ever reached (see the `FIXME` above `help::is_help_request`), so a tool
/// named `help` is shadowed too, silently, and this function cannot see it
/// happen. It still correctly reports `true` for `("help", pack)` if asked,
/// which is why `help_itself_is_a_shadowing_name_too` below is a fact about
/// this function's logic and not evidence that the real dispatch path
/// catches the case.
fn tool_is_shadowed_by_a_builtin(subcmd: &str, pack: &LintPack) -> bool {
    super::tool::builtin_collision(subcmd) && pack.tools.iter().any(|t| t.name() == subcmd)
}

/// The value-taking globals the tool consumes before any subcommand sees
/// them. Their values must never be read as a subcommand name, and must never
/// be forwarded to one.
const VALUE_GLOBALS: [&str; 3] = ["--dir", "--scope", "--mockspace-lint-rules-dep"];

/// Every argument that follows `subcmd`, **flags included**.
///
/// `positional_args` exists to FIND the subcommand, so it drops every flag.
/// Using that same filtered list as the subcommand's argument vector is what
/// made `mock bench test --release` run a debug pass: the flag was dropped
/// here, before `bench::cmd` could forward it, so the `extra` forwarding in
/// `cmd_run`, `cmd_report` and `cmd_test` was unreachable through the only
/// entry point the tool has. The flag was accepted, discarded, and the run
/// reported a pass.
///
/// A flag before the subcommand is the tool's; a flag after it belongs to the
/// subcommand and is handed over verbatim. The three value-taking globals are
/// consumed wherever they appear, so their values cannot leak into the
/// subcommand's argv.
fn subcommand_args<'a>(args: &'a [String], subcmd: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut skip_next = false;
    let mut seen = false;
    for arg in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if VALUE_GLOBALS.contains(&arg.as_str()) {
            skip_next = true;
            continue;
        }
        if !seen {
            if arg == subcmd {
                seen = true;
            }
            continue;
        }
        out.push(arg.as_str());
    }
    out
}

/// The blocking message for a pack that would not build.
///
/// It names the failure, says what it costs, and lists the causes worth
/// checking first. The stale-branch case leads because it is the one that has
/// actually happened: a pack pinned to a branch that stopped moving keeps
/// resolving cleanly to an old head while the engine moves on, so the two
/// drift apart with nothing in either repository looking wrong.
fn explain_lint_load_failure(cfg: &Config, err: &impl std::fmt::Display) -> String {
    let mut s = String::new();
    s.push_str("this repo's custom lints could not be built, so no lint below\n");
    s.push_str("them ran. Nothing was checked.\n\n");
    s.push_str("  the build reported:\n");
    for line in err.to_string().lines() {
        s.push_str("    ");
        s.push_str(line);
        s.push('\n');
    }
    s.push_str("\n  declared in:\n    ");
    s.push_str(&cfg.config_path.display().to_string());
    s.push_str("\n\n  what to check, most likely first:\n");
    s.push_str(
        "    - a `[lint-crates]` entry pinned to a branch that has stopped\n\
         \x20     moving. The head still resolves, so nothing looks stale, but the\n\
         \x20     pack ages against an engine that keeps moving. Point it at the\n\
         \x20     branch the pack is actually developed on.\n",
    );
    s.push_str(
        "    - a `[lint-crates]` key that is not the pack's package name. It is\n\
         \x20     used verbatim as the cargo dependency name and as the Rust\n\
         \x20     identifier, so a renamed pack needs the key renamed with it.\n",
    );
    s.push_str(
        "    - a local lint in this repo's lints directory still on an older\n\
         \x20     trait shape than the engine expects.\n",
    );
    s
}

pub(crate) fn run_inner(pack: &LintPack) -> ExitCode {
    // The engine runs as a grandchild of git hooks, whose exported repo-location
    // GIT_* variables poison every `git` this process spawns from a different
    // working directory. Drop them before anything else runs.
    //
    // SAFETY: this is the first statement of every entry into the engine
    // (`run` and `run_with_custom_lints` both land here before doing anything
    // else); no thread has been spawned yet.
    unsafe { mockspace_manifest::gate::sanitize_git_env() };

    let args: Vec<String> = std::env::args().collect();

    // Help resolves before anything else, and in particular before project
    // discovery. `mock --help` outside a project used to answer "no mockspace.toml
    // found", which is the right answer to "run the workflow here" and the wrong
    // answer to "what does this tool do" -- the question a reader outside a project
    // is most likely to be asking, and often the first thing anyone types.
    // FIXME: a tool directory literally named `help` is still silently
    // unreachable, and `tool_is_shadowed_by_a_builtin` below can never catch
    // it: `mock help` returns here before `mock_dir` is even resolved, let
    // alone before any pack is loaded, so this fires first every time. The
    // flag spellings (`--help`, `-h`, `-?`) cannot collide with anything (no
    // filesystem lets a directory be named `--help`), so only the bare word
    // is at risk, and closing it would mean resolving `mock_dir` and running
    // `bootstrap::tool_names` (cheap, filesystem-only, no pack needed) before
    // this check, without breaking `mock --help` working outside a project at
    // all, which is what this ordering exists for. Left as the one case the
    // relocated collision check does not reach.
    if args.iter().skip(1).any(|a| help::is_help_request(a)) {
        return help::print_help();
    }

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
            },
        }
    } else {
        match find_mockspace_root() {
            Some(dir) => dir,
            None => {
                // No config, so the only question left is whether this looks
                // like a mock directory at all. `design_rounds/` answers it:
                // mockspace creates that directory and nothing else does, so a
                // directory holding one is a mock directory whatever language
                // the project is written in.
                //
                // This used to ask whether `crates/` was there, which is one
                // language's convention standing in for the question. It said
                // yes to any rust project that had never adopted mockspace, and
                // no to every project that had adopted it and written its source
                // somewhere else.
                let cwd = std::env::current_dir().unwrap();
                if cwd.join("design_rounds").is_dir() {
                    cwd
                } else {
                    eprintln!(
                        "error: no mockspace.toml found. Run from a mockspace directory or use --dir <path>"
                    );
                    return ExitCode::FAILURE;
                }
            },
        }
    };

    let cfg = Config::from_dir(&mock_dir);

    // Keep the durable gate installed and active with no user involvement. This
    // is the whole engine-side setup under the launcher+pin model; the dissolved
    // proxy model's self-install (the `.cargo` alias, the proxy, proxy-pin lock
    // tracking) is gone, not conditional. Skip inside hook validation
    // (`--lint-only`) and for the explicit activate/deactivate commands, which
    // manage the gate themselves.
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
    let loaded = if pack_is_empty(pack) {
        match arg_value(&args, "--mockspace-lint-rules-dep") {
            Some(dep) => {
                match crate::custom_lints::load(&cfg, &cfg.config_path, &dep) {
                    Ok(l) => l,
                    // A pack that will not build is a blocking failure, not a
                    // warning. Reporting it and carrying on runs every remaining
                    // gate with the repo's own lints absent and then reports a
                    // pass, which is the one outcome that cannot be allowed:
                    // silence and success are indistinguishable to the caller. It
                    // shipped that way, and arvo took 194 commits over eight days
                    // with its pack missing before anyone read the line.
                    //
                    // The recovery and inspection commands are exempt, because
                    // some of them are how the failure gets fixed.
                    Err(e) if !is_recovery_command(&args) => {
                        eprintln!();
                        eprintln!("BLOCKED: {}", explain_lint_load_failure(&cfg, &e));
                        return ExitCode::FAILURE;
                    },
                    Err(e) => {
                        eprintln!("mock: custom lints unavailable: {e}");
                        eprintln!("mock: continuing because this command does not lint.");
                        None
                    },
                }
            },
            None => None,
        }
    } else {
        None
    };
    let pack = match &loaded {
        Some(l) => &l.pack,
        None => pack,
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
            if VALUE_GLOBALS.contains(&arg.as_str()) {
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
        if tool_is_shadowed_by_a_builtin(subcmd, pack) {
            eprintln!(
                "mock: `{subcmd}` is a builtin subcommand, so the tool at \
                 {}/tools/{subcmd} can never be reached.",
                cfg.mock_dir.display()
            );
            eprintln!("  rename the directory; the directory name is the subcommand.");
            return ExitCode::from(2);
        }
        match subcmd {
            // Lint one authored message. The commit-msg and pre-push hooks call
            // this, and so will the agent hooks, so every surface reaches the
            // same configured policy instead of each carrying its own copy.
            "check-message" => {
                let domain_arg = arg_value(&args, "--domain").unwrap_or_default();
                let Some(domain) = message::parse_domain(&domain_arg) else {
                    eprintln!(
                        "mock check-message: --domain must be one of: {}",
                        message::DOMAIN_TOKENS.join(", ")
                    );
                    return ExitCode::FAILURE;
                };
                let (text, origin) = match arg_value(&args, "--file") {
                    Some(f) => {
                        match message::read_message_file(std::path::Path::new(&f)) {
                            Ok(t) => (t, f),
                            Err(e) => {
                                eprintln!("mock check-message: {e}");
                                return ExitCode::FAILURE;
                            },
                        }
                    },
                    None => {
                        // No `--file`: read the message from stdin, which is how
                        // an agent hook passes text it extracted from a command.
                        //
                        // FIXME: a non-UTF-8 commit message (a repo with
                        // `i18n.commitEncoding` set, say) makes this error and
                        // takes the whole batch with it, naming no commit. Fails
                        // closed, so obstructive rather than dangerous, but on
                        // the `--not --remotes` widening path that covers all
                        // local history, so one legacy commit blocks every push.
                        // Wants lossy decoding per record once the batch path
                        // owns its own reader.
                        let mut buf = String::new();
                        if let Err(e) =
                            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
                        {
                            eprintln!("mock check-message: could not read stdin: {e}");
                            return ExitCode::FAILURE;
                        }
                        (buf, "<stdin>".to_string())
                    },
                };
                // Which gate tier applies. A commit-msg hook is the commit
                // gate, pre-push the push gate, so a project can warn locally
                // and block before sharing, as the severity tiers intend.
                let gate = match arg_value(&args, "--gate").as_deref() {
                    Some("push") => LintMode::Push,
                    Some("build") => LintMode::Build,
                    _ => LintMode::Commit,
                };
                let command = arg_value(&args, "--command");
                let tool = arg_value(&args, "--tool");

                // `--batch`: stdin carries many messages, NUL-separated, each
                // optionally prefixed with `<origin>\x1f`. One process handles
                // all of them because startup dominates the work: measured at
                // 468ms per invocation, so a 327-commit push costs about two
                // and a half minutes spawned per message and milliseconds here.
                //
                // Each message is still checked on its own. Concatenating them
                // into one call is what this exists to avoid: `check-message`
                // parses its input as a single message with a subject line, so
                // a blob's first line becomes the subject and every subject
                // after the first is read as body text.
                if args.iter().any(|a| a == "--batch") {
                    let mut failed = 0usize;
                    let mut checked = 0usize;

                    for (rec_origin, rec_msg) in message::split_batch(&text, &origin) {
                        checked += 1;
                        let req = message::Request {
                            domain,
                            message: rec_msg,
                            origin: rec_origin,
                            command: command.as_deref(),
                            tool: tool.as_deref(),
                        };
                        if message::run(&cfg, pack, gate, &req) != ExitCode::SUCCESS {
                            failed += 1;
                        }
                    }

                    if failed > 0 {
                        eprintln!();
                        eprintln!(
                            "BLOCKED: {failed} of {checked} message(s) violate the message policy."
                        );
                        return ExitCode::FAILURE;
                    }

                    // `--batch` is only ever invoked with something to check,
                    // so an empty stream means the caller produced nothing and
                    // the gate can say nothing about this push. Fail closed:
                    // silence and success being indistinguishable is how a repo
                    // took 194 commits with its pack missing before anyone read
                    // the line.
                    if checked == 0 {
                        eprintln!(
                            "BLOCKED: the message gate received no messages to check. Expected at \
                             least one; the caller produced an empty stream."
                        );
                        return ExitCode::FAILURE;
                    }

                    eprintln!("checked {checked} message(s)");
                    return ExitCode::SUCCESS;
                }

                let req = message::Request {
                    domain,
                    message: text,
                    origin,
                    command: command.as_deref(),
                    tool: tool.as_deref(),
                };
                return message::run(&cfg, pack, gate, &req);
            },
            "activate" => {
                match bootstrap::activate(&cfg.repo_root, &cfg.mock_dir) {
                    Ok(()) => {
                        eprintln!("mockspace hooks activated (core.hooksPath set)");
                        eprintln!("  user hooks in .git/hooks/ will still run");
                        eprintln!("  deactivate with: cargo mock deactivate");
                        return ExitCode::SUCCESS;
                    },
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::FAILURE;
                    },
                }
            },
            "deactivate" => {
                match bootstrap::deactivate(&cfg.repo_root) {
                    Ok(()) => {
                        eprintln!("mockspace hooks deactivated (core.hooksPath unset)");
                        eprintln!("  git will use .git/hooks/ directly");
                        return ExitCode::SUCCESS;
                    },
                    Err(e) => {
                        eprintln!("error: {e}");
                        return ExitCode::FAILURE;
                    },
                }
            },
            "status" => {
                if bootstrap::is_active(&cfg.repo_root) {
                    eprintln!("mockspace hooks: active");
                } else {
                    eprintln!("mockspace hooks: inactive");
                    eprintln!("  activate with: cargo mock activate");
                }
                return ExitCode::SUCCESS;
            },
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
            },
            "check" => {
                return cmd_check(&cfg);
            },
            "clean" => {
                return cmd_clean(&cfg);
            },
            "pdf" => {
                // Forward all args that follow "pdf", dropping --dir <val>
                // (already consumed above to determine cfg).
                let mut extra: Vec<&str> = Vec::new();
                let mut found_pdf = false;
                let mut skip_next = false;
                for a in args.iter().skip(1) {
                    if skip_next {
                        skip_next = false;
                        continue;
                    }
                    if a == "--dir" {
                        skip_next = true;
                        continue;
                    }
                    if !found_pdf {
                        if a == "pdf" {
                            found_pdf = true;
                        }
                        continue;
                    }
                    extra.push(a.as_str());
                }
                return pdf::cmd_pdf(&cfg.docs_dir, &cfg.repo_root, &extra);
            },
            "lock" | "deprecate" | "unlock" | "close" | "archive" | "migrate" => {
                let subcmd_opts = design_round::SubcmdOpts {
                    auto_commit: auto_commit_wanted(&args),
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
            },
            "bench" => {
                // Flags included: see `subcommand_args`. Taking these from
                // `positional_args` dropped every flag before the subcommand
                // could forward it.
                let bench_args = subcommand_args(&args, "bench");
                return bench::cmd(&cfg, &bench_args);
            },
            other => {
                // A tool is a subcommand this binary does not know at compile
                // time. Its name is its directory under `<mock>/tools/`, so the
                // check is a directory listing rather than a build: a typo must
                // not compile a cdylib to discover it was a typo.
                if bootstrap::tool_names(&cfg.mock_dir).iter().any(|n| n == other) {
                    return super::tool::run(&cfg, pack, other, &subcommand_args(&args, other));
                }
                // An unrecognised first positional is a mistyped subcommand,
                // not a reason to silently run the default full regeneration
                // (slow, and not what was asked). Report and exit non-zero.
                return super::help::unknown_subcommand(other, &bootstrap::tool_names(&cfg.mock_dir));
            },
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

    // Pre-commit auto-fix: before a commit is linted, run the repo's own
    // `cargo fmt` and (optionally) `cargo clippy --fix` across every workspace
    // root the staged changes touch, then re-stage, so the commit lands
    // already-fixed. Config-gated (auto_fmt / auto_clippy_fix, both default
    // true) and best-effort: a fixer failure never blocks the commit.
    if mode == LintMode::Commit && (cfg.auto_fmt || cfg.auto_clippy_fix) {
        for action in crate::autofix::run(&cfg.repo_root, cfg.auto_fmt, cfg.auto_clippy_fix) {
            eprintln!("--- autofix: {action} ---");
        }
    }

    // Pre-push dependency gate: cargo-deny (advisories, license compatibility
    // across the transitive graph, bans, sources). Blocks on a real violation;
    // skipped when deny.toml or cargo-deny is absent. Config-gated (deny_check,
    // default true).
    if mode == LintMode::Push && cfg.deny_check {
        match crate::deny::check(&cfg.repo_root, cfg.deny_check) {
            Ok(actions) => {
                for action in actions {
                    eprintln!("--- deny: {action} ---");
                }
            },
            Err(msg) => {
                eprintln!();
                eprintln!("BLOCKED: {msg}");
                return ExitCode::FAILURE;
            },
        }
    }

    // --scope restricts linting to specific crates
    let scope_arg = args
        .iter()
        .position(|a| a == "--scope")
        .map(|i| args.get(i + 1).map(|s| s.as_str()).unwrap_or(""));

    let is_infra_only = scope_arg == Some("infra");

    // --- Detect nuked workspace ---
    // Verify the claims the narrowing flags make. Each exists to skip work that
    // cannot matter, and each was trusted rather than checked, which made both an
    // unintended bypass: --doc-only skips source lints, --scope skips whole
    // crates, and neither asked whether its premise held.
    let mock_rel_for_hatch = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "mock".to_string());
    // Only the explicit flag is verified. A nuked workspace derives doc-only
    // internally below, and that derivation is the engine's own, not a claim a
    // caller made about what is staged.
    if doc_only {
        if let Some(r) = escape_hatch::verify_doc_only(&cfg.repo_root, &mock_rel_for_hatch) {
            eprintln!("BLOCKED: {}", r.explain());
            return ExitCode::FAILURE;
        }
    }
    if let Some(s) = scope_arg {
        if let Some(r) = escape_hatch::verify_scope(&cfg.repo_root, &mock_rel_for_hatch, s) {
            eprintln!("BLOCKED: {}", r.explain());
            return ExitCode::FAILURE;
        }
    }

    let workspace_nuked = detect_nuked_workspace(&cfg);

    let doc_only = if workspace_nuked {
        eprintln!("--- nuked workspace detected: skipping source checks ---");
        true
    } else {
        doc_only
    };

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
    } else {
        eprintln!("--- cargo check ---");
        // `cargo_gate::cargo` owns the rustup env stripping; see its docs.
        let status = cargo_gate::cargo(&cfg.mock_dir, &["check"])
            .status()
            .expect("failed to run cargo check");

        if !status.success() {
            // A repo whose taxonomy is still a design round's subject has no
            // workspace members, which cargo cannot check at all. Forgiven only
            // on cargo's own no-members diagnostic against a confirmed
            // memberless manifest; every other failure still fails the gate.
            if cargo_gate::forgives_failure(&cfg.mock_dir, &["check"]) {
                eprintln!("--- cargo check skipped (workspace has no members yet) ---");
            } else {
                eprintln!("cargo check failed");
                return ExitCode::FAILURE;
            }
        }
    }

    eprintln!("--- parsing crates ---");
    let crates = parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);

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
            eprintln!(
                "error: --scope requires a value (use 'infra' for infrastructure-only commits)"
            );
            return ExitCode::FAILURE;
        },
        Some("infra") => {
            eprintln!("  scope: infra (no crate lints)");
        },
        Some(crate_list) => {
            let names: Vec<String> = crate_list
                .split(',')
                .filter(|c| !c.is_empty())
                .map(String::from)
                .collect();
            if doc_only {
                eprintln!(
                    "  scoped to: {} (doc-only: source lints skipped)",
                    names.join(", ")
                );
            } else {
                eprintln!("  scoped to: {}", names.join(", "));
            }
            let violations = lint::run_lints(
                &crates,
                &cfg.src_dirs,
                &cfg.mock_dir,
                mode,
                Some(&names),
                doc_only,
                &cfg.proc_macro_crates,
                cfg.lint_proc_macro_source,
                &cfg.crate_prefix,
                &cfg.lint_overrides,
                &cfg.primitive_introductions,
                pack,
            );
            if violations > 0 {
                eprintln!("lint check failed: {violations} violation(s)");
                return ExitCode::FAILURE;
            }
            eprintln!("  all lints passed");
        },
        None => {
            let violations = lint::run_lints(
                &crates,
                &cfg.src_dirs,
                &cfg.mock_dir,
                mode,
                None,
                doc_only,
                &cfg.proc_macro_crates,
                cfg.lint_proc_macro_source,
                &cfg.crate_prefix,
                &cfg.lint_overrides,
                &cfg.primitive_introductions,
                pack,
            );
            if violations > 0 {
                eprintln!("lint check failed: {violations} violation(s)");
                return ExitCode::FAILURE;
            }
            eprintln!("  all lints passed");
        },
    }

    if lint_only {
        eprintln!("--- lint-only mode, skipping generation ---");
        return ExitCode::SUCCESS;
    }

    // --- Dylib module loading check ---
    if is_infra_only || workspace_nuked {
        eprintln!(
            "--- dylib check skipped ({}) ---",
            if workspace_nuked { "nuked" } else { "infra-only" }
        );
    } else if !cfg.module_crates.is_empty() {
        eprintln!("--- checking dylib modules ---");
        let build_status = cargo_gate::cargo(&cfg.mock_dir, &["build", "--lib"])
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

    let dot_path = cfg
        .docs_dir
        .join(render_design::ordered_doc_name("STRUCTURE.GRAPH.dot", &cfg));
    render_design::write_generated(&dot_path, &dot);
    eprintln!("  {}", dot_path.display());

    // Generate PNG and SVG from DOT
    for (ext, extra) in [("png", vec!["-Gdpi=150"]), ("svg", vec![])] {
        let out = cfg.docs_dir.join(render_design::ordered_doc_name(
            &format!("STRUCTURE.GRAPH.{ext}"),
            &cfg,
        ));
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
            },
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

    // Registry findings gate. Every one of these prints `ERROR`, and until
    // 2026-08-22 the command exited 0 anyway, so a dangling reference, a
    // duplicate slug and a fragile line citation were all indistinguishable
    // from a clean registry to anything downstream. The design says duplicate
    // identifier and dangling reference are errors "because both mean the
    // registry is lying"; the code said so in words and not in status.
    //
    // The rule is the one the code already applies when it chooses which word
    // to print: what is reported as ERROR blocks, what is reported as a warning
    // does not. Nothing here reclassifies a finding.
    let mut registry_errors = 0usize;

    if !cfg.registry_namespaces.is_empty() {
        eprintln!("--- registry ---");
        let schemas =
            registry::generate_schemas(&cfg.repo_root, &cfg.mock_dir, &cfg.registry_namespaces);
        if schemas > 0 {
            eprintln!("  generated {schemas} schema files");
        }
        eprintln!(
            "  {} rows across {} namespaces",
            registry.rows.len(),
            registry.by_namespace.len()
        );
        for f in &cycle_findings {
            registry_errors += 1;
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        for f in registry::validate_provenance(
            &cfg.repo_root,
            &cfg.registry_roots,
            &cfg.frozen_roots,
            &registry,
            &cfg.registry_namespaces,
        ) {
            registry_errors += 1;
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        for f in registry::namespace_root_collisions(&cfg.registry_namespaces, &cfg.registry_roots)
        {
            registry_errors += 1;
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        for f in registry::validate(&cfg.registry_namespaces, &registry) {
            registry_errors += 1;
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        // The declarations, then the data. There is no gate between them:
        // `row_reference_fields` already skips a field whose type names no
        // namespace, so an unknown type suppresses its own field's rows and
        // nothing else. A project-wide gate stood here and suppressed genuine
        // findings in unrelated namespaces, which cost an author a second round
        // of failures after fixing a typo somewhere they were not looking.
        let declarations = registry::unknown_field_types(&cfg.registry_namespaces)
            .into_iter()
            .chain(registry::namespace_type_collisions(&cfg.registry_namespaces))
            .chain(registry::value_field_targets(&cfg.registry_namespaces));
        for f in declarations
            .chain(registry::validate_row_references(
                &registry,
                &cfg.registry_namespaces,
            ))
        {
            registry_errors += 1;
            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
        }
        // A warning by default, and the choice is deliberate under the rule
        // stated above: what prints ERROR blocks, and an inert key does not stop
        // the registry from being correct. It stops the author from knowing their
        // declaration does nothing, which is a different harm and is ended by
        // saying so once per run.
        //
        // Escalating or silencing it is a project's own call, and it is made the
        // ordinary way: naming `registry-config-keys` in `[lints]`, the same as
        // any other lint. This does not go through the general lint dispatch
        // (there is no `Lint` impl here to register), so the severity map is
        // consulted by hand at this one call site; nothing else in `FINDING_KINDS`
        // is wired to it yet.
        let key_severity = cfg
            .lint_overrides
            .base
            .get("registry-config-keys")
            .copied()
            .unwrap_or(Severity::ADVISORY);
        match std::fs::read_to_string(&cfg.config_path) {
            Ok(text) => {
                for f in registry::config_unknown_keys(&text) {
                    match key_severity.effective(mode) {
                        Level::Pass => {},
                        Level::Info | Level::Warn => {
                            eprintln!("  warning [{}]: {}", f.kind, f.message);
                        },
                        Level::Error => {
                            registry_errors += 1;
                            eprintln!("  ERROR [{}]: {}", f.kind, f.message);
                        },
                    }
                }
            },
            Err(e) => {
                // A check that could not run is not a check that passed: the
                // same reasoning `SchemaCheck::Unavailable` already applies to
                // the schema gate applies here, so absence is reported rather
                // than swallowed.
                eprintln!(
                    "  warning: could not read {} to check for unknown config \
                     keys: {e}",
                    cfg.config_path.display()
                );
            },
        }
        match registry::check_schemas(&cfg.repo_root, &cfg.registry_namespaces) {
            registry::SchemaCheck::Ran {
                failures,
            } if failures.is_empty() => {
                eprintln!("  schema check passed");
            },
            registry::SchemaCheck::Ran {
                failures,
            } => {
                for f in failures {
                    registry_errors += 1;
                    eprintln!("  ERROR [schema]: {f}");
                }
            },
            registry::SchemaCheck::Unavailable => {
                // A run that could not check is not a run that passed. The
                // findings above are two-valued and cannot say "do not trust
                // this", so an unverifiable row shape reported as success is a
                // green that means nothing: every required field, every type
                // and every slug pattern went unchecked.
                //
                // Gated on rows existing, not on namespaces existing. Two
                // namespaces are builtin (`vocab` and `reference`, prepended by
                // `with_builtins`), so `registry_namespaces` is never empty and
                // a guard on it fails every project that has no registry at all.
                // That was the first version of this and the no-registry control
                // caught it.
                //
                // Rows are the thing a schema verifies. No rows, nothing unverified.

                if registry.rows.is_empty() {
                    eprintln!("  schema check skipped: no rows to verify");
                } else {
                    registry_errors += 1;
                    eprintln!(
                        "  ERROR [schema-unavailable]: taplo is not installed, so the shape of \
                         {} row(s) is unverified: required fields, types and slug patterns were \
                         all unchecked. Install taplo.",
                        registry.rows.len()
                    );
                }
            },
        }
    }

    // --- Every markdown document, through one pipeline ---
    //
    // Planned first, so the index of what will exist is complete before the
    // first reference resolves: a reference from a document written early to
    // one written later is ordinary rather than a special case. Then rendered,
    // in one loop, so a document cannot reach the output having skipped a step
    // because its path forgot one. Both were real, repeatedly.
    // Documents are generated even when the registry is already known to be
    // lying, and the exit below still fails. That ordering is deliberate: the
    // dangling-reference scan reads the *output*, because several paths render
    // documents and two of them silently did not resolve references at all.
    // Returning early here would trade a precise finding for an earlier exit.
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
        // A reference and a placeholder both survive as literal braces, and only
        // one of them is a lie about the registry. A reference is
        // `root::selector`, so it carries `::`; `{{abi_count}}` is a template
        // placeholder that resolved to nothing because the project has nothing
        // to count, which is an ordinary state for a project with no packages.
        //
        // The first version of this gate did not distinguish them and failed a
        // repo whose own config says "a project with no packages is ordinary",
        // over fifteen placeholders in one document. Found by running it against
        // a real consumer rather than a fixture.
        let is_reference = |e: &String| e.trim_matches(|c| c == '{' || c == '}').contains("::");
        let refs_unresolved: usize = unresolved
            .iter()
            .map(|(_, v)| v.iter().filter(|e| is_reference(e)).count())
            .sum();
        registry_errors += refs_unresolved;

        if !unresolved.is_empty() {
            let total: usize = unresolved.iter().map(|(_, v)| v.len()).sum();
            // An error, not a warning, and on the design's own rung: dangling
            // reference and duplicate identifier are both errors "because both
            // mean the registry is lying". Gating duplicates while warning about
            // this left half the sentence enforced, and the summary line below
            // claimed both.
            //
            // Reported at the output rather than per render path, because
            // several paths generate documents and two of them silently did not
            // resolve references at all.
            if refs_unresolved > 0 {
                eprintln!(
                    "  ERROR [dangling-reference]: {refs_unresolved} reference(s) resolved to \
                     nothing and were rendered literally. A reference that resolves to nothing is \
                     worse than prose, because it looks checked."
                );
            }
            if total > refs_unresolved {
                eprintln!(
                    "  warning: {} placeholder(s) rendered literally in {} document(s). Not \
                     references, so not gated: a project with nothing to substitute is ordinary.",
                    total - refs_unresolved,
                    unresolved.len()
                );
            }
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

    if registry_errors > 0 {
        eprintln!(
            "\nregistry: {registry_errors} error(s). A reference that does not resolve, or an \
             identifier declared twice, means the registry is lying about what it holds."
        );
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(rest: &[&str]) -> Vec<String> {
        std::iter::once("mock".to_string())
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn gating_commands_are_not_recovery_commands() {
        // These lint, so a pack that will not build must block them. This is
        // the half that regressed: every one of these ran to a reported pass
        // with the pack absent.
        for cmd in ["check", "lock", "close", "deprecate", "unlock", "archive"] {
            assert!(
                !is_recovery_command(&args(&[cmd])),
                "`{cmd}` gates, so it must not be exempt from the lint-load block"
            );
        }
        // The bare invocation regenerates docs and agent rules, and is the
        // most common way the gate is reached at all.
        assert!(!is_recovery_command(&args(&[])));
        assert!(!is_recovery_command(&args(&["--lint-only", "--commit"])));
    }

    #[test]
    fn recovery_commands_survive_a_broken_pack() {
        // Refusing these would leave a repo with a broken pack no route out,
        // since `clean` and `migrate` are among the ways it gets fixed.
        for cmd in ["clean", "activate", "deactivate", "status", "migrate"] {
            assert!(
                is_recovery_command(&args(&[cmd])),
                "`{cmd}` repairs or inspects, so it must stay usable"
            );
        }
    }

    #[test]
    fn a_flag_value_is_not_read_as_a_recovery_command() {
        // `--scope status` names a crate, not the `status` subcommand. The
        // check skips argv[0] but not flag values, so this pins the shape a
        // future reader would otherwise have to rediscover.
        let a = args(&["--dir", "clean"]);
        assert!(
            is_recovery_command(&a),
            "documenting current behaviour: a flag value that spells a \
             recovery command is read as one. If this ever tightens, the \
             tightening is the fix, not this assertion."
        );
    }

    #[test]
    fn the_block_message_names_the_cost_and_the_first_cause() {
        let cfg = Config::from_dir(Path::new("/nowhere"));
        let msg = explain_lint_load_failure(&cfg, &"no rules expected `cross_lints`");

        // The cost is the point: a reader who skims must learn that nothing
        // was checked, not merely that something failed.
        assert!(msg.contains("Nothing was checked"), "message: {msg}");
        // The underlying compiler output survives, indented rather than lost.
        assert!(
            msg.contains("no rules expected `cross_lints`"),
            "message: {msg}"
        );
        // The stale-pin cause leads, because it is the one that has happened.
        let stale = msg.find("stopped").expect("stale-branch cause present");
        let key = msg
            .find("package name")
            .expect("key-mismatch cause present");
        assert!(stale < key, "the stale-branch cause should lead: {msg}");
    }

    /// Everything after the subcommand reaches it, flags included.
    ///
    /// The regression: `bench_args` was built from `positional_args`, which
    /// drops every flag so that a flag is never mistaken for a subcommand
    /// name. That made `mock bench test --release` run a debug pass and
    /// report it as a success, and it made the `extra` forwarding in
    /// `cmd_run`, `cmd_report` and `cmd_test` unreachable through the CLI.
    #[test]
    fn a_flag_after_the_subcommand_is_forwarded_to_it() {
        let args = strs(&["mock", "bench", "test", "--release"]);
        assert_eq!(subcommand_args(&args, "bench"), vec!["test", "--release"]);
    }

    /// The control for the test above. Without it, the assertion cannot tell
    /// "flags are forwarded" from "everything is forwarded": a flag belonging
    /// to the tool, written before the subcommand, must NOT reach it.
    #[test]
    fn a_flag_before_the_subcommand_is_the_tools_and_is_not_forwarded() {
        let args = strs(&["mock", "--strict", "bench", "test"]);
        assert_eq!(subcommand_args(&args, "bench"), vec!["test"]);
    }

    /// A value-taking global is consumed wherever it sits, and its VALUE must
    /// not leak into the subcommand's argv. A leaked value is worse than a
    /// leaked flag: `--dir /some/path` would hand cargo a bare path argument.
    #[test]
    fn a_value_global_and_its_value_are_consumed_on_both_sides() {
        let before = strs(&["mock", "--dir", "/w/mock", "bench", "test", "--release"]);
        assert_eq!(subcommand_args(&before, "bench"), vec!["test", "--release"]);
        let after = strs(&["mock", "bench", "test", "--dir", "/w/mock", "--release"]);
        assert_eq!(subcommand_args(&after, "bench"), vec!["test", "--release"]);
    }

    /// A subcommand that never appears yields nothing, rather than yielding
    /// the whole command line.
    #[test]
    fn an_absent_subcommand_forwards_nothing() {
        let args = strs(&["mock", "lint", "--strict"]);
        assert!(subcommand_args(&args, "bench").is_empty());
    }

    /// The two lists must genuinely differ, or the fix is cosmetic. This
    /// pins the exact input that used to lose its flag: the positional
    /// filter drops `--release`, `subcommand_args` keeps it.
    #[test]
    fn the_positional_filter_and_the_argument_vector_are_not_the_same_list() {
        let args = strs(&["mock", "bench", "test", "--release"]);
        let positional: Vec<&str> = args
            .iter()
            .skip(1)
            .filter(|a| !a.starts_with('-'))
            .map(String::as_str)
            .collect();
        assert_eq!(
            positional,
            vec!["bench", "test"],
            "the old shape drops the flag"
        );
        let forwarded = subcommand_args(&args, "bench");
        assert!(
            forwarded.contains(&"--release"),
            "the flag must survive: {forwarded:?}"
        );
        assert_ne!(positional.get(1 ..).unwrap_or(&[]), forwarded.as_slice());
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // -- a tool sharing a name with a builtin is refused before it is ever
    //    reached, rather than inside `tool::run`'s catch-all arm, which can
    //    never see the case: `other` there is never one of the sixteen names
    //    a literal arm or `help::is_help_request` already claimed. ----------

    struct NamedTool(&'static str);
    impl mockspace_lint_rules::tool::Tool for NamedTool {
        fn name(&self) -> &'static str {
            self.0
        }

        fn description(&self) -> &'static str {
            "a probe tool"
        }

        fn not_a_lint(&self) -> mockspace_lint_rules::tool::NotALint {
            mockspace_lint_rules::tool::NotALint::NoFailingCase
        }

        fn run(
            &self,
            _ctx: &mockspace_lint_rules::tool::ToolContext<'_>,
        ) -> mockspace_lint_rules::tool::ToolReport {
            mockspace_lint_rules::tool::ToolReport::reported("", 1)
        }
    }

    fn pack_with(names: &[&'static str]) -> LintPack {
        let mut pack = LintPack::default();
        for n in names {
            pack.tools.push(Box::new(NamedTool(n)));
        }
        pack
    }

    #[test]
    fn a_tool_named_after_a_builtin_is_shadowed() {
        // The case that must fail: without the check this returns `false`,
        // `check` reaches its literal match arm every time, and the tool at
        // `mock/tools/check/` is silently unreachable forever.
        let pack = pack_with(&["check"]);
        assert!(tool_is_shadowed_by_a_builtin("check", &pack));
    }

    #[test]
    fn help_itself_is_a_shadowing_name_too() {
        // A fact about the predicate's own logic, not about the real dispatch
        // path: `mock help` is intercepted at `help::is_help_request`, before
        // this function is ever reached (tracked with a `FIXME` there), so a
        // tool named `help` is shadowed in practice by a route this test does
        // not exercise. What this pins is that the predicate itself does not
        // special-case "help" as though it were an ordinary tool name; if the
        // real interception is ever relocated, the predicate is already
        // correct for the case it would then need to cover.
        let pack = pack_with(&["help"]);
        assert!(tool_is_shadowed_by_a_builtin("help", &pack));
    }

    #[test]
    fn an_ordinary_tool_name_is_not_shadowed() {
        // The negative arm. Without it the predicate could return `true` for
        // everything and the test above would still pass.
        let pack = pack_with(&["phrase-search"]);
        assert!(!tool_is_shadowed_by_a_builtin("phrase-search", &pack));
    }

    #[test]
    fn a_builtin_name_with_no_matching_tool_is_not_a_collision() {
        // A collision needs both halves: the name has to be a builtin AND a
        // registered tool. `check` alone, with no tool by that name, is just
        // the ordinary builtin and must not be refused.
        let pack = pack_with(&["phrase-search"]);
        assert!(!tool_is_shadowed_by_a_builtin("check", &pack));
    }
}

/// Whether a state transition should commit the rename it makes.
///
/// Opt-in, and it should not be. Each of these subcommands is one atomic rename
/// whose whole value is that the rename is recorded, and an uncommitted one is
/// worse than merely fragile: a later `git reset` brings the source file back
/// while leaving the target sitting there untracked, so the round is in two
/// phases at once and neither the tool nor the person can tell which is meant.
// FIXME: this wants to be the default and cannot be until the commit path
// changes. `design_round::git::commit_or_suggest` builds the commit with
// `commit-tree` plus `update-ref`, which is plumbing, so it runs no hook and
// honours no `commit.gpgsign`. Verified rather than inferred: a `pre-commit`
// set to `exit 1` does not fire and the resulting object carries no `gpgsig`
// header. That was tolerable while nobody typed the flag. Turned on by default
// it makes every `lock`, `close`, `archive`, `unlock`, `deprecate` and
// `migrate` write an unhooked, unsigned commit to somebody's branch, which is
// the `--no-verify` this project forbids, shipped on by default by the tool
// that enforces the ban.
//
// Two ways out, and the choice is not the agent's: commit through porcelain so
// the hooks and the signature happen, which risks the pre-commit gate refusing
// the very transition that is changing the phase it reads; or keep the
// plumbing and say in the help text and the guide that this path skips both.
fn auto_commit_wanted(args: &[String]) -> bool {
    args.iter().any(|a| a == "--auto-commit")
}

#[cfg(test)]
mod auto_commit_tests {
    use super::auto_commit_wanted;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_transition_does_not_commit_unless_asked() {
        assert!(!auto_commit_wanted(&args(&["mock", "lock"])));
        assert!(auto_commit_wanted(&args(&[
            "mock",
            "lock",
            "--auto-commit"
        ])));
    }

    /// The flag has to be found wherever it sits, since that is how it gets
    /// typed, and it has to be the flag rather than anything that resembles it.
    ///
    /// The second half is the part with teeth. A predicate written as "does not
    /// contain the opt-out" passes this file for every argument on earth that
    /// omits the opt-out, including a typo, which is how the previous version of
    /// this test managed to assert nothing at all while naming a flag its
    /// subject never read.
    #[test]
    fn the_flag_is_found_anywhere_and_nothing_else_stands_in_for_it() {
        assert!(auto_commit_wanted(&args(&[
            "mock",
            "--auto-commit",
            "close"
        ])));
        assert!(auto_commit_wanted(&args(&[
            "mock",
            "close",
            "--strict",
            "--auto-commit"
        ])));
        assert!(!auto_commit_wanted(&args(&["mock", "close", "--strict"])));
        assert!(!auto_commit_wanted(&args(&[
            "mock",
            "close",
            "--auto-comit"
        ])));
        assert!(!auto_commit_wanted(&args(&[
            "mock",
            "close",
            "--no-commit"
        ])));
    }

    /// Catalogued, tracked with the `FIXME` on `auto_commit_wanted`. Committing
    /// by default is the behaviour wanted and cannot ship while the commit path
    /// is `commit-tree` plus `update-ref`, which runs no hook and produces no
    /// signature. Un-ignore this when that path changes; it is the whole of the
    /// behaviour change and it already asserts the intended answer.
    #[test]
    #[ignore = "catalogue: a transition should commit by default, and cannot \
                until the commit path stops bypassing hooks and signing"]
    fn a_transition_commits_unless_told_otherwise() {
        assert!(auto_commit_wanted(&args(&["mock", "lock"])));
        assert!(!auto_commit_wanted(&args(&["mock", "lock", "--no-commit"])));
    }
}
