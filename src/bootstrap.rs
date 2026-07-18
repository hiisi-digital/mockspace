//! Bootstrap and health-check for mockspace workspaces.
//!
//! Called from the consuming mock workspace's `build.rs` to ensure:
//! - The `cargo mock` alias exists in `.cargo/config.toml`
//! - Generated hooks are up-to-date in the hooks output directory
//!
//! Also callable at runtime from `cargo mock` as a health check.
//!
//! # How it works
//!
//! mockspace bakes `env!("CARGO_MANIFEST_DIR")` at compile time: its own
//! source path, wherever cargo cached it. The cargo alias points `cargo mock`
//! at that path.
//!
//! # Hook model
//!
//! mockspace never touches `.git/hooks/`. Those are the user's hooks and
//! always run: with or without mockspace.
//!
//! Instead, mockspace generates intermediate hooks into a build-artifact
//! directory (default: `<mock_dir>/target/hooks/`). These generated hooks
//! **source the user's `.git/hooks/*` first**, then run mockspace validation.
//!
//! Activation is explicit:
//! - `cargo mock activate`  → `git config core.hooksPath <hooks_dir>`
//! - `cargo mock deactivate` → `git config --unset core.hooksPath`
//!
//! When active: git calls mockspace's hooks → they source `.git/hooks/*` →
//! then run mockspace validation. User's hooks run in both cases.
//!
//! When deactivated (or mockspace removed): `core.hooksPath` unset → git
//! falls back to `.git/hooks/` → user's hooks run directly. Identical
//! behavior as if mockspace was never there.
//!
//! # Custom lints
//!
//! Two mechanisms, both wired through the generated proxy crate in
//! `target/mockspace-proxy/`:
//!
//! 1. **In-tree lint files**: `.rs` files under `{mock_dir}/lints/`. Each
//!    file defines `pub fn lint()` and/or `pub fn cross_lint()` (singular,
//!    one lint per file). Good for quick project-specific rules.
//!
//! 2. **External lint-pack crates**: cargo dependencies declared under
//!    `[lint-crates]` in `mockspace.toml`. Each pack must expose:
//!    - `pub fn lints() -> Vec<Box<dyn mockspace_lint_rules::Lint>>`
//!    - `pub fn cross_lints() -> Vec<Box<dyn mockspace_lint_rules::CrossCrateLint>>`
//!
//!    Good for lint rules shared across multiple mockspaces. Cargo-dep
//!    syntax: `pack-name = { path = "..." }` / `{ git = "..." }` /
//!    `{ version = "..." }`. The generated proxy pulls them in as normal
//!    cargo dependencies; types match so long as the pack and the proxy
//!    resolve the same `mockspace-lint-rules` source.

use std::env;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Marker in generated hooks for identification and versioning.
const MANAGED_MARKER: &str = "# mockspace-managed";

/// Bump when hook templates change → triggers regeneration.
const HOOK_VERSION: u32 = 1;

/// Hook names that mockspace generates.
const HOOK_NAMES: &[&str] = &["pre-commit", "pre-push"];

// ──────────────────────────────────────────────────────────────────────
// Public API
// ──────────────────────────────────────────────────────────────────────

/// Run bootstrap from a consuming crate's `build.rs`.
///
/// # Usage
///
/// ```toml
/// [build-dependencies]
/// mockspace = { git = "ssh://git@github.com/hiisi-digital/mockspace.git" }
/// ```
///
/// ```rust,ignore
/// fn main() { mockspace::bootstrap_from_buildscript(); }
/// ```
pub fn bootstrap_from_buildscript() {
    let build_crate_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set: call from build.rs"),
    );
    let mockspace_manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mock_dir = match find_ancestor_with(&build_crate_dir, "mockspace.toml") {
        Some(d) => d,
        None => {
            println!(
                "cargo::warning=mockspace: no mockspace.toml found above {}",
                build_crate_dir.display()
            );
            return;
        }
    };

    let repo_root = match find_ancestor_with(&mock_dir, ".git") {
        Some(r) => r,
        None => {
            println!("cargo::warning=mockspace: not in a git repo, skipping bootstrap");
            return;
        }
    };

    let actions = run(&repo_root, &mock_dir, &mockspace_manifest_dir);

    for action in &actions {
        println!("cargo::warning=mockspace: {action}");
    }

    // Rerun triggers.
    println!(
        "cargo::rerun-if-changed={}",
        mock_dir.join("mockspace.toml").display()
    );
    println!(
        "cargo::rerun-if-changed={}",
        repo_root.join(".cargo/config.toml").display()
    );
    // Rerun when user's git hooks change (they're sourced by our hooks).
    let user_hooks = resolve_git_dir(&repo_root).join("hooks");
    for name in HOOK_NAMES {
        println!(
            "cargo::rerun-if-changed={}",
            user_hooks.join(name).display()
        );
    }
    // Rerun when custom lint files change.
    let custom_lints_dir = mock_dir.join("lints");
    println!(
        "cargo::rerun-if-changed={}",
        custom_lints_dir.display()
    );
}

/// Run bootstrap health checks, fixing anything missing or stale.
///
/// Returns a list of human-readable actions taken. Empty = healthy.
pub fn run(
    repo_root: &Path,
    mock_dir: &Path,
    mockspace_manifest_dir: &Path,
) -> Vec<String> {
    let mut actions = Vec::new();

    ensure_cargo_alias(repo_root, mock_dir, mockspace_manifest_dir, &mut actions);
    ensure_generated_hooks(repo_root, mock_dir, &mut actions);
    ensure_gitignore(repo_root, &mut actions);
    check_activation(repo_root, mock_dir, &mut actions);

    actions
}

/// Ensure the repo-root `.gitignore` ignores every cargo `target/` build dir.
///
/// Cargo build dirs appear not only at the repo root but nested under any
/// standalone crate inside `benches/`, `tests/`, and `mock/research/sketches/`.
/// A root-anchored `/target` ignore misses those nested ones, so they show up
/// as untracked noise and can be committed by accident. A catch-all `target/`
/// line (no leading slash, so git matches a directory named `target` at any
/// depth) covers all of them at once.
///
/// Idempotent: if any line already reads exactly `target/`, this is a no-op.
/// Otherwise a small marked block is appended; existing entries are left
/// untouched.
fn ensure_gitignore(repo_root: &Path, actions: &mut Vec<String>) {
    let path = repo_root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();

    if existing.lines().any(|l| l.trim() == "target/") {
        return;
    }

    let block = "\
# === mockspace-managed build artifacts (do not edit) ===
# Catch-all: every cargo build dir, including nested ones under benches/,
# tests/, and research sketches. A leading-slash /target would miss those.
target/
# === end mockspace-managed build artifacts ===
";

    let new_content = if existing.is_empty() {
        block.to_string()
    } else {
        let sep = if existing.ends_with('\n') { "\n" } else { "\n\n" };
        format!("{existing}{sep}{block}")
    };

    if fs::write(&path, new_content).is_ok() {
        actions.push("added catch-all target/ to .gitignore".into());
    }
}

/// Set `core.hooksPath` to mockspace's generated hooks directory.
pub fn activate(repo_root: &Path, mock_dir: &Path) -> Result<(), String> {
    let hooks_dir = generated_hooks_dir(mock_dir);
    if !hooks_dir.exists() {
        return Err(format!(
            "generated hooks not found at {}. Run `cargo mock` first.",
            hooks_dir.display()
        ));
    }

    let status = std::process::Command::new("git")
        .args(["config", "--local", "core.hooksPath"])
        .arg(hooks_dir.to_str().unwrap_or(""))
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("git config failed: {e}"))?;

    if !status.success() {
        return Err("git config core.hooksPath failed".into());
    }

    Ok(())
}

/// Unset `core.hooksPath`, restoring git's default `.git/hooks/`.
pub fn deactivate(repo_root: &Path) -> Result<(), String> {
    let status = std::process::Command::new("git")
        .args(["config", "--local", "--unset", "core.hooksPath"])
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("git config failed: {e}"))?;

    // Exit code 5 = key not found (already deactivated). That's fine.
    if !status.success() && status.code() != Some(5) {
        return Err("git config --unset core.hooksPath failed".into());
    }

    Ok(())
}

