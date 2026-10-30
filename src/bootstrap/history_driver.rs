//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Test-history files merge by what they record, not by line.
//!
//! Each branch that reruns a member rewrites `<mock>/test-history/<flavour>/
//! <member>.json`, so every other branch that reran it conflicts at the merge.
//! `mock merge-test-history` (see [`crate::suite::history_merge`]) settles what git
//! conflicted on, and this wires git to call it for every `.json` file under the
//! directory. It is wired the way [`super::registry_driver`] wires the registry's:
//! one attribute line naming the directory, the same text on every branch, and the
//! driver in the clone's config, with the same fallback to git's text merge when
//! the engine cannot be run at all.
//!
//! The pre-commit hook runs `mock check-test-history` for a commit that stages one
//! of these files (see [`crate::suite::history_guard`]).

use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const HISTORY_DRIVER: &str = "mockspace-test-history";

pub(crate) const HISTORY_NOTE: &str =
    "# mockspace: test-history files merge by what they record, see the test-history merge driver";

/// The attribute line for the test-history directory, `None` when it is outside
/// the repository.
pub(crate) fn history_attributes_line(repo_root: &Path, history_dir: &Path) -> Option<String> {
    let rel = relative(repo_root, history_dir).filter(|r| !r.is_empty())?;
    Some(format!(
        "{}/**/*.json merge={HISTORY_DRIVER}",
        pattern(&rel)
    ))
}

/// What git runs for a conflicting test-history file.
pub(crate) fn history_driver_command(launcher: &str) -> String {
    engine_driver_command(launcher, "merge-test-history")
}

/// Register the driver in the clone's config. Idempotent.
pub(crate) fn register_history_driver(target: &ConfigTarget, launcher: &str, actions: &mut Vec<String>) {
    register_engine_driver(
        target,
        HISTORY_DRIVER,
        "mockspace test history",
        &history_driver_command(launcher),
        actions,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_covers_the_history_at_any_depth_and_names_no_file() {
        let root = Path::new("/r");
        assert_eq!(
            history_attributes_line(root, &root.join("mock/test-history")).unwrap(),
            format!("/mock/test-history/**/*.json merge={HISTORY_DRIVER}")
        );
        assert_eq!(
            history_attributes_line(root, &root.join("linux/test-history")).unwrap(),
            format!("/linux/test-history/**/*.json merge={HISTORY_DRIVER}")
        );
        assert_eq!(
            history_attributes_line(root, Path::new("/elsewhere/test-history")),
            None
        );
    }

    #[test]
    fn the_command_is_valid_shell_and_falls_back_only_past_a_refusal() {
        let cmd = history_driver_command("cargo mock");
        let out = Command::new("sh")
            .arg("-n")
            .arg("-c")
            .arg(&cmd)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            cmd.contains("cargo mock merge-test-history %O %A %B %P %L"),
            "{cmd}"
        );
        assert!(
            cmd.contains("-gt 1"),
            "exit 1 is a refusal and is final: {cmd}"
        );
    }
}
