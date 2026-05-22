//! Anchor capture: snapshot the source-side branch tip at APPLY
//! entry into an [`Anchor`] + the content-addressed blob storage
//! tree per spec §23 and §25.
//!
//! Slice E5 of the Phase 5 IO plan. Composes the gix object walk
//! shape introduced in E2 (ref-tree reader) but targets a commit
//! OID rather than a ref path: callers pass in the source-side
//! branch tip OID they already resolved (spec §24 step 5), and
//! capture produces the anchor document plus the content-addressed
//! blob bytes.
//!
//! The output is a `BTreeMap<String, Vec<u8>>` shaped exactly like
//! the anchor portion of a round ref tree:
//!
//! - `.anchor.<side>.toml` -> TOML-serialised [`Anchor`]
//! - `.anchor.<side>.blobs/<sha-prefix>/<sha-rest>` -> blob bytes
//!
//! Merging the result into a [`crate::RoundRefTree`] (see slice E3)
//! produces the new round-ref tree fragment seal_manifest commits
//! at APPLY entry.

use std::collections::BTreeMap;

use crate::anchor::{Anchor, BlobSha, FileEntry, BlobShaError};
use crate::io::repo::RepoHandle;
use crate::io::time::current_iso8601;
use crate::phase::ManifestSide;

/// Schema version stamp written into newly-captured anchors.
const ANCHOR_SCHEMA_VERSION: &str = "1.0";

/// Failure modes for [`RepoHandle::capture_anchor`].
#[derive(Debug)]
pub enum AnchorCaptureError {
    /// The given `source_tip` OID does not resolve to a commit. The
    /// caller passed a ref pointing at a tag or a tree, neither of
    /// which carries the per-file blob set we need.
    NotACommit { tip: String },
    /// A tree entry was not a blob or subtree (e.g. a gitlink). Spec
    /// §23 expects the source-side tree to be ordinary blobs +
    /// subtrees only.
    UnexpectedTreeEntry { path: String, kind: &'static str },
    /// gix's blob OID did not parse as a hex SHA after stringifying.
    /// Should not happen on a healthy odb; surfaces as a typed
    /// variant rather than a panic.
    BadBlobSha {
        path: String,
        raw: String,
        error: BlobShaError,
    },
    /// gix returned an error reading objects out of the odb.
    GixOdb { source: Box<dyn std::error::Error + Send + Sync> },
    /// `Anchor::to_toml` failed to serialise the assembled document.
    /// Should not happen on a well-formed Anchor; surfaces as a
    /// typed variant.
    SerialiseFailed { error: toml::ser::Error },
}

impl core::fmt::Display for AnchorCaptureError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotACommit { tip } => {
                write!(f, "source-tip `{tip}` does not resolve to a commit")
            }
            Self::UnexpectedTreeEntry { path, kind } => {
                write!(f, "unexpected tree entry `{path}` of kind `{kind}`")
            }
            Self::BadBlobSha { path, raw, error } => write!(
                f,
                "blob OID `{raw}` at path `{path}` not a valid SHA: {error}"
            ),
            Self::GixOdb { source } => write!(f, "object database read failed: {source}"),
            Self::SerialiseFailed { error } => {
                write!(f, "anchor TOML serialisation failed: {error}")
            }
        }
    }
}

impl std::error::Error for AnchorCaptureError {}

impl RepoHandle {
    /// Snapshot the tree at `source_tip` into an anchor fragment.
    ///
    /// Walks every blob reachable from the commit's tree (recursive
    /// subtree walk; same shape as the ref-tree reader from slice
    /// E2). For each blob: records (path, blob_sha) into the
    /// [`Anchor::files`] vector, and stores the blob bytes
    /// content-addressed under `<sha-prefix>/<sha-rest>` in the
    /// blobs map.
    ///
    /// Returns a map ready to merge into a [`crate::RoundRefTree`]:
    /// keys are `/`-separated paths starting with `.anchor.<side>.`
    /// per spec §25.
    ///
    /// The anchor's `captured_at` is the wall-clock ISO-8601 UTC
    /// timestamp; `captured_from_source_branch_tip` is the
    /// stringified OID for provenance.
    pub fn capture_anchor(
        &self,
        source_tip: gix::ObjectId,
        side: ManifestSide,
    ) -> Result<BTreeMap<String, Vec<u8>>, AnchorCaptureError> {
        let repo = self.repo();
        let object = repo
            .find_object(source_tip)
            .map_err(|e| AnchorCaptureError::GixOdb {
                source: Box::new(e),
            })?;
        let commit = match object.try_into_commit() {
            Ok(c) => c,
            Err(_) => {
                return Err(AnchorCaptureError::NotACommit {
                    tip: source_tip.to_string(),
                });
            }
        };
        let tree = commit.tree().map_err(|e| AnchorCaptureError::GixOdb {
            source: Box::new(e),
        })?;

        // Walk the commit's tree recursively, collecting (path,
        // BlobSha, bytes) entries. Path order from gix's iter is
        // tree-canonical; we re-sort into a BTreeMap at insertion
        // time so the resulting Anchor.files vector and the blob
        // storage map are both deterministic.
        let mut files: BTreeMap<String, BlobSha> = BTreeMap::new();
        let mut blob_bytes: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        walk_source_tree(repo, &tree, String::new(), &mut files, &mut blob_bytes)?;

        let anchor = Anchor {
            mockspace_version: ANCHOR_SCHEMA_VERSION.to_owned(),
            captured_at: current_iso8601(),
            captured_from_source_branch_tip: source_tip.to_string(),
            files: files
                .into_iter()
                .map(|(path, blob_sha)| FileEntry { path, blob_sha })
                .collect(),
        };
        let anchor_toml = anchor
            .to_toml()
            .map_err(|error| AnchorCaptureError::SerialiseFailed { error })?;

        let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let side_marker = side.marker();
        out.insert(
            format!(".anchor.{side_marker}.toml"),
            anchor_toml.into_bytes(),
        );
        let blobs_prefix = format!(".anchor.{side_marker}.blobs");
        for (sha_storage_path, bytes) in blob_bytes {
            out.insert(format!("{blobs_prefix}/{sha_storage_path}"), bytes);
        }
        Ok(out)
    }
}

