//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the registry reads of the project it sits in.
//!
//! The registry asks for a handful of facts: where the repository and its mock directory are,
//! which namespaces and stores are declared, which roots a citation may name. [`RegistryEnv`] is
//! that question. The mockspace engine answers it from its own config, and a tool that keeps a
//! registry without the engine answers it with an [`Env`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{RefDecl, RegistryNamespace};

/// The project a registry sits in, as far as the registry reads it.
pub trait RegistryEnv {
    /// The root of the repository.
    fn repo_root(&self) -> &Path;
    /// The mock directory, which holds `registry/`.
    fn mock_dir(&self) -> &Path;
    /// Where generated documents are written, which a citation of one is read against.
    fn docs_dir(&self) -> &Path;
    /// The declared namespaces, the built-in ones among them.
    fn namespaces(&self) -> &[RegistryNamespace];
    /// The `[[registry.store]]` declarations: directories of the registry kept on a git ref.
    fn stores(&self) -> &[RefDecl];
    /// Named roots a citation may use, each a path from the repository root.
    fn roots(&self) -> &BTreeMap<String, String>;
    /// Roots whose files are settled, so a citation by line into them holds.
    fn frozen_roots(&self) -> &BTreeSet<String>;
    /// Roots whose citations render as prose, with the name each goes by.
    fn prose_roots(&self) -> &BTreeMap<String, String>;
    /// Roots whose citations are dropped from generated documents.
    fn internal_roots(&self) -> &BTreeSet<String>;
    /// The stores of this project's registry: its directory, with each `[[registry.store]]`
    /// directory mounted on its ref.
    fn registry_stores(&self) -> Result<crate::RegistryStores, crate::RegistryStoreError> {
        crate::RegistryStores::from_decls(self.mock_dir(), self.repo_root(), self.stores())
    }
    /// The link a `crates::<name>` reference renders as, read from `docs_dir`, where the project
    /// has a document for that crate. A project with no crates has none.
    fn crate_link(&self, _name: &str, _docs_dir: &Path) -> Option<String> {
        None
    }
    /// The directory name a `crates::<name>::..` citation reads files under, where `name` is a
    /// crate of the project.
    fn crate_dir(&self, _name: &str) -> Option<PathBuf> {
        None
    }
}

/// A [`RegistryEnv`] held as plain values, for a tool that keeps a registry without the engine.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub repo_root:      PathBuf,
    pub mock_dir:       PathBuf,
    pub docs_dir:       PathBuf,
    pub namespaces:     Vec<RegistryNamespace>,
    pub stores:         Vec<RefDecl>,
    pub roots:          BTreeMap<String, String>,
    pub frozen_roots:   BTreeSet<String>,
    pub prose_roots:    BTreeMap<String, String>,
    pub internal_roots: BTreeSet<String>,
}

impl RegistryEnv for Env {
    fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    fn mock_dir(&self) -> &Path {
        &self.mock_dir
    }

    fn docs_dir(&self) -> &Path {
        &self.docs_dir
    }

    fn namespaces(&self) -> &[RegistryNamespace] {
        &self.namespaces
    }

    fn stores(&self) -> &[RefDecl] {
        &self.stores
    }

    fn roots(&self) -> &BTreeMap<String, String> {
        &self.roots
    }

    fn frozen_roots(&self) -> &BTreeSet<String> {
        &self.frozen_roots
    }

    fn prose_roots(&self) -> &BTreeMap<String, String> {
        &self.prose_roots
    }

    fn internal_roots(&self) -> &BTreeSet<String> {
        &self.internal_roots
    }
}

/// Where saved verdicts of the schema check live, so a run over the same inputs replays its
/// verdict. A tool with nowhere to keep them passes none.
pub trait VerdictCache {
    /// A fixed-width digest of `parts`, stable across runs and machines for the same code. The
    /// implementation frames the parts, so two splittings of the same bytes give two digests.
    fn digest(&self, parts: &[&[u8]]) -> String;

    /// The encoded findings saved under `key`, if any.
    fn replay(&self, key: &str) -> Option<String>;

    /// Save `encoded` under `key`. A store that cannot write says nothing: a verdict not saved
    /// is a verdict recomputed next run, never a wrong one.
    fn remember(&self, key: &str, encoded: &str);

    /// Told of each lookup, for a cache that counts its hits. Counting nothing is the default.
    fn tally(&self, _hit: bool, _what: &str) {}
}

/// `<root>/target`, where what the registry generates and its scratch copies are kept.
pub(crate) fn target_dir(root: &Path) -> PathBuf {
    root.join("target")
}

