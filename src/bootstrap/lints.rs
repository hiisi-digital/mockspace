//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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

// ──────────────────────────────────────────────────────────────────────
// Tool crate discovery
// ──────────────────────────────────────────────────────────────────────

/// One tool crate discovered under `<mock>/tools/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolCrate {
    /// The subdirectory name, which is the subcommand: `mock <dir>`.
    pub(crate) dir:     String,
    /// The cargo package name from its `Cargo.toml`, which is what the
    /// generated cdylib depends on and is **not** always the directory name.
    pub(crate) package: String,
    /// Absolute path to the crate directory.
    pub(crate) path:    PathBuf,
}

impl ToolCrate {
    /// The Rust identifier the generated collector calls into.
    pub(crate) fn ident(&self) -> String {
        self.package.replace('-', "_")
    }
}

/// Read `[package] name` out of a `Cargo.toml`.
///
/// A tool crate may name its package anything; the directory is the
/// subcommand and the package name is what cargo needs on the dependency
/// line. Conflating the two would force every tool's crate to be named after
/// its command, which is a constraint with no benefit and one real cost: two
/// projects could not both ship a `check` tool without a package-name clash on
/// any registry they later published to.
fn package_name(manifest: &Path) -> Option<String> {
    let text = fs::read_to_string(manifest).ok()?;
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    Some(doc.get("package")?.get("name")?.as_str()?.to_string())
}

/// Discover tool crates under `<mock>/tools/`: every immediate subdirectory
/// carrying a `Cargo.toml`.
///
/// A subdirectory without one is skipped **loudly**. Silence there is the
/// failure worth avoiding: a tool whose manifest is missing or misnamed simply
/// would not exist, `mock <name>` would report an unknown subcommand, and
/// nothing would connect the two facts for whoever wrote it.
///
/// Sorted, so a generated manifest and collector are byte-stable across runs
/// and `write_if_changed` can do its job.
pub(crate) fn discover_tool_crates(tools_dir: &Path) -> Vec<ToolCrate> {
    let mut found = Vec::new();
    if !tools_dir.is_dir() {
        return found;
    }
    let Ok(entries) = fs::read_dir(tools_dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir = entry.file_name().to_string_lossy().to_string();
        if dir.starts_with('.') {
            continue;
        }
        let manifest = path.join("Cargo.toml");
        if !manifest.is_file() {
            eprintln!(
                "warning: skipping `{}`: a tools/ subdirectory needs a Cargo.toml to be built. \
                 Without one it is not a tool and `mock {dir}` will not find it.",
                path.display()
            );
            continue;
        }
        let Some(package) = package_name(&manifest) else {
            eprintln!(
                "warning: skipping `{}`: its Cargo.toml declares no [package] name.",
                path.display()
            );
            continue;
        };
        found.push(ToolCrate {
            dir,
            package,
            path,
        });
    }
    found.sort_by(|a, b| a.dir.cmp(&b.dir));
    found
}

/// The tool names a project offers, read from the directory alone.
///
/// Deliberately does **not** build or load anything. `mock <name>` has to know
/// whether `<name>` is a tool before deciding what to do with it, and the
/// alternative is compiling a cdylib to find out that a typo was a typo. The
/// directory listing is the cheap authority, which is exactly why the
/// subdirectory name is the subcommand rather than something only the compiled
/// code knows.
pub(crate) fn tool_names(mock_dir: &Path) -> Vec<String> {
    discover_tool_crates(&mock_dir.join("tools"))
        .into_iter()
        .map(|t| t.dir)
        .collect()
}

#[cfg(test)]
mod tool_discovery_tests {
    use super::*;

    fn tools_dir() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("mock").join("tools");
        fs::create_dir_all(&dir).unwrap();
        (tmp, dir)
    }

    fn write_crate(dir: &Path, name: &str, package: &str) {
        let c = dir.join(name);
        fs::create_dir_all(c.join("src")).unwrap();
        fs::write(
            c.join("Cargo.toml"),
            format!("[package]\nname = \"{package}\"\nversion = \"0.0.0\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn a_subdirectory_with_a_manifest_is_a_tool() {
        let (_t, dir) = tools_dir();
        write_crate(&dir, "phrase-search", "kamu-phrase-search");
        let found = discover_tool_crates(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].dir, "phrase-search");
        assert_eq!(found[0].package, "kamu-phrase-search");
        assert_eq!(found[0].ident(), "kamu_phrase_search");
    }

    #[test]
    fn a_subdirectory_without_a_manifest_is_not_a_tool() {
        // The case that must fail. A directory of loose scripts is exactly
        // what tools/ replaces, so it must not be silently half-adopted.
        let (_t, dir) = tools_dir();
        fs::create_dir_all(dir.join("not-a-crate")).unwrap();
        fs::write(dir.join("not-a-crate").join("check.py"), "print(1)\n").unwrap();
        assert_eq!(discover_tool_crates(&dir), Vec::new());
    }

    #[test]
    fn a_manifest_with_no_package_name_is_not_a_tool() {
        // A virtual manifest carries [workspace] and no [package], so it has
        // no name to put on a dependency line. Taking the directory name
        // instead would generate a manifest that does not resolve.
        let (_t, dir) = tools_dir();
        let c = dir.join("virtual");
        fs::create_dir_all(&c).unwrap();
        fs::write(c.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
        assert_eq!(discover_tool_crates(&dir), Vec::new());
    }

    #[test]
    fn the_package_name_is_read_rather_than_assumed_from_the_directory() {
        // The negative that matters: if discovery inferred the package from
        // the directory it would emit `phrase-search = { path = ... }` and
        // cargo would not resolve it.
        let (_t, dir) = tools_dir();
        write_crate(&dir, "phrase-search", "kamu-phrase-search");
        let found = discover_tool_crates(&dir);
        assert_ne!(found[0].package, found[0].dir);
    }

    #[test]
    fn a_file_in_the_tools_directory_is_not_a_tool() {
        let (_t, dir) = tools_dir();
        fs::write(dir.join("README.md"), "# tools\n").unwrap();
        assert_eq!(discover_tool_crates(&dir), Vec::new());
    }

    #[test]
    fn dot_directories_are_skipped() {
        let (_t, dir) = tools_dir();
        write_crate(&dir, ".cargo-cache", "cache");
        assert_eq!(discover_tool_crates(&dir), Vec::new());
    }

    #[test]
    fn discovery_is_sorted_so_generation_is_byte_stable() {
        let (_t, dir) = tools_dir();
        for n in ["zeta", "alpha", "middle"] {
            write_crate(&dir, n, n);
        }
        let names: Vec<String> = discover_tool_crates(&dir)
            .into_iter()
            .map(|t| t.dir)
            .collect();
        assert_eq!(names, vec!["alpha", "middle", "zeta"]);
    }

    #[test]
    fn a_project_with_no_tools_directory_has_no_tools() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(tool_names(tmp.path()), Vec::<String>::new());
    }

    #[test]
    fn tool_names_reads_the_directory_without_building_anything() {
        // The property the dispatcher depends on: names are known before any
        // cdylib exists, so an unknown subcommand does not trigger a build.
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        fs::create_dir_all(mock.join("tools")).unwrap();
        write_crate(&mock.join("tools"), "corpus-talk", "corpus-talk");
        assert_eq!(tool_names(&mock), vec!["corpus-talk".to_string()]);
        // nothing was compiled: no target dir exists anywhere under mock
        assert!(!mock.join("target").exists());
    }
}
