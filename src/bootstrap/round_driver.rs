//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Two closed rounds with one id do not merge quietly.
//!
//! A closed round's `.meta` is written once, so the same path added on two
//! branches is two rounds that took the same id. `mock merge-round-meta` (see
//! `crate::design_round::round_merge`) refuses that merge and says which id and
//! the one command that renames a side; this wires git to call it for the
//! `.meta` of every directory under `<mock>/design_rounds/`.
//!
//! Wired as the registry driver is: the attribute line goes into `.gitattributes`
//! in the one write (see [`super::write_gitattributes`]), the driver into the
//! clone's config with the hooks, and a launcher that cannot be run falls back
//! to git's text merge, which conflicts on an add/add exactly as it did before.

use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const ROUND_DRIVER: &str = "mockspace-round";

pub(crate) const ROUND_NOTE: &str =
    "# mockspace: two closed rounds with one id do not merge, see the round merge driver";

/// The attribute line for the `.meta` of every closed round, `None` when the
/// design rounds directory is outside the repository.
pub(crate) fn round_attributes_line(repo_root: &Path, rounds_dir: &Path) -> Option<String> {
    let rel = relative(repo_root, rounds_dir).filter(|r| !r.is_empty())?;
    Some(format!("{}/*/.meta merge={ROUND_DRIVER}", pattern(&rel)))
}

/// What git runs for a `.meta` changed on both sides, `launcher` being the
/// command that reaches the engine (`cargo mock`). Exit 1 is a refusal and is
/// final; anything higher means the engine did not get to look, and git's own
/// text merge is what would have run with no driver.
pub(crate) fn round_driver_command(launcher: &str) -> String {
    format!(
        "s=0; {launcher} merge-round-meta %O %A %B %P || s=$?; if [ $s -gt 1 ]; then git \
         merge-file -L ours -L base -L theirs %A %O %B; else exit $s; fi"
    )
}

/// Register the driver in the clone's config. Idempotent.
pub(crate) fn register_round_driver(target: &ConfigTarget, launcher: &str, actions: &mut Vec<String>) {
    register_engine_driver(
        target,
        ROUND_DRIVER,
        "mockspace closed rounds",
        &round_driver_command(launcher),
        actions,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_covers_every_round_directory_and_only_the_meta() {
        let root = Path::new("/r");
        assert_eq!(
            round_attributes_line(root, &root.join("mock/design_rounds")).unwrap(),
            format!("/mock/design_rounds/*/.meta merge={ROUND_DRIVER}")
        );
        assert_eq!(round_attributes_line(root, Path::new("/elsewhere/design_rounds")), None);
    }

    #[test]
    fn the_command_is_valid_shell_and_a_refusal_is_final() {
        let cmd = round_driver_command("cargo mock");
        let out = Command::new("sh").arg("-n").arg("-c").arg(&cmd).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(cmd.contains("cargo mock merge-round-meta %O %A %B %P"));
        assert!(cmd.contains("-gt 1"), "exit 1 is a refusal and is final: {cmd}");
    }
}