fn walk_source_tree(
    repo: &gix::Repository,
    tree: &gix::Tree<'_>,
    prefix: String,
    files: &mut BTreeMap<String, BlobSha>,
    blob_bytes: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), AnchorCaptureError> {
    for entry in tree.iter() {
        let entry = entry.map_err(|e| AnchorCaptureError::GixOdb {
            source: Box::new(e),
        })?;
        let name = entry.filename().to_string();
        let child_path = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let mode = entry.mode();
        if mode.is_tree() {
            let subtree_obj = entry.id().object().map_err(|e| AnchorCaptureError::GixOdb {
                source: Box::new(e),
            })?;
            let subtree = match subtree_obj.try_into_tree() {
                Ok(t) => t,
                Err(_) => {
                    return Err(AnchorCaptureError::UnexpectedTreeEntry {
                        path: child_path,
                        kind: "tree-typed-entry-not-a-tree-object",
                    });
                }
            };
            walk_source_tree(repo, &subtree, child_path, files, blob_bytes)?;
        } else if mode.is_blob() {
            let blob_id = entry.id();
            let id_str = blob_id.to_string();
            let blob_sha = BlobSha::parse(&id_str).map_err(|error| {
                AnchorCaptureError::BadBlobSha {
                    path: child_path.clone(),
                    raw: id_str.clone(),
                    error,
                }
            })?;
            let blob = repo
                .find_object(blob_id.detach())
                .map_err(|e| AnchorCaptureError::GixOdb {
                    source: Box::new(e),
                })?;
            // Content-addressed storage: one entry per unique SHA.
            // Multiple files with the same content share the same
            // entry, which is the whole point of content-addressing.
            let storage_path = blob_sha.storage_path();
            blob_bytes
                .entry(storage_path)
                .or_insert_with(|| blob.data.clone());
            files.insert(child_path, blob_sha);
        } else if mode.is_link() {
            return Err(AnchorCaptureError::UnexpectedTreeEntry {
                path: child_path,
                kind: "symlink",
            });
        } else if mode.is_commit() {
            return Err(AnchorCaptureError::UnexpectedTreeEntry {
                path: child_path,
                kind: "gitlink-submodule",
            });
        } else {
            return Err(AnchorCaptureError::UnexpectedTreeEntry {
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
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    fn run(args: &[&str], dir: &Path) -> std::process::Output {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("git should run")
    }

    fn init_with_commit(dir: &Path, files: &[(&str, &str)]) -> gix::ObjectId {
        let out = run(&["init", "--quiet"], dir);
        assert!(out.status.success());
        for (path, content) in files {
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir.join(parent)).unwrap();
                }
            }
            std::fs::write(dir.join(path), content).unwrap();
            let out = run(&["add", path], dir);
            assert!(
                out.status.success(),
                "git add {path} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let out = run(&["commit", "--quiet", "-m", "test"], dir);
        assert!(
            out.status.success(),
            "git commit failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = run(&["rev-parse", "HEAD"], dir);
        let sha_str = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        gix::ObjectId::from_hex(sha_str.as_bytes()).unwrap()
    }

    #[test]
    fn capture_anchor_emits_toml_and_blob_entries() {
        let dir = TempDir::new().unwrap();
        let tip = init_with_commit(
            dir.path(),
            &[
                ("src/lib.rs", "pub fn one() {}\n"),
                ("README.md", "hello\n"),
            ],
        );

        let handle = RepoHandle::open(dir.path()).expect("open");
        let map = handle
            .capture_anchor(tip, ManifestSide::Doc)
            .expect("capture succeeds");

        // Toml entry present, plus one blob storage entry per
        // unique blob (two distinct files -> two blob storage
        // entries).
        assert!(map.contains_key(".anchor.doc.toml"));
        let toml_bytes = map.get(".anchor.doc.toml").unwrap();
        let anchor = Anchor::from_toml(std::str::from_utf8(toml_bytes).unwrap()).unwrap();
        assert_eq!(anchor.mockspace_version, "1.0");
        assert_eq!(anchor.captured_from_source_branch_tip, tip.to_string());
        assert_eq!(anchor.files.len(), 2);

        // Every recorded file's blob_sha must have a matching
        // storage entry under .anchor.doc.blobs/<prefix>/<rest>.
        for entry in &anchor.files {
            let path = format!(".anchor.doc.blobs/{}", entry.blob_sha.storage_path());
            assert!(
                map.contains_key(&path),
                "missing blob storage path {path} for file {}",
                entry.path
            );
        }

        // Total entries: 1 toml + 2 unique blobs.
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn capture_anchor_deduplicates_identical_blobs() {
        // Two files with identical content share one blob object
        // in git, so the content-addressed storage emits only one
        // entry. The anchor's `files` vector still has both paths.
        let dir = TempDir::new().unwrap();
        let tip = init_with_commit(
            dir.path(),
            &[("a.txt", "duplicate-content\n"), ("b.txt", "duplicate-content\n")],
        );

        let handle = RepoHandle::open(dir.path()).expect("open");
        let map = handle
            .capture_anchor(tip, ManifestSide::Doc)
            .expect("capture");
        let anchor =
            Anchor::from_toml(std::str::from_utf8(map.get(".anchor.doc.toml").unwrap()).unwrap())
                .unwrap();
        assert_eq!(anchor.files.len(), 2);
        assert_eq!(anchor.files[0].blob_sha, anchor.files[1].blob_sha);
        // Map has 1 toml + 1 deduplicated blob = 2 entries.
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn capture_anchor_side_marker_picks_doc_vs_src() {
        let dir = TempDir::new().unwrap();
        let tip = init_with_commit(dir.path(), &[("file.txt", "x\n")]);
        let handle = RepoHandle::open(dir.path()).expect("open");

        let doc_map = handle.capture_anchor(tip, ManifestSide::Doc).expect("doc");
        assert!(doc_map.contains_key(".anchor.doc.toml"));
        assert!(!doc_map.contains_key(".anchor.src.toml"));

        let src_map = handle.capture_anchor(tip, ManifestSide::Src).expect("src");
        assert!(src_map.contains_key(".anchor.src.toml"));
        assert!(!src_map.contains_key(".anchor.doc.toml"));
    }

    #[test]
    fn capture_anchor_on_empty_tree_emits_only_the_toml_entry() {
        // Commit with zero files. The walker's recursion terminates
        // on the empty iter; the resulting Anchor has files=[] and
        // the map carries only the .anchor.<side>.toml entry.
        let dir = TempDir::new().unwrap();
        let out = run(&["init", "--quiet"], dir.path());
        assert!(out.status.success());
        // git refuses to commit with no changes; create an empty
        // tree object directly via plumbing.
        let tree_oid_out = run(&["write-tree"], dir.path());
        assert!(tree_oid_out.status.success());
        let tree_oid = String::from_utf8(tree_oid_out.stdout).unwrap().trim().to_owned();
        let commit_oid_out = run(
            &["commit-tree", &tree_oid, "-m", "empty"],
            dir.path(),
        );
        assert!(commit_oid_out.status.success());
        let commit_oid = String::from_utf8(commit_oid_out.stdout).unwrap().trim().to_owned();

        let tip = gix::ObjectId::from_hex(commit_oid.as_bytes()).unwrap();
        let handle = RepoHandle::open(dir.path()).expect("open");
        let map = handle
            .capture_anchor(tip, ManifestSide::Doc)
            .expect("capture");
        assert_eq!(map.len(), 1);
        let anchor =
            Anchor::from_toml(std::str::from_utf8(map.get(".anchor.doc.toml").unwrap()).unwrap())
                .unwrap();
        assert!(anchor.files.is_empty());
    }

    #[test]
    fn capture_anchor_walks_subtrees() {
        // Nested directory should show up in the anchor's files
        // vector with `/`-separated path. Tests the recursive walk.
        let dir = TempDir::new().unwrap();
        let tip = init_with_commit(
            dir.path(),
            &[
                ("top.txt", "top\n"),
                ("nested/dir/inner.txt", "inner\n"),
            ],
        );
        let handle = RepoHandle::open(dir.path()).expect("open");
        let map = handle.capture_anchor(tip, ManifestSide::Doc).expect("capture");
        let anchor =
            Anchor::from_toml(std::str::from_utf8(map.get(".anchor.doc.toml").unwrap()).unwrap())
                .unwrap();
        let paths: Vec<&str> = anchor.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"top.txt"));
        assert!(paths.contains(&"nested/dir/inner.txt"));
    }
}
