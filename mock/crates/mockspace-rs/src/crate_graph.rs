//! Crate graph builder.
//!
//! Scans `Cargo.toml` files discovered during the project walk and assembles
//! a [`CrateGraph`] of crate names, paths, proc-macro-ness, and dependency
//! lists. The implementation parses each `Cargo.toml` as a TOML table; no
//! cargo subprocess.
//!
//! What we extract per crate:
//!
//! - `[package].name` → [`CrateInfo::name`].
//! - `[lib].proc-macro = true` → [`CrateInfo::is_proc_macro`].
//! - `[dependencies]` keys → [`CrateInfo::deps`].
//! - the directory containing the manifest → [`CrateInfo::root_path`].
//!
//! What we do NOT extract: dev-dependencies / build-dependencies; target-
//! specific deps; workspace-resolved versions; per-target features. Those
//! land if a future lint asks for them. The current shape satisfies:
//!
//! - stack-lints proc-macro skip (the most load-bearing consumer).
//! - workflow-state lints that want to enumerate crates.
//! - cross-doc-symbol primitives that index over crates.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::project::{CrateGraph, CrateInfo};

/// Walk `root`, locate every `Cargo.toml`, and build a [`CrateGraph`].
/// Failures to parse an individual `Cargo.toml` are logged to stderr and
/// the manifest is skipped; the rest of the workspace still scans.
///
/// Each `[dependencies]` entry contributes its manifest key to
/// [`CrateInfo::deps`], not its `package = "..."` rename target. Consumers
/// that need canonical names should look up against the crate-graph's
/// `by_name` map themselves; today's only consumer is the proc-macro
/// skip set, which is keyed on `[package].name`.
pub fn build_crate_graph(root: &Path) -> CrateGraph {
    // Resolve workspace-member directories from a root Cargo.toml first.
    // Empty set means we couldn't determine membership; everything found
    // is then conservatively treated as a workspace member.
    let workspace_members = read_workspace_members(root);

    let mut crates: Vec<CrateInfo> = Vec::new();
    let mut by_name: HashMap<String, usize> = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !is_skipped_dir(e))
    {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.file_name() != "Cargo.toml" {
            continue;
        }
        let manifest_path = entry.path();
        let Some(mut info) = parse_manifest(manifest_path) else {
            continue;
        };
        if !workspace_members.is_empty() {
            info.is_workspace_member = workspace_members
                .iter()
                .any(|m| info.root_path.starts_with(m));
        }
        let idx = crates.len();
        by_name.insert(info.name.clone(), idx);
        crates.push(info);
    }

    CrateGraph { crates, by_name }
}

/// Read the root `Cargo.toml`'s `[workspace].members` array, resolving
/// each entry to an absolute directory. Glob entries (`crates/*`) are
/// expanded via filesystem read. Returns an empty Vec when no root
/// workspace manifest is found or when the `members` array is absent.
fn read_workspace_members(root: &Path) -> Vec<PathBuf> {
    let root_manifest = root.join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(&root_manifest) else {
        return Vec::new();
    };
    let toml: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let Some(members) = toml
        .get("workspace")
        .and_then(|w| w.as_table())
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array())
    else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in members {
        let Some(s) = entry.as_str() else { continue };
        if s.contains('*') {
            // Glob entry. Resolve the directory prefix and read it.
            let prefix = s.split('*').next().unwrap_or("");
            let base = root.join(prefix.trim_end_matches('/'));
            if let Ok(read) = std::fs::read_dir(&base) {
                for sub in read.flatten() {
                    if sub.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        out.push(sub.path());
                    }
                }
            }
        } else {
            out.push(root.join(s));
        }
    }
    out
}

fn is_skipped_dir(entry: &walkdir::DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return true;
    };
    matches!(
        name,
        "target"
            | ".git"
            | "node_modules"
            | ".cargo"
            | "dist"
            | "build"
            | ".idea"
            | ".vscode"
            | ".direnv"
    )
}

