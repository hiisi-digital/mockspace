#![allow(unused_imports)]
use super::*;

/// Outcome of one readiness probe. Pass / warn / fail.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum CheckResult {
    Pass,
    Warn,
    Fail,
}

impl CheckResult {
    pub(crate) fn icon(self) -> &'static str {
        match self {
            CheckResult::Pass => "✓",
            CheckResult::Warn => "!",
            CheckResult::Fail => "✗",
        }
    }
}

pub(crate) fn print_row(section: &str, result: CheckResult, msg: &str) {
    eprintln!("  {} {:<10} {}", result.icon(), section, msg);
}

pub(crate) fn cmd_check(cfg: &Config) -> ExitCode {
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
        },
        _ => {
            print_row("git", CheckResult::Warn, "not a git repo (or git failed)");
        },
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
                        (a, 0) => {
                            (
                                CheckResult::Warn,
                                format!("{branch} {a} ahead of {up}, push needed"),
                            )
                        },
                        (0, b) => {
                            (
                                CheckResult::Warn,
                                format!("{branch} {b} behind {up}, pull needed"),
                            )
                        },
                        (a, b) => {
                            (
                                CheckResult::Warn,
                                format!("{branch} diverged from {up} ({a} ahead, {b} behind)"),
                            )
                        },
                    };
                    print_row("remote", result, &msg);
                },
                _ => {
                    print_row(
                        "remote",
                        CheckResult::Warn,
                        &format!("{branch}: could not compare against upstream"),
                    );
                },
            }
        },
        _ => {
            print_row(
                "remote",
                CheckResult::Warn,
                &format!("{branch} has no upstream; `git push -u` first"),
            );
        },
    }

    // --- phase detection ---
    let design_rounds = cfg.mock_dir.join("design_rounds");
    let phase = changelist_helpers::current_phase(&design_rounds);
    print_row("phase", CheckResult::Pass, phase.label());

    // --- cargo check ---
    // A repo whose taxonomy is still a design round's subject has no workspace
    // members, which cargo cannot check at all. That failure is forgiven (see
    // `cargo_gate`); every other one fails the row.
    let memberless = cargo_gate::is_memberless_virtual_workspace(&cfg.mock_dir);
    let check_out = cargo_gate::cargo(&cfg.mock_dir, &["check"]).output();
    match check_out {
        Ok(o) if o.status.success() => print_row("build", CheckResult::Pass, "cargo check green"),
        Ok(o)
            if memberless
                && cargo_gate::diagnostic_is_no_members(&String::from_utf8_lossy(&o.stderr)) =>
        {
            print_row("build", CheckResult::Pass, "no workspace members yet");
        },
        Ok(_) => {
            print_row(
                "build",
                CheckResult::Fail,
                "cargo check failed; run `cargo check` in mock/ for details",
            );
            any_fail = true;
        },
        Err(e) => {
            print_row(
                "build",
                CheckResult::Fail,
                &format!("could not run cargo check: {e}"),
            );
            any_fail = true;
        },
    }

    // --- cargo test ---
    // Tests in this repo exercise the designed API surface. If the CL
    // promises functionality and tests assert it, missing impl fails here.
    let test_out = cargo_gate::cargo(&cfg.mock_dir, &["test"]).output();
    match test_out {
        Ok(o) if o.status.success() => print_row("tests", CheckResult::Pass, "cargo test green"),
        Ok(o)
            if memberless
                && cargo_gate::diagnostic_is_no_members(&String::from_utf8_lossy(&o.stderr)) =>
        {
            print_row("tests", CheckResult::Pass, "no workspace members yet");
        },
        Ok(_) => {
            print_row(
                "tests",
                CheckResult::Fail,
                "cargo test failed; run `cargo test` in mock/ for details",
            );
            any_fail = true;
        },
        Err(e) => {
            print_row(
                "tests",
                CheckResult::Fail,
                &format!("could not run cargo test: {e}"),
            );
            any_fail = true;
        },
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
        Ok(s) if s.success() => {
            print_row(
                "lints",
                CheckResult::Pass,
                "mockspace lint pipeline green (strict)",
            )
        },
        Ok(_) => {
            print_row(
                "lints",
                CheckResult::Fail,
                "mockspace lints failed; run `cargo mock --lint-only --strict` for details",
            );
            any_fail = true;
        },
        Err(e) => {
            print_row(
                "lints",
                CheckResult::Fail,
                &format!("could not run cargo mock lints: {e}"),
            );
            any_fail = true;
        },
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
        },
        Phase::Doc => {
            print_row(
                "advance",
                CheckResult::Pass,
                "`cargo mock lock` when doc edits done (DOC → DRAFT)",
            );
        },
        Phase::SrcPlan => {
            print_row(
                "advance",
                CheckResult::Pass,
                "author a src changelist to enter IMPL phase",
            );
        },
        Phase::Src => {
            let msg = if any_fail {
                "IMPL in progress: build failing; fix before `cargo mock lock`"
            } else {
                "IMPL ready for `cargo mock lock` (IMPL → CLOSED) once CL is fulfilled"
            };
            print_row(
                "advance",
                if any_fail { CheckResult::Fail } else { CheckResult::Pass },
                msg,
            );
        },
        Phase::Done => {
            print_row(
                "advance",
                CheckResult::Pass,
                "round complete; `cargo mock close` to archive",
            );
        },
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

