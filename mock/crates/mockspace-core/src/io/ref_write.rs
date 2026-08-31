//! Ref-tree writer: build a new orphan-ref commit from a
//! [`RoundRefTree`] snapshot and atomically advance a ref to it.
//!
//! Slice E3 of the Phase 5 IO plan. Pairs with the reader in
//! [`crate::io::ref_tree`]: read returns a `RoundRefTree`, the
//! consumer mutates it in memory, write commits the result.
//!
//! Orphan-ref shape: each commit's `parents` list is empty (no
//! history linkage between rounds). The CAS contract lives on the
//! ref-edit step: callers pass the previously-observed commit OID
//! to require that the ref still points at it. The first creation
//! of a ref passes `None` and the edit requires the ref not exist
//! yet.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use gix::objs::Tree;
use gix::objs::tree::EntryKind;
use gix::refs::Target;
use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};

use crate::io::ref_tree::RoundRefTree;
use crate::io::repo::RepoHandle;
use crate::ref_path::RefPath;

/// Failure modes for [`RepoHandle::write_round_ref`].
#[derive(Debug)]
pub enum RefTreeWriteError {
    /// CAS failure: caller passed `Some(expected)` but the ref
    /// currently points at a different OID (or does not exist).
    /// The full set of `RefEdit` rejections from gix surfaces here
    /// rather than just the headline OID mismatch.
    NonFastForward {
        ref_path: String,
        expected: Option<gix::ObjectId>,
    },
    /// Path in the tree contained an empty component (e.g. leading
    /// or doubled `/`). Git trees do not allow these.
    EmptyPathComponent {
        path: String,
    },
    /// gix returned an error writing objects to the odb or editing
    /// the ref.
    GixOdb {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl core::fmt::Display for RefTreeWriteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NonFastForward {
                ref_path,
                expected,
            } => {
                write!(f, "ref `{ref_path}` CAS failed; expected {expected:?}")
            },
            Self::EmptyPathComponent {
                path,
            } => {
                write!(f, "tree path `{path}` has an empty component")
            },
            Self::GixOdb {
                source,
            } => {
                write!(f, "object database write failed: {source}")
            },
        }
    }
}

impl std::error::Error for RefTreeWriteError {}