fn parse_manifest(path: &Path) -> Option<CrateInfo> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let toml: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("warning: failed to parse {}: {e}", path.display());
            return None;
        }
    };

    // Workspace-only manifests (no [package]) are not crates per se;
    // skip silently. The CrateGraph cares about individual crates.
    let name = toml
        .get("package")
        .and_then(|p| p.as_table())
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())?
        .to_string();

    let is_proc_macro = toml
        .get("lib")
        .and_then(|l| l.as_table())
        .and_then(|l| l.get("proc-macro"))
        .and_then(|p| p.as_bool())
        .unwrap_or(false);

    // Resolve canonical crate names: when a dep is rename-aliased
    // (`alias = { package = "real-name" }`), record "real-name" rather
    // than the manifest key. Plain entries (`alias = "1.0"`) record
    // the key itself. The intent is that CrateGraph::by_name lookups
    // against [package].name find a match for both shapes.
    let deps: Vec<String> = toml
        .get("dependencies")
        .and_then(|d| d.as_table())
        .map(|t| {
            t.iter()
                .map(|(key, value)| {
                    value
                        .as_table()
                        .and_then(|tbl| tbl.get("package"))
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| key.clone())
                })
                .collect()
        })
        .unwrap_or_default();

    let root_path = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    Some(CrateInfo {
        name,
        root_path,
        is_proc_macro,
        // Default false; build_crate_graph upgrades to true when the
        // root manifest's [workspace].members covers this path.
        is_workspace_member: false,
        deps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write(p: &Path, contents: &str) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        let mut f = fs::File::create(p).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn extracts_crate_name_and_deps() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            r#"
[package]
name = "foo"
version = "0.1.0"

[dependencies]
serde = "1"
thiserror = "1"
"#,
        );
        let graph = build_crate_graph(tmp.path());
        let foo = graph.get("foo").expect("foo present");
        assert_eq!(foo.name, "foo");
        assert!(foo.deps.contains(&"serde".to_string()));
        assert!(foo.deps.contains(&"thiserror".to_string()));
        assert!(!foo.is_proc_macro);
    }

    #[test]
    fn flags_proc_macro_crate() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("crates/mac/Cargo.toml"),
            r#"
[package]
name = "mac"
version = "0.1.0"

[lib]
proc-macro = true
"#,
        );
        let graph = build_crate_graph(tmp.path());
        assert!(graph.is_proc_macro("mac"));
    }

    #[test]
    fn workspace_only_manifest_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["crates/foo"]
"#,
        );
        let graph = build_crate_graph(tmp.path());
        // No crate name was extracted from the workspace root manifest.
        assert!(graph.crates.is_empty());
    }

    #[test]
    fn target_dir_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"",
        );
        write(
            &tmp.path().join("target/debug/deps/something/Cargo.toml"),
            "[package]\nname = \"vendored\"",
        );
        let graph = build_crate_graph(tmp.path());
        assert!(graph.get("foo").is_some());
        assert!(graph.get("vendored").is_none(), "target/ should be skipped");
    }

    #[test]
    fn dep_rename_resolves_to_canonical_package_name() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            r#"
[package]
name = "foo"

[dependencies]
plain = "1"
renamed = { version = "1", package = "real-name" }
"#,
        );
        let graph = build_crate_graph(tmp.path());
        let foo = graph.get("foo").unwrap();
        assert!(foo.deps.contains(&"plain".to_string()));
        assert!(
            foo.deps.contains(&"real-name".to_string()),
            "canonical name should land instead of alias `renamed`. got: {:?}",
            foo.deps
        );
        assert!(!foo.deps.contains(&"renamed".to_string()));
    }

    #[test]
    fn workspace_member_glob_resolution() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("Cargo.toml"),
            r#"
[workspace]
members = ["crates/*"]
"#,
        );
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            "[package]\nname = \"foo\"",
        );
        write(
            &tmp.path().join("crates/bar/Cargo.toml"),
            "[package]\nname = \"bar\"",
        );
        // Vendored crate outside the members glob:
        write(
            &tmp.path().join("vendored/baz/Cargo.toml"),
            "[package]\nname = \"baz\"",
        );
        let graph = build_crate_graph(tmp.path());
        assert!(graph.get("foo").map_or(false, |c| c.is_workspace_member));
        assert!(graph.get("bar").map_or(false, |c| c.is_workspace_member));
        assert!(!graph.get("baz").map_or(true, |c| c.is_workspace_member));
    }

    #[test]
    fn handles_malformed_toml_without_aborting() {
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("crates/foo/Cargo.toml"),
            "[package\nname = bad",
        );
        write(
            &tmp.path().join("crates/bar/Cargo.toml"),
            "[package]\nname = \"bar\"",
        );
        let graph = build_crate_graph(tmp.path());
        // foo failed to parse; bar still landed.
        assert!(graph.get("foo").is_none());
        assert!(graph.get("bar").is_some());
    }
}
