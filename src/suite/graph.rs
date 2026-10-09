//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The workspace's own packages and which of them each one depends on.
//!
//! Read from `cargo metadata`, resolving, for one reason beyond the graph: a
//! resolve writes `Cargo.lock` where it is missing or stale, exactly as the
//! build about to run would. Taken before the fingerprints, that keeps the
//! lockfile they hash the one the tests are built against; read with
//! `--no-deps`, a first run in a fresh tree recorded its passes against a
//! lockfile that did not exist yet, and the next run found every member moved.
//!
//! Only path dependencies onto other members are edges here; a registry
//! crate's sources are pinned by `Cargo.lock`, which every fingerprint already
//! covers whole.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// How a package names a dependency, which decides whether that dependency is
/// compiled into another package's tests at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DepKind {
    Normal,
    Build,
    /// Compiled into this package's own tests and nothing else's.
    Dev,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Package {
    pub name: String,
    /// The directory holding its manifest.
    pub dir:  PathBuf,
    /// Its dependencies that are members of this workspace.
    pub deps: Vec<(String, DepKind)>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graph {
    pub packages:        BTreeMap<String, Package>,
    /// What a bare `cargo test` in the workspace root reaches.
    pub default_members: Vec<String>,
}

impl Graph {
    /// Asks cargo.
    pub fn read(mock_dir: &Path) -> Result<Graph, String> {
        let out = crate::entry::cargo_gate::cargo(mock_dir, &["metadata", "--format-version", "1"])
            .output()
            .map_err(|e| format!("could not run cargo metadata: {e}"))?;
        if !out.status.success() {
            return Err(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
        }
        Graph::from_metadata(&String::from_utf8_lossy(&out.stdout))
    }

    /// The graph out of `cargo metadata --format-version 1`. Every package
    /// that is not a workspace member is passed over.
    pub fn from_metadata(json: &str) -> Result<Graph, String> {
        let v: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("cargo metadata is not json: {e}"))?;
        let empty = Vec::new();
        let packages = v["packages"].as_array().unwrap_or(&empty);
        let members: BTreeSet<&str> = v["workspace_members"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|m| m.as_str())
            .collect();

        // A member is matched to a dependency by directory rather than by name,
        // since a dependency may be renamed and a registry crate may share a
        // member's name.
        let mut by_dir: BTreeMap<PathBuf, String> = BTreeMap::new();
        let mut id_to_name: BTreeMap<&str, String> = BTreeMap::new();
        for p in packages {
            let (Some(id), Some(name), Some(manifest)) = (
                p["id"].as_str(),
                p["name"].as_str(),
                p["manifest_path"].as_str(),
            ) else {
                continue;
            };
            if !members.contains(id) {
                continue;
            }
            let dir = Path::new(manifest)
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            by_dir.insert(dir, name.to_string());
            id_to_name.insert(id, name.to_string());
        }

        let mut graph = Graph::default();
        for p in packages {
            let Some(name) = p["id"].as_str().and_then(|id| id_to_name.get(id)) else {
                continue;
            };
            let dir = Path::new(p["manifest_path"].as_str().unwrap_or_default())
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let mut deps: Vec<(String, DepKind)> = p["dependencies"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter_map(|d| {
                    let member = by_dir.get(Path::new(d["path"].as_str()?))?;
                    let kind = match d["kind"].as_str() {
                        Some("dev") => DepKind::Dev,
                        Some("build") => DepKind::Build,
                        _ => DepKind::Normal,
                    };
                    Some((member.clone(), kind))
                })
                .collect();
            deps.sort();
            deps.dedup();
            graph.packages.insert(name.clone(), Package {
                name: name.clone(),
                dir,
                deps,
            });
        }

        // Older cargo omits the default members; the workspace members are
        // then what a bare `cargo test` reaches, which is cargo's own fallback.
        graph.default_members = match v["workspace_default_members"].as_array() {
            Some(ids) => {
                ids.iter()
                    .filter_map(|m| id_to_name.get(m.as_str()?).cloned())
                    .collect()
            },
            None => graph.packages.keys().cloned().collect(),
        };
        graph.default_members.sort();
        Ok(graph)
    }

    /// Every member whose sources are compiled into `name`'s tests, `name`
    /// itself excluded.
    ///
    /// Its own dependencies of every kind, since its tests compile its dev and
    /// build dependencies too; and from those, only what they need to build
    /// themselves, since a dependency's own dev-dependencies are compiled into
    /// that dependency's tests and never into anyone else's. Cycles, which
    /// dev-dependencies allow, end at a package already seen.
    pub fn test_closure(&self, name: &str) -> BTreeSet<String> {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut stack: Vec<String> = match self.packages.get(name) {
            Some(p) => p.deps.iter().map(|(d, _)| d.clone()).collect(),
            None => return seen,
        };
        while let Some(next) = stack.pop() {
            if next == name || !seen.insert(next.clone()) {
                continue;
            }
            if let Some(p) = self.packages.get(&next) {
                stack.extend(
                    p.deps
                        .iter()
                        .filter(|(_, k)| *k != DepKind::Dev)
                        .map(|(d, _)| d.clone()),
                );
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `cargo metadata` prints, cut to the keys read here, with the
    /// registry package `serde` among the packages as a resolve lists it.
    /// `c` dev-depends on `a`, which normally depends on `b`; `b` dev-depends
    /// on `c`, a cycle cargo accepts; `serde` is a registry dependency and
    /// `d` a member nothing depends on.
    fn fixture() -> String {
        let pkg = |id: &str, deps: &str| {
            format!(
                r#"{{"id":"{id}","name":"{id}","manifest_path":"/w/{id}/Cargo.toml","dependencies":[{deps}]}}"#
            )
        };
        let dep = |name: &str, kind: &str| {
            let kind = if kind.is_empty() { "null".to_string() } else { format!("\"{kind}\"") };
            format!(r#"{{"name":"{name}","kind":{kind},"path":"/w/{name}"}}"#)
        };
        let registry = r#"{"name":"serde","kind":null}"#;
        format!(
            r#"{{"packages":[{},{},{},{},{}],"workspace_members":["a","b","c","d"],"workspace_default_members":["a","b","c"]}}"#,
            pkg("a", &format!("{},{registry}", dep("b", ""))),
            pkg("b", &dep("c", "dev")),
            pkg("c", &dep("a", "dev")),
            pkg("d", ""),
            r#"{"id":"serde","name":"serde","manifest_path":"/registry/serde/Cargo.toml","dependencies":[]}"#,
        )
    }

    #[test]
    fn only_path_dependencies_on_members_are_edges() {
        let g = Graph::from_metadata(&fixture()).unwrap();
        assert_eq!(g.packages["a"].deps, vec![(
            "b".to_string(),
            DepKind::Normal
        )]);
        assert_eq!(g.packages["b"].deps, vec![("c".to_string(), DepKind::Dev)]);
        assert_eq!(g.packages["d"].deps, vec![]);
        assert_eq!(g.default_members, vec!["a", "b", "c"]);
        assert!(
            !g.packages.contains_key("serde"),
            "a registry package is not a member"
        );
    }

    /// `c`'s tests compile `a` (its dev-dependency) and `b` (what `a` needs),
    /// and not `b`'s own dev-dependency, which would be `c` itself anyway.
    #[test]
    fn a_closure_takes_own_dev_deps_and_only_the_build_graph_beyond() {
        let g = Graph::from_metadata(&fixture()).unwrap();
        let c: Vec<_> = g.test_closure("c").into_iter().collect();
        assert_eq!(c, vec!["a", "b"]);
        // `a`'s tests reach `b`, and stop: `b`'s dev-dependency on `c` is
        // compiled into `b`'s tests only.
        let a: Vec<_> = g.test_closure("a").into_iter().collect();
        assert_eq!(a, vec!["b"]);
        // `b`'s tests compile `c`, whose dev-dependency `a` is not compiled
        // into them. Following every kind of edge here would add `a`.
        let b: Vec<_> = g.test_closure("b").into_iter().collect();
        assert_eq!(b, vec!["c"]);
        assert!(g.test_closure("d").is_empty());
        assert!(g.test_closure("not-a-member").is_empty());
    }

    #[test]
    fn a_cargo_without_default_members_falls_back_to_every_member() {
        let json = fixture().replace(r#","workspace_default_members":["a","b","c"]"#, "");
        let g = Graph::from_metadata(&json).unwrap();
        assert_eq!(g.default_members, vec!["a", "b", "c", "d"]);
    }
}
