//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Files that only grow keep both sides' additions in a merge.
//!
//! A list of `mod` lines, a table of strings, a log of known gaps: two branches
//! that each add to the same spot touch the same lines and conflict as text.
//! `mock merge-append` (see [`crate::append_merge`]) keeps both when neither side
//! touched a line already there, and leaves the file to git's own merge when
//! either did. This wires git to call it, the way [`super::registry_driver`]
//! wires the registry's: attribute lines that are the same text on every
//! branch, written into `.gitattributes` with the others (see
//! [`super::write_gitattributes`]), and the driver registered in the clone's
//! config with the hooks.
//!
//! **Which files** are named by the narrowest pattern that covers them, never by
//! a directory. Mockspace names the two it owns the shape of: each source
//! package's `SHAME.md.tmpl` and `BACKLOG.md.tmpl`, which are written by adding
//! a section. A project names the rest in `mockspace.toml` under `append_only`,
//! as patterns relative to the repository root.

use super::*;

/// The driver's name, as `.gitattributes` and the clone's config both spell it.
pub(crate) const APPEND_DRIVER: &str = "mockspace-append";

pub(crate) const APPEND_NOTE: &str =
    "# mockspace: files that only grow keep both sides' additions, see the append merge driver";

/// The templates every source package keeps by adding a section to them.
const PACKAGE_TEMPLATES: [&str; 2] = ["SHAME.md.tmpl", "BACKLOG.md.tmpl"];

/// Whether `declared` can be written as an attribute pattern: no whitespace
/// (it would end the pattern), not a comment or negation, and no `..`.
fn writable(declared: &str) -> bool {
    !declared.is_empty()
        && !declared.contains(char::is_whitespace)
        && !declared.starts_with(['#', '!'])
        && !declared.split('/').any(|part| part == "..")
}

/// Every attribute line for the append driver: the package templates under each
/// source directory, then the project's declared patterns. Sorted and unique, so
/// the text does not depend on the order anything was declared in.
///
/// A source directory outside the repository, or a declared pattern that cannot
/// be written, contributes nothing; the second is reported, because a pattern
/// that silently governs nothing reads exactly like one that works.
pub(crate) fn append_attribute_lines(
    repo_root: &Path,
    src_dirs: &[PathBuf],
    declared: &[String],
) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for dir in src_dirs {
        let Some(rel) = relative(repo_root, dir).filter(|r| !r.is_empty()) else {
            continue;
        };
        for name in PACKAGE_TEMPLATES {
            lines.push(format!("{}/*/{name} merge={APPEND_DRIVER}", pattern(&rel)));
        }
    }
    for entry in declared {
        if writable(entry) {
            lines.push(format!(
                "/{} merge={APPEND_DRIVER}",
                entry.trim_start_matches('/')
            ));
        } else {
            eprintln!(
                "warning: mockspace.toml `append_only` entry {entry:?} cannot be a merge attribute \
                 (no whitespace, no leading # or !, no ..), so it governs nothing"
            );
        }
    }
    lines.sort();
    lines.dedup();
    lines
}

/// `existing` with the note and every line of `lines` it does not already hold
/// at the end. Whatever else the file holds is left exactly as it was.
pub(crate) fn with_lines(existing: &str, note: &str, lines: &[String]) -> String {
    let missing: Vec<&String> = lines
        .iter()
        .filter(|l| !existing.lines().any(|e| e == l.as_str()))
        .collect();
    if missing.is_empty() {
        return existing.to_string();
    }
    let mut text = existing.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(note);
    text.push('\n');
    for line in missing {
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// What git runs for an append-only file both sides changed.
pub(crate) fn append_driver_command(launcher: &str) -> String {
    engine_driver_command(launcher, "merge-append")
}

/// Register the driver in the clone's config. Idempotent.
pub(crate) fn register_append_driver(repo_root: &Path, launcher: &str, actions: &mut Vec<String>) {
    register_engine_driver(
        repo_root,
        APPEND_DRIVER,
        "mockspace append-only files",
        &append_driver_command(launcher),
        actions,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_package_template_is_named_under_each_source_directory_and_no_directory_is() {
        let root = Path::new("/r");
        let lines = append_attribute_lines(
            root,
            &[root.join("mock/crates"), root.join("mock/groups/a")],
            &[],
        );
        assert_eq!(lines, vec![
            format!("/mock/crates/*/BACKLOG.md.tmpl merge={APPEND_DRIVER}"),
            format!("/mock/crates/*/SHAME.md.tmpl merge={APPEND_DRIVER}"),
            format!("/mock/groups/a/*/BACKLOG.md.tmpl merge={APPEND_DRIVER}"),
            format!("/mock/groups/a/*/SHAME.md.tmpl merge={APPEND_DRIVER}"),
        ]);
    }

    #[test]
    fn a_declared_pattern_is_anchored_at_the_repository_root() {
        let root = Path::new("/r");
        let lines = append_attribute_lines(root, &[], &[
            "content/x/en.toml".to_string(),
            "/mock/crates/*/src/tests/mod.rs".to_string(),
        ]);
        assert_eq!(lines, vec![
            format!("/content/x/en.toml merge={APPEND_DRIVER}"),
            format!("/mock/crates/*/src/tests/mod.rs merge={APPEND_DRIVER}"),
        ]);
    }

    #[test]
    fn a_pattern_that_cannot_be_written_is_dropped_and_the_rest_stand() {
        let root = Path::new("/r");
        let declared = ["a b", "#c", "!d", "../e", "", "ok.md"].map(String::from);
        assert_eq!(append_attribute_lines(root, &[], &declared), vec![format!(
            "/ok.md merge={APPEND_DRIVER}"
        )]);
    }

    #[test]
    fn writing_the_lines_keeps_the_rest_and_adds_each_once() {
        let lines = vec![format!("/a merge={APPEND_DRIVER}"), format!("/b merge={APPEND_DRIVER}")];
        let once = with_lines("*.png binary\n", APPEND_NOTE, &lines);
        assert!(once.starts_with("*.png binary\n\n"), "{once}");
        assert_eq!(once.matches(APPEND_NOTE).count(), 1, "{once}");
        assert_eq!(
            with_lines(&once, APPEND_NOTE, &lines),
            once,
            "a second pass changes nothing"
        );
        // a line added later is appended without a second note
        let more = vec![lines[0].clone(), format!("/c merge={APPEND_DRIVER}")];
        let grown = with_lines(&once, APPEND_NOTE, &more);
        assert!(
            grown.ends_with(&format!("{APPEND_NOTE}\n/c merge={APPEND_DRIVER}\n")),
            "{grown}"
        );
    }

    #[test]
    fn the_command_is_valid_shell_and_falls_back_only_past_a_refusal() {
        let cmd = append_driver_command("cargo mock");
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
            cmd.contains("cargo mock merge-append %O %A %B %P %L"),
            "{cmd}"
        );
        assert!(
            cmd.contains("-gt 1"),
            "exit 1 is a conflict and is final: {cmd}"
        );
    }
}
