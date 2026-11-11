//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A path of a repository as `.gitattributes` writes one, for whatever names a merge driver there.

use std::path::Path;

/// `path` relative to `repo_root`, `/` separated, or `None` where it is outside the repository.
/// The root itself is the empty string.
pub fn relative(repo_root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(repo_root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// A repository-relative path as a `.gitattributes` pattern: `/` separated,
/// anchored at the root, and with what a pattern reads as syntax escaped.
pub fn pattern(rel: &str) -> String {
    let mut out = String::from("/");
    for c in rel.chars() {
        if matches!(c, ' ' | '#' | '!' | '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
