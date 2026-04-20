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
//!
//! # Custom lints
//!
//! Two mechanisms, both wired through the generated proxy crate in
//! `target/mockspace-proxy/`:
//!
//! 1. **In-tree lint files** — `.rs` files under `{mock_dir}/lints/`. Each
//!    file defines `pub fn lint()` and/or `pub fn cross_lint()` (singular,
//!    one lint per file). Good for quick project-specific rules.
//!
//! 2. **External lint-pack crates** — cargo dependencies declared under
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
    ensure_proxy_crate(repo_root, mock_dir, mockspace_dir, actions);

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

    let _ = fs::create_dir_all(&proxy_src);
    let _ = fs::write(&proxy_cargo, &cargo_content);
    let _ = fs::write(&proxy_main, &main_content);
    actions.push("generated target/mockspace-proxy/".into());
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
                            "warning: skipping custom lint file `{}` — stem `{}` is not a valid Rust identifier (only [a-z0-9_] allowed)",
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
