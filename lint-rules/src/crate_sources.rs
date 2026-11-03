//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The source files of a crate, read the way the lint pass reads them.
//!
//! A lint is handed these through its context. A tool has no context and reads the same files from
//! here, so the files a tool counts for a crate are the files the lints count and not a second
//! reading of the directory.

use std::path::{Path, PathBuf};

use crate::CrateSourceFile;

/// Walk `crate_dir/src/**/*.rs` and return every file's (rel_path, text), with `src/lib.rs` first
/// when it exists and the rest in path order. A file that cannot be read is skipped, and surfaces
/// through the lint pass seeing missing content.
#[must_use]
pub fn collect_crate_sources(crate_dir: &Path) -> Vec<CrateSourceFile> {
    let src_dir = crate_dir.join("src");
    if !src_dir.is_dir() {
        return Vec::new();
    }
    let mut out: Vec<CrateSourceFile> = Vec::new();
    walk_rs(&src_dir, crate_dir, &mut out);
    // Sort so `src/lib.rs` lands first, then everything else in path order.
    out.sort_by(|a, b| {
        let a_is_lib = a.rel_path == PathBuf::from("src/lib.rs");
        let b_is_lib = b.rel_path == PathBuf::from("src/lib.rs");
        match (a_is_lib, b_is_lib) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.rel_path.cmp(&b.rel_path),
        }
    });
    out
}

fn walk_rs(dir: &Path, crate_dir: &Path, out: &mut Vec<CrateSourceFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rs(&path, crate_dir, out);
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let rel_path = path.strip_prefix(crate_dir).unwrap_or(&path).to_path_buf();
        out.push(CrateSourceFile { rel_path, text });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn rel_paths(files: &[CrateSourceFile]) -> Vec<String> {
        files.iter().map(|f| f.rel_path.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn a_directory_with_no_src_has_no_sources() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "build.rs", "fn main() {}");
        assert!(collect_crate_sources(dir.path()).is_empty());
    }

    #[test]
    fn the_root_comes_first_and_the_rest_in_path_order() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/zeta.rs", "");
        write(dir.path(), "src/lib.rs", "");
        write(dir.path(), "src/alpha/mod.rs", "");
        write(dir.path(), "src/alpha/deep/inner.rs", "");
        let found = collect_crate_sources(dir.path());
        assert_eq!(
            rel_paths(&found),
            ["src/lib.rs", "src/alpha/deep/inner.rs", "src/alpha/mod.rs", "src/zeta.rs"]
        );
    }

    #[test]
    fn only_rust_files_under_src_are_read_and_their_text_comes_with_them() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "pub struct A;");
        write(dir.path(), "src/notes.md", "not source");
        write(dir.path(), "tests/it.rs", "not under src");
        write(dir.path(), "fixtures/beta/src/lib.rs", "not this crate's");
        let found = collect_crate_sources(dir.path());
        assert_eq!(rel_paths(&found), ["src/lib.rs"]);
        assert_eq!(found[0].text, "pub struct A;");
    }
}
