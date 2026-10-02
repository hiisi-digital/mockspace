//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Unit tests for what the tree on disk adds to resolving a maker's writes:
//! a symlink already in the tree is refused before the run, over a real
//! directory with a real symlink in it.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;

use super::*;

/// Writes `{output}.rs` and under `{output}/`, with `output` under `src/icons/*`.
struct Bake;
impl Tool for Bake {
    fn name(&self) -> &'static str {
        "bake"
    }

    fn description(&self) -> &'static str {
        "bake icons into a module"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Make {
            writes: &["{output}.rs", "{output}/**", "assets/index.md"],
            roots:  &[ArgRoot {
                arg:   "output",
                under: "src/icons/*",
            }],
        }
    }

    fn args(&self) -> &[ArgSpec] {
        &[ArgSpec {
            name:        "output",
            required:    true,
            description: "the module stem",
        }]
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

/// A repository root with `src/icons/` real, and a directory outside it.
fn tree() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    let outside = tmp.path().join("outside");
    fs::create_dir_all(root.join("src/icons")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    (tmp, root, outside)
}

#[test]
fn an_argument_through_a_symlinked_directory_is_refused() {
    // The case that must fail: `src/icons/ui` passes every string check and
    // is a symlink out of the tree, so every write would land where nothing
    // observes it.
    let (_tmp, root, outside) = tree();
    symlink(&outside, root.join("src/icons/ui")).unwrap();
    assert_eq!(
        resolve_writes(&Bake, &["src/icons/ui"]).map(|w| w.len()),
        Ok(3)
    );
    let found = symlinked_writes(&Bake, &["src/icons/ui"], &root);
    assert!(!found.is_empty(), "a symlinked value must be refused");
    assert!(found[0].contains("`src/icons/ui`"), "{found:?}");
    assert!(found[0].contains("symlink"), "{found:?}");
}

#[test]
fn a_symlink_above_the_value_or_at_its_written_file_is_refused() {
    // A symlinked parent carries the value out as surely, and so does a
    // committed `<output>.rs` that is itself a link: the write follows it.
    let (_tmp, root, outside) = tree();
    fs::remove_dir(root.join("src/icons")).unwrap();
    symlink(&outside, root.join("src/icons")).unwrap();
    assert!(!symlinked_writes(&Bake, &["src/icons/ui"], &root).is_empty());

    let (_tmp, root, outside) = tree();
    fs::write(outside.join("target.rs"), "x\n").unwrap();
    symlink(outside.join("target.rs"), root.join("src/icons/ui.rs")).unwrap();
    let found = symlinked_writes(&Bake, &["src/icons/ui"], &root);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("`src/icons/ui.rs`"), "{found:?}");
}

#[test]
fn a_value_over_real_or_absent_paths_is_not_refused() {
    // The control: nothing existing yet, and a real directory, are both fine.
    let (_tmp, root, _) = tree();
    assert_eq!(
        symlinked_writes(&Bake, &["src/icons/ui"], &root),
        Vec::<String>::new()
    );
    fs::create_dir_all(root.join("src/icons/ui")).unwrap();
    fs::write(root.join("src/icons/ui.rs"), "x\n").unwrap();
    assert_eq!(
        symlinked_writes(&Bake, &["src/icons/ui"], &root),
        Vec::<String>::new()
    );
}
