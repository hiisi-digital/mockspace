//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A listing of changed paths, made to decide what is linted or gated, names the
//! source of a move as well as its destination.
//!
//! Git detects renames by default, and then `diff --name-only` prints the
//! destination alone: `git mv mock/crates/a/src/x.rs mock/crates/b/src/x.rs`
//! lists `b` and never `a`. The crate that lost the file is not in the scope,
//! is not linted, and the move that broke it is found by whichever commit next
//! touches it. A scoping listing is therefore asked with `--no-renames`.
//!
//! What this checks is every `--name-only` in the non-test source of the packages
//! of this repository: each is within a few lines of `--no-renames`, or of a
//! command that lists a tree and not a change, or is declared in `EXEMPT` with
//! its reason. A listing spelled some other way (`--name-status`, `--raw`, a
//! `status --porcelain`) is not read, which is the one thing this cannot see; those
//! are the rename-aware ones (`src_layout` asks `-M100%` on purpose, and the
//! porcelain readers parse the `R` entry).

use std::path::PathBuf;

#[path = "common/ambient.rs"]
mod ambient;
#[path = "common/sources.rs"]
mod sources;

/// Where the non-test source of the repository's packages lives.
const ROOTS: &[&str] = &[
    "src",
    "mockspace-manifest/src",
    "lint-rules/src",
    "cargo-mock/src",
    "bench-harness/src",
];

/// Files that list changed paths without `--no-renames`, with the reason.
const EXEMPT: &[(&str, &str)] = &[
    (
        "src/autofix.rs",
        "it lists what to format and stage again, and the source of a move is a deletion with \
         nothing in it to format",
    ),
    (
        "src/render_agent/builtins.rs",
        "its `git diff --name-only` is the unstaged working tree of an edit hook, where git \
         detects no rename at all",
    ),
];

/// How many lines either side of a `--name-only` may carry the other flag.
const REACH: usize = 6;

/// A `--name-only` that lists a tree or an index and not a change.
const NOT_A_CHANGE: &[&str] = &["ls-tree", "ls-files"];

/// The `file:line` of each `--name-only` in `text` with no `--no-renames` near it,
/// stopping at the file's test module.
fn unguarded(rel: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let end = lines
        .iter()
        .position(|l| l.trim_start().starts_with("#[cfg(test)]"))
        .unwrap_or(lines.len());
    (0 .. end)
        .filter(|&i| !lines[i].trim_start().starts_with("//") && lines[i].contains("--name-only"))
        .filter(|&i| {
            let window = lines[i.saturating_sub(REACH) .. (i + REACH + 1).min(end)].join("\n");
            !window.contains("--no-renames") && !NOT_A_CHANGE.iter().any(|c| window.contains(c))
        })
        .map(|i| format!("{rel}:{}: {}", i + 1, lines[i].trim()))
        .collect()
}

#[test]
fn every_scoping_listing_of_changed_paths_is_asked_without_renames() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut scanned = 0;
    let mut found = Vec::new();
    for dir in ROOTS {
        let mut files = Vec::new();
        sources::rust_files(&root.join(dir), &mut files);
        for path in files {
            let rel = sources::relative(&root, &path);
            let name = rel.rsplit('/').next().unwrap();
            if name == "tests.rs" || name.ends_with("_tests.rs") || rel.contains("/tests/") {
                continue;
            }
            scanned += 1;
            if EXEMPT.iter().any(|(e, _)| *e == rel) {
                continue;
            }
            found.extend(unguarded(&rel, &std::fs::read_to_string(&path).unwrap()));
        }
    }
    assert!(
        scanned > 100,
        "the walk found {scanned} files; it is looking in the wrong place"
    );
    assert!(
        found.is_empty(),
        "these list changed paths and would name only the destination of a move; add \
         `--no-renames`:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_exempt_file_exists_and_still_lists_without_the_flag() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for (rel, why) in EXEMPT {
        let text = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel} is exempt ({why}) and cannot be read: {e}"));
        assert!(
            !unguarded(rel, &text).is_empty(),
            "{rel} is exempt ({why}) but needs no exemption"
        );
    }
}

#[test]
fn a_listing_without_the_flag_is_named_and_one_with_it_is_not() {
    let bare = "let staged = git(&[\"diff\", \"--cached\", \"--name-only\", \"--\", mock_rel]);\n";
    assert_eq!(unguarded("src/x.rs", bare).len(), 1);
    let shell = "STAGED=$(git diff --cached --name-only -- \"$MOCK_DIR\")\n";
    assert_eq!(unguarded("src/x.rs", shell).len(), 1);
    let guarded = "\"diff\",\n\"--cached\",\n\"--name-only\",\n\"--no-renames\",\n\"-z\",\n";
    assert!(unguarded("src/x.rs", guarded).is_empty());
    assert!(unguarded("src/x.rs", "git ls-tree -r --name-only HEAD\n").is_empty());
    // the test module of a file is not read
    assert!(
        unguarded(
            "src/x.rs",
            "fn f() {}\n#[cfg(test)]\nmod t { const A: &str = \"--name-only\"; }\n"
        )
        .is_empty()
    );
}
