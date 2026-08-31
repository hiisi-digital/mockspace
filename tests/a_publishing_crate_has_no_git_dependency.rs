//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A crate marked `publish = true` carries no dependency crates.io will refuse,
//! because the refusal arrives at `cargo publish` rather than at any gate
//! before it.
//!
//! Two shapes are refused and they are one class. A dependency pinned to a git
//! repository: crates.io takes registry versions and nothing else. And a
//! dependency named by path with no `version`: cargo strips the path on
//! publish and is left with nothing to resolve against, which it reports as
//! "all dependencies must have a version requirement specified when
//! publishing".
//!
//! The failure is quiet in the direction that costs most. Everything builds,
//! every test passes, `cargo package` succeeds, and the manifest reads as
//! deliberate: `publish = true` beside a comment naming the crate as the one
//! that publishes. Nothing between committing and the publish itself looks at
//! the pair, so the defect surfaces at the one moment when the surrounding work
//! is already finished and a release is half done.
//!
//! Two path shapes are allowed. A path carrying a `version` resolves against
//! the registry. And a path-only *dev*-dependency is stripped whole, so there
//! is nothing left to resolve; that is how a crate tests against a sibling it
//! does not ship with.

use std::fs;
use std::path::{Path, PathBuf};

/// Why crates.io will refuse a dependency of a crate that publishes.
#[derive(Debug, PartialEq)]
enum Why {
    /// Pinned to a git repository, which the registry does not take.
    Git,
    /// Named by path with no `version`, so the stripped path leaves nothing to
    /// resolve. Not reported under `dev-dependencies`, which are stripped
    /// whole.
    PathWithoutVersion,
}

/// One dependency of a manifest that publishes and cannot.
#[derive(Debug, PartialEq)]
struct Refused {
    manifest: String,
    table:    String,
    dep:      String,
    why:      Why,
}

/// Every dependency crates.io would refuse, in a manifest that publishes.
///
/// Returns nothing for a manifest that does not publish, whatever it depends
/// on, since an unpublished crate may name anything it likes.
fn refused_by_the_registry(manifest_label: &str, text: &str) -> Vec<Refused> {
    let doc: toml_edit::DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    // `publish` is a bool or a list of registries, and a list containing
    // `crates-io` publishes there. Reading it as a bool alone would take the
    // list form for "does not publish" and skip the crate entirely.
    let publishes = match doc.get("package").and_then(|p| p.get("publish")) {
        None => false,
        Some(v) if v.as_bool() == Some(true) => true,
        Some(v) if v.as_bool() == Some(false) => false,
        Some(v) => {
            v.as_array()
                .map(|a| a.iter().any(|r| r.as_str() == Some("crates-io")))
                .unwrap_or(false)
        },
    };
    if !publishes {
        return Vec::new();
    }

    let mut found = Vec::new();
    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = doc.get(table).and_then(|t| t.as_table_like()) else {
            continue;
        };
        for (dep, spec) in deps.iter() {
            let at = |why| {
                Refused {
                    manifest: manifest_label.to_string(),
                    table: table.to_string(),
                    dep: dep.to_string(),
                    why,
                }
            };
            let Some(spec) = spec.as_table_like() else {
                // A bare string is a version requirement, which is the shape
                // the registry wants.
                continue;
            };
            if spec.get("git").is_some() {
                found.push(at(Why::Git));
            } else if spec.get("path").is_some()
                && spec.get("version").is_none()
                && table != "dev-dependencies"
            {
                found.push(at(Why::PathWithoutVersion));
            }
        }
    }
    found
}

