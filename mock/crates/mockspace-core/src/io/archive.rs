//! `archive_round` executor: move a DONE round into the unified
//! `refs/mock/round-archive` ref and delete the source round-mock
//! ref.
//!
//! Slice E8 of the Phase 5 IO plan. Composes the E1-E5 building
//! blocks (no seal involvement) to handle the DONE -> archived
//! lifecycle step per spec §26's "unified closed-rounds archive".
//!
//! The archive ref carries one subtree per archived round, keyed by
//! the round's slug. The round's full tree at archive time becomes
//! the contents of that subtree. Each archive update writes a new
//! orphan commit whose tree includes:
//!
//! - All entries from the prior archive tree (if any).
//! - A fresh `<slug>/<entry>` for every entry in the round's tree.
//!
//! The source `refs/mock/round/<slug>` is then deleted in a
//! separate `edit_reference` step (gix-ref's transaction layer
//! does not bundle update + delete across distinct refs in a
//! single atomic op; the caller-held `FlockTransitionLock`
//! provides the serialisation guarantee).
//!
//! Recovery story: if archive-write succeeds but source-delete
//! fails, the round is present in BOTH the archive and the
//! source-side ref. Re-running `archive_round` is idempotent on
//! the archive side (writes the same `<slug>/` subtree contents
//! atop the previous commit) and retries the delete. The lock is
//! the serialisation primitive; the in-flight inconsistency
//! window is bounded by the lock holder's session lifetime.

use std::collections::BTreeMap;

use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};

use crate::io::lock::FlockTransitionLock;
use crate::io::ref_tree::{RefTreeReadError, RoundRefTree};
use crate::io::ref_write::RefTreeWriteError;
use crate::io::repo::RepoHandle;
use crate::phase::Phase;
use crate::ref_path::RefPath;
use crate::slug::Slug;

/// Outcome of a successful [`RepoHandle::archive_round`] call.
///
/// Not `Clone` because [`source_delete_error`](Self::source_delete_error)
/// holds a boxed trait object that is not itself `Clone`. Callers
/// inspect the report by value.
#[derive(Debug)]
pub struct ArchiveReport {
    /// New commit OID on `refs/mock/round-archive` after this
    /// round was added.
    pub archive_commit: gix::ObjectId,
    /// Number of entries the round contributed to the archive
    /// tree (every entry from the round tree gets a `<slug>/`
    /// prefix in the merged archive).
    pub entries_archived: usize,
    /// Whether the source `refs/mock/round/<slug>` ref was
    /// successfully deleted. `false` indicates the archive write
    /// succeeded but the delete failed; re-run is idempotent and
    /// will retry the delete.
    pub source_ref_deleted: bool,
    /// Underlying cause when `source_ref_deleted` is `false`. The
    /// archive write succeeded (see `archive_commit`) but the
    /// delete on `refs/mock/round/<slug>` failed with this error.
    /// `None` when the delete succeeded.
    pub source_delete_error: Option<Box<dyn std::error::Error + Send + Sync>>,
}

/// Failure modes for [`RepoHandle::archive_round`].
#[derive(Debug)]
pub enum ArchiveError {
    /// The round ref does not exist; nothing to archive.
    RoundRefMissing { slug: String },
    /// The round tree has no `.phase` blob.
    PhaseMarkerMissing,
    /// The `.phase` blob did not parse as a known phase.
    PhaseMarkerInvalid { raw: String },
    /// The round is not in DONE phase. Archive only accepts
    /// rounds whose lifecycle is complete.
    NotDone { current: Phase },
    /// Reading either the source round ref or the prior archive
    /// ref failed.
    ReadFailed(RefTreeReadError),
    /// Writing the new archive commit failed.
    WriteFailed(RefTreeWriteError),
}

impl core::fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RoundRefMissing { slug } => {
                write!(f, "round ref for slug `{slug}` does not exist")
            }
            Self::PhaseMarkerMissing => write!(f, ".phase blob missing from round tree"),
            Self::PhaseMarkerInvalid { raw } => {
                write!(f, "`.phase` blob did not parse: {raw:?}")
            }
            Self::NotDone { current } => {
                write!(f, "round is in phase {current:?}; only DONE rounds may be archived")
            }
            Self::ReadFailed(e) => write!(f, "ref read failed: {e}"),
            Self::WriteFailed(e) => write!(f, "archive write failed: {e}"),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<RefTreeReadError> for ArchiveError {
    fn from(e: RefTreeReadError) -> Self {
        Self::ReadFailed(e)
    }
}

impl From<RefTreeWriteError> for ArchiveError {
    fn from(e: RefTreeWriteError) -> Self {
        Self::WriteFailed(e)
    }
}

