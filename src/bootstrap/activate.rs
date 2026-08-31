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
        eprintln!("--- gate: {msg} ---");
    }
    // Point at whatever `ensure_durable_hooks` actually produced: the same
    // choice `hooks_path_target` reports to `check_activation`, so the two
    // never disagree. `generated` remains the guaranteed floor.
    let hooks_dir = hooks_path_target(mock_dir);

    // Whatever the repository had before this, kept so `deactivate` can put it
    // back. Without it, activating in a repository already on husky, lefthook or
    // pre-commit destroys that setting and nothing anywhere remembers what it
    // was, while the documentation promises deactivation is reversible.
    //
    // Only saved when there is something to save and it is not already ours, so
    // running `activate` twice does not overwrite the real previous value with
    // mockspace's own path.
    if let Some(existing) = local_config(repo_root, "core.hooksPath") {
        let ours = hooks_dir.to_str().unwrap_or("");
        if !existing.is_empty() && existing != ours {
            let _ = std::process::Command::new("git")
                .args(["config", "--local", PREVIOUS_HOOKS_PATH, &existing])
                .current_dir(repo_root)
                .status();
            eprintln!(
                "--- gate: core.hooksPath was {existing}; saved, and `mock deactivate` \
                 puts it back ---"
            );
        }
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

/// Where a repository's own `core.hooksPath` is kept while mockspace holds it.
///
/// Local to the repository, so it travels with the checkout that was changed
/// and with nothing else.
const PREVIOUS_HOOKS_PATH: &str = "mockspace.previousHooksPath";

/// Put `core.hooksPath` back the way [`activate`] found it.
///
/// Where the repository had its own value, that value is restored. Where it had
/// none, the key is unset and git falls back to `.git/hooks/`. **Those are
/// different outcomes and unsetting is only correct for the second**: doing it
/// unconditionally is what silently destroyed a husky or lefthook setting, while
/// the documentation said deactivation was reversible and git then fell back to
/// whatever the user already had.
pub fn deactivate(repo_root: &Path) -> Result<(), String> {
    if let Some(previous) = local_config(repo_root, PREVIOUS_HOOKS_PATH) {
        if !previous.is_empty() {
            let status = std::process::Command::new("git")
                .args(["config", "--local", "core.hooksPath", &previous])
                .current_dir(repo_root)
                .status()
                .map_err(|e| format!("git config failed: {e}"))?;
            if !status.success() {
                return Err(format!("could not restore core.hooksPath to {previous}"));
            }
            let _ = std::process::Command::new("git")
                .args(["config", "--local", "--unset", PREVIOUS_HOOKS_PATH])
                .current_dir(repo_root)
                .status();
            eprintln!("--- gate: core.hooksPath restored to {previous} ---");
            return Ok(());
        }
    }

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

/// One local config value, or `None` where the key is unset.
///
/// `None` and `Some("")` are kept apart deliberately: an unset key and a key set
/// to the empty string mean different things to `activate`, and collapsing them
/// would make it save an empty previous value over a real one.
fn local_config(repo_root: &Path, key: &str) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["config", "--local", key])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
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
            // The same predicate the gate uses to decide whether it may take the
            // key, and it has to be the same one: a path this reports as active
            // while the gate reads as somebody else's is a repo that believes it
            // is gated and is not. It was spelled out twice, identically, which
            // is two places to fix and one of them would have stayed wrong.
            mockspace_manifest::gate::is_ours(&path)
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{PREVIOUS_HOOKS_PATH, deactivate, local_config};

    /// A repository with no remote and no hooks configuration.
    fn repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ms-activate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&dir)
                .output()
                .expect("git");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        dir
    }

    fn set(dir: &PathBuf, key: &str, value: &str) {
        std::process::Command::new("git")
            .args(["config", "--local", key, value])
            .current_dir(dir)
            .output()
            .expect("git config");
    }

    #[test]
    fn a_repositorys_own_hooks_path_comes_back_on_deactivate() {
        // The defect this exists for. Deactivate used to unset unconditionally,
        // so a repository on husky or lefthook lost its setting and nothing
        // anywhere remembered it, while the documentation promised the opposite.
        let dir = repo();
        set(&dir, "core.hooksPath", ".husky");
        set(&dir, PREVIOUS_HOOKS_PATH, ".husky");
        set(&dir, "core.hooksPath", "mock/target/hooks");

        deactivate(&dir).expect("deactivate");

        assert_eq!(
            local_config(&dir, "core.hooksPath").as_deref(),
            Some(".husky"),
            "the repository's own hooks path must come back"
        );
        assert!(
            local_config(&dir, PREVIOUS_HOOKS_PATH).is_none(),
            "and the saved copy goes, or a second activate would restore a stale one"
        );
    }

    #[test]
    fn a_repository_that_had_none_ends_with_none() {
        // The control, and it is not the same assertion inverted. Restoring is
        // correct only when there was something to restore; a deactivate that
        // set the key to an empty or invented value here would break git's
        // fallback to `.git/hooks/` while looking like it worked.
        let dir = repo();
        set(&dir, "core.hooksPath", "mock/target/hooks");

        deactivate(&dir).expect("deactivate");

        assert!(
            local_config(&dir, "core.hooksPath").is_none(),
            "with nothing saved, the key is unset rather than given a value"
        );
    }

    #[test]
    fn deactivating_twice_is_not_an_error() {
        // The hook path calls this on repositories in unknown states, so the
        // already-deactivated case is ordinary rather than exceptional. Git
        // reports exit 5 for an unset key and that is not a failure.
        let dir = repo();
        deactivate(&dir).expect("first");
        deactivate(&dir).expect("second");
        assert!(local_config(&dir, "core.hooksPath").is_none());
    }

    #[test]
    fn an_unset_key_reads_as_none_and_an_empty_one_does_not() {
        // `activate` decides whether to save a previous value from this, so
        // collapsing the two would let it write an empty string over a real
        // setting and lose exactly what it was added to protect.
        let dir = repo();
        assert!(local_config(&dir, "core.hooksPath").is_none());
        set(&dir, "core.hooksPath", "");
        assert_eq!(local_config(&dir, "core.hooksPath").as_deref(), Some(""));
    }
}
