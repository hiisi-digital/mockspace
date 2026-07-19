#![allow(unused_imports)]
use super::*;

/// Point `core.hooksPath` at the durable fallback hooks and record the
/// mock-dir locator the durable hook needs to find this repo's validator.
///
/// The durable dir lives in the user config home, so the gate survives a
/// `target/` clean that deletes the generated validator. Requires the
/// generated validator to exist (it is what the durable hook execs).
pub fn activate(repo_root: &Path, mock_dir: &Path) -> Result<(), String> {
    let generated = generated_hooks_dir(mock_dir);
    if !generated.exists() {
        return Err(format!(
            "generated hooks not found at {}. Run `cargo mock` first.",
            generated.display()
        ));
    }

    // Make sure the durable fallback exists before pointing git at it, and
    // surface any write diagnostics rather than swallowing them.
    let mut actions = Vec::new();
    ensure_durable_hooks(&mut actions);
    for msg in &actions {
        eprintln!("--- bootstrap: {msg} ---");
    }
    // Point at whatever `ensure_durable_hooks` actually produced: the same
    // choice `hooks_path_target` reports to `check_activation`, so the two
    // never disagree. `generated` remains the guaranteed floor.
    let hooks_dir = hooks_path_target(mock_dir);

    let status = std::process::Command::new("git")
        .args(["config", "--local", "core.hooksPath"])
        .arg(hooks_dir.to_str().unwrap_or(""))
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("git config failed: {e}"))?;
    if !status.success() {
        return Err("git config core.hooksPath failed".into());
    }

    // Record where this repo's mock workspace lives so the generic durable
    // hook can locate the live validator at `<root>/<mockdir>/target/hooks`.
    let mock_rel = mock_dir
        .strip_prefix(repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| mock_dir.display().to_string());
    let status = std::process::Command::new("git")
        .args(["config", "--local", "mockspace.mockdir"])
        .arg(&mock_rel)
        .current_dir(repo_root)
        .status()
        .map_err(|e| format!("git config failed: {e}"))?;
    if !status.success() {
        return Err("git config mockspace.mockdir failed".into());
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
        },
        _ => false,
    }
}

// ──────────────────────────────────────────────────────────────────────
// Cargo alias
// ──────────────────────────────────────────────────────────────────────

pub(crate) fn check_activation(repo_root: &Path, mock_dir: &Path, actions: &mut Vec<String>) {
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
        let expected = hooks_path_target(mock_dir);
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
             MOCKSPACE_NO_AUTO_ACTIVATE; run `cargo mock activate` manually)"
                .into(),
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
            if !path.is_empty() && !path.contains("mockspace") && !path.contains("target/hooks") {
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
        },
        Err(e) => {
            actions.push(format!(
                "mockspace hooks not active (auto-activate failed: {e}; \
                 run `cargo mock activate` manually)"
            ));
        },
    }
}

// ──────────────────────────────────────────────────────────────────────
// Hook templates
// ──────────────────────────────────────────────────────────────────────