impl RepoHandle {
    /// Archive a DONE round to the unified
    /// `refs/mock/round-archive` ref and delete the source
    /// `refs/mock/round/<slug>`.
    ///
    /// The caller must hold `lock`; the parameter is borrowed so
    /// the lock outlives this call. The function is idempotent on
    /// retry: re-archiving an already-archived round (when the
    /// previous attempt's source-delete step failed) writes the
    /// same subtree contents atop the archive and retries the
    /// delete.
    pub fn archive_round(
        &self,
        _lock: &FlockTransitionLock,
        slug: &Slug,
    ) -> Result<ArchiveReport, ArchiveError> {
        let round_ref = RefPath::round_mock(slug);
        let archive_ref = RefPath::round_archive();

        // 1. Read the round ref + verify it is in DONE phase.
        let round_oid = match self.resolve_ref_oid(&round_ref) {
            Ok(oid) => oid,
            Err(RefTreeReadError::RefNotFound { .. }) => {
                return Err(ArchiveError::RoundRefMissing {
                    slug: slug.as_ref().to_owned(),
                });
            }
            Err(other) => return Err(other.into()),
        };
        let round_tree = self.read_ref_tree(&round_ref)?;
        let current_phase = read_phase(&round_tree)?;
        if current_phase != Phase::Done {
            return Err(ArchiveError::NotDone {
                current: current_phase,
            });
        }

        // 2. Read the prior archive tree (if any) and pin its OID
        //    for CAS.
        let (prior_archive_tree, prior_archive_oid) =
            match self.resolve_ref_oid(&archive_ref) {
                Ok(oid) => {
                    let tree = self.read_ref_tree(&archive_ref)?;
                    (tree, Some(oid))
                }
                Err(RefTreeReadError::RefNotFound { .. }) => (RoundRefTree::default(), None),
                Err(other) => return Err(other.into()),
            };

        // 3. Build the new archive tree: existing entries +
        //    `<slug>/<entry>` for every entry in the round tree.
        let mut new_entries: BTreeMap<String, Vec<u8>> = prior_archive_tree
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_vec()))
            .collect();
        let slug_prefix = slug.as_ref();
        let mut contributed = 0usize;
        // Strip any prior `<slug>/...` entries before re-inserting
        // so idempotent retries do not stack stale + fresh views.
        let stale_prefix = format!("{slug_prefix}/");
        let stale_keys: Vec<String> = new_entries
            .keys()
            .filter(|k| k.starts_with(&stale_prefix))
            .cloned()
            .collect();
        for k in stale_keys {
            new_entries.remove(&k);
        }
        for (path, bytes) in round_tree.iter() {
            new_entries.insert(format!("{slug_prefix}/{path}"), bytes.to_vec());
            contributed += 1;
        }

        let new_tree = RoundRefTree::from_entries(new_entries);
        let message = format!("archive: round `{slug_prefix}` -> round-archive");
        let archive_commit =
            self.write_round_ref(&archive_ref, &new_tree, &message, prior_archive_oid)?;

        // 4. Delete the source round ref. If this fails the
        //    archive write succeeded; the caller can retry and
        //    the second attempt is idempotent (we strip any
        //    `<slug>/` entries before re-inserting).
        let (source_ref_deleted, source_delete_error) =
            match self.delete_ref(&round_ref, round_oid) {
                Ok(()) => (true, None),
                Err(e) => {
                    // Surface the partial-success outcome via the
                    // report. The archive write IS persisted, so
                    // returning an error variant here would
                    // discard the archive_commit OID the caller
                    // needs for retry decisions. The report shape
                    // preserves both the OID and the underlying
                    // delete error.
                    (false, Some(e))
                }
            };

        Ok(ArchiveReport {
            archive_commit,
            entries_archived: contributed,
            source_ref_deleted,
            source_delete_error,
        })
    }

    /// Delete a ref via [`gix::Repository::edit_reference`] with
    /// a CAS expectation on the current OID.
    fn delete_ref(
        &self,
        ref_path: &RefPath,
        expected_oid: gix::ObjectId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let repo = self.repo();
        let edit = RefEdit {
            change: Change::Delete {
                expected: PreviousValue::ExistingMustMatch(gix::refs::Target::Object(
                    expected_oid,
                )),
                log: RefLog::AndReference,
            },
            name: ref_path
                .as_str()
                .try_into()
                .map_err(|e: gix::refs::name::Error| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(e)
                })?,
            deref: false,
        };
        repo.edit_reference(edit)
            .map(|_| ())
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
    }
}

