//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A crate that publishes MPL-2.0 source ships the licence text with it.
//!
//! Section 3.1 requires the notice to travel with the source, and cargo will
//! not put it there by accident. Only a short list survives an `include`
//! list: the manifest, the readme, and whatever `license-file` names. A crate
//! whose licence sits one directory up, which is every crate in a repository
//! with one `LICENSE` at the root, ships none unless it says so.
//!
//! Nothing reports this. `cargo package` succeeds, `cargo publish` succeeds,
//! the crate appears on the registry with the right licence *label*, and the
//! tarball is the only place the absence shows. By then the version is on the
//! registry and cannot be replaced.
//!
//! Checked against the manifest rather than by unpacking a tarball, because
//! the manifest is what decides and the check should run without building
//! anything.

use std::fs;
use std::path::{Path, PathBuf};

/// Why a publishing crate's licence would not reach the tarball.
#[derive(Debug, PartialEq)]
enum Why {
    /// An `include` list is present, so cargo's defaults are gone, and no
    /// `license-file` names the text to copy in.
    IncludeWithoutLicenseFile,
    /// `license-file` names a path that is not there.
    LicenseFileMissing(String),
}

/// A publishing manifest whose licence would not ship, or nothing.
fn licence_would_not_ship(dir: &Path, text: &str) -> Option<Why> {
    let doc: toml_edit::DocumentMut = text.parse().ok()?;
    let package = doc.get("package")?;

    // Same reading as the sibling check: `publish` is a bool or a list of
    // registries, and a list naming `crates-io` publishes there.
    let publishes = match package.get("publish") {
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
        return None;
    }

    match package.get("license-file").and_then(|v| v.as_str()) {
        Some(rel) => {
            // Relative to the manifest's own directory, which is how cargo
            // reads it.
            if dir.join(rel).is_file() {
                None
            } else {
                Some(Why::LicenseFileMissing(rel.to_string()))
            }
        },
        // No `license-file`. That is fine only while cargo's defaults are
        // intact, which an `include` list removes.
        None if package.get("include").is_some() => Some(Why::IncludeWithoutLicenseFile),
        None => None,
    }
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
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

#[test]
fn every_publishing_crate_ships_its_licence_text() {
    let root = repo_root();
    let manifests = authored_manifests(&root);
    assert!(
        manifests.len() > 1,
        "the walk found {} manifests, which is fewer than this repository has, \
         so it is reporting on a tree it did not read",
        manifests.len()
    );

    let mut publishing = 0;
    let mut problems = Vec::new();
    for manifest in &manifests {
        let Ok(text) = fs::read_to_string(manifest) else {
            continue;
        };
        let dir = manifest.parent().unwrap_or(&root);
        if text.contains("publish = true") {
            publishing += 1;
        }
        if let Some(why) = licence_would_not_ship(dir, &text) {
            let rel = manifest.strip_prefix(&root).unwrap_or(manifest);
            problems.push(format!("{}: {why:?}", rel.display()));
        }
    }

    assert!(
        publishing > 0,
        "no manifest in this repository publishes, so this check passed over \
         an empty set rather than over the crates it is for"
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_check_catches_both_ways_a_licence_goes_missing() {
    // The control, and it is the shape both mockspace publishing crates had:
    // an `include` list, a licence at the repository root, and nothing naming
    // it.
    let dir = repo_root();

    let stripped = "[package]\nname = \"x\"\npublish = true\nlicense = \"MPL-2.0\"\n\
                    include = [\"src/**/*.rs\"]\n";
    assert_eq!(
        licence_would_not_ship(&dir, stripped),
        Some(Why::IncludeWithoutLicenseFile)
    );

    let dangling = "[package]\nname = \"x\"\npublish = true\nlicense = \"MPL-2.0\"\n\
                    include = [\"src/**/*.rs\"]\nlicense-file = \"../nope\"\n";
    assert_eq!(
        licence_would_not_ship(&dir, dangling),
        Some(Why::LicenseFileMissing("../nope".to_string()))
    );

    // And the shapes that are fine, so the check is not simply always positive.
    let named = "[package]\nname = \"x\"\npublish = true\nlicense = \"MPL-2.0\"\n\
                 include = [\"src/**/*.rs\"]\nlicense-file = \"LICENSE\"\n";
    assert_eq!(licence_would_not_ship(&dir, named), None);

    let no_include = "[package]\nname = \"x\"\npublish = true\nlicense = \"MPL-2.0\"\n";
    assert_eq!(licence_would_not_ship(&dir, no_include), None);

    let private = "[package]\nname = \"x\"\npublish = false\nlicense = \"MPL-2.0\"\n\
                   include = [\"src/**/*.rs\"]\n";
    assert_eq!(licence_would_not_ship(&dir, private), None);

    // A registry list rather than a bool, which the bool reading would take
    // for "does not publish" and skip.
    let listed = "[package]\nname = \"x\"\npublish = [\"crates-io\"]\nlicense = \"MPL-2.0\"\n\
                  include = [\"src/**/*.rs\"]\n";
    assert_eq!(
        licence_would_not_ship(&dir, listed),
        Some(Why::IncludeWithoutLicenseFile)
    );
}
