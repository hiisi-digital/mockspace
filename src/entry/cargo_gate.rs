//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Forgiving the one cargo failure a design-first repo legitimately produces.
//!
//! A repo whose crate taxonomy is still a design round's subject has a mock
//! workspace with no members. Cargo treats a virtual manifest with no members
//! as a hard error, so `cargo check` and `cargo test` cannot pass there however
//! correct the repo is. That state is the workflow working as designed (docs
//! before source, taxonomy settled by a round rather than guessed up front), so
//! the cargo gates forgive it.
//!
//! Forgiveness is post-hoc, never predicted. The command runs, and its failure
//! is forgiven only when BOTH hold: cargo's own diagnostic says the workspace
//! has no members, and the manifest is confirmed virtual with no members. Any
//! other failure fails the gate exactly as before, so a real breakage can never
//! be mistaken for the empty-workspace case.

use std::path::Path;
use std::process::Command;

/// Whether cargo's stderr is the virtual-manifest-with-no-members diagnostic.
///
/// Matched on two substrings rather than the whole line: the message embeds an
/// absolute manifest path, and cargo has reworded the surrounding prose before.
pub(crate) fn diagnostic_is_no_members(stderr: &str) -> bool {
    let s = stderr.to_ascii_lowercase();
    s.contains("contains no package") && s.contains("workspace has no members")
}