/// Check if mockspace hooks are currently active.
pub fn is_active(repo_root: &Path) -> bool {
    let output = std::process::Command::new("git")
        .args(["config", "--local", "core.hooksPath"])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            // Active if it points to a mockspace-generated hooks dir.
            path.contains("mockspace") || path.contains("target/hooks")
        }
        _ => false,
    }
}

// ──────────────────────────────────────────────────────────────────────
// Cargo alias
// ──────────────────────────────────────────────────────────────────────

fn ensure_cargo_alias(
    repo_root: &Path,
    mock_dir: &Path,
    mockspace_dir: &Path,
    actions: &mut Vec<String>,
) {
    // Generate the proxy crate that delegates to mockspace.
    // Lives in target/ (gitignored), contains the machine-specific dep path.
    ensure_proxy_crate(repo_root, mock_dir, mockspace_dir, actions);

    let config_dir = repo_root.join(".cargo");
    let config_path = config_dir.join("config.toml");

    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| mock_dir.display().to_string());

    // Both paths are relative to repo root: fully portable.
    // The machine-specific mockspace path lives inside the generated
    // proxy crate at target/mockspace-proxy/ (gitignored).
    let alias_value = format!(
        "run --manifest-path target/mockspace-proxy/Cargo.toml -- --dir {}",
        mock_rel,
    );
    let alias_line = format!("mock = \"{alias_value}\"");

    let current = fs::read_to_string(&config_path).unwrap_or_default();

    // Check if alias already exists and is correct.
    for line in current.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("mock") && trimmed.contains('=') {
            if let Some((_, val)) = trimmed.split_once('=') {
                let val = val.trim().trim_matches('"');
                if val == alias_value {
                    return; // Healthy.
                }
            }
            // Stale: update in place.
            let updated: Vec<&str> = current
                .lines()
                .map(|l| {
                    if l.trim().starts_with("mock") && l.contains('=') {
                        alias_line.as_str()
                    } else {
                        l
                    }
                })
                .collect();
            let _ = fs::write(&config_path, updated.join("\n") + "\n");
            actions.push(format!("updated cargo mock alias"));
            return;
        }
    }

    // Missing: add.
    let _ = fs::create_dir_all(&config_dir);
    let new_content = if current.is_empty() {
        format!("[alias]\n{alias_line}\n")
    } else if current.contains("[alias]") {
        let mut result = String::new();
        let mut inserted = false;
        for line in current.lines() {
            result.push_str(line);
            result.push('\n');
            if !inserted && line.trim() == "[alias]" {
                result.push_str(&alias_line);
                result.push('\n');
                inserted = true;
            }
        }
        result
    } else {
        format!("{current}\n[alias]\n{alias_line}\n")
    };
    let _ = fs::write(&config_path, &new_content);
    actions.push(format!("wrote cargo mock alias"));
}

// ──────────────────────────────────────────────────────────────────────
// Proxy crate (target/mockspace-proxy/)
// ──────────────────────────────────────────────────────────────────────

/// Generate a tiny proxy crate in `target/mockspace-proxy/` that depends on
/// mockspace and delegates to `mockspace::run()`.
///
/// The proxy's Cargo.toml contains the machine-specific absolute path to the
/// mockspace source. Since it lives in `target/` (gitignored), the checked-in
/// `.cargo/config.toml` alias can use a portable relative path:
/// `run --manifest-path target/mockspace-proxy/Cargo.toml -- --dir <mock_rel>`
///
/// If `{mock_dir}/lints/` exists and contains `.rs` files, the proxy is
/// generated with custom lint support. Each `.rs` file must define:
/// - `pub fn lint() -> Box<dyn mockspace_lint_rules::Lint>` for per-crate lints
/// - `pub fn cross_lint() -> Box<dyn mockspace_lint_rules::CrossCrateLint>` for cross-crate lints
fn ensure_proxy_crate(repo_root: &Path, mock_dir: &Path, mockspace_dir: &Path, actions: &mut Vec<String>) {
    let proxy_dir = repo_root.join("target").join("mockspace-proxy");
    let proxy_cargo = proxy_dir.join("Cargo.toml");
    let proxy_src = proxy_dir.join("src");
    let proxy_main = proxy_src.join("main.rs");

    // The mockspace source the proxy should pin. The passed `mockspace_dir` is
    // the RUNNING binary's baked path, which self-perpetuates: a proxy built
    // against an old mockspace re-pins itself to that same old checkout every
    // run, so `cargo mock` can never advance past whatever the proxy was last
    // built with. The consumer's own resolved lock is the authority instead, so
    // the pin tracks what the consumer actually depends on. Network-free: only
    // an already-present git checkout is used, and anything unresolvable falls
    // back to the baked path (the original behaviour).
    let mockspace_dir = &resolve_mockspace_pin(mock_dir, mockspace_dir, actions);

    // In-tree lint files from {mock_dir}/lints/
    let lints_dir = mock_dir.join("lints");
    let custom_lint_files = discover_custom_lint_files(&lints_dir);

    // External lint-pack crates from [lint-crates] in mockspace.toml
    let lint_packs = parse_lint_crates(&mock_dir.join("mockspace.toml"));

    let has_custom_lints = !custom_lint_files.is_empty() || !lint_packs.is_empty();

    let lint_rules_path = mockspace_dir.join("lint-rules");

    let cargo_content = if has_custom_lints {
        let mut out = String::new();
        out.push_str(&format!(
            "[package]\n\
             name = \"mockspace-proxy\"\n\
             version = \"0.1.0\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             [workspace]\n\
             \n\
             [dependencies]\n\
             mockspace = {{ path = \"{}\" }}\n\
             mockspace-lint-rules = {{ path = \"{}\" }}\n",
            mockspace_dir.display(),
            lint_rules_path.display(),
        ));
        for (name, spec) in &lint_packs {
            out.push_str(&format!("{name} = {spec}\n"));
        }
        if !lint_packs.is_empty() {
            // Third-party lint packs depend on mockspace-lint-rules via git
            // (that's the canonical source spec); the proxy has it as a path
            // dep (from cargo's git cache). Without a patch cargo treats them
            // as two different source identities and fails trait-compat unify.
            // Patch both ssh+https variants of the hiisi-digital canonical URL
            // back to the proxy's local path.
            out.push_str(&format!(
                "\n[patch.\"ssh://git@github.com/hiisi-digital/mockspace.git\"]\n\
                 mockspace = {{ path = \"{ms}\" }}\n\
                 mockspace-lint-rules = {{ path = \"{lr}\" }}\n\
                 \n\
                 [patch.\"https://github.com/hiisi-digital/mockspace.git\"]\n\
                 mockspace = {{ path = \"{ms}\" }}\n\
                 mockspace-lint-rules = {{ path = \"{lr}\" }}\n",
                ms = mockspace_dir.display(),
                lr = lint_rules_path.display(),
            ));
        }
        out
    } else {
        format!(
            "[package]\n\
             name = \"mockspace-proxy\"\n\
             version = \"0.1.0\"\n\
             edition = \"2021\"\n\
             publish = false\n\
             \n\
             [workspace]\n\
             \n\
             [dependencies]\n\
             mockspace = {{ path = \"{}\" }}\n",
            mockspace_dir.display(),
        )
    };

    let main_content = if has_custom_lints {
        generate_custom_lint_main(&custom_lint_files, &lints_dir, &lint_packs)
    } else {
        "fn main() -> std::process::ExitCode {\n\
        \x20   mockspace::run()\n\
        }\n".to_string()
    };

    // Check if already up-to-date.
    let cargo_ok = fs::read_to_string(&proxy_cargo)
        .map(|c| c == cargo_content)
        .unwrap_or(false);
    let main_ok = fs::read_to_string(&proxy_main)
        .map(|c| c == main_content)
        .unwrap_or(false);

    if cargo_ok && main_ok {
        return; // Healthy.
    }

    // Name a mockspace pin change explicitly, since a silent re-pin was the
    // whole confusion this resolver exists to remove.
    let old_pin = fs::read_to_string(&proxy_cargo)
        .ok()
        .and_then(|c| pinned_mockspace_path(&c));
    let new_pin = mockspace_dir.display().to_string();
    if let Some(old) = old_pin {
        if old != new_pin {
            actions.push(format!("re-pinned proxy mockspace: {old} -> {new_pin}"));
            // Discard the built binary, so the next invocation cannot run code
            // from the revision just re-pinned away from.
            //
            // A stale lock is visible; a binary built from a revision the
            // manifest no longer names is not, and it means a landed fix is
            // silently not running. That happened: a dependency-parser fix was
            // on the branch, the lock advanced to it, and the cached binary
            // kept producing the old answer, which looked like the fix not
            // working. Removing the artifact makes the rebuild unconditional
            // rather than left to fingerprinting to notice.
            discard_proxy_binary(&proxy_dir, actions);
        }
    }

    let _ = fs::create_dir_all(&proxy_src);
    let _ = fs::write(&proxy_cargo, &cargo_content);
    let _ = fs::write(&proxy_main, &main_content);
    actions.push("generated target/mockspace-proxy/".into());
}

