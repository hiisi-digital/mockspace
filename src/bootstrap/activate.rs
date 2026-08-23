//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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

    // Record where this repo's mock workspace lives so the agent hook can
    // locate the live validator at `<root>/<mockdir>/target/hooks`. The hook
    // reads it with a `mock` fallback, so only non-default layouts need it;
    // folding this into `mock locate` for every reader is task #21.
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

