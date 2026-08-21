#![allow(unused_imports)]
use super::*;

/// Parse the `[lint-crates]` section from mockspace.toml.
///
/// Returns a list of (crate_name, cargo_dep_spec_as_toml_string) pairs in
/// declaration order. Each value is re-emitted verbatim into the proxy's
/// Cargo.toml so any cargo-accepted dep form works: `"0.1"`, `{ path = ... }`,
/// `{ git = ..., branch = ... }`, etc.
///
/// Returns empty vec if mockspace.toml is missing, unparseable, or has no
/// `[lint-crates]` section.
pub(crate) fn parse_lint_crates(mockspace_toml: &Path) -> Vec<(String, String)> {
    let content = match fs::read_to_string(mockspace_toml) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let section = match doc.get("lint-crates").and_then(|i| i.as_table()) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut result = Vec::new();
    for (name, item) in section.iter() {
        // Value form (string like "0.1" or inline table `{ path = ... }`).
        if let Some(v) = item.as_value() {
            result.push((name.to_string(), v.to_string().trim().to_string()));
            continue;
        }
        // Sub-table form: [lint-crates.foo]\n path = "..."
        if let Some(tbl) = item.as_table() {
            // Re-emit as an inline table so it fits on the [dependencies] line.
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in tbl.iter() {
                if let Some(val) = v.as_value() {
                    inline.insert(k, val.clone());
                }
            }
            result.push((name.to_string(), inline.to_string().trim().to_string()));
        }
    }
    result
}

/// Discover `.rs` files in the custom lints directory.
/// Returns a sorted list of file stems (e.g., "my_lint" from "my_lint.rs").
pub(crate) fn discover_custom_lint_files(lints_dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    if !lints_dir.is_dir() {
        return files;
    }

    if let Ok(entries) = fs::read_dir(lints_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "rs").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    let stem_str = stem.to_string_lossy().to_string();
                    if is_valid_rust_ident(&stem_str) {
                        files.push(stem_str);
                    } else {
                        eprintln!(
                            "warning: skipping custom lint file `{}`: stem `{}` is not a valid Rust identifier (only [a-z0-9_] allowed)",
                            path.display(),
                            stem_str,
                        );
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// Check if a string is a valid Rust identifier (only `[a-z0-9_]`, must not start with a digit).
pub(crate) fn is_valid_rust_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.as_bytes()[0];
    if first.is_ascii_digit() {
        return false;
    }
    s.bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Which custom lint entry points a repo's own `mock/lints/*.rs` file defines.
///
/// One field per lint kind in [`LintPack`]. A repo that can only register some
/// of the kinds cannot express the others at all, which is the defect this
/// struct exists to remove: `RepoLint` and `MessageLint` were reachable only
/// from a pack crate, so a repository whose lintable material is documentation
/// rather than source had no way to write a lint for it locally.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LintEntryPoints {
    /// `pub fn lint()` -> a lint handed one package at a time.
    pub(crate) lint:         bool,
    /// `pub fn cross_lint()` -> a lint handed every package at once.
    pub(crate) cross_lint:   bool,
    /// `pub fn repo_lint()` -> a lint handed repository paths, with no packages.
    pub(crate) repo_lint:    bool,
    /// `pub fn message_lint()` -> a lint handed an authored message.
    pub(crate) message_lint: bool,
}

/// Scan a `.rs` file to determine which custom lint functions it defines.
pub(crate) fn scan_lint_functions(lints_dir: &Path, stem: &str) -> LintEntryPoints {
    let path = lints_dir.join(format!("{stem}.rs"));
    let content = fs::read_to_string(&path).unwrap_or_default();

    LintEntryPoints {
        // `cross_lint(` and `message_lint(` both end in `lint(`, so the plain
        // lint probe must not match them. Anchored on the space after `fn`.
        lint:         content.contains("pub fn lint("),
        cross_lint:   content.contains("pub fn cross_lint("),
        repo_lint:    content.contains("pub fn repo_lint("),
        message_lint: content.contains("pub fn message_lint("),
    }
}


// ──────────────────────────────────────────────────────────────────────
// Generated hooks (core.hooksPath target)
// ──────────────────────────────────────────────────────────────────────
