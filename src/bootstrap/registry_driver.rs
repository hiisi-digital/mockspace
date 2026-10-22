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
/// that reaches the engine (`cargo mock`).
pub(crate) fn registry_driver_command(launcher: &str) -> String {
    format!(
        "s=0; {launcher} merge-registry %O %A %B %P || s=$?; if [ $s -gt 1 ]; then git merge-file \
         -L ours -L base -L theirs %A %O %B; else exit $s; fi"
    )
}

/// Register the driver in the clone's config. Idempotent.
pub(crate) fn register_registry_driver(
    repo_root: &Path,
    launcher: &str,
    actions: &mut Vec<String>,
) {
    let key = format!("merge.{REGISTRY_DRIVER}.driver");
    let wanted = registry_driver_command(launcher);
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
    if current.as_deref() == Some(wanted.as_str()) {
        return;
    }
    let ok = |args: &[&str]| git_config(args).is_some_and(|o| o.status.success());
    if ok(&[&format!("merge.{REGISTRY_DRIVER}.name"), "mockspace registry rows"])
        && ok(&[&key, &wanted])
    {
        actions.push(format!("registered the {REGISTRY_DRIVER} merge driver"));
    } else {
        actions.push(format!(
            "could not register the {REGISTRY_DRIVER} merge driver"
        ));
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
        assert!(cmd.contains("cargo mock merge-registry %O %A %B %P"));
        assert!(
            cmd.contains("-gt 1"),
            "exit 1 is a conflict and is final: {cmd}"
        );
    }
}
