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
    // `is_ours` rather than a comparison against the path we are about to write,
    // because mockspace writes more than one value. `hooks_path_target` returns
    // the durable dir where it exists and `<mock>/target/hooks` otherwise, and
    // the durable one is version-keyed, so the two differ routinely: the first
    // run after the durable dir becomes writable, and every `HOOK_VERSION` bump.
    //
    // An exact comparison there records mockspace's own previous path as the
    // repository's, and `deactivate` then restores the gate while reporting that
    // it handed the repository back. `is_active` agrees it is still on, so
    // nothing downstream contradicts it either.
    if let Some(existing) = local_config(repo_root, "core.hooksPath") {
        if !existing.is_empty() && !mockspace_manifest::gate::is_ours(&existing) {
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
    ///
    /// The counter is what makes it unique, not the clock. Keying on pid plus
    /// nanoseconds looks unique and is not: these tests run on parallel threads,
    /// the clock's resolution is coarser than the gap between two of them, and
    /// two tests then share one repository. That is not a flake, it is one test
    /// reading another's git config, and it presented as the version-bump arm
    /// failing while the code under it was correct.
    fn repo() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ms-activate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Cleared first, because the counter only fixes collisions inside one
        // run. Nothing deletes these afterwards, `create_dir_all` succeeds on an
        // existing directory and `git init` on an existing repository keeps its
        // config, so a later process drawing a recycled pid inherits the earlier
        // run's `mockspace.previousHooksPath`. The two tests asserting that key
        // is absent then fail on correct code.
        //
        // The same bug as the one these tests were written for, with threads
        // swapped for processes, which is worth saying: fixing an isolation
        // defect on one axis does not fix it on the other, and I had already
        // done exactly that once here.
        init_repo_at(&dir);
        dir
    }

    /// Clear `dir` and put a fresh repository in it.
    ///
    /// Split out of [`repo`] so the isolation property can be tested against a
    /// directory a test chooses, rather than one drawn from the shared counter.
    /// Reading that counter and then calling `repo` is not atomic, and a
    /// parallel test taking the number in between would make such a test pass
    /// for the wrong reason.
    fn init_repo_at(dir: &PathBuf) {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir).expect("temp dir");
        let run = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .expect("git");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
    }

    fn set(dir: &PathBuf, key: &str, value: &str) {
        std::process::Command::new("git")
            .args(["config", "--local", key, value])
            .current_dir(dir)
            .output()
            .expect("git config");
    }

    /// A repository with a generated hooks dir, so `activate` gets past its
    /// precondition and reaches the save decision.
    fn repo_ready_to_activate() -> (PathBuf, PathBuf) {
        let dir = repo();
        let mock = dir.join("mock");
        std::fs::create_dir_all(mock.join("target").join("hooks")).expect("hooks dir");
        (dir, mock)
    }

    #[test]
    fn a_fresh_repo_carries_nothing_from_a_previous_run() {
        // The helper's own contract, asserted rather than assumed, because two
        // tests here assert a config key is *absent* and would pass or fail on
        // whatever a previous process left behind.
        //
        // Plants the exact leftover into the directory this helper is about to
        // choose, then calls it. Without the `remove_dir_all`, `create_dir_all`
        // succeeds on the existing directory, `git init` preserves the existing
        // config, and the key survives into a test that is about to assert it is
        // gone. A recycled pid is all that is needed, and there were over a
        // hundred of these directories on the machine when this was written.
        // A directory of this test's own, rather than one read off the shared
        // counter: reading the counter and then calling `repo()` is not atomic,
        // and a parallel test taking the number in between would make this pass
        // for the wrong reason.
        let planned =
            std::env::temp_dir().join(format!("ms-activate-plant-{}", std::process::id()));
        std::fs::create_dir_all(&planned).expect("plant dir");
        let plant = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&planned)
                .output()
                .expect("git");
        };
        plant(&["init", "-q"]);
        plant(&["config", "--local", PREVIOUS_HOOKS_PATH, ".husky-from-a-previous-run"]);
        assert_eq!(
            local_config(&planned, PREVIOUS_HOOKS_PATH).as_deref(),
            Some(".husky-from-a-previous-run"),
            "the control: the leftover must really be there before the helper runs"
        );

        init_repo_at(&planned);
        assert!(
            local_config(&planned, PREVIOUS_HOOKS_PATH).is_none(),
            "a repository handed to a test carries nothing from a previous process"
        );
    }

    #[test]
    fn activating_over_our_own_path_saves_nothing() {
        // The case the first four tests could not see, because every one of them
        // set `mockspace.previousHooksPath` by hand and none went through
        // `activate`. The save decision was uncovered, and it was wrong.
        //
        // It compared against the exact path about to be written, but mockspace
        // writes more than one: the durable dir when it exists and
        // `<mock>/target/hooks` otherwise, with the durable one version-keyed.
        // So on the first run after the durable dir appears, and on every
        // `HOOK_VERSION` bump, activate recorded mockspace's own previous path
        // as the repository's and deactivate restored the gate while saying it
        // had handed the repository back.
        let (dir, mock) = repo_ready_to_activate();
        set(&dir, "core.hooksPath", "mock/target/hooks");

        let _ = super::activate(&dir, &mock);

        assert!(
            local_config(&dir, PREVIOUS_HOOKS_PATH).is_none(),
            "a mockspace path is not the repository's previous setting, whichever \
             of ours it happens to be"
        );
    }

    #[test]
    fn activating_over_a_different_mockspace_version_saves_nothing() {
        // The version-bump arm specifically, since that is the one that fires on
        // an ordinary upgrade rather than on a first install.
        let (dir, mock) = repo_ready_to_activate();
        set(&dir, "core.hooksPath", "/home/x/.config/mockspace/hooks-v2");

        let _ = super::activate(&dir, &mock);

        assert!(
            local_config(&dir, PREVIOUS_HOOKS_PATH).is_none(),
            "an older mockspace hooks dir is still ours"
        );
    }

    #[test]
    fn activating_over_a_foreign_path_saves_it() {
        // The positive control, and it is what makes the two above mean
        // something: without it they are satisfied by an activate that never
        // saves anything at all, which is the behaviour before the fix that
        // introduced saving.
        let (dir, mock) = repo_ready_to_activate();
        set(&dir, "core.hooksPath", ".husky");

        let _ = super::activate(&dir, &mock);

        assert_eq!(
            local_config(&dir, PREVIOUS_HOOKS_PATH).as_deref(),
            Some(".husky"),
            "a path we do not own is exactly what must be kept"
        );
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