/// Remove nested cargo build dirs under `benches/`, `tests/`, and research
/// `sketches/`. The repo-root `target/` and the mockspace install at
/// `mock/target/` are spared: the former is the active build dir, the latter
/// holds the proxy crate + git hooks (removing it uninstalls mockspace).
pub(crate) fn cmd_clean(cfg: &Config) -> ExitCode {
    let targets = nested_artifact_targets(&cfg.repo_root);
    if targets.is_empty() {
        eprintln!("clean: no nested build dirs under benches/, tests/, or sketches/");
        return ExitCode::SUCCESS;
    }
    let mut removed = 0usize;
    for t in &targets {
        match fs::remove_dir_all(t) {
            Ok(()) => {
                eprintln!("  removed {}", t.display());
                removed += 1;
            },
            Err(e) => eprintln!("  failed to remove {}: {e}", t.display()),
        }
    }
    eprintln!("clean: removed {removed} build dir(s) (root target/ and mock/target/ left intact)");
    ExitCode::SUCCESS
}

/// Collect `target` directories nested under a `benches`, `tests`, `sketches`,
/// or `research` path segment. Does not descend into a `target` dir once
/// found, and skips dotfiles (e.g. `.git`).
pub(crate) fn nested_artifact_targets(repo_root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_artifact_targets(repo_root, repo_root, &mut out);
    out
}

pub(crate) fn collect_artifact_targets(dir: &Path, repo_root: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if name == "target" {
            if is_cleanable_target(&path, repo_root) {
                out.push(path);
            }
            // Never descend into a build dir.
            continue;
        }
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        collect_artifact_targets(&path, repo_root, out);
    }
}

/// A `target` dir is cleanable when both hold: a `Cargo.toml` sits beside it
/// (so it is a real cargo build dir, not a coincidentally-named directory), and
/// its path below the repo root passes through a `benches`, `tests`, or
/// `sketches` segment. This excludes the repo-root `target/` and `mock/target/`
/// (neither has such a segment) and spares research memos that are not under
/// `sketches/` (audit-trail artifacts, not disposable sketches).
pub(crate) fn is_cleanable_target(target_dir: &Path, repo_root: &Path) -> bool {
    let has_manifest = target_dir
        .parent()
        .map(|p| p.join("Cargo.toml").is_file())
        .unwrap_or(false);
    if !has_manifest {
        return false;
    }
    let rel = match target_dir.strip_prefix(repo_root) {
        Ok(r) => r,
        Err(_) => return false,
    };
    rel.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("benches") | Some("tests") | Some("sketches")
        )
    })
}
