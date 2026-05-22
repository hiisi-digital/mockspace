//! In-memory snapshot of an orphan ref's tree, plus the reader that
//! produces one.
//!
//! Slice E2 of the Phase 5 IO plan. The snapshot is path-to-bytes
//! map: keys are `/`-separated paths (e.g. `manifest.doc.toml`,
//! `.phase`, `.anchor.doc.blobs/ab/cdef0123`), values are the raw
//! blob bytes. Submodule and non-blob tree entries are not expected
//! on the flat orphan-ref shape from spec §25, so the reader errors
//! out if it encounters them rather than silently dropping.
//!
//! The map is `BTreeMap` for deterministic iteration order; tests
//! and downstream slices both depend on the lexicographic walk for
//! their own determinism.

use std::collections::BTreeMap;

use crate::io::repo::RepoHandle;
use crate::ref_path::DefaultRefPath;

/// Failure modes for [`RepoHandle::read_ref_tree`].
#[derive(Debug)]
pub enum RefTreeReadError {
    /// The named ref does not exist in the repository.
    RefNotFound { ref_path: String },
    /// The ref exists but does not resolve to a commit (e.g. a tag
    /// or a tree without a commit wrapper). Orphan mock refs should
    /// always point at a commit per spec §25; encountering anything
    /// else is a data-integrity issue worth surfacing loudly.
    NotACommit { ref_path: String },
    /// The walker hit a tree entry that is neither a blob nor a
    /// subtree (e.g. a gitlink/submodule). Spec §25 expects the
    /// orphan ref tree to be flat blobs plus the anchor-blobs
    /// subtree only.
    UnexpectedTreeEntry { path: String, kind: &'static str },
    /// gix returned an error reading objects out of the odb.
    GixOdb { source: Box<dyn std::error::Error + Send + Sync> },
}

impl core::fmt::Display for RefTreeReadError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RefNotFound { ref_path } => {
                write!(f, "ref `{ref_path}` not found")
            }
            Self::NotACommit { ref_path } => {
                write!(f, "ref `{ref_path}` does not resolve to a commit")
            }
            Self::UnexpectedTreeEntry { path, kind } => {
                write!(f, "unexpected tree entry `{path}` of kind `{kind}`")
            }
            Self::GixOdb { source } => write!(f, "object database read failed: {source}"),
        }
    }
}

impl std::error::Error for RefTreeReadError {}

/// In-memory snapshot of an orphan ref's tree.
///
/// Paths use `/` separators per git tree convention. The snapshot
/// stores entries in a `BTreeMap`, so the public [`Self::iter`] yields
/// them in lexicographic path order regardless of the order the gix
/// walker discovered them. Callers that need a stable walk for
/// hashing, diffing, or rebuilding tree objects can rely on this.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoundRefTree {
    entries: BTreeMap<String, Vec<u8>>,
}

impl RoundRefTree {
    /// Construct from a raw entry map. Crate-visible so other IO
    /// slices (e.g. `seal_manifest`) can compose snapshots without
    /// going through gix on the read path. Not exposed to downstream
    /// consumers, who use the reader. If a downstream test fixture
    /// ever needs this, promote with a typed builder rather than
    /// re-publishing the raw map.
    pub(crate) fn from_entries_pub(entries: BTreeMap<String, Vec<u8>>) -> Self {
        Self { entries }
    }

    /// Alias retained for the existing test-only call sites that
    /// constructed `RoundRefTree` via `from_entries`.
    #[cfg(test)]
    pub(crate) fn from_entries(entries: BTreeMap<String, Vec<u8>>) -> Self {
        Self::from_entries_pub(entries)
    }