fn read_phase(tree: &RoundRefTree) -> Result<Phase, ArchiveError> {
    let bytes = tree
        .get(".phase")
        .ok_or(ArchiveError::PhaseMarkerMissing)?;
    let s = core::str::from_utf8(bytes)
        .map_err(|_| ArchiveError::PhaseMarkerInvalid {
            raw: format!("{bytes:?}"),
        })?
        .trim();
    Phase::from_marker(s).ok_or_else(|| ArchiveError::PhaseMarkerInvalid {
        raw: s.to_owned(),
    })
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

    fn init_repo(dir: &Path) {
        let out = run(&["init", "--quiet"], dir);
        assert!(out.status.success());
    }

    fn seed_done_round(
        repo_dir: &Path,
        slug: &Slug,
        extra: &[(&str, &[u8])],
    ) {
        let handle = RepoHandle::open(repo_dir).expect("open");
        let ref_path = RefPath::round_mock(slug);
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"done\n".to_vec());
        for (k, v) in extra {
            entries.insert((*k).to_owned(), v.to_vec());
        }
        let tree = RoundRefTree::from_entries(entries);
        handle
            .write_round_ref(&ref_path, &tree, "seed DONE", None)
            .expect("seed");
    }

    #[test]
    fn archive_done_round_writes_archive_and_deletes_source() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("done-round-a").unwrap();
        seed_done_round(
            dir.path(),
            &slug,
            &[
                ("manifest.doc.locked.toml", b"doc-locked"),
                ("manifest.src.locked.toml", b"src-locked"),
            ],
        );

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle.archive_round(&lock, &slug).expect("archive");
        assert!(report.source_ref_deleted);
        // 3 entries contributed: .phase + manifest.doc.locked + manifest.src.locked.
        assert_eq!(report.entries_archived, 3);

        // Archive ref carries the slug-prefixed entries.
        let archive_tree = handle
            .read_ref_tree(&RefPath::round_archive())
            .expect("read archive");
        assert_eq!(archive_tree.get("done-round-a/.phase").unwrap(), b"done\n");
        assert_eq!(
            archive_tree.get("done-round-a/manifest.doc.locked.toml").unwrap(),
            b"doc-locked"
        );

        // Source round ref is gone.
        let err = handle
            .resolve_ref_oid(&RefPath::round_mock(&slug))
            .unwrap_err();
        assert!(
            matches!(err, RefTreeReadError::RefNotFound { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn archive_round_errors_when_not_done() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("plan-doc-round").unwrap();
        let handle = RepoHandle::open(dir.path()).expect("open");
        let ref_path = RefPath::round_mock(&slug);
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
        let tree = RoundRefTree::from_entries(entries);
        handle
            .write_round_ref(&ref_path, &tree, "seed PLAN(doc)", None)
            .unwrap();

        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let err = handle.archive_round(&lock, &slug).unwrap_err();
        assert!(
            matches!(
                err,
                ArchiveError::NotDone {
                    current: Phase::PlanDoc
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn archive_round_errors_when_round_ref_missing() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let slug = Slug::new("nothing-there").unwrap();
        let err = handle.archive_round(&lock, &slug).unwrap_err();
        assert!(matches!(err, ArchiveError::RoundRefMissing { .. }), "got {err:?}");
    }

    #[test]
    fn archive_round_merges_into_existing_archive() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");

        // First archive.
        let slug_a = Slug::new("first-done").unwrap();
        seed_done_round(dir.path(), &slug_a, &[("manifest.doc.locked.toml", b"a-doc")]);
        handle.archive_round(&lock, &slug_a).expect("first archive");

        // Second archive must preserve the first.
        let slug_b = Slug::new("second-done").unwrap();
        seed_done_round(dir.path(), &slug_b, &[("manifest.doc.locked.toml", b"b-doc")]);
        let report = handle.archive_round(&lock, &slug_b).expect("second archive");
        assert!(report.source_ref_deleted);

        let archive_tree = handle
            .read_ref_tree(&RefPath::round_archive())
            .expect("read archive");
        // Both slugs present, each with their own entries.
        assert!(archive_tree.get("first-done/.phase").is_some());
        assert!(archive_tree.get("first-done/manifest.doc.locked.toml").is_some());
        assert!(archive_tree.get("second-done/.phase").is_some());
        assert!(archive_tree.get("second-done/manifest.doc.locked.toml").is_some());
    }

    #[test]
    fn archive_round_strips_stale_entries_on_idempotent_retry() {
        // Simulate the partial-failure recovery case: round was
        // archived once with a manifest of bytes "v1"; the source
        // delete failed; the round tree was edited to have new
        // content "v2"; a second archive_round picks up the new
        // content and the archive carries the v2 view, not a
        // mixed v1+v2 stale residue.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");

        let slug = Slug::new("idempotent-retry").unwrap();
        seed_done_round(dir.path(), &slug, &[("data.toml", b"v1")]);
        handle.archive_round(&lock, &slug).expect("first archive");

        // The first archive deleted the source ref, so re-seed it
        // with the v2 content as if the delete had failed and the
        // caller re-authored. seed_done_round writes with
        // expected_current = None which means the ref must not
        // exist; with the source deleted that is fine.
        seed_done_round(dir.path(), &slug, &[("data.toml", b"v2")]);
        handle.archive_round(&lock, &slug).expect("retry archive");

        let archive_tree = handle
            .read_ref_tree(&RefPath::round_archive())
            .expect("read archive");
        // The archive carries the v2 contents, not v1.
        assert_eq!(
            archive_tree.get("idempotent-retry/data.toml").unwrap(),
            b"v2",
            "stale v1 should be stripped before fresh v2 is inserted"
        );
    }
}
