//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Registry rows merge by row, not by line.
//!
//! Two branches that each add a row to the same spot of a registry file touch
//! the same lines and conflict as text, though they agree on everything that
//! matters. `mock merge-registry` (see [`crate::registry_merge`]) merges by what
//! a row is, and this wires git to call it for every `*.toml` under
//! `<mock>/registry/`, at any depth, since the loader reads them all by their
//! `[[namespace]]` key and never by what the file is called.
//!
//! The attribute line is the same text on every branch, so it cannot conflict
//! itself, and it goes into `.gitattributes` with the documents line, in the one
//! write (see [`super::write_gitattributes`]). The driver is registered in the
//! clone's config with the hooks, the only place git takes one from.
//!
//! The command runs `cargo mock`, so the pinned engine merges, and falls back
//! to git's own text merge when the engine cannot be run at all (no `cargo`, no
//! network for a launcher that wants a build). The engine exits 0 for a clean
//! merge and 1 for a conflict, and either is final; anything higher means it
//! did not get to look at the files, and `git merge-file` is what git would
//! have done with no driver.

use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const REGISTRY_DRIVER: &str = "mockspace-registry";

pub(crate) const REGISTRY_NOTE: &str =
    "# mockspace: registry rows merge by namespace and id, see the registry merge driver";

/// The attribute line for the registry directory, `None` when it is outside
/// the repository.
pub(crate) fn registry_attributes_line(repo_root: &Path, registry_dir: &Path) -> Option<String> {
    let rel = relative(repo_root, registry_dir).filter(|r| !r.is_empty())?;
    Some(format!(
        "{}/**/*.toml merge={REGISTRY_DRIVER}",
        pattern(&rel)
    ))
}

/// What git runs for a conflicting registry file, with `launcher` the command
/// that reaches the engine (`cargo mock`). `%L` is the conflict marker length git
/// wants for the path, from its `conflict-marker-size` attribute, and both the
/// engine and the text-merge fallback are given it.
pub(crate) fn registry_driver_command(launcher: &str) -> String {
    engine_driver_command(launcher, "merge-registry")
}

/// The same command for any driver that is a subcommand of the engine taking
/// `%O %A %B %P %L`.
pub(crate) fn engine_driver_command(launcher: &str, subcommand: &str) -> String {
    format!(
        "s=0; {launcher} {subcommand} %O %A %B %P %L || s=$?; if [ $s -gt 1 ]; then git \
         merge-file --marker-size=%L -L ours -L base -L theirs %A %O %B; else exit $s; fi"
    )
}

/// Register the driver in the clone's config. Idempotent.
pub(crate) fn register_registry_driver(
    repo_root: &Path,
    launcher: &str,
    actions: &mut Vec<String>,
) {
    register_engine_driver(
        repo_root,
        REGISTRY_DRIVER,
        "mockspace registry rows",
        &registry_driver_command(launcher),
        actions,
    );
}

/// Register the merge driver `driver` in the clone's config, running `wanted`: the one
/// registration every driver goes through, whether it is an engine subcommand or not.
/// Idempotent: a second call with the same command changes nothing.
pub(crate) fn register_engine_driver(
    repo_root: &Path,
    driver: &str,
    title: &str,
    wanted: &str,
    actions: &mut Vec<String>,
) {
    let key = format!("merge.{driver}.driver");
    let git_config = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(repo_root)
            .args(["config", "--local"])
            .args(args)
            .output()
            .ok()
    };
    let current = git_config(&[&key])
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if current.as_deref() == Some(wanted) {
        return;
    }
    let ok = |args: &[&str]| git_config(args).is_some_and(|o| o.status.success());
    if ok(&[&format!("merge.{driver}.name"), title]) && ok(&[&key, wanted]) {
        actions.push(format!("registered the {driver} merge driver"));
    } else {
        actions.push(format!("could not register the {driver} merge driver"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_covers_the_registry_at_any_depth_and_names_no_file() {
        let root = Path::new("/r");
        assert_eq!(
            registry_attributes_line(root, &root.join("mock/registry")).unwrap(),
            format!("/mock/registry/**/*.toml merge={REGISTRY_DRIVER}")
        );
        assert_eq!(
            registry_attributes_line(root, &root.join("linux/registry")).unwrap(),
            format!("/linux/registry/**/*.toml merge={REGISTRY_DRIVER}")
        );
        assert_eq!(
            registry_attributes_line(root, Path::new("/elsewhere/registry")),
            None
        );
    }

    #[test]
    fn the_command_is_valid_shell_and_falls_back_only_past_a_conflict() {
        let cmd = registry_driver_command("cargo mock");
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
        assert!(cmd.contains("cargo mock merge-registry %O %A %B %P %L"));
        assert!(
            cmd.contains("-gt 1"),
            "exit 1 is a conflict and is final: {cmd}"
        );
    }

    #[test]
    fn the_marker_length_git_asks_for_reaches_the_engine_and_the_fallback() {
        let cmd = registry_driver_command("cargo mock");
        assert!(
            cmd.contains("%P %L ||"),
            "the engine is told the length: {cmd}"
        );
        assert!(
            cmd.contains("git merge-file --marker-size=%L"),
            "the fallback is told it too: {cmd}"
        );
    }
}