    /// Lookup the bytes at `path`. Returns None when the path is
    /// absent from the tree.
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.entries.get(path).map(Vec::as_slice)
    }

    /// Iterate over all (path, bytes) entries in deterministic
    /// lexicographic path order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Number of entries in the tree.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when the tree has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl RepoHandle {
    /// Resolve a ref to its current commit OID. Returns
    /// [`RefTreeReadError::RefNotFound`] when the ref does not
    /// exist. Useful as the "expected current" input for a CAS
    /// update via [`RepoHandle::write_round_ref`]: a caller reads
    /// the tree (via [`read_ref_tree`](Self::read_ref_tree)) AND
    /// the OID, mutates the tree in memory, and writes back with
    /// the OID as the CAS expectation.
    pub fn resolve_ref_oid(
        &self,
        ref_path: &DefaultRefPath,
    ) -> Result<gix::ObjectId, RefTreeReadError> {
        use gix::reference::find::existing::Error as ExistingErr;
        let repo = self.repo();
        let mut reference = match repo.find_reference(ref_path.as_str()) {
            Ok(r) => r,
            Err(ExistingErr::NotFound { .. }) => {
                return Err(RefTreeReadError::RefNotFound {
                    ref_path: ref_path.as_str().to_owned(),
                });
            }
            Err(e) => {
                return Err(RefTreeReadError::GixOdb {
                    source: Box::new(e),
                });
            }
        };
        let id = reference
            .peel_to_id_in_place()
            .map_err(|e| RefTreeReadError::GixOdb {
                source: Box::new(e),
            })?;
        Ok(id.detach())
    }

    /// Read the orphan ref's tree into an in-memory snapshot.
    ///
    /// The ref is expected to point at a commit whose tree carries
    /// the round's flat blob set (per spec §25). Recursive subtrees
    /// (e.g. `.anchor.<phase>.blobs/<sha-prefix>/`) are walked
    /// transparently; subtree paths use `/` separators in the
    /// returned map.
    pub fn read_ref_tree(&self, ref_path: &DefaultRefPath) -> Result<RoundRefTree, RefTreeReadError> {
        use gix::reference::find::existing::Error as ExistingErr;
        let repo = self.repo();
        let mut reference = match repo.find_reference(ref_path.as_str()) {
            Ok(r) => r,
            Err(ExistingErr::NotFound { .. }) => {
                return Err(RefTreeReadError::RefNotFound {
                    ref_path: ref_path.as_str().to_owned(),
                });
            }
            Err(e) => {
                return Err(RefTreeReadError::GixOdb {
                    source: Box::new(e),
                });
            }
        };
        let id = reference
            .peel_to_id_in_place()
            .map_err(|e| RefTreeReadError::GixOdb {
                source: Box::new(e),
            })?;
        let object = id.object().map_err(|e| RefTreeReadError::GixOdb {
            source: Box::new(e),
        })?;
        let commit = match object.try_into_commit() {
            Ok(c) => c,
            Err(_) => {
                return Err(RefTreeReadError::NotACommit {
                    ref_path: ref_path.as_str().to_owned(),
                });
            }
        };
        let tree = commit.tree().map_err(|e| RefTreeReadError::GixOdb {
            source: Box::new(e),
        })?;

        let mut entries = BTreeMap::new();
        walk_tree(repo, &tree, String::new(), &mut entries)?;
        Ok(RoundRefTree { entries })
    }
}

