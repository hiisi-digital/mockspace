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
//! mockspace bakes `env!("CARGO_MANIFEST_DIR")` at compile time — its own
//! source path, wherever cargo cached it. The cargo alias points `cargo mock`
//! at that path.
//!
//! # Hook model
//!
//! mockspace never touches `.git/hooks/`. Those are the user's hooks and
//! always run — with or without mockspace.
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
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set — call from build.rs"),
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
    check_activation(repo_root, mock_dir, &mut actions);

    actions
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
    ensure_proxy_crate(repo_root, mockspace_dir, actions);

    let config_dir = repo_root.join(".cargo");
    let config_path = config_dir.join("config.toml");

    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| mock_dir.display().to_string());

    // Both paths are relative to repo root — fully portable.
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
            // Stale — update in place.
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

    // Missing — add.
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
fn ensure_proxy_crate(repo_root: &Path, mockspace_dir: &Path, actions: &mut Vec<String>) {
    let proxy_dir = repo_root.join("target").join("mockspace-proxy");
    let proxy_cargo = proxy_dir.join("Cargo.toml");
    let proxy_src = proxy_dir.join("src");
    let proxy_main = proxy_src.join("main.rs");

    let cargo_content = format!(
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
    );

    let main_content = "\
        fn main() -> std::process::ExitCode {\n\
        \x20   mockspace::run()\n\
        }\n";

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

    let _ = fs::create_dir_all(&proxy_src);
    let _ = fs::write(&proxy_cargo, &cargo_content);
    let _ = fs::write(&proxy_main, main_content);
    actions.push("generated target/mockspace-proxy/".into());
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
    if is_active(repo_root) {
        // Verify it points to the right directory.
        let expected = generated_hooks_dir(mock_dir);
        let output = std::process::Command::new("git")
            .args(["config", "--local", "core.hooksPath"])
            .current_dir(repo_root)
            .output();

        if let Ok(o) = output {
            let current_path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            let expected_str = expected.display().to_string();
            if current_path != expected_str {
                actions.push(format!(
                    "core.hooksPath points to {current_path}, expected {expected_str}"
                ));
            }
        }
    } else {
        actions.push(
            "mockspace hooks not active (run `cargo mock activate` to enable)".into()
        );
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

if grep -rq "Nuked by" "$MOCK_DIR/crates/"*/src/lib.rs 2>/dev/null; then
    echo "  nuked workspace — skipping source checks"
    ARGS=(--lint-only --strict --doc-only)
else
    ARGS=(--lint-only --strict)
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