/// Remove the built proxy binary so the next run must rebuild it.
///
/// Only the executable, never the whole target directory: the rest is
/// dependency compilation that is still valid and expensive to redo. Both
/// profiles are removed, since which one a project builds is its own choice.
fn discard_proxy_binary(proxy_dir: &Path, actions: &mut Vec<String>) {
    let mut removed = false;
    for profile in ["debug", "release"] {
        let bin = proxy_dir.join("target").join(profile).join("mockspace-proxy");
        if bin.exists() && fs::remove_file(&bin).is_ok() {
            removed = true;
        }
    }
    if removed {
        actions.push("discarded the built proxy so the new revision is what runs".into());
    }
}

/// The `mockspace = { path = "..." }` value from a proxy Cargo.toml, if present.
fn pinned_mockspace_path(cargo_toml: &str) -> Option<String> {
    for line in cargo_toml.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("mockspace = { path = \"") {
            if let Some(end) = rest.find('"') {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

/// The mockspace source path the proxy should pin.
///
/// Reads the consumer's mock-workspace `Cargo.lock` for the `mockspace`
/// package's resolved git revision, then locates the matching checkout in
/// cargo's git cache. This makes the proxy track what the consumer actually
/// resolved, rather than the running binary's own (possibly stale) baked path.
///
/// Falls back to `fallback` (the baked path) when the consumer uses a path or
/// registry dependency (no git revision to track), when the lock cannot be
/// read, or when the resolved checkout is not present on disk. Never fetches.
fn resolve_mockspace_pin(mock_dir: &Path, fallback: &Path, actions: &mut Vec<String>) -> PathBuf {
    let rev = match mockspace_rev_from_lock(&mock_dir.join("Cargo.lock")) {
        Some(r) => r,
        None => return fallback.to_path_buf(),
    };
    match find_git_checkout("mockspace", &rev) {
        Some(dir) => dir,
        // The lock names a git revision but its checkout is not present (cargo
        // GC, or a fresh clone before the first build). Falling back to the
        // baked path is the exact self-perpetuation this resolver removes, so
        // make the degraded case visible rather than silent.
        None => {
            let short: String = rev.chars().take(7).collect();
            actions.push(format!(
                "re-pin skipped: mockspace {short} checkout absent; keeping baked path"
            ));
            fallback.to_path_buf()
        }
    }
}

/// The full git revision the mock-workspace lock resolved for `mockspace`.
///
/// Returns `None` for a path or registry source (no git revision), or when the
/// lock is absent or unparseable.
fn mockspace_rev_from_lock(lock_path: &Path) -> Option<String> {
    let content = fs::read_to_string(lock_path).ok()?;
    let doc = content.parse::<toml_edit::DocumentMut>().ok()?;
    let packages = doc.get("package")?.as_array_of_tables()?;
    for pkg in packages.iter() {
        let name = pkg.get("name").and_then(|n| n.as_str());
        if name != Some("mockspace") {
            continue;
        }
        let source = pkg.get("source").and_then(|s| s.as_str())?;
        // Shape: "git+<url>?<query>#<full-rev>". No '#' means a path/registry
        // source, which this resolver does not track.
        let rev = source.rsplit_once('#').map(|(_, r)| r.to_string());
        return rev.filter(|_| source.starts_with("git+"));
    }
    None
}

/// A git dependency as the lock resolved it: the remote URL, the tracked
/// branch (if the source pins one), and the locked revision.
#[derive(Debug, PartialEq)]
struct GitSource {
    url: String,
    branch: Option<String>,
    rev: String,
}

/// Every package in the lock pinned to a git BRANCH, by name.
///
/// A branch pin is a moving target by construction: the whole point of tracking
/// `dev` rather than a tag is that the dependency advances. What makes that
/// dangerous rather than useful is that nothing advances the lock, so a project
/// tracking a branch silently runs whatever revision it first resolved, for
/// however long, while reading as though it follows the branch.
///
/// Tags and exact revisions are excluded deliberately: those are pins someone
/// chose, and advancing them would be overriding a decision rather than
/// honouring one.
///
/// Duplicates by name are kept apart: a lock can hold two entries for one
/// package tracking different branches, and collapsing them would advance the
/// wrong one.
fn branch_tracked_git_deps(lock_path: &Path) -> Vec<(String, GitSource)> {
    let Ok(content) = fs::read_to_string(lock_path) else {
        return Vec::new();
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let Some(packages) = doc.get("package").and_then(|p| p.as_array_of_tables()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pkg in packages.iter() {
        let Some(name) = pkg.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let Some(source) = pkg.get("source").and_then(|s| s.as_str()) else {
            continue;
        };
        if let Some(src) = parse_git_source(source) {
            if src.branch.is_some() {
                out.push((name.to_string(), src));
            }
        }
    }
    out
}

/// Split a `git+<url>[?<query>]#<rev>` source into url, branch, and rev.
fn parse_git_source(source: &str) -> Option<GitSource> {
    let body = source.strip_prefix("git+")?;
    let (locator, rev) = body.rsplit_once('#')?;
    let (url, query) = match locator.split_once('?') {
        Some((u, q)) => (u, Some(q)),
        None => (locator, None),
    };
    let branch = query.and_then(|q| {
        q.split('&').find_map(|kv| kv.strip_prefix("branch=").map(str::to_string))
    });
    Some(GitSource {
        url: url.to_string(),
        branch,
        rev: rev.to_string(),
    })
}

/// Whether `cargo mock` may auto-advance a behind mockspace with `cargo update`.
///
/// Read from `[proxy] auto_update` in mockspace.toml. Defaults to `true`: when
/// the locked mockspace is behind its branch's remote head, an interactive
/// `cargo mock` advances it. Set to `false` to only warn.
fn proxy_auto_update(mockspace_toml: &Path) -> bool {
    let Ok(content) = fs::read_to_string(mockspace_toml) else {
        return true;
    };
    let Ok(doc) = content.parse::<toml_edit::DocumentMut>() else {
        return true;
    };
    doc.get("proxy")
        .and_then(|p| p.get("auto_update"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// Locate the cargo git checkout for `name` whose directory matches `rev`.
///
/// Cargo checks a git dependency out to
/// `$CARGO_HOME/git/checkouts/<name>-<hash>/<short-rev>/`, where `<short-rev>`
/// is a prefix of the full revision. Globs those directories (via `read_dir`,
/// no glob crate) and returns the first whose subdirectory name is a prefix of
/// `rev`. Returns `None` when no matching checkout is present. Never fetches.
fn find_git_checkout(name: &str, rev: &str) -> Option<PathBuf> {
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")))?;
    find_git_checkout_in(&cargo_home.join("git").join("checkouts"), name, rev)
}

/// [`find_git_checkout`] against an explicit `checkouts` root, so it is
/// testable without mutating the process environment.
fn find_git_checkout_in(checkouts: &Path, name: &str, rev: &str) -> Option<PathBuf> {
    let prefix = format!("{name}-");
    for source_entry in fs::read_dir(checkouts).ok()?.flatten() {
        let source_name = source_entry.file_name();
        let source_name = source_name.to_string_lossy();
        // Cargo names a checkout `<repo>-<16-hex-source-hash>`. Require the
        // suffix to be exactly that hash, so `mockspace-<hash>` does not also
        // match a differently-named repo like `mockspace-stack-lints-<hash>`.
        let is_this_source = source_name
            .strip_prefix(&prefix)
            .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        if !is_this_source {
            continue;
        }
        let Ok(revs) = fs::read_dir(source_entry.path()) else {
            continue;
        };
        for rev_entry in revs.flatten() {
            if !rev_entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let short = rev_entry.file_name();
            let short = short.to_string_lossy();
            // The checkout dir name is a prefix of the full revision. The first
            // match wins: a within-source short-rev collision needs a 7-hex
            // clash (astronomically unlikely), and two sources containing the
            // same rev are checkouts of the same commit, so either is
            // content-identical.
            if !short.is_empty() && rev.starts_with(short.as_ref()) {
                return Some(rev_entry.path());
            }
        }
    }
    None
}

/// How long between remote-head checks. The check runs at most once per this
/// interval, so a routine `cargo mock` almost never touches the network.
const REMOTE_CHECK_TTL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Keep the consumer's locked mockspace current with its branch's remote head.
///
/// When the mock-workspace lock tracks a mockspace git BRANCH and the locked
/// revision is behind that branch's remote head, this advances the lock with
/// `cargo update` (when `allow_update` and `[proxy] auto_update` both permit) or
/// records a warning naming the exact command.
///
/// Called only from an interactive `cargo mock` (never a git hook or a
/// build script), so it never mutates `Cargo.lock` mid-commit and never runs
/// the network in the commit path. Throttled to once per `REMOTE_CHECK_TTL` and
/// offline-safe: a recent prior check, a non-branch source, or any network
/// failure is a silent skip.
pub fn ensure_mockspace_current(
    repo_root: &Path,
    mock_dir: &Path,
    allow_update: bool,
    actions: &mut Vec<String>,
) {
    let deps = branch_tracked_git_deps(&mock_dir.join("Cargo.lock"));
    if deps.is_empty() {
        return; // every dependency is a path, a registry, a tag, or an exact rev.
    }

    let marker = repo_root.join("target/mockspace-proxy/.remote-check");
    if !remote_check_due(&marker, REMOTE_CHECK_TTL) {
        return;
    }
    // Consume the window before the network calls, on purpose: a persistently
    // offline machine then pays the ls-remote timeout at most once per TTL
    // rather than on every `cargo mock`. The cost is that recovery after a
    // transient failure waits until the next window, which is acceptable for a
    // freshness convenience the user can always force with `cargo update`.
    touch(&marker);

    // Auto-advancing mutates the consumer's tracked Cargo.lock. That is a
    // deliberate default (`[proxy] auto_update` defaults to true): tracking a
    // branch is a statement that the dependency should follow it, and a lock
    // that never advances turns that statement into a lie. The mutation is
    // throttled, interactive-only, reversible, and reported with its opt-out.
    let auto = allow_update && proxy_auto_update(&mock_dir.join("mockspace.toml"));

    for (name, source) in deps {
        let Some(branch) = source.branch.clone() else {
            continue;
        };
        let Some(remote) = git_ls_remote_head(&source.url, &branch) else {
            continue; // offline, auth-less, or the ref is gone: skip quietly.
        };
        if remote == source.rev {
            continue; // current.
        }

        let (locked_short, remote_short) = (short_rev(&source.rev), short_rev(&remote));
        if auto {
            match cargo_update_dep(mock_dir, &name, &source) {
                Ok(()) => actions.push(format!(
                    "{name} was behind origin/{branch} ({locked_short} -> {remote_short}); ran cargo update (set [proxy] auto_update = false to only warn)"
                )),
                Err(e) => actions.push(format!(
                    "{name} is behind origin/{branch} ({locked_short} -> {remote_short}); cargo update failed: {e}; run cargo update with the full source spec manually"
                )),
            }
        } else {
            actions.push(format!(
                "{name} is behind origin/{branch} (locked {locked_short}, remote {remote_short})"
            ));
        }
    }
}

/// First seven characters of a git revision, for readable log lines.
fn short_rev(rev: &str) -> String {
    rev.chars().take(7).collect()
}

/// True when no check has run within `ttl` (marker missing or older than `ttl`).
fn remote_check_due(marker: &Path, ttl: std::time::Duration) -> bool {
    match fs::metadata(marker).and_then(|m| m.modified()) {
        Ok(t) => t.elapsed().map(|e| e > ttl).unwrap_or(true),
        Err(_) => true,
    }
}

/// Record that a check ran now, by writing the marker (creating its dir).
fn touch(marker: &Path) {
    if let Some(parent) = marker.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(marker, b"");
}

/// The maximum time a remote-head query may take before it is abandoned.
const LS_REMOTE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

/// The remote head revision of `branch` at `url`, or `None` on any failure.
///
/// Never prompts and never hangs: `GIT_TERMINAL_PROMPT=0` suppresses credential
/// prompts, the SSH command runs in batch mode with a short connect timeout,
/// and the whole invocation is bounded by [`LS_REMOTE_TIMEOUT`] so a slow or
/// black-holed remote over any transport (including HTTP, which the SSH connect
/// timeout does not cover) returns `None` rather than blocking.
fn git_ls_remote_head(url: &str, branch: &str) -> Option<String> {
    // Defence in depth against argv flag smuggling. The url and branch come from
    // Cargo.lock, normally cargo-generated and trusted, but a crafted lock must
    // not be able to pass a leading-dash value that git parses as an option. The
    // `--` separator ends option parsing; the explicit reject refuses the
    // pathological case outright rather than relying on the separator alone.
    if url.starts_with('-') || branch.starts_with('-') {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes -o ConnectTimeout=5")
        // Bound an HTTP transfer that stalls below 1 byte/s for 5s; the overall
        // timeout below is the real backstop for any transport.
        .env("GIT_HTTP_LOW_SPEED_LIMIT", "1")
        .env("GIT_HTTP_LOW_SPEED_TIME", "5")
        .args(["ls-remote", "--", url, &format!("refs/heads/{branch}")]);
    let output = output_with_timeout(cmd, LS_REMOTE_TIMEOUT)?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line is "<sha>\t<ref>"; take the first field of the first line.
    stdout.split_whitespace().next().map(str::to_string)
}

/// Run `cmd` and return its output, or `None` if it fails to spawn or exceeds
/// `timeout` (in which case the child is killed).
///
/// The child's output is expected to be small (a ref line), so its pipes are
/// drained only after exit; a command that floods stdout is not a use here.
fn output_with_timeout(
    mut cmd: std::process::Command,
    timeout: std::time::Duration,
) -> Option<std::process::Output> {
    use std::process::Stdio;
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok()?;
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return None,
        }
    }
}

/// Advance the locked mockspace to its branch head.
///
/// Addressed by the full source spec rather than the bare package name. A lock
/// can hold more than one `mockspace` (a project that once tracked `main` and
/// now tracks `dev` keeps both entries), and cargo rejects the bare name as
/// ambiguous. The auto-advance then reported being behind on every run and
/// never advanced, which reads as the feature not working rather than as one
/// stale lock entry.
fn cargo_update_dep(mock_dir: &Path, name: &str, source: &GitSource) -> Result<(), String> {
    let spec = match &source.branch {
        Some(b) => format!("{}?branch={}#{name}", source.url, b),
        None => format!("{}#{name}", source.url),
    };
    let output = std::process::Command::new("cargo")
        .args(["update", "-p", &spec])
        .current_dir(mock_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Parse the `[lint-crates]` section from mockspace.toml.
///
/// Returns a list of (crate_name, cargo_dep_spec_as_toml_string) pairs in
/// declaration order. Each value is re-emitted verbatim into the proxy's
/// Cargo.toml so any cargo-accepted dep form works: `"0.1"`, `{ path = ... }`,
/// `{ git = ..., branch = ... }`, etc.
///
/// Returns empty vec if mockspace.toml is missing, unparseable, or has no
/// `[lint-crates]` section.
fn parse_lint_crates(mockspace_toml: &Path) -> Vec<(String, String)> {
    let content = match fs::read_to_string(mockspace_toml) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let section = match doc.get("lint-crates").and_then(|i| i.as_table()) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for (name, item) in section.iter() {
        // Value form (string like "0.1" or inline table `{ path = ... }`).
        if let Some(v) = item.as_value() {
            result.push((name.to_string(), v.to_string().trim().to_string()));
            continue;
        }
        // Sub-table form: [lint-crates.foo]\n path = "..."
        if let Some(tbl) = item.as_table() {
            // Re-emit as an inline table so it fits on the [dependencies] line.
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in tbl.iter() {
                if let Some(val) = v.as_value() {
                    inline.insert(k, val.clone());
                }
            }
            result.push((name.to_string(), inline.to_string().trim().to_string()));
        }
    }
    result
}

/// Discover `.rs` files in the custom lints directory.
/// Returns a sorted list of file stems (e.g., "my_lint" from "my_lint.rs").
fn discover_custom_lint_files(lints_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if !lints_dir.is_dir() {
        return files;
    }

    if let Ok(entries) = fs::read_dir(lints_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    let stem_str = stem.to_string_lossy().to_string();
                    if is_valid_rust_ident(&stem_str) {
                        files.push(stem_str);
                    } else {
                        eprintln!(
                            "warning: skipping custom lint file `{}`: stem `{}` is not a valid Rust identifier (only [a-z0-9_] allowed)",
                            path.display(),
                            stem_str,
                        );
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// Check if a string is a valid Rust identifier (only `[a-z0-9_]`, must not start with a digit).
fn is_valid_rust_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.as_bytes()[0];
    if first.is_ascii_digit() {
        return false;
    }
    s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Scan a `.rs` file to determine which custom lint functions it defines.
///
/// Looks for `pub fn lint()` and `pub fn cross_lint()` signatures.
fn scan_lint_functions(lints_dir: &Path, stem: &str) -> (bool, bool) {
    let path = lints_dir.join(format!("{stem}.rs"));
    let content = fs::read_to_string(&path).unwrap_or_default();

    let has_lint = content.contains("pub fn lint(");
    let has_cross_lint = content.contains("pub fn cross_lint(");

    (has_lint, has_cross_lint)
}

/// Generate the proxy's main.rs with custom lint module includes.
///
/// In-tree lint files: each `.rs` file under `{mock_dir}/lints/` is included
/// via `#[path]` attribute. Each file must define:
/// - `pub fn lint() -> Box<dyn mockspace_lint_rules::Lint>` for per-crate lints
/// - `pub fn cross_lint() -> Box<dyn mockspace_lint_rules::CrossCrateLint>` for cross-crate lints
///
/// External lint packs: each crate named in `[lint-crates]` is pulled in as
/// a normal cargo dependency. Each pack must expose:
/// - `pub fn lints() -> Vec<Box<dyn mockspace_lint_rules::Lint>>`
/// - `pub fn cross_lints() -> Vec<Box<dyn mockspace_lint_rules::CrossCrateLint>>`
fn generate_custom_lint_main(
    lint_files: &[String],
    lints_dir: &Path,
    lint_packs: &[(String, String)],
) -> String {
    let mut out = String::new();

    // Module declarations with absolute paths (forward slashes for cross-platform compat)
    for name in lint_files {
        let abs_path = lints_dir.join(format!("{name}.rs"));
        let path_str = abs_path.display().to_string().replace('\\', "/");
        out.push_str(&format!(
            "#[path = \"{path_str}\"]\nmod {name};\n",
        ));
    }
    out.push('\n');

    // Scan each file to determine which functions it provides
    let mut lint_mods = Vec::new();
    let mut cross_lint_mods = Vec::new();

    for name in lint_files {
        let (has_lint, has_cross_lint) = scan_lint_functions(lints_dir, name);
        if has_lint {
            lint_mods.push(name.as_str());
        }
        if has_cross_lint {
            cross_lint_mods.push(name.as_str());
        }
    }

    // Cargo names with `-` become `_` for Rust paths.
    let pack_idents: Vec<String> = lint_packs
        .iter()
        .map(|(name, _)| name.replace('-', "_"))
        .collect();

    // custom_lints() function
    out.push_str("fn custom_lints() -> Vec<Box<dyn mockspace::Lint>> {\n");
    out.push_str("    let mut v: Vec<Box<dyn mockspace::Lint>> = Vec::new();\n");
    for name in &lint_mods {
        out.push_str(&format!("    v.push({name}::lint());\n"));
    }
    for ident in &pack_idents {
        out.push_str(&format!("    v.extend({ident}::lints());\n"));
    }
    out.push_str("    v\n");
    out.push_str("}\n\n");

    // custom_cross_lints() function
    out.push_str("fn custom_cross_lints() -> Vec<Box<dyn mockspace::CrossCrateLint>> {\n");
    out.push_str("    let mut v: Vec<Box<dyn mockspace::CrossCrateLint>> = Vec::new();\n");
    for name in &cross_lint_mods {
        out.push_str(&format!("    v.push({name}::cross_lint());\n"));
    }
    for ident in &pack_idents {
        out.push_str(&format!("    v.extend({ident}::cross_lints());\n"));
    }
    out.push_str("    v\n");
    out.push_str("}\n\n");

    out.push_str("fn main() -> std::process::ExitCode {\n");
    out.push_str("    mockspace::run_with_custom_lints(custom_lints(), custom_cross_lints())\n");
    out.push_str("}\n");

    out
}

// ──────────────────────────────────────────────────────────────────────
// Generated hooks (core.hooksPath target)
// ──────────────────────────────────────────────────────────────────────

/// Where generated hooks live. Build artifact, gitignored.
fn generated_hooks_dir(mock_dir: &Path) -> PathBuf {
    mock_dir.join("target").join("hooks")
}

/// Resolve the actual .git directory (handles worktrees).
fn resolve_git_dir(repo_root: &Path) -> PathBuf {
    let git_path = repo_root.join(".git");
    if git_path.is_file() {
        // Worktree: .git file contains "gitdir: <path>"
        if let Ok(content) = fs::read_to_string(&git_path) {
            if let Some(gitdir) = content.trim().strip_prefix("gitdir: ") {
                return PathBuf::from(gitdir.trim());
            }
        }
    }
    git_path
}

fn ensure_generated_hooks(repo_root: &Path, mock_dir: &Path, actions: &mut Vec<String>) {
    let out_dir = generated_hooks_dir(mock_dir);
    let _ = fs::create_dir_all(&out_dir);

    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| mock_dir.display().to_string());

    // Path from the generated hooks dir to .git/hooks/ (the user's hooks).
    // The generated hooks source these so user logic always runs.
    let git_dir = resolve_git_dir(repo_root);
    let user_hooks_dir = git_dir.join("hooks");

    for hook_name in HOOK_NAMES {
        let path = out_dir.join(hook_name);
        let user_hook = user_hooks_dir.join(hook_name);

        let content = gen_hook(hook_name, &mock_rel, &user_hook);
        let fingerprint = content_fingerprint(&content);
        let fp_line = format!("{MANAGED_MARKER} v{HOOK_VERSION} fp:{fingerprint:016x}");

        // Skip if already up-to-date.
        if path.exists() {
            if let Ok(current) = fs::read_to_string(&path) {
                if current.contains(&fp_line) {
                    continue;
                }
            }
        }

        let final_content = content.replacen(MANAGED_MARKER, &fp_line, 1);

        if let Err(e) = fs::write(&path, &final_content) {
            actions.push(format!("failed to write {hook_name} hook: {e}"));
            continue;
        }

        #[cfg(unix)]
        {
            if let Ok(meta) = fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(&path, perms);
            }
        }

        actions.push(format!("generated {hook_name} hook"));
    }
}

fn check_activation(repo_root: &Path, mock_dir: &Path, actions: &mut Vec<String>) {
    // Opt-out for CI and sandboxed environments where git config edits are
    // unwanted. Set `MOCKSPACE_NO_AUTO_ACTIVATE=1` to skip auto-activation.
    let opt_out = std::env::var("MOCKSPACE_NO_AUTO_ACTIVATE").is_ok();

    if is_active(repo_root) {
        // Verify it points to the right directory. The is_active check
        // accepts any path containing "mockspace" or "target/hooks", so the
        // value can be stale after a repo rename or path move. Update in
        // place when it differs from the canonical generated_hooks_dir.
        // Respects MOCKSPACE_NO_AUTO_ACTIVATE the same way as initial
        // activation: if the user opted out, just warn.
        let expected = generated_hooks_dir(mock_dir);
        let output = std::process::Command::new("git")
            .args(["config", "--local", "core.hooksPath"])
            .current_dir(repo_root)
            .output();

        if let Ok(o) = output {
            let current_path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let expected_str = expected.display().to_string();
            if current_path != expected_str {
                if opt_out {
                    actions.push(format!(
                        "core.hooksPath stale ({current_path} vs {expected_str}); \
                         auto-update opted out via MOCKSPACE_NO_AUTO_ACTIVATE; \
                         run `cargo mock activate` manually"
                    ));
                } else {
                    match activate(repo_root, mock_dir) {
                        Ok(()) => actions.push(format!(
                            "core.hooksPath updated from {current_path} to {expected_str}"
                        )),
                        Err(e) => actions.push(format!(
                            "core.hooksPath stale ({current_path} vs {expected_str}); auto-update failed: {e}"
                        )),
                    }
                }
            }
        }
        return;
    }

    if opt_out {
        actions.push(
            "mockspace hooks not active (auto-activate opted out via \
             MOCKSPACE_NO_AUTO_ACTIVATE; run `cargo mock activate` manually)".into(),
        );
        return;
    }

    // Auto-activate. Only if `.git` is present (it was checked earlier in
    // bootstrap_from_buildscript, but re-check defensively) and the user
    // hasn't set `core.hooksPath` to a non-mockspace directory.
    if !repo_root.join(".git").exists() {
        actions.push("mockspace hooks not active (no .git directory)".into());
        return;
    }

    let existing = std::process::Command::new("git")
        .args(["config", "--local", "--get", "core.hooksPath"])
        .current_dir(repo_root)
        .output();
    if let Ok(o) = existing {
        if o.status.success() {
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !path.is_empty()
                && !path.contains("mockspace")
                && !path.contains("target/hooks")
            {
                actions.push(format!(
                    "mockspace hooks not active: core.hooksPath already points at \
                     {path} (non-mockspace); not overwriting. Run \
                     `cargo mock activate` to take over (or unset core.hooksPath)."
                ));
                return;
            }
        }
    }

    match activate(repo_root, mock_dir) {
        Ok(()) => {
            actions.push("activated mockspace hooks (core.hooksPath set)".into());
        }
        Err(e) => {
            actions.push(format!(
                "mockspace hooks not active (auto-activate failed: {e}; \
                 run `cargo mock activate` manually)"
            ));
        }
    }
}

// ──────────────────────────────────────────────────────────────────────
// Hook templates
// ──────────────────────────────────────────────────────────────────────

fn gen_hook(name: &str, mock_rel: &str, user_hook: &Path) -> String {
    match name {
        "pre-commit" => gen_pre_commit(mock_rel, user_hook),
        "pre-push" => gen_pre_push(mock_rel, user_hook),
        _ => String::new(),
    }
}

/// Generate the source-user-hook preamble. This runs the user's original
/// `.git/hooks/<name>` if it exists, so their hooks always execute
/// regardless of whether mockspace is active.
fn source_user_hook(user_hook: &Path) -> String {
    let path = user_hook.display();
    format!(
        r#"# Run the user's original hook if it exists.
USER_HOOK="{path}"
if [ -x "$USER_HOOK" ]; then
    "$USER_HOOK" "$@" || exit $?
fi
"#
    )
}

fn gen_pre_commit(mock_rel: &str, user_hook: &Path) -> String {
    let user_section = source_user_hook(user_hook);

    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# Generated by mockspace. User hooks sourced from .git/hooks/.

set -e

{user_section}
MOCK_DIR="{mock_rel}"

# Only run mockspace validation when staged files touch the mock workspace.
STAGED=$(git diff --cached --name-only -- "$MOCK_DIR" 2>/dev/null || true)
[ -z "$STAGED" ] && exit 0

echo "pre-commit: mockspace changes detected, running validation..."

# Extract changed crate names from staged paths.
CHANGED_CRATES=$(echo "$STAGED" \
    | grep "^$MOCK_DIR/crates/" \
    | sed "s|^$MOCK_DIR/crates/||" \
    | cut -d/ -f1 \
    | sort -u \
    | tr '\n' ',' \
    | sed 's/,$//' \
    || true)

ARGS=(--lint-only --commit)

if [ -n "$CHANGED_CRATES" ]; then
    STAGED_RS=$(echo "$STAGED" \
        | grep "^$MOCK_DIR/crates/.*\.rs$" \
        || true)

    if [ -z "$STAGED_RS" ]; then
        echo "  crates: $CHANGED_CRATES (doc-only)"
        ARGS+=(--scope "$CHANGED_CRATES" --doc-only)
    else
        echo "  crates: $CHANGED_CRATES"
        ARGS+=(--scope "$CHANGED_CRATES")
    fi
else
    echo "  infrastructure-only (no crate files staged)"
    ARGS+=(--scope infra)
fi

if ! cargo mock "${{ARGS[@]}}" 2>&1; then
    echo ""
    echo "BLOCKED: mockspace validation failed."
    exit 1
fi

echo "pre-commit: validation passed."
"##
    )
}

fn gen_pre_push(mock_rel: &str, user_hook: &Path) -> String {
    let user_section = source_user_hook(user_hook);

    format!(
        r##"#!/usr/bin/env bash
{MANAGED_MARKER}
# Generated by mockspace. User hooks sourced from .git/hooks/.

set -e

{user_section}
MOCK_DIR="{mock_rel}"

echo "pre-push: running mockspace validation..."

# Compute changed crates between remote and local across every ref being
# pushed. Git pre-push hooks receive `<local-ref> <local-sha> <remote-ref>
# <remote-sha>` lines on stdin (per `git help githooks`).
#
# - Existing branches: diff `<remote-sha>..<local-sha>` for crate-path
#   changes; union across all pushed refs.
# - New branches (remote-sha = all zeros): no upstream reference; fall
#   back to full project scope so we don't miss anything.
# `set -e` is in effect from the prelude. `&&-continue` short-circuit
# in the loop body would exit the script if the left side is false, so
# use explicit `if` blocks for every short-circuit.
#
# Empty stdin (no refs piped, rare with `push --tags` variants on some
# git versions) → the loop simply does not run; the post-loop fall-
# through handles full-scope correctly.
NEW_BRANCH=0
CHANGED_CRATES=""
while IFS=' ' read -r _local_ref local_sha _remote_ref remote_sha; do
    if [ -z "$local_sha" ]; then
        continue
    fi
    # Delete-only push (local_sha all zeros): nothing to lint.
    if [ "$local_sha" = "0000000000000000000000000000000000000000" ]; then
        continue
    fi
    if [ "$remote_sha" = "0000000000000000000000000000000000000000" ]; then
        NEW_BRANCH=1
        break
    fi
    # Unknown remote_sha (we never fetched the remote, or it's a stale
    # sha that no longer exists locally): cat-file -e returns non-zero.
    # Treat as new-branch-equivalent and force full scope; otherwise
    # `git diff` would fail closed and silently drop this ref's changes.
    if ! git cat-file -e "$remote_sha" 2>/dev/null; then
        NEW_BRANCH=1
        break
    fi
    PUSH_CHANGED=$(git diff --name-only "$remote_sha".."$local_sha" -- "$MOCK_DIR/crates/" 2>/dev/null \
        | sed "s|^$MOCK_DIR/crates/||" \
        | cut -d/ -f1 \
        | sort -u \
        | tr '\n' ',' \
        | sed 's/,$//' \
        || true)
    if [ -z "$PUSH_CHANGED" ]; then
        continue
    fi
    if [ -z "$CHANGED_CRATES" ]; then
        CHANGED_CRATES="$PUSH_CHANGED"
    else
        CHANGED_CRATES="$CHANGED_CRATES,$PUSH_CHANGED"
    fi
done

# De-duplicate the comma-joined crate list (multiple refs may touch the
# same crate). tr-sort-uniq round trip.
if [ -n "$CHANGED_CRATES" ]; then
    CHANGED_CRATES=$(echo "$CHANGED_CRATES" \
        | tr ',' '\n' \
        | sort -u \
        | grep -v '^$' \
        | tr '\n' ',' \
        | sed 's/,$//')
fi

if grep -rq "Nuked by" "$MOCK_DIR/crates/"*/src/lib.rs 2>/dev/null; then
    echo "  nuked workspace: skipping source checks"
    ARGS=(--lint-only --strict --doc-only)
elif [ "$NEW_BRANCH" = "1" ] || [ -z "$CHANGED_CRATES" ]; then
    echo "  scope: full project ($([ "$NEW_BRANCH" = "1" ] && echo "new branch" || echo "no crate changes"))"
    ARGS=(--lint-only --strict)
else
    echo "  scope: $CHANGED_CRATES"
    ARGS=(--lint-only --strict --scope "$CHANGED_CRATES")
fi

if ! cargo mock "${{ARGS[@]}}" 2>&1; then
    echo ""
    echo "BLOCKED: mockspace validation failed."
    exit 1
fi

echo "pre-push: validation passed."
"##
    )
}

// ──────────────────────────────────────────────────────────────────────
// Utilities
// ──────────────────────────────────────────────────────────────────────

fn content_fingerprint(content: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in content.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

fn find_ancestor_with(start: &Path, target_name: &str) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(target_name).exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod lint_crates_tests {
    use super::*;

    fn write_toml(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mockspace.toml");
        fs::write(&path, contents).unwrap();
        (dir, path)
    }

    #[test]
    fn missing_file_returns_empty() {
        let result = parse_lint_crates(Path::new("/definitely/does/not/exist"));
        assert!(result.is_empty());
    }

    #[test]
    fn gen_pre_push_passes_bash_syntax_check() {
        // Run `bash -n` on the generated script to catch syntax errors
        // (unbalanced quotes, missing fi/done, format-string slips). Does
        // not execute the script, just parses it.
        let script = gen_pre_push("mock", Path::new("/dev/null"));
        let mut path = std::env::temp_dir();
        path.push(format!(
            "mockspace_pre_push_test_{}.sh",
            std::process::id()
        ));
        std::fs::write(&path, &script).unwrap();
        let output = std::process::Command::new("bash")
            .arg("-n")
            .arg(&path)
            .output()
            .expect("bash -n");
        let _ = std::fs::remove_file(&path);
        assert!(
            output.status.success(),
            "bash -n rejected the generated pre-push hook:\n{}\n--- script ---\n{}",
            String::from_utf8_lossy(&output.stderr),
            script
        );
    }

    #[test]
    fn gen_pre_push_includes_scope_branch() {
        // Sanity check: the generated script names the new-branch /
        // changed-crates scopes the way the hook expects.
        let script = gen_pre_push("mock", Path::new("/dev/null"));
        assert!(
            script.contains("CHANGED_CRATES"),
            "expected CHANGED_CRATES var in generated pre-push hook"
        );
        assert!(
            script.contains("NEW_BRANCH"),
            "expected NEW_BRANCH var in generated pre-push hook"
        );
        assert!(
            script.contains("--scope"),
            "expected --scope flag in generated pre-push hook"
        );
    }

    #[test]
    fn absent_section_returns_empty() {
        let (_dir, path) = write_toml("project_name = \"foo\"\n");
        assert!(parse_lint_crates(&path).is_empty());
    }

    #[test]
    fn inline_table_form() {
        let toml = r#"
[lint-crates]
foo-pack = { path = "../foo-pack" }
bar-pack = { git = "https://example.com/bar.git", branch = "main" }
"#;
        let (_dir, path) = write_toml(toml);
        let result = parse_lint_crates(&path);
        assert_eq!(result.len(), 2);
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"foo-pack"));
        assert!(names.contains(&"bar-pack"));
        for (_, spec) in &result {
            assert!(spec.starts_with('{') && spec.ends_with('}'), "got: {spec}");
        }
    }

    #[test]
    fn version_string_form() {
        let toml = r#"
[lint-crates]
foo-pack = "0.1.2"
"#;
        let (_dir, path) = write_toml(toml);
        let result = parse_lint_crates(&path);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "foo-pack");
        assert_eq!(result[0].1, "\"0.1.2\"");
    }

    #[test]
    fn sub_table_form_rendered_as_inline() {
        let toml = r#"
[lint-crates.foo-pack]
path = "../foo-pack"
version = "0.1"
"#;
        let (_dir, path) = write_toml(toml);
        let result = parse_lint_crates(&path);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "foo-pack");
        let spec = &result[0].1;
        assert!(spec.starts_with('{'), "got: {spec}");
        assert!(spec.contains("path"), "got: {spec}");
        assert!(spec.contains("version"), "got: {spec}");
    }
}

#[cfg(test)]
mod gitignore_tests {
    use super::*;

    #[test]
    fn adds_catch_all_target_to_empty_repo() {
        let dir = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        ensure_gitignore(dir.path(), &mut actions);
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(
            content.lines().any(|l| l.trim() == "target/"),
            "expected a catch-all `target/` line, got:\n{content}"
        );
        assert_eq!(actions.len(), 1, "expected one action recorded");
    }

    #[test]
    fn preserves_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "/target\n.DS_Store\n*.swp\n").unwrap();
        let mut actions = Vec::new();
        ensure_gitignore(dir.path(), &mut actions);
        let content = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(content.contains(".DS_Store"), "clobbered existing entries:\n{content}");
        assert!(content.contains("*.swp"), "clobbered existing entries:\n{content}");
        assert!(
            content.lines().any(|l| l.trim() == "target/"),
            "did not add catch-all target/:\n{content}"
        );
    }

    #[test]
    fn idempotent_when_catch_all_present() {
        let dir = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        ensure_gitignore(dir.path(), &mut actions);
        let after_first = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        let mut actions2 = Vec::new();
        ensure_gitignore(dir.path(), &mut actions2);
        let after_second = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert_eq!(after_first, after_second, "second run mutated the file");
        assert!(actions2.is_empty(), "second run recorded an action: {actions2:?}");
    }
}

#[cfg(test)]
mod proxy_pin_tests {
    use super::*;

    const LOCK_GIT: &str = r#"
version = 4

[[package]]
name = "arvo"
version = "0.1.0"
source = "git+ssh://git@github.com/orgrinrt/arvo.git?branch=dev#f5cf3063aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[package]]
name = "mockspace"
version = "0.1.0"
source = "git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#d50b59cd461f12958ebfcc3a6a19a7c62d1a472b"
"#;

    #[test]
    fn extracts_git_rev_for_mockspace() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(&lock, LOCK_GIT).unwrap();
        assert_eq!(
            mockspace_rev_from_lock(&lock).as_deref(),
            Some("d50b59cd461f12958ebfcc3a6a19a7c62d1a472b")
        );
    }

    #[test]
    fn path_source_yields_no_rev() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        // A path/workspace dependency has no `source` line at all.
        fs::write(
            &lock,
            "version = 4\n\n[[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(mockspace_rev_from_lock(&lock), None);
    }

    #[test]
    fn missing_lock_yields_no_rev() {
        assert_eq!(mockspace_rev_from_lock(Path::new("/no/such/Cargo.lock")), None);
    }

    #[test]
    fn find_git_checkout_matches_by_rev_prefix() {
        // Fake a git/checkouts/mockspace-<hash>/<short-rev>/ tree.
        let checkouts = tempfile::tempdir().unwrap();
        let checkout = checkouts.path().join("mockspace-abc123def4560789/d50b59c");
        fs::create_dir_all(&checkout).unwrap();
        // A sibling source that must not match.
        fs::create_dir_all(checkouts.path().join("arvo-999/f5cf306")).unwrap();

        let found = find_git_checkout_in(
            checkouts.path(),
            "mockspace",
            "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b",
        );
        assert_eq!(found.as_deref(), Some(checkout.as_path()));
    }

    #[test]
    fn find_git_checkout_none_when_absent() {
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            find_git_checkout_in(empty.path(), "mockspace", "d50b59cd461f"),
            None
        );
    }

    #[test]
    fn find_git_checkout_does_not_match_a_differently_named_repo() {
        // `mockspace-hilavitkutin-stack-lints-<hash>` shares the `mockspace-`
        // prefix but is a different repo; it must not match.
        let checkouts = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            checkouts
                .path()
                .join("mockspace-hilavitkutin-stack-lints-e5dc0929ff6a2451/d50b59c"),
        )
        .unwrap();
        assert_eq!(
            find_git_checkout_in(
                checkouts.path(),
                "mockspace",
                "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b"
            ),
            None,
            "a sibling repo sharing the name prefix must not match"
        );
    }

    #[test]
    fn resolve_falls_back_on_path_source() {
        // A path/workspace mockspace has no git rev, so the resolver returns
        // the fallback (the baked path) unchanged, with no environment access.
        let mock = tempfile::tempdir().unwrap();
        fs::write(
            mock.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        assert_eq!(
            resolve_mockspace_pin(mock.path(), fallback.path(), &mut actions),
            fallback.path()
        );
        // No git rev, so no "checkout absent" action: this is a clean fallback.
        assert!(actions.is_empty());
    }

    #[test]
    fn resolve_reports_absent_checkout_for_a_git_rev() {
        // The lock names a git rev whose checkout does not exist, so the
        // resolver falls back AND records the degraded case rather than hiding
        // it. The rev is all-f so it cannot collide with a real checkout under
        // the machine's CARGO_HOME (find_git_checkout reads the real cache).
        let mock = tempfile::tempdir().unwrap();
        fs::write(
            mock.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\nsource = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#ffffffffffffffffffffffffffffffffffffffff\"\n",
        )
        .unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        let resolved = resolve_mockspace_pin(mock.path(), fallback.path(), &mut actions);
        assert_eq!(resolved, fallback.path());
        assert!(
            actions.iter().any(|a| a.contains("checkout absent")),
            "the absent-checkout case must be reported, got {actions:?}"
        );
    }

    #[test]
    fn pinned_path_is_extracted_from_proxy_cargo() {
        let cargo = "[package]\nname = \"mockspace-proxy\"\n\n[dependencies]\nmockspace = { path = \"/some/where/mockspace\" }\n";
        assert_eq!(
            pinned_mockspace_path(cargo).as_deref(),
            Some("/some/where/mockspace")
        );
        assert_eq!(pinned_mockspace_path("no pin here"), None);
    }
}

#[cfg(test)]
mod remote_head_tests {
    use super::*;

    #[test]
    fn parses_branch_source() {
        let s = parse_git_source(
            "git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#d50b59cd461f12958ebfcc3a6a19a7c62d1a472b",
        )
        .unwrap();
        assert_eq!(s.url, "ssh://git@github.com/hiisi-digital/mockspace.git");
        assert_eq!(s.branch.as_deref(), Some("dev"));
        assert_eq!(s.rev, "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b");
    }

    #[test]
    fn parses_source_without_branch() {
        // A bare git dep (default branch) has no `branch=` query, so there is
        // no moving target to track.
        let s = parse_git_source("git+https://example.com/mockspace.git#abcdef0").unwrap();
        assert_eq!(s.url, "https://example.com/mockspace.git");
        assert_eq!(s.branch, None);
        assert_eq!(s.rev, "abcdef0");
    }

    #[test]
    fn rejects_non_git_source() {
        assert_eq!(parse_git_source("registry+https://crates.io/#1.0.0"), None);
        assert_eq!(parse_git_source("git+https://example.com/no-rev.git"), None);
    }

    #[test]
    fn tag_and_rev_pins_have_no_branch() {
        // A tag or exact-rev pin is not a moving target, so branch is None and
        // ensure_mockspace_current early-returns on it.
        let tag = parse_git_source("git+https://e.com/m.git?tag=v1.2.3#abcdef0").unwrap();
        assert_eq!(tag.branch, None);
        assert_eq!(tag.rev, "abcdef0");
        let rev = parse_git_source("git+https://e.com/m.git?rev=abcdef0#abcdef0").unwrap();
        assert_eq!(rev.branch, None);
    }

    #[test]
    fn multi_param_query_extracts_branch() {
        // A query may carry several &-separated params in any order; the branch
        // is found among them.
        let s = parse_git_source("git+https://e.com/m.git?rev=x&branch=main#deadbee").unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.rev, "deadbee");
    }

    #[test]
    fn every_branch_tracked_dep_is_found_not_only_mockspace() {
        // The freshness problem is not mockspace's. Any dependency tracking a
        // branch is a moving target whose lock nothing advances.
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(
            &lock,
            "version = 4\n\n             [[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\n             source = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#d50b59cd461f12958ebfcc3a6a19a7c62d1a472b\"\n\n             [[package]]\nname = \"arvo\"\nversion = \"0.1.0\"\n             source = \"git+ssh://git@github.com/orgrinrt/arvo.git?branch=dev#aaaa59cd461f12958ebfcc3a6a19a7c62d1a472b\"\n\n             [[package]]\nname = \"pinned\"\nversion = \"0.1.0\"\n             source = \"git+ssh://git@example.com/pinned.git?tag=v1#bbbb59cd461f12958ebfcc3a6a19a7c62d1a472b\"\n\n             [[package]]\nname = \"local\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let deps = branch_tracked_git_deps(&lock);
        let names: Vec<&str> = deps.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["mockspace", "arvo"], "{names:?}");

        // A tag and a path are pins someone chose. Advancing either would be
        // overriding a decision rather than honouring one.
        assert!(!names.contains(&"pinned"));
        assert!(!names.contains(&"local"));

        assert_eq!(deps[0].1.branch.as_deref(), Some("dev"));
        assert_eq!(deps[0].1.rev, "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b");
    }

    #[test]
    fn a_lock_with_no_git_deps_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(&lock, "version = 4\n\n[[package]]\nname = \"local\"\nversion = \"0.1.0\"\n").unwrap();
        assert!(branch_tracked_git_deps(&lock).is_empty());
        // A missing lock is a skip, not a panic.
        assert!(branch_tracked_git_deps(&dir.path().join("nope.lock")).is_empty());
    }

    #[test]
    fn auto_update_defaults_to_true() {
        let dir = tempfile::tempdir().unwrap();
        let toml = dir.path().join("mockspace.toml");
        // No [proxy] section: the default is auto.
        fs::write(&toml, "project_name = \"x\"\n").unwrap();
        assert!(proxy_auto_update(&toml));
        // Missing file: also the default.
        assert!(proxy_auto_update(&dir.path().join("nope.toml")));
    }

    #[test]
    fn auto_update_reads_explicit_false() {
        let dir = tempfile::tempdir().unwrap();
        let toml = dir.path().join("mockspace.toml");
        fs::write(&toml, "[proxy]\nauto_update = false\n").unwrap();
        assert!(!proxy_auto_update(&toml));
    }

    #[test]
    fn ls_remote_rejects_flag_smuggling_url() {
        // A leading-dash url or branch could be parsed by git as an option.
        // The guard refuses it before spawning, so no subprocess runs.
        assert_eq!(git_ls_remote_head("--upload-pack=touch pwned", "dev"), None);
        assert_eq!(git_ls_remote_head("ssh://ok.example/x.git", "-x"), None);
    }

    #[test]
    fn remote_check_due_respects_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("target/mockspace-proxy/.remote-check");
        // No marker yet: a check is due.
        assert!(remote_check_due(&marker, REMOTE_CHECK_TTL));
        touch(&marker);
        // Just touched: not due under a real TTL.
        assert!(!remote_check_due(&marker, REMOTE_CHECK_TTL));
        // Due under a zero TTL (any elapsed time exceeds it).
        assert!(remote_check_due(&marker, std::time::Duration::ZERO));
    }
}

