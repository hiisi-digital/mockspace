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

/// The arguments of the gate's `cargo check`, bounded to the scoped packages.
///
/// `scope` is the crate list `--scope` carries, by directory name, and each name
/// is looked up under every declared source directory. The bound names only
/// packages a bare `cargo check` already covers, which is the workspace's
/// default members (every member when none are declared), read from
/// `cargo metadata` rather than from the manifest's globs. A crate outside that
/// set is left out of the list, since the whole check never covered it either:
/// one in no workspace of the mock manifest, or one `default-members` leaves
/// out, which `-p` would force to build.
///
/// The check stays the whole workspace when there is no crate list, when scope
/// is `infra`, when a shared input is staged (the registry, the config, the
/// workspace manifest and lockfile reach every crate), when a named crate has no
/// directory, when cargo cannot say what the workspace holds, and when nothing
/// scoped is covered.
pub(crate) fn check_args(
    mock_dir: &Path,
    src_dirs: &[std::path::PathBuf],
    scope: Option<&str>,
    shared_staged: bool,
) -> Vec<String> {
    let metadata = cargo(mock_dir, &[
        "metadata",
        "--no-deps",
        "--format-version",
        "1",
    ])
    .output()
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    check_args_from(metadata.as_deref(), src_dirs, scope, shared_staged)
}

/// [`check_args`] with the `cargo metadata` output already in hand, `None` when
/// cargo could not produce it.
pub(crate) fn check_args_from(
    metadata: Option<&str>,
    src_dirs: &[std::path::PathBuf],
    scope: Option<&str>,
    shared_staged: bool,
) -> Vec<String> {
    let full = vec!["check".to_string()];
    let Some(scope) = scope else {
        return full;
    };
    if shared_staged || scope == "infra" {
        return full;
    }
    let names: Vec<&str> = scope
        .split(',')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    let Some(covered) = metadata.and_then(default_member_packages) else {
        return full;
    };
    let mut args = full.clone();
    for name in names {
        let Some(dir) = src_dirs.iter().map(|d| d.join(name)).find(|d| d.is_dir()) else {
            return full;
        };
        let dir = dir.canonicalize().unwrap_or(dir);
        if let Some(package) = covered.get(&dir) {
            args.push("-p".to_string());
            args.push(package.clone());
        }
    }
    if args.len() == 1 { full } else { args }
}