impl RepoHandle {
    /// Write `new_tree` as a new commit on `ref_path`.
    ///
    /// The commit is orphan: empty parents list, no history linkage
    /// to whatever the ref previously pointed at. This matches spec
    /// §19's orphan-flat ref shape.
    ///
    /// `expected_current` carries the CAS expectation:
    ///
    /// - `None`: the ref must not exist; the call creates it.
    /// - `Some(oid)`: the ref must currently point at `oid`. If it
    ///   points at any other OID (or has been deleted), the call
    ///   fails with [`RefTreeWriteError::NonFastForward`].
    ///
    /// Returns the new commit's OID on success.
    ///
    /// Author and committer are synthesised as
    /// `mockspace <noreply@mockspace.local>` with the wall-clock
    /// time at write. The slice plan flags this as an open question
    /// (do we want user.name/user.email when set?); for now the
    /// synthetic identity is uniform across runs and keeps round
    /// authoring decoupled from local git config.
    pub fn write_round_ref(
        &self,
        ref_path: &RefPath,
        new_tree: &RoundRefTree,
        message: &str,
        expected_current: Option<gix::ObjectId>,
    ) -> Result<gix::ObjectId, RefTreeWriteError> {
        let repo = self.repo();

        // 1. Write each blob into the odb and collect (path,
        //    blob_oid) pairs.
        //
        // If a later step (tree write, commit write, ref edit)
        // fails after some blobs are already in the odb, those
        // blobs become loose unreferenced objects. Git's GC will
        // clean them up eventually; mockspace does not roll them
        // back here. Same shape as standard git plumbing.
        let mut blob_oids: BTreeMap<String, gix::ObjectId> = BTreeMap::new();
        for (path, bytes) in new_tree.iter() {
            for component in path.split('/') {
                if component.is_empty() {
                    return Err(RefTreeWriteError::EmptyPathComponent {
                        path: path.to_owned(),
                    });
                }
            }
            let id = repo.write_blob(bytes).map_err(|e| {
                RefTreeWriteError::GixOdb {
                    source: Box::new(e),
                }
            })?;
            blob_oids.insert(path.to_owned(), id.detach());
        }

        // 2. Build the tree object recursively, bottom-up.
        let tree_oid = build_tree(repo, &blob_oids, "")?;

        // 3. Write the orphan commit object (empty parents).
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let signature = gix::actor::Signature {
            name:  "mockspace".into(),
            email: "noreply@mockspace.local".into(),
            time:  gix::date::Time::new(now, 0),
        };
        let commit = gix::objs::Commit {
            message:       message.into(),
            tree:          tree_oid,
            author:        signature.clone(),
            committer:     signature,
            encoding:      None,
            parents:       Default::default(),
            extra_headers: Default::default(),
        };
        let commit_id = repo
            .write_object(&commit)
            .map_err(|e| {
                RefTreeWriteError::GixOdb {
                    source: Box::new(e),
                }
            })?
            .detach();

        // 4. Atomically advance the ref via edit_reference with the
        //    CAS expectation.
        let expected = match expected_current {
            None => PreviousValue::MustNotExist,
            Some(oid) => PreviousValue::ExistingMustMatch(Target::Object(oid)),
        };
        let edit = RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode:                RefLog::AndReference,
                    force_create_reflog: false,
                    message:             format!("mockspace: {message}").into(),
                },
                expected,
                new: Target::Object(commit_id),
            },
            name:   ref_path
                .as_str()
                .try_into()
                .map_err(|e: gix::refs::name::Error| {
                    RefTreeWriteError::GixOdb {
                        source: Box::new(e),
                    }
                })?,
            deref:  false,
        };
        repo.edit_reference(edit).map_err(|e| {
            // gix's ref-edit failures bubble up as a single
            // `reference::edit::Error`; CAS misses appear inside
            // `FileTransactionPrepare(ReferenceOutOfDate { .. })`
            // when the expected previous-value does not match, and
            // similarly for the must-not-exist-but-does case.
            // Verified against gix 0.66. The Debug-repr scan is the
            // single point of fragility across gix bumps; the two
            // CAS-failure tests (stale-expected, must-not-exist)
            // catch any future drift loudly.
            // TODO(gix-upgrade): re-verify token set on bump.
            let dbg = format!("{e:?}");
            if dbg.contains("ReferenceOutOfDate")
                || dbg.contains("MustNotExist")
                || dbg.contains("MustExist")
                || dbg.contains("Rejection")
            {
                RefTreeWriteError::NonFastForward {
                    ref_path: ref_path.as_str().to_owned(),
                    expected: expected_current,
                }
            } else {
                RefTreeWriteError::GixOdb {
                    source: Box::new(e),
                }
            }
        })?;

        Ok(commit_id)
    }
}

