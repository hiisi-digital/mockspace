//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The list of variables that select a repository is written once for the
//! tests, in `mockspace_manifest::ambient::REPOSITORY_VARIABLES`.
//!
//! It had been typed three times, in the scrub of a child's environment, in the
//! scrub of the process, and in the launcher's own test, and a list typed in
//! three places narrows in one of them without anything noticing. What holds it
//! to one is this check: `GIT_ALTERNATE_OBJECT_DIRECTORIES`, the name only such
//! a list carries, appears in a source file only where that list or the
//! engine's own is declared.

use std::path::PathBuf;

#[path = "common/ambient.rs"]
mod ambient;
#[path = "common/sources.rs"]
mod sources;

/// The name that only a list of the repository-selecting variables carries.
const MARKER: &str = "GIT_ALTERNATE_OBJECT_DIRECTORIES";

/// Files that may name it, with what each is.
const DECLARES: &[(&str, &str)] = &[
    (
        "mockspace-manifest/src/ambient.rs",
        "the tests' list, the one",
    ),
    (
        "src/entry/env.rs",
        "the engine's own set, which it drops from its process at entry and pins in a test",
    ),
    (
        "tests/the_repository_variables_are_listed_once.rs",
        "this check",
    ),
];

/// The files in `named` (path and text) that name the marker and are not declared.
fn stray_lists(named: &[(String, String)]) -> Vec<String> {
    named
        .iter()
        .filter(|(rel, text)| {
            text.contains(MARKER) && !DECLARES.iter().any(|(declared, _)| declared == rel)
        })
        .map(|(rel, _)| rel.clone())
        .collect()
}

#[test]
fn the_list_of_repository_variables_is_declared_in_the_places_that_may() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources::rust_files(&root, &mut files);
    assert!(
        files.len() > 40,
        "the walk found {} files; it is looking in the wrong place",
        files.len()
    );
    let named: Vec<(String, String)> = files
        .iter()
        .map(|p| {
            (
                sources::relative(&root, p),
                std::fs::read_to_string(p).unwrap(),
            )
        })
        .collect();
    for (rel, why) in DECLARES {
        let text = &named
            .iter()
            .find(|(r, _)| r == rel)
            .unwrap_or_else(|| panic!("{rel} ({why}) is not in the walk"))
            .1;
        assert!(
            text.contains(MARKER),
            "{rel} ({why}) no longer names the list; drop it from DECLARES"
        );
    }
    assert_eq!(
        stray_lists(&named),
        Vec::<String>::new(),
        "a second copy of the list; use `mockspace_manifest::ambient`"
    );
}

#[test]
fn a_copy_of_the_list_elsewhere_is_named() {
    let copy = "for k in [\"GIT_DIR\", \"GIT_ALTERNATE_OBJECT_DIRECTORIES\"] { c.env_remove(k); }";
    let named = vec![
        ("cargo-mock/tests/x.rs".to_string(), copy.to_string()),
        (
            "mockspace-manifest/src/ambient.rs".to_string(),
            copy.to_string(),
        ),
        (
            "tests/y.rs".to_string(),
            "c.env_remove(\"GIT_DIR\");".to_string(),
        ),
    ];
    assert_eq!(stray_lists(&named), vec![
        "cargo-mock/tests/x.rs".to_string()
    ]);
}