/// The default members of the workspace `cargo metadata` described, as package
/// directory to package name. `None` when the output is not metadata.
pub(crate) fn default_member_packages(
    metadata: &str,
) -> Option<std::collections::BTreeMap<std::path::PathBuf, String>> {
    let doc: serde_json::Value = serde_json::from_str(metadata).ok()?;
    let defaults: Vec<&str> = doc
        .get("workspace_default_members")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    let mut out = std::collections::BTreeMap::new();
    for pkg in doc.get("packages")?.as_array()? {
        let id = pkg.get("id")?.as_str()?;
        if !defaults.contains(&id) {
            continue;
        }
        let manifest = std::path::PathBuf::from(pkg.get("manifest_path")?.as_str()?);
        let dir = manifest.parent()?;
        let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        out.insert(dir, pkg.get("name")?.as_str()?.to_string());
    }
    Some(out)
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
    /// A mock workspace with a default member, a member `default-members`
    /// leaves out, and a crate under a second source directory that no
    /// workspace of the mock manifest holds.
    fn workspace() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        Vec<std::path::PathBuf>,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        let write = |rel: &str, body: &str| {
            let p = mock.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        write(
            "Cargo.toml",
            "[workspace]\nresolver = \"3\"\nmembers = [\"crates/member\", \"crates/wasm-only\"]\n\
             default-members = [\"crates/member\"]\n",
        );
        for (dir, name) in
            [("crates/member", "kaski-member"), ("crates/wasm-only", "kaski-wasm-only")]
        {
            write(
                &format!("{dir}/Cargo.toml"),
                &format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n"),
            );
            write(&format!("{dir}/src/lib.rs"), "");
        }
        // its own workspace, so not a member of the mock manifest
        write(
            "applets/console/Cargo.toml",
            "[workspace]\nmembers = [\"crates/console-core\"]\n",
        );
        write(
            "applets/console/crates/console-core/Cargo.toml",
            "[package]\nname = \"console-core\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
        );
        write("applets/console/crates/console-core/src/lib.rs", "");
        let dirs = vec![mock.join("crates"), mock.join("applets/console/crates")];
        (tmp, mock, dirs)
    }

    #[test]
    fn a_default_member_is_named_and_the_others_are_left_out() {
        let (_tmp, mock, dirs) = workspace();
        assert_eq!(check_args(&mock, &dirs, Some("member"), false), [
            "check",
            "-p",
            "kaski-member"
        ]);
        // `-p kaski-wasm-only` would force a build a bare check never makes
        assert_eq!(check_args(&mock, &dirs, Some("wasm-only"), false), [
            "check"
        ]);
        // in no workspace of the mock manifest: `-p` would not match any package
        assert_eq!(check_args(&mock, &dirs, Some("console-core"), false), [
            "check"
        ]);
        assert_eq!(
            check_args(&mock, &dirs, Some("member,console-core,wasm-only"), false),
            ["check", "-p", "kaski-member"]
        );
    }

    /// The control: the first version named every scoped crate by its manifest.
    /// Cargo refuses that for a crate outside the workspace and builds the
    /// wasm-only page for the one `default-members` leaves out, so its answer
    /// is exactly what the assertions above reject.
    #[test]
    fn naming_every_scoped_crate_would_have_failed_the_assertions_above() {
        let (_tmp, mock, dirs) = workspace();
        let naive = |names: &[&str]| -> Vec<String> {
            let mut args = vec!["check".to_string()];
            for name in names {
                let dir = dirs
                    .iter()
                    .map(|d| d.join(name))
                    .find(|d| d.is_dir())
                    .unwrap();
                let text = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
                let pkg = text
                    .lines()
                    .find_map(|l| l.strip_prefix("name = "))
                    .unwrap();
                args.push("-p".into());
                args.push(pkg.trim_matches('"').to_string());
            }
            args
        };
        for scope in [vec!["console-core"], vec!["wasm-only"]] {
            let fixed = check_args(&mock, &dirs, Some(&scope.join(",")), false);
            assert_ne!(naive(&scope), fixed, "{scope:?}");
            assert_eq!(fixed, ["check"]);
        }
    }

    #[test]
    fn check_stays_whole_without_a_crate_list_or_with_a_shared_input() {
        let (_tmp, mock, dirs) = workspace();
        assert_eq!(check_args(&mock, &dirs, None, false), ["check"]);
        assert_eq!(check_args(&mock, &dirs, Some("infra"), false), ["check"]);
        assert_eq!(check_args(&mock, &dirs, Some("member"), true), ["check"]);
    }

    #[test]
    fn check_is_not_narrowed_by_a_crate_with_no_directory() {
        let (_tmp, mock, dirs) = workspace();
        assert_eq!(check_args(&mock, &dirs, Some("member,nope"), false), [
            "check"
        ]);
    }

    #[test]
    fn check_is_whole_when_cargo_cannot_say_what_the_workspace_holds() {
        let (_tmp, _mock, dirs) = workspace();
        assert_eq!(check_args_from(None, &dirs, Some("member"), false), [
            "check"
        ]);
        assert_eq!(
            check_args_from(Some("not json"), &dirs, Some("member"), false),
            ["check"]
        );
    }

    #[test]
    fn the_default_members_are_read_from_the_metadata() {
        let json = r#"{"packages":[
            {"id":"a 0.0.0 (path+file:///w/a)","name":"a","manifest_path":"/w/a/Cargo.toml"},
            {"id":"b 0.0.0 (path+file:///w/b)","name":"b","manifest_path":"/w/b/Cargo.toml"}],
            "workspace_default_members":["a 0.0.0 (path+file:///w/a)"]}"#;
        let got = default_member_packages(json).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            got.get(std::path::Path::new("/w/a")).map(String::as_str),
            Some("a")
        );
        assert!(default_member_packages("{}").is_none());
    }
}
