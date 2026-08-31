//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Ensure the repo-root `.gitignore` ignores every cargo `target/` build dir.
///
/// Cargo build dirs appear not only at the repo root but nested under any
/// standalone crate inside `benches/`, `tests/`, and `mock/research/sketches/`.
/// A root-anchored `/target` ignore misses those nested ones, so they show up
/// as untracked noise and can be committed by accident. A catch-all `target/`
/// line (no leading slash, so git matches a directory named `target` at any
/// depth) covers all of them at once.
///
/// Idempotent: if any line already reads exactly `target/`, this is a no-op.
/// Otherwise a small marked block is appended; existing entries are left
/// untouched.
pub(crate) fn ensure_gitignore(repo_root: &Path, actions: &mut Vec<String>) {
    let path = repo_root.join(".gitignore");
    let existing = fs::read_to_string(&path).unwrap_or_default();

    if existing.lines().any(|l| l.trim() == "target/") {
        return;
    }

    let block = "\
# === mockspace-managed build artifacts (do not edit) ===
# Catch-all: every cargo build dir, including nested ones under benches/,
# tests/, and research sketches. A leading-slash /target would miss those.
target/
# === end mockspace-managed build artifacts ===
";

    let new_content = if existing.is_empty() {
        block.to_string()
    } else {
        let sep = if existing.ends_with('\n') { "\n" } else { "\n\n" };
        format!("{existing}{sep}{block}")
    };

    if fs::write(&path, new_content).is_ok() {
        actions.push("added catch-all target/ to .gitignore".into());
    }
}
