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


mod gitignore;
pub(crate) use gitignore::*;
mod activate;
pub(crate) use activate::*;
pub use activate::{activate, deactivate, is_active};
mod alias;
pub(crate) use alias::*;
mod proxy;
pub(crate) use proxy::*;
mod remote;
pub use remote::ensure_mockspace_current;
// The rest of `remote` (ls-remote head, TTL freshness helpers) is exercised
// only by the in-module tests; re-export it into scope for them.
#[cfg(test)]
pub(crate) use remote::*;
mod lints;
pub(crate) use lints::*;
mod hooks;
pub(crate) use hooks::*;
mod launcher;
pub(crate) use launcher::*;
mod durable;
pub(crate) use durable::*;
mod repair;
pub(crate) use repair::*;
#[cfg(test)]
mod lint_crates_tests;
#[cfg(test)]
mod gitignore_tests;
#[cfg(test)]
mod proxy_pin_tests;
#[cfg(test)]
mod remote_head_tests;
#[cfg(test)]
mod proxy_freshness_tests;
#[cfg(test)]
mod bootstrap_guard_tests;

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

    // Guard A: dependency builds out of the cargo cache must not bootstrap.
    // A git checkout under ~/.cargo/git/checkouts/ carries the full repo tree
    // (mockspace.toml AND .git), so the ancestor probes below cannot tell it
    // from a working repo. Installing there pollutes the shared cache, and
    // under sudo poisons it with root-owned files (2026-06/07 incident).
    // Skipping forfeits nothing: nobody commits from the cache, and a
    // consumer's working repo gets its hooks from its own build.rs.
    let cargo_home = resolve_cargo_home(env::var_os("CARGO_HOME"), env::var_os("HOME"));
    if is_inside_cargo_home(&build_crate_dir, cargo_home.as_deref()) {
        println!("cargo::warning=mockspace: dependency build inside the cargo cache, skipping bootstrap");
        return;
    }

    // Under sudo the bootstrap must still install its safeguards: skipping
    // would leave the repo working ungated, the one state mockspace exists
    // to prevent. Instead the install proceeds and every path it wrote is
    // chowned back to the invoking user (sudo publishes SUDO_UID/SUDO_GID,
    // and root may chown). A sudo environment too broken to identify the
    // user fails the build outright: a failed build blocks work with
    // safeguards intact, where a skipped bootstrap would permit unguarded
    // work. Fail closed, never open.
    let sudo_ids = match parse_sudo_ids(
        env::var_os("SUDO_USER"),
        env::var_os("SUDO_UID"),
        env::var_os("SUDO_GID"),
    ) {
        Ok(ids) => ids,
        Err(why) => panic!(
            "mockspace: running under sudo but cannot identify the invoking user ({why}). \
             Re-run the build without sudo; installing root-owned safeguards would brick \
             later unprivileged builds."
        ),
    };

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

    // Sudo repair: hand everything the bootstrap owns back to the invoking
    // user. Any failure here panics (fails the build): leaving a partial
    // root-owned install behind is the incident this exists to prevent.
    #[cfg(unix)]
    if let Some((uid, gid)) = sudo_ids {
        if let Err(why) = repair_ownership(&repo_root, &mock_dir, uid, gid) {
            panic!(
                "mockspace: bootstrap installed under sudo but could not return \
                 ownership to uid {uid} ({why}). Fix ownership manually or re-run \
                 the build without sudo."
            );
        }
        println!("cargo::warning=mockspace: sudo detected, bootstrap outputs chowned to uid {uid}");
    }
    #[cfg(not(unix))]
    let _ = sudo_ids;

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
    ensure_durable_hooks(&mut actions);
    ensure_launcher(&mut actions);
    ensure_gitignore(repo_root, &mut actions);
    check_activation(repo_root, mock_dir, &mut actions);

    actions
}

/// The slim, launcher-era setup the engine runs on a normal invocation,
/// replacing what the `build.rs` bootstrap did: ensure the durable hooks
/// exist, point `core.hooksPath` at them (with no user involvement), and keep
/// `target/` gitignored. None of the dissolved plumbing (proxy, `.cargo`
/// alias, launcher install, generated-hooks layer, build-cache guard).
///
/// Best-effort and quiet: setup failures never block the command the user
/// actually ran. Idempotent and cheap on the common path (fingerprint-guarded
/// hook writes, a single `git config` read).
pub fn ensure_gate(repo_root: &Path, mock_dir: &Path) {
    let mut actions = Vec::new();
    let target = hooks_path_target(mock_dir);
    let want = target.to_string_lossy();
    let current = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["config", "--local", "core.hooksPath"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if current.as_deref() != Some(want.as_ref()) {
        // not pointing at the durable gate yet: activate (also (re)writes the
        // durable hooks and records the mock-dir locator).
        let _ = activate(repo_root, mock_dir);
    } else {
        // already active: just keep the hook content current.
        ensure_durable_hooks(&mut actions);
    }
    ensure_gitignore(repo_root, &mut actions);
}

/// Where `core.hooksPath` should point: the durable home dir when a
/// config home exists, otherwise the generated (non-durable) dir. The
/// single source of truth so `activate` and `check_activation` agree.
fn hooks_path_target(mock_dir: &Path) -> PathBuf {
    // The durable dir only when it actually exists (was created by
    // `ensure_durable_hooks`). Resolvable-but-unwritable must fall back to
    // the generated dir, and `activate` must make the same choice, or
    // `check_activation` will re-activate every build chasing a target
    // `activate` never set.
    match durable_hooks_dir() {
        Some(d) if d.exists() => d,
        _ => generated_hooks_dir(mock_dir),
    }
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