/// Whether `mock_dir`'s manifest is a virtual workspace with no members.
///
/// A manifest carrying its own `[package]` gives cargo something to check, so
/// it is never forgiven. An empty `members` list and an absent one are both
/// zero members; cargo can infer members only from a package or from listed
/// members, and has neither here.
pub(crate) fn is_memberless_virtual_workspace(mock_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(mock_dir.join("Cargo.toml")) else {
        return false;
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    if doc.get("package").is_some() {
        return false;
    }
    // `as_table_like` rather than `as_table`: an inline
    // `workspace = { members = [] }` is a manifest cargo accepts and which
    // produces exactly the diagnostic below, so rejecting it here would fail the
    // gate on a repo that is legitimately memberless.
    let Some(workspace) = doc.get("workspace").and_then(|w| w.as_table_like()) else {
        return false;
    };
    match workspace.get("members") {
        Some(members) => members.as_array().is_some_and(toml_edit::Array::is_empty),
        None => true,
    }
}

/// Whether a just-failed cargo invocation is forgiven as the empty-workspace
/// case, for a caller that streamed the command's output rather than capturing
/// it.
///
/// The cheap manifest read gates first, so a workspace with real members never
/// pays for a second invocation. When it does re-run, the manifest is memberless
/// and cargo errors out before compiling anything, so the confirming run is
/// immediate.
pub(crate) fn forgives_failure(mock_dir: &Path, args: &[&str]) -> bool {
    if !is_memberless_virtual_workspace(mock_dir) {
        return false;
    }
    let Ok(out) = cargo(mock_dir, args).output() else {
        return false;
    };
    diagnostic_is_no_members(&String::from_utf8_lossy(&out.stderr))
}

/// A cargo command against the mock workspace, with the inherited rustup env
/// stripped so `mock/rust-toolchain.toml` wins over the toolchain the outer
/// process already resolved.
///
/// The stripping is load-bearing. When the engine is launched from the repo
/// root, the outer cargo has already resolved a toolchain (the repo-root
/// default, typically stable) and propagates `RUSTUP_TOOLCHAIN` to children.
/// That env var beats the file-based override in `mock/rust-toolchain.toml`, so
/// an inner cargo would silently run under the outer toolchain. Removing these
/// lets rustup re-detect from the working directory instead.
pub(crate) fn cargo(mock_dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args(args)
        .current_dir(mock_dir)
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("RUSTC")
        .env_remove("RUSTDOC");
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(tmp: &Path, contents: &str) {
        std::fs::write(tmp.join("Cargo.toml"), contents).unwrap();
    }

    #[test]
    fn cargos_real_diagnostic_is_recognised() {
        // the verbatim message from cargo 1.98.0-nightly.
        let stderr = "error: manifest path `/repo/mock` contains no package: The manifest is \
                      virtual, and the workspace has no members.\n";
        assert!(diagnostic_is_no_members(stderr));
    }

    #[test]
    fn unrelated_failures_are_not_recognised() {
        assert!(!diagnostic_is_no_members(
            "error[E0425]: cannot find value `x` in this scope"
        ));
        assert!(!diagnostic_is_no_members(
            "error: failed to parse manifest at `/repo/mock/Cargo.toml`"
        ));
        // a compile error naming a package must not trip the substring match.
        assert!(!diagnostic_is_no_members(
            "error: package `foo` contains no package.rs file"
        ));
    }

    #[test]
    fn empty_members_list_is_memberless() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace]\nresolver = \"2\"\nmembers = []\n");
        assert!(is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn multiline_empty_members_list_is_memberless() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace]\nmembers = [\n]\n");
        assert!(is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn absent_members_is_memberless() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace]\nresolver = \"2\"\n");
        assert!(is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn a_populated_members_list_is_not_memberless() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            "[workspace]\nmembers = [\n    \"crates/foo\",\n]\n",
        );
        assert!(!is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn a_package_manifest_is_never_memberless() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            "[package]\nname = \"foo\"\n\n[workspace]\nmembers = []\n",
        );
        assert!(!is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn a_manifest_that_is_not_a_workspace_is_not_memberless() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "resolver = \"2\"\n");
        assert!(!is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn an_unparseable_manifest_is_not_forgiven() {
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace\nmembers = ");
        assert!(!is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn a_missing_manifest_is_not_forgiven() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn forgives_failure_short_circuits_on_a_populated_workspace() {
        // no cargo invocation happens, so this holds even though the listed
        // member does not exist on disk.
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace]\nmembers = [\"crates/foo\"]\n");
        assert!(!forgives_failure(tmp.path(), &["check"]));
    }

    #[test]
    fn an_inline_workspace_table_is_memberless() {
        // `workspace = { members = [] }` is a manifest cargo accepts, and it
        // produces the same no-members diagnostic. Matching only on a
        // dotted-header table would fail the gate on a legitimately memberless
        // repo.
        let tmp = tempfile::tempdir().unwrap();
        manifest(
            tmp.path(),
            "workspace = { resolver = \"2\", members = [] }\n",
        );
        assert!(is_memberless_virtual_workspace(tmp.path()));
    }

    #[test]
    fn forgives_failure_actually_forgives_a_memberless_workspace() {
        // The behaviour the whole module exists to deliver, asserted against a
        // real cargo invocation. Every other test here covers the refusing
        // side; without this one, nothing proves forgiveness ever happens, and
        // a predicate that always returned false would still pass the suite.
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace]\nresolver = \"2\"\nmembers = []\n");
        assert!(
            forgives_failure(tmp.path(), &["check"]),
            "a memberless virtual workspace must be forgiven"
        );
    }

    #[test]
    #[ignore = "catalogue: members globbing to nothing yields a different cargo \
                diagnostic, so a pre-first-round repo seeding a glob member list \
                is still blocked; needs its own tolerated diagnostic"]
    fn a_members_glob_matching_nothing_is_also_a_pre_taxonomy_state() {
        // `members = ["crates/*"]` with an empty crates/ dir is the same
        // situation as an empty list from the repo's point of view, but cargo
        // reports it as a failed member load rather than as no members, so the
        // gate does not tolerate it. Asserting the intended behaviour, red until
        // that diagnostic is handled.
        let tmp = tempfile::tempdir().unwrap();
        manifest(tmp.path(), "[workspace]\nmembers = [\"crates/*\"]\n");
        std::fs::create_dir_all(tmp.path().join("crates")).unwrap();
        assert!(
            forgives_failure(tmp.path(), &["check"]),
            "a members glob matching nothing is the same pre-taxonomy state"
        );
    }
}