/// Build a tree object from the (path, blob_oid) map, rooted at
/// `prefix` (use `""` for the top level). Walks paths into nested
/// subtree map structures, writes the deepest subtrees first, and
/// composes them upward.
fn build_tree(
    repo: &gix::Repository,
    blobs: &BTreeMap<String, gix::ObjectId>,
    prefix: &str,
) -> Result<gix::ObjectId, RefTreeWriteError> {
    // Group entries at this level by their immediate child name.
    // Direct children (no further `/`) are blobs; entries with a
    // further `/` are subtree members.
    let mut blob_children: BTreeMap<String, gix::ObjectId> = BTreeMap::new();
    let mut subtree_names: BTreeMap<String, ()> = BTreeMap::new();
    for (path, oid) in blobs {
        let rel = if prefix.is_empty() {
            path.as_str()
        } else if let Some(rest) = path.strip_prefix(&format!("{prefix}/")) {
            rest
        } else {
            // Not under this prefix; skip.
            continue;
        };
        match rel.split_once('/') {
            None => {
                blob_children.insert(rel.to_owned(), *oid);
            },
            Some((dir, _)) => {
                subtree_names.insert(dir.to_owned(), ());
            },
        }
    }

    let mut entries: Vec<gix::objs::tree::Entry> = Vec::new();
    for (name, oid) in &blob_children {
        entries.push(gix::objs::tree::Entry {
            mode:     EntryKind::Blob.into(),
            filename: name.as_str().into(),
            oid:      *oid,
        });
    }
    for (name, _) in &subtree_names {
        let child_prefix =
            if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let subtree_oid = build_tree(repo, blobs, &child_prefix)?;
        entries.push(gix::objs::tree::Entry {
            mode:     EntryKind::Tree.into(),
            filename: name.as_str().into(),
            oid:      subtree_oid,
        });
    }
    // git requires tree entries sorted by name (with trailing `/`
    // on tree entries for ordering). gix_object::Tree::write_to
    // expects this ordering; sort here to match.
    entries.sort_by(|a, b| {
        let a_name = sort_key(a.filename.as_slice(), a.mode.is_tree());
        let b_name = sort_key(b.filename.as_slice(), b.mode.is_tree());
        a_name.cmp(&b_name)
    });

    let tree = Tree {
        entries,
    };
    let id = repo.write_object(&tree).map_err(|e| {
        RefTreeWriteError::GixOdb {
            source: Box::new(e),
        }
    })?;
    Ok(id.detach())
}

