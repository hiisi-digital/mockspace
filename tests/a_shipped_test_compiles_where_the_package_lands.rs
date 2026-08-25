//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A test that ships in a crate has to compile where that crate lands, and
//! `cargo publish` is no help here: verification builds the lib and the bins
//! and no test at all, so a test that only compiles from a checkout passes
//! every gate on the way out and fails for whoever unpacks the crate.
//!
//! Two ways it happens, and both are ordinary while writing the test.
//!
//! An `include_str!` reaching above the package root reads a file that is in
//! the repository and not in the tarball. The repository's own readme is the
//! usual one, since `readme = "../README.md"` puts a copy at the package root
//! and makes the checkout path look harmless.
//!
//! An import of a dev-dependency declared by path alone resolves in a checkout
//! and nowhere else, because cargo strips a path-only dev-dependency whole.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The manifests of the crates in this repository that publish.
fn publishing_manifests() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let root = repo_root();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                // `target` is build output and `mock/research` is the audit
                // trail, neither of which publishes anything.
                if name == "target" || name == ".git" || name == "research" {
                    continue;
                }
                stack.push(path);
            } else if name == "Cargo.toml" {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                // Read keys, not the file. `publish = false` inside a comment
                // is prose about a neighbouring crate, and matching it read
                // the one crate that does publish as one that does not.
                let keys: Vec<&str> = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.starts_with('#'))
                    .collect();
                if keys.iter().any(|l| l.starts_with("publish") && l.contains("false"))
                    || !keys.iter().any(|l| l.starts_with("name"))
                {
                    continue;
                }
                let label = keys
                    .iter()
                    .find_map(|l| l.strip_prefix("name = "))
                    .map(|n| n.trim().trim_matches('"').to_string())
                    .unwrap_or_default();
                out.push((label, path));
            }
        }
    }
    out.sort();
    out
}

/// The test files an `include` list names. An absent `include` ships every
/// test in the crate, which is reported as such rather than guessed at.
fn shipped_tests(manifest: &Path, text: &str) -> Result<Vec<PathBuf>, String> {
    let dir = manifest.parent().expect("a manifest has a directory");
    let tests_dir = dir.join("tests");
    let all: BTreeSet<PathBuf> = std::fs::read_dir(&tests_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "rs"))
        .collect();

    if all.is_empty() {
        return Ok(Vec::new());
    }

    let Some(at) = text.find("\ninclude = [") else {
        return Err(format!(
            "{} has {} test file(s) and no `include` list, so every one of them \
             ships. Name what should ship, one file at a time.",
            manifest.display(),
            all.len()
        ));
    };
    let body = &text[at..];
    let end = body.find(']').unwrap_or(body.len());
    let listed = &body[..end];

    Ok(all
        .into_iter()
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
            listed.contains(&name) || listed.contains("tests/**")
        })
        .collect())
}

/// Every path an `include*!` macro in `text` names.
fn included_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for macro_name in ["include_str!", "include_bytes!", "include!"] {
        let mut rest = text;
        while let Some(at) = rest.find(macro_name) {
            let after = &rest[at + macro_name.len()..];
            let Some(open) = after.find('"') else { break };
            let body = &after[open + 1..];
            let Some(close) = body.find('"') else { break };
            out.push(body[..close].to_string());
            rest = &body[close..];
        }
    }
    out
}

/// Dev-dependency names cargo strips on publish: declared by path, with no
/// `version` to resolve against the registry. Returned as module names.
fn stripped_dev_deps(text: &str) -> BTreeSet<String> {
    let Some(at) = text.find("[dev-dependencies]") else {
        return BTreeSet::new();
    };
    let body = &text[at..];
    let end = body[1..].find("\n[").map(|i| i + 1).unwrap_or(body.len());
    body[..end]
        .lines()
        .filter_map(|line| {
            let (name, rhs) = line.split_once('=')?;
            let name = name.trim();
            if name.is_empty() || name.starts_with('#') || name.starts_with('[') {
                return None;
            }
            (rhs.contains("path") && !rhs.contains("version")).then(|| name.replace('-', "_"))
        })
        .collect()
}

#[test]
fn no_shipped_test_reads_a_file_above_the_package_root() {
    let manifests = publishing_manifests();
    // A sweep that found no publishing crate would pass in silence, which is
    // the one result this test must never give quietly.
    assert!(
        !manifests.is_empty(),
        "no publishing crate was found in this repository, so this test \
         checked nothing"
    );

    let mut checked = 0usize;
    let mut problems = Vec::new();

    for (name, manifest) in &manifests {
        let text = std::fs::read_to_string(manifest).expect("manifest reads");
        let tests = match shipped_tests(manifest, &text) {
            Ok(t) => t,
            Err(e) => {
                problems.push(e);
                continue;
            }
        };
        for test in tests {
            checked += 1;
            let body = std::fs::read_to_string(&test).expect("test reads");
            let dir_depth = test
                .strip_prefix(manifest.parent().unwrap())
                .expect("a test sits under its own crate")
                .components()
                .count()
                - 1;
            for path in included_paths(&body) {
                // Each leading `../` climbs one level. More of them than the
                // file sits deep in the package means the path leaves it.
                let ups = path.matches("../").count();
                if ups > dir_depth {
                    problems.push(format!(
                        "{name}: {} includes `{path}`, which climbs out of the \
                         package and is not in the tarball",
                        test.display()
                    ));
                }
            }
            for dep in stripped_dev_deps(&text) {
                if body.contains(&format!("use {dep}"))
                    || body.contains(&format!("{dep}::"))
                {
                    problems.push(format!(
                        "{name}: {} names `{dep}`, a dev-dependency declared by \
                         path alone, which cargo strips on publish",
                        test.display()
                    ));
                }
            }
        }
    }

    assert!(
        checked > 0,
        "{} publishing crate(s) were found and not one of them ships a test, \
         so nothing was checked",
        manifests.len()
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_check_recognises_both_shapes_it_is_for() {
    // The control. Neither of these is in the tree; they are what the two
    // failures looked like before they were fixed, kept so the reader can see
    // the check would catch them.
    let escaping = r#"const README: &str = include_str!("../../README.md");"#;
    assert_eq!(included_paths(escaping), vec!["../../README.md".to_string()]);
    assert_eq!(escaping.matches("../").count(), 2);

    let manifest = "[dev-dependencies]\nmockspace-manifest = { path = \"../x\" }\n\
                    tempfile = \"3\"\nother = { path = \"../y\", version = \"0.1\" }\n";
    let stripped = stripped_dev_deps(manifest);
    assert!(stripped.contains("mockspace_manifest"), "{stripped:?}");
    assert!(!stripped.contains("tempfile"), "a registry dev-dep is not stripped");
    assert!(!stripped.contains("other"), "a path dep with a version is not stripped");
}
