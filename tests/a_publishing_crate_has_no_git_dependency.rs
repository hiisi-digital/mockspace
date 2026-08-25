//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A crate marked `publish = true` carries no dependency pinned to a git
//! repository, because crates.io refuses one and the refusal arrives at
//! `cargo publish` rather than at any gate before it.
//!
//! The failure is quiet in the direction that costs most. Everything builds,
//! every test passes, `cargo package` succeeds, and the manifest reads as
//! deliberate: `publish = true` beside a comment naming the crate as the one
//! that publishes. Nothing between committing and the publish itself looks at
//! the pair, so the defect surfaces at the one moment when the surrounding work
//! is already finished and a release is half done.
//!
//! A path dependency is a different case and is allowed here. Cargo strips a
//! path-only dev-dependency on publish, and a path dependency carrying a
//! `version` resolves against the registry, so neither is a git pin.

use std::fs;
use std::path::{Path, PathBuf};

/// One manifest that says it publishes and names a dependency by git.
#[derive(Debug, PartialEq)]
struct GitPin {
    manifest: String,
    table:    String,
    dep:      String,
}

/// Every git-pinned dependency of a manifest that publishes.
///
/// Returns nothing for a manifest that does not publish, whatever it depends
/// on, since an unpublished crate may pin anything it likes.
fn git_pins_of(manifest_label: &str, text: &str) -> Vec<GitPin> {
    let doc: toml_edit::DocumentMut = match text.parse() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let publishes = doc
        .get("package")
        .and_then(|p| p.get("publish"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !publishes {
        return Vec::new();
    }

    let mut found = Vec::new();
    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(deps) = doc.get(table).and_then(|t| t.as_table_like()) else {
            continue;
        };
        for (dep, spec) in deps.iter() {
            let pinned_to_git = match spec.as_table_like() {
                Some(t) => t.get("git").is_some(),
                None => false,
            };
            if pinned_to_git {
                found.push(GitPin {
                    manifest: manifest_label.to_string(),
                    table:    table.to_string(),
                    dep:      dep.to_string(),
                });
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
                if name == "target" || name == ".git" || name == "research" {
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

/// Red until `renki` is on crates.io.
///
/// `cargo-mock` publishes and pins `renki` by revision, which is correct while
/// `renki` is unpublished: a branch pin is not a version constraint, and there
/// is no version to ask for yet. The moment `renki` has one this becomes a
/// version dependency and the ignore comes off in the same change.
///
/// Left with its real assertion rather than softened, so removing one line is
/// the whole of the flip. The four cases below it are the controls and run
/// normally, so the check itself is exercised on every run.
#[ignore = "catalogue: cargo-mock pins renki by rev until renki publishes 0.0.1"]
#[test]
fn no_publishing_crate_pins_a_dependency_to_git() {
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
        offenders.extend(git_pins_of(&label, &text));
    }

    assert!(
        offenders.is_empty(),
        "a crate that publishes pins a dependency to a git repository, which crates.io \
         refuses at `cargo publish`. Give the dependency a registry version, or set \
         `publish = false` until it has one: {offenders:#?}"
    );
}

#[test]
fn a_publishing_crate_pinned_to_git_is_reported() {
    let offenders = git_pins_of(
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
        vec![GitPin {
            manifest: "fixture/Cargo.toml".to_string(),
            table:    "dependencies".to_string(),
            dep:      "elsewhere".to_string(),
        }],
        "the check cannot see the defect it exists for"
    );
}

#[test]
fn a_git_pin_under_dev_or_build_dependencies_is_reported_too() {
    for table in ["dev-dependencies", "build-dependencies"] {
        let offenders = git_pins_of(
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
        let offenders = git_pins_of(
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
fn a_path_or_registry_dependency_is_not_a_git_pin() {
    let offenders = git_pins_of(
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
        "a dependency with no git pin was reported: {offenders:#?}"
    );
}