#[cfg(test)]
mod proxy_freshness_tests {
    use super::*;

    #[test]
    fn re_pinning_discards_the_built_binary() {
        // The stale binary is the invisible half of a stale pin: the manifest
        // names one revision and the running code comes from another, so a
        // landed fix appears not to work.
        let tmp = tempfile::tempdir().unwrap();
        let proxy = tmp.path().join("mockspace-proxy");
        let debug = proxy.join("target").join("debug");
        fs::create_dir_all(&debug).unwrap();
        let bin = debug.join("mockspace-proxy");
        fs::write(&bin, b"stale").unwrap();
        // A sibling artifact stands in for the dependency compilation that is
        // still valid: removing it would make every re-pin a full rebuild.
        let dep = debug.join("libmockspace.rlib");
        fs::write(&dep, b"deps").unwrap();

        let mut actions = Vec::new();
        discard_proxy_binary(&proxy, &mut actions);

        assert!(!bin.exists(), "the built proxy survived a re-pin");
        assert!(dep.exists(), "unrelated build output was removed");
        assert_eq!(actions.len(), 1, "{actions:?}");
    }

    #[test]
    fn discarding_is_quiet_when_there_is_nothing_built() {
        // A first run has no binary. Reporting a discard that did not happen
        // would make the common case look like a recovery.
        let tmp = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        discard_proxy_binary(&tmp.path().join("mockspace-proxy"), &mut actions);
        assert!(actions.is_empty(), "{actions:?}");
    }
}