/// Sort key matching git's tree-entry ordering: tree entries sort
/// as if their name had a trailing `/`.
fn sort_key(name: &[u8], is_tree: bool) -> Vec<u8> {
    let mut v: Vec<u8> = name.to_vec();
    if is_tree {
        v.push(b'/');
    }
    v
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;
    use crate::slug::Slug;

    fn init_repo(dir: &Path) {
        let out = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .output()
            .expect("git should run");
        assert!(out.status.success(), "git init failed");
    }

    fn tree_with(entries: &[(&str, &[u8])]) -> RoundRefTree {
        let map: BTreeMap<String, Vec<u8>> = entries
            .iter()
            .map(|(p, b)| ((*p).to_owned(), b.to_vec()))
            .collect();
        RoundRefTree::from_entries(map)
    }

    #[test]
    fn write_creates_new_orphan_ref_and_round_trips_through_reader() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let slug = Slug::new("write-create").unwrap();
        let ref_path = RefPath::round_mock(&slug);

        let new_tree =
            tree_with(&[(".phase", b"PLAN.DOC\n"), ("manifest.doc.toml", b"hello = \"world\"\n")]);
        let _commit_id = handle
            .write_round_ref(&ref_path, &new_tree, "create round", None)
            .expect("write succeeds");

        let read_back = handle.read_ref_tree(&ref_path).expect("read");
        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back.get(".phase").unwrap(), b"PLAN.DOC\n");
        assert_eq!(
            read_back.get("manifest.doc.toml").unwrap(),
            b"hello = \"world\"\n"
        );
    }

    #[test]
    fn write_with_subtree_round_trips() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let slug = Slug::new("write-subtree").unwrap();
        let ref_path = RefPath::round_mock(&slug);

        let new_tree = tree_with(&[
            (".phase", b"APPLY.DOC\n"),
            (".anchor.doc.toml", b"version = 1\n"),
            (".anchor.doc.blobs/ab/cdef0123", b"blob-a"),
            (".anchor.doc.blobs/ab/feed4567", b"blob-b"),
            (".anchor.doc.blobs/cd/9999", b"blob-c"),
        ]);
        handle
            .write_round_ref(&ref_path, &new_tree, "apply doc", None)
            .expect("write");

        let read_back = handle.read_ref_tree(&ref_path).expect("read");
        assert_eq!(read_back.len(), 5);
        assert_eq!(
            read_back.get(".anchor.doc.blobs/ab/cdef0123").unwrap(),
            b"blob-a"
        );
        assert_eq!(
            read_back.get(".anchor.doc.blobs/cd/9999").unwrap(),
            b"blob-c"
        );
    }

    #[test]
    fn write_with_matching_expected_updates_ref() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let slug = Slug::new("cas-match").unwrap();
        let ref_path = RefPath::round_mock(&slug);

        let first = tree_with(&[(".phase", b"PLAN.DOC\n")]);
        let first_oid = handle
            .write_round_ref(&ref_path, &first, "first", None)
            .expect("first write");

        let second = tree_with(&[(".phase", b"APPLY.DOC\n")]);
        let _second_oid = handle
            .write_round_ref(&ref_path, &second, "second", Some(first_oid))
            .expect("second write with matching CAS");

        let read_back = handle.read_ref_tree(&ref_path).expect("read");
        assert_eq!(read_back.get(".phase").unwrap(), b"APPLY.DOC\n");
    }

    #[test]
    fn write_with_stale_expected_fails_with_non_fast_forward() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let slug = Slug::new("cas-stale").unwrap();
        let ref_path = RefPath::round_mock(&slug);

        let first = tree_with(&[(".phase", b"PLAN.DOC\n")]);
        let first_oid = handle
            .write_round_ref(&ref_path, &first, "first", None)
            .expect("first write");

        // Second writer observed `first_oid` and tries to advance,
        // but a third writer raced in between and updated the ref.
        let racer = tree_with(&[(".phase", b"PLAN.SRC\n")]);
        let _racer_oid = handle
            .write_round_ref(&ref_path, &racer, "race", Some(first_oid))
            .expect("racer write");

        // Second writer's CAS now mismatches because the ref no
        // longer points at first_oid.
        let second = tree_with(&[(".phase", b"APPLY.DOC\n")]);
        let err = handle
            .write_round_ref(&ref_path, &second, "second", Some(first_oid))
            .unwrap_err();
        assert!(
            matches!(err, RefTreeWriteError::NonFastForward { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn write_sorts_tree_entries_with_trailing_slash_for_subtrees() {
        // Sort-key correctness: git requires tree entries sorted as
        // if tree names had a trailing `/`. Blob `foo` and subtree
        // `foo-bar/x` would sort `foo` < `foo-bar` byte-wise, but
        // git's required order treats the subtree as `foo-bar/`,
        // which still sorts after `foo`. Conversely, blob `foo`
        // and subtree `foo/x` would have the subtree name `foo`
        // collide with the blob name; git rejects same-name
        // collisions outright, so this test instead picks
        // names where the trailing-slash rule reorders entries:
        // blob `foo-bar` (no slash) versus subtree `foo` (with
        // slash, sorts as `foo/`). Byte order: `foo` < `foo-bar`.
        // Trailing-slash order: `foo/` > `foo-bar`. The writer
        // must use trailing-slash order or gix's Tree validator
        // rejects the write with a sorted-out-of-order error.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let slug = Slug::new("sort-key").unwrap();
        let ref_path = RefPath::round_mock(&slug);

        let new_tree = tree_with(&[("foo-bar", b"blob-bytes"), ("foo/inner", b"sub-bytes")]);
        handle
            .write_round_ref(&ref_path, &new_tree, "sort", None)
            .expect("write succeeds with correct sort key");

        let read_back = handle.read_ref_tree(&ref_path).expect("read");
        assert_eq!(read_back.len(), 2);
        assert_eq!(read_back.get("foo-bar").unwrap(), b"blob-bytes");
        assert_eq!(read_back.get("foo/inner").unwrap(), b"sub-bytes");
    }

    #[test]
    fn write_with_must_not_exist_fails_when_ref_already_exists() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let slug = Slug::new("must-not-exist").unwrap();
        let ref_path = RefPath::round_mock(&slug);

        let first = tree_with(&[(".phase", b"PLAN.DOC\n")]);
        handle
            .write_round_ref(&ref_path, &first, "first", None)
            .expect("first write");

        let second = tree_with(&[(".phase", b"APPLY.DOC\n")]);
        let err = handle
            .write_round_ref(&ref_path, &second, "second", None)
            .unwrap_err();
        assert!(
            matches!(err, RefTreeWriteError::NonFastForward { .. }),
            "got {err:?}"
        );
    }
}
