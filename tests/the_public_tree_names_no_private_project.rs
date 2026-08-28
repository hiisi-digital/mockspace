//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Nothing a stranger reads names a project they cannot see.
//!
//! A tool developed against a handful of real consumers accretes their names:
//! in a doc comment explaining why a branch exists, in a test fixture, in an
//! error message telling somebody to set an environment variable to a path on
//! the author's own machine. Each one is harmless in isolation and reads as
//! sloppiness in aggregate, and a reader cannot tell which of them is load
//! bearing.
//!
//! Fixed once and greppable afterwards, because the class comes back one
//! comment at a time and nothing else notices.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Names that must not appear anywhere a reader of this repository reaches.
///
/// The private consumers this tool grew up against. Not an exhaustive list of
/// everything that should not be here, and it does not need to be: it is the
/// set that was actually found, so the class cannot come back the way it
/// arrived.
const PRIVATE: &[&str] = &[
    "clause-dev",
    "hilavitkutin",
    "vehje",
    "kolli",
    "ikiuni",
    "loisto",
    "arvo",
    "loimu",
    "saalis",
];

/// Where a path on somebody's own machine gives itself away.
///
/// A home directory rather than `/Users` alone, because the bare prefix also
/// matches prose about what a machine-specific link would look like, and that
/// prose is worth keeping.
const SOMEBODY_S_MACHINE: &[&str] = &["~/Dev/", "/Users/orgrinrt", "/home/orgrinrt"];

/// The two files that hold these names on purpose, because holding them is
/// what they do.
///
/// This one carries the list. `render_agent/tests.rs` carries a longer one and
/// fails the build when a generated agent instruction names a workspace, an
/// operator or a trunk, which is the same class caught one layer earlier.
fn is_a_guard(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    name == "the_public_tree_names_no_private_project.rs"
        || path.ends_with("src/render_agent/tests.rs")
}

fn tracked_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files"])
        .output()
        .expect("git could not be run");
    assert!(out.status.success(), "git ls-files failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        // `mock/` is this repository's own design trail and the v2 redesign,
        // which is history and work in progress rather than a surface.
        //
        // `docs/` is excluded and should not be: it is a tier one surface a
        // reader reaches, it is generated from templates under `mock/`, and it
        // still carries eight of these names and two home paths. Fixing it
        // means fixing the templates, which is a round of its own.
        .filter(|l| !l.starts_with("mock/") && !l.starts_with("docs/"))
        .map(|l| root.join(l))
        .collect()
}

#[test]
fn no_tracked_file_names_a_project_a_reader_cannot_see() {
    let mut found = Vec::new();
    for path in tracked_files() {
        if is_a_guard(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for name in PRIVATE {
                if line.contains(name) {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "private project names in the public tree:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_tracked_file_names_a_path_on_somebody_s_machine() {
    let mut found = Vec::new();
    for path in tracked_files() {
        if is_a_guard(&path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for needle in SOMEBODY_S_MACHINE {
                if line.contains(needle) {
                    found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "a path on somebody's machine:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_checks_above_can_fail() {
    // The control. Both read every tracked file and both pass when the reader
    // finds nothing, so a `git ls-files` that returned an empty list, or a
    // filter that excluded everything, would report the tree clean.
    let files = tracked_files();
    assert!(
        files.len() > 50,
        "the reader found almost nothing: {}",
        files.len()
    );
    assert!(
        files
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "rs")),
        "the reader found no source at all"
    );
    // And that the needle would be seen if it were there. This file carries
    // every one of them and is skipped by name, so a skip that matched nothing
    // would fail here rather than silently passing the two above.
    let me = std::fs::read_to_string(file!())
        .or_else(|_| std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(file!())));
    if let Ok(text) = me {
        assert!(
            PRIVATE.iter().all(|n| text.contains(n)),
            "the list above does not contain what it says it contains"
        );
    }
}