fn walk_tree(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: String,
    out: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), RefTreeReadError> {
    for entry in tree.iter() {
        let entry = entry.map_err(|e| RefTreeReadError::GixOdb {
            source: Box::new(e),
        })?;
        let name_bstr = entry.filename();
        let name = name_bstr.to_string();
        let child_path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let mode = entry.mode();
        if mode.is_tree() {
            let subtree_id = entry.id();
            let subtree_obj = subtree_id.object().map_err(|e| RefTreeReadError::GixOdb {
                source: Box::new(e),
            })?;
            let subtree = match subtree_obj.try_into_tree() {
                Ok(t) => t,
                Err(_) => {
                    return Err(RefTreeReadError::UnexpectedTreeEntry {
                        path: child_path,
                        kind: "tree-typed-entry-not-a-tree-object",
                    });
                }
            };
            walk_tree(repo, &subtree, child_path, out)?;
        } else if mode.is_blob() {
            let blob_id = entry.id();
            let blob = repo
                .find_object(blob_id.detach())
                .map_err(|e| RefTreeReadError::GixOdb {
                    source: Box::new(e),
                })?;
            out.insert(child_path, blob.data.clone());
        } else if mode.is_link() {
            // Symlinks in v2 orphan refs are unexpected; spec §25 lists
            // only blobs + the anchor-blobs subtree. Surface loudly.
            return Err(RefTreeReadError::UnexpectedTreeEntry {
                path: child_path,
                kind: "symlink",
            });
        } else if mode.is_commit() {
            // Gitlink / submodule. Unexpected on orphan refs.
            return Err(RefTreeReadError::UnexpectedTreeEntry {
                path: child_path,
                kind: "gitlink-submodule",
            });
        } else {
            return Err(RefTreeReadError::UnexpectedTreeEntry {
                path: child_path,
                kind: "unknown-tree-mode",
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slug::DefaultSlug;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run(args: &[&str], dir: &Path) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git {args:?} failed; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_out(args: &[&str], dir: &Path) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("git should run");
        assert!(
            out.status.success(),
            "git {:?} failed; stderr: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("git output is UTF-8")
    }

    /// Author an orphan ref with the given (path, content) entries
    /// using git plumbing. Returns the commit OID as a string. Uses
    /// the system `git` binary so we exercise the same tree shape a
    /// real mockspace run would produce; this keeps the test
    /// agnostic to how mockspace itself writes refs (which is slice
    /// E3 territory).
    fn author_orphan_ref(dir: &Path, ref_name: &str, entries: &[(&str, &str)]) -> String {
        // Write each blob via hash-object; assemble paths into a
        // tree via update-index; commit-tree with no parent.
        run(&["init", "--quiet"], dir);
        for (path, content) in entries {
            // Ensure parent dirs exist for files like `a/b/c`.
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir.join(parent)).unwrap();
                }
            }
            std::fs::write(dir.join(path), content).unwrap();
            run(&["add", path], dir);
        }
        let tree_oid = run_out(&["write-tree"], dir).trim().to_owned();
        let commit_oid = run_out(
            &["commit-tree", &tree_oid, "-m", "orphan ref test fixture"],
            dir,
        )
        .trim()
        .to_owned();
        run(&["update-ref", ref_name, &commit_oid], dir);
        // Each test owns its own TempDir, so cross-test leakage is
        // structurally impossible; no working-tree cleanup needed.
        commit_oid
    }

    #[test]
    fn read_ref_tree_round_trips_a_flat_orphan_ref() {
        let dir = TempDir::new().unwrap();
        let slug = DefaultSlug::new("test-round").unwrap();
        let ref_path = DefaultRefPath::round_mock(&slug);
        author_orphan_ref(
            dir.path(),
            ref_path.as_str(),
            &[
                (".phase", "PLAN.DOC\n"),
                ("manifest.doc.toml", "round_slug = \"test-round\"\n"),
                ("round.toml", "slug = \"test-round\"\n"),
            ],
        );

        let handle = RepoHandle::open(dir.path()).expect("open repo");
        let tree = handle.read_ref_tree(&ref_path).expect("read ref tree");
        assert_eq!(tree.len(), 3);
        assert_eq!(tree.get(".phase").unwrap(), b"PLAN.DOC\n");
        assert_eq!(
            tree.get("manifest.doc.toml").unwrap(),
            b"round_slug = \"test-round\"\n"
        );
        assert_eq!(tree.get("round.toml").unwrap(), b"slug = \"test-round\"\n");
        assert!(tree.get("missing.toml").is_none());
    }

    #[test]
    fn read_ref_tree_walks_subtrees_for_anchor_blobs() {
        let dir = TempDir::new().unwrap();
        let slug = DefaultSlug::new("anchor-round").unwrap();
        let ref_path = DefaultRefPath::round_mock(&slug);
        // Mimic spec §25 anchor blob path: .anchor.doc.blobs/<2-hex>/<rest>.
        author_orphan_ref(
            dir.path(),
            ref_path.as_str(),
            &[
                (".phase", "APPLY.DOC\n"),
                (".anchor.doc.toml", "format = \"v1\"\n"),
                (".anchor.doc.blobs/ab/cdef0123", "blob-bytes"),
                (".anchor.doc.blobs/ab/feed4567", "more-blob-bytes"),
            ],
        );

        let handle = RepoHandle::open(dir.path()).expect("open repo");
        let tree = handle.read_ref_tree(&ref_path).expect("read ref tree");
        // Subtree paths land as `/`-separated keys.
        assert_eq!(tree.len(), 4);
        assert_eq!(
            tree.get(".anchor.doc.blobs/ab/cdef0123").unwrap(),
            b"blob-bytes"
        );
        assert_eq!(
            tree.get(".anchor.doc.blobs/ab/feed4567").unwrap(),
            b"more-blob-bytes"
        );
    }

    #[test]
    fn read_ref_tree_iter_yields_lex_order() {
        let dir = TempDir::new().unwrap();
        let slug = DefaultSlug::new("order-round").unwrap();
        let ref_path = DefaultRefPath::round_mock(&slug);
        author_orphan_ref(
            dir.path(),
            ref_path.as_str(),
            &[("z.toml", "z"), ("a.toml", "a"), ("m.toml", "m")],
        );

        let handle = RepoHandle::open(dir.path()).expect("open repo");
        let tree = handle.read_ref_tree(&ref_path).expect("read ref tree");
        let paths: Vec<&str> = tree.iter().map(|(k, _)| k).collect();
        assert_eq!(paths, vec!["a.toml", "m.toml", "z.toml"]);
    }

    #[test]
    fn read_ref_tree_errors_on_missing_ref() {
        let dir = TempDir::new().unwrap();
        run(&["init", "--quiet"], dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open repo");
        let slug = DefaultSlug::new("not-there").unwrap();
        let err = handle
            .read_ref_tree(&DefaultRefPath::round_mock(&slug))
            .unwrap_err();
        assert!(matches!(err, RefTreeReadError::RefNotFound { .. }), "got {err:?}");
    }
}
