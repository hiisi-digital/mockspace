//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The source files of this repository, for the tests that read them as text.
//!
//! Included by path. It names nothing but `std`.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, for a check over the repository's own sources.
/// `mock/` is its own workspace, `target/` is built output, and `.worktrees/`
/// and `.git` are other trees.
pub fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !matches!(name.as_ref(), "target" | "mock" | ".git" | ".worktrees") {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// `path` relative to `root`, with forward slashes.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}