/// Where the generated schemas of the registry of the repository at `repo_root` are kept.
pub fn schemas_dir(repo_root: &Path) -> PathBuf {
    target_dir(repo_root).join("mockspace").join("registry-schemas")
}

/// One `[ref.roots.<name>]` table: a named root a citation may use.
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "test-support"), derive(serde::Serialize))]
#[serde(default)]
pub struct RefRoot {
    /// The root's directory, from the repository root.
    pub path:     String,
    /// Whether this root's contents are settled and will not shift.
    ///
    /// Line citations are only honest into a frozen root. Everywhere else an edit above a cited
    /// line silently repoints it, which is the failure this project treats as worst: the check
    /// passes and the answer is wrong. Declaring a root frozen is a claim that its files do not
    /// move, and it is what turns a line citation from a hazard into a fact.
    pub frozen:   bool,
    /// Whether a citation into this root renders as a link. Absent means it does.
    #[serde(default = "links_by_default")]
    pub links:    bool,
    /// The name a root that does not link goes by in prose. Absent means the root's own name.
    pub label:    Option<String>,
    /// Whether citations into this root are dropped from generated documents.
    pub internal: bool,
}

fn links_by_default() -> bool {
    true
}

/// The `[registry]` table: the namespaces, and the directories of the registry kept on a git ref.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[cfg_attr(any(test, feature = "test-support"), derive(serde::Serialize))]
#[serde(default)]
pub struct RegistryDecl {
    pub namespace: Vec<RegistryNamespace>,
    pub store:     Vec<RefDecl>,
}

/// What of a `mockspace.toml` the registry reads; every other key is the engine's and is passed
/// over.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Declared {
    registry: RegistryDecl,
    #[serde(rename = "ref")]
    refs:     DeclaredRefs,
}

#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct DeclaredRefs {
    roots: BTreeMap<String, RefRoot>,
}

impl Env {
    /// The environment of the declarations given: the built-in namespaces and roots first, then
    /// the project's own, so a project may repoint a built-in root and never has to declare one.
    pub fn from_parts(
        repo_root: PathBuf,
        mock_dir: PathBuf,
        docs_dir: PathBuf,
        registry: RegistryDecl,
        ref_roots: &BTreeMap<String, RefRoot>,
    ) -> Self {
        let mock_rel = mock_dir.strip_prefix(&repo_root).unwrap_or(&mock_dir).to_string_lossy().replace('\\', "/");
        let mut roots = crate::builtin_roots(&mock_rel);
        roots.extend(ref_roots.iter().map(|(name, root)| (name.clone(), root.path.clone())));
        let named = |keep: fn(&RefRoot) -> bool| ref_roots.iter().filter(move |(_, v)| keep(v)).map(|(k, _)| k.clone());
        Self {
            namespaces: crate::with_builtins(&registry.namespace),
            stores: registry.store,
            roots,
            frozen_roots: named(|v| v.frozen).collect(),
            prose_roots: ref_roots
                .iter()
                .filter(|(_, v)| !v.links)
                .map(|(k, v)| (k.clone(), v.label.clone().unwrap_or_else(|| k.clone())))
                .collect(),
            internal_roots: named(|v| v.internal).collect(),
            repo_root,
            mock_dir,
            docs_dir,
        }
    }

    /// The environment of the project whose mock directory is `mock_dir`, read from its
    /// `mockspace.toml`: the one in the mock directory, else the one at the repository root. The
    /// repository root is the nearest directory above that holds `.git`, else the nearest that
    /// holds a `mockspace.toml`, else the mock directory. No config file is a project that
    /// declares nothing; one that does not parse is an error, since reading it as empty would
    /// pass every check over a registry that was never opened.
    pub fn from_dir(mock_dir: &Path) -> Result<Self, String> {
        let above = |marker: &str| mock_dir.ancestors().find(|d| d.join(marker).exists()).map(Path::to_path_buf);
        let repo_root = above(".git").or_else(|| above("mockspace.toml")).unwrap_or_else(|| mock_dir.to_path_buf());
        let in_mock = mock_dir.join("mockspace.toml");
        let path = if in_mock.is_file() { in_mock } else { repo_root.join("mockspace.toml") };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let declared: Declared = toml_edit::de::from_str(&text)
            .map_err(|e| format!("{} does not parse, so nothing in it was read: {e}", path.display()))?;
        let docs_dir = repo_root.join("docs");
        Ok(Self::from_parts(repo_root, mock_dir.to_path_buf(), docs_dir, declared.registry, &declared.refs.roots))
    }
}