/// Every `Cargo.toml` this repository authors.
///
/// `target/` is build output and `mock/research/` holds probe fixtures whose
/// manifests are deliberately odd, so neither is this check's business.
fn authored_manifests(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // `mock/research/` specifically, by path rather than by name:
                // a directory called `research` anywhere else authors real
                // manifests and this check is their business too.
                if name == "target" || name == ".git" || path.ends_with("mock/research") {
                    continue;
                }
                walk(&path, out);
            } else if name == "Cargo.toml" {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// crates.io refuses at `cargo publish`, and nothing before that point says
/// so. A manifest that both publishes and names a dependency it will not take
/// is therefore fine on every local check and fails at the one step that
/// cannot be retried against a version already taken.
///
/// The cases below it are the controls, so the check itself is exercised
/// rather than assumed.
#[test]
fn no_publishing_crate_names_a_dependency_the_registry_refuses() {
    let root = repo_root();
    let manifests = authored_manifests(&root);

    // The walk finding nothing would pass this test while checking nothing, so
    // the count is asserted before the contents are.
    assert!(
        manifests.len() >= 5,
        "expected the workspace's manifests, found {}: the walk is looking in the wrong place",
        manifests.len()
    );

    let mut offenders = Vec::new();
    for manifest in &manifests {
        let label = manifest
            .strip_prefix(&root)
            .unwrap_or(manifest)
            .to_string_lossy()
            .to_string();
        let text = fs::read_to_string(manifest).unwrap();
        offenders.extend(refused_by_the_registry(&label, &text));
    }

    assert!(
        offenders.is_empty(),
        "a crate that publishes names a dependency crates.io refuses at `cargo publish`. \
         Give the dependency a registry version, or set `publish = false` until it has \
         one: {offenders:#?}"
    );
}

#[test]
fn a_publishing_crate_pinned_to_git_is_reported() {
    let offenders = refused_by_the_registry(
        "fixture/Cargo.toml",
        r#"
            [package]
            name = "thing"
            publish = true

            [dependencies]
            elsewhere = { git = "ssh://git@example.invalid/elsewhere.git", rev = "abc123" }
        "#,
    );
    assert_eq!(
        offenders,
        vec![Refused {
            manifest: "fixture/Cargo.toml".to_string(),
            table:    "dependencies".to_string(),
            dep:      "elsewhere".to_string(),
            why:      Why::Git,
        }],
        "the check cannot see the defect it exists for"
    );
}

#[test]
fn a_git_pin_under_dev_or_build_dependencies_is_reported_too() {
    for table in ["dev-dependencies", "build-dependencies"] {
        let offenders = refused_by_the_registry(
            "fixture/Cargo.toml",
            &format!(
                r#"
                    [package]
                    name = "thing"
                    publish = true

                    [{table}]
                    elsewhere = {{ git = "ssh://git@example.invalid/e.git" }}
                "#
            ),
        );
        assert_eq!(offenders.len(), 1, "a git pin under `{table}` went unseen");
        assert_eq!(offenders[0].table, table);
    }
}

#[test]
fn a_crate_that_does_not_publish_may_pin_anything() {
    for package in [
        "name = \"thing\"\npublish = false",
        // Absent `publish` defaults to publishable in cargo, but this repository
        // states it explicitly on the one crate that does, so absence here means
        // an unpublished workspace member and the check stays out of its way.
        "name = \"thing\"",
    ] {
        let offenders = refused_by_the_registry(
            "fixture/Cargo.toml",
            &format!(
                r#"
                    [package]
                    {package}

                    [dependencies]
                    elsewhere = {{ git = "ssh://git@example.invalid/e.git" }}
                "#
            ),
        );
        assert!(
            offenders.is_empty(),
            "an unpublished crate was reported for a git pin it is entitled to: {offenders:#?}"
        );
    }
}

#[test]
fn a_versioned_path_a_registry_version_or_a_path_only_dev_dependency_is_fine() {
    let offenders = refused_by_the_registry(
        "fixture/Cargo.toml",
        r#"
            [package]
            name = "thing"
            publish = true

            [dependencies]
            sibling = { path = "../sibling", version = "0.0.1" }
            plain = "1"
            featured = { version = "0.8", features = ["parse"] }

            [dev-dependencies]
            local_only = { path = ".." }
        "#,
    );
    assert!(
        offenders.is_empty(),
        "a dependency the registry would take was reported: {offenders:#?}"
    );
}

#[test]
fn a_path_dependency_with_no_version_is_reported_outside_dev_dependencies() {
    for table in ["dependencies", "build-dependencies"] {
        let offenders = refused_by_the_registry(
            "fixture/Cargo.toml",
            &format!(
                r#"
                    [package]
                    name = "thing"
                    publish = true

                    [{table}]
                    sibling = {{ path = "../sibling" }}
                "#
            ),
        );
        assert_eq!(
            offenders.len(),
            1,
            "a versionless path under `{table}` went unseen"
        );
        assert_eq!(offenders[0].why, Why::PathWithoutVersion);
        assert_eq!(offenders[0].table, table);
    }
}

#[test]
fn publish_as_a_registry_list_naming_crates_io_still_publishes() {
    // Read as a bool this is `None`, which the check would take for "does not
    // publish" and skip. The crate publishes.
    let offenders = refused_by_the_registry(
        "fixture/Cargo.toml",
        r#"
            [package]
            name = "thing"
            publish = ["crates-io"]

            [dependencies]
            elsewhere = { git = "ssh://git@example.invalid/e.git" }
        "#,
    );
    assert_eq!(offenders.len(), 1, "a list-form publish was skipped");

    // And a list naming somewhere else does not publish to crates.io, so the
    // refusal never arrives and the check stays out of the way.
    let offenders = refused_by_the_registry(
        "fixture/Cargo.toml",
        r#"
            [package]
            name = "thing"
            publish = ["my-registry"]

            [dependencies]
            elsewhere = { git = "ssh://git@example.invalid/e.git" }
        "#,
    );
    assert!(offenders.is_empty(), "{offenders:#?}");
}
