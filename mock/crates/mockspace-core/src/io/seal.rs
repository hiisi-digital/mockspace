//! `seal_manifest` executor: PLAN(side) → APPLY(side) transition.
//!
//! Slice E6 of the Phase 5 IO plan. Composes the building blocks
//! shipped in E1-E5 into the seal-time portion of spec §24's
//! transition sequence. The caller holds the transition lock
//! ([`FlockTransitionLock`]) for the full sequence; this function
//! does not acquire or release it. The caller also pre-resolves the
//! source-side branch tip OID (spec §24 step 5) and the verifier
//! run (step 7); seal does not perform those.
//!
//! Steps covered by `seal_manifest` (numbering per spec §24):
//!
//! 2. Read the current round mock-ref tree (via [`read_ref_tree`]).
//! 4. Reserved for caller-side "verify clean .mock/round/". Not
//!    relevant in the orphan-ref model; mockspace renders into
//!    `.mock/` separately and the ref is the source of truth.
//! 6. Validate the authoring manifest (parse + `validate_structural`).
//! 8. Capture the anchor (via [`capture_anchor`]).
//! 9. Build the new tree: rename `manifest.<side>.toml` to
//!    `manifest.<side>.locked.toml`, splice in the anchor entries,
//!    rewrite `.phase` from `plan_<side>` to `apply_<side>`.
//! 11. `update-ref refs/mock/round/<slug>` to the new commit (via
//!     [`write_round_ref`] with CAS).
//!
//! Steps 1, 3, 12-14 (lock acquire, early-detection check, push CAS,
//! forge announcement) live in the higher-level orchestration that
//! composes this executor with the network slice (E9) and the lock
//! lifecycle. Step 5 is the caller's input. Steps 7 (verifier) and
//! 10 (render) are caller-side too.
//!
//! [`read_ref_tree`]: crate::RepoHandle::read_ref_tree
//! [`capture_anchor`]: crate::RepoHandle::capture_anchor
//! [`write_round_ref`]: crate::RepoHandle::write_round_ref
//! [`FlockTransitionLock`]: crate::FlockTransitionLock

use std::collections::BTreeMap;

use crate::io::anchor_capture::AnchorCaptureError;
use crate::io::lock::FlockTransitionLock;
use crate::io::ref_tree::{RefTreeReadError, RoundRefTree};
use crate::io::ref_write::RefTreeWriteError;
use crate::io::repo::RepoHandle;
use crate::manifest::{validate_structural, Manifest, ValidationError};
use crate::phase::{ManifestSide, Phase};
use crate::ref_path::DefaultRefPath;
use crate::round::ManifestStage;
use crate::slug::DefaultSlug;

/// Outcome of a successful [`RepoHandle::seal_manifest`] call.
#[derive(Debug, Clone)]
pub struct SealReport {
    /// New commit OID on `refs/mock/round/<slug>` after seal.
    pub new_commit: gix::ObjectId,
    /// Path of the sealed manifest within the round-ref tree,
    /// e.g. `manifest.doc.locked.toml`. Useful for logging /
    /// follow-up surfaces.
    pub locked_manifest_path: String,
    /// Number of unique blob storage entries spliced into the round
    /// tree under `.anchor.<side>.blobs/`. Content-addressed: two
    /// source-side files with identical content count once. To get
    /// the source-side file count, parse the `.anchor.<side>.toml`
    /// entry from the sealed tree and read `Anchor.files.len()`.
    pub anchor_blob_count: usize,
}

/// Failure modes for [`RepoHandle::seal_manifest`].
#[derive(Debug)]
pub enum SealError {
    /// The round ref does not exist; nothing to seal.
    RoundRefMissing { slug: String },
    /// The round ref exists but does not contain a `.phase` blob.
    /// Indicates the ref was not authored by mockspace or has been
    /// corrupted.
    PhaseMarkerMissing,
    /// The `.phase` blob did not parse as a known phase marker.
    PhaseMarkerInvalid { raw: String },
    /// The round is in a phase other than PLAN(side); seal only
    /// transitions PLAN(side) -> APPLY(side).
    WrongPhase { expected: Phase, actual: Phase },
    /// The authoring manifest file `manifest.<side>.toml` is absent
    /// from the round tree.
    AuthoringManifestMissing { path: String },
    /// The authoring manifest bytes are not valid UTF-8. Manifests
    /// are TOML; non-UTF-8 input is a corruption signal.
    ManifestNotUtf8 { path: String },
    /// The authoring manifest did not parse as TOML.
    ManifestParseFailed {
        path: String,
        error: toml::de::Error,
    },
    /// The parsed manifest failed structural validation.
    ManifestInvalid(ValidationError),
    /// A locked manifest already exists at the target path. Sealing
    /// twice is forbidden.
    AlreadyLocked { path: String },
    /// Read of the current round ref failed.
    ReadFailed(RefTreeReadError),
    /// Anchor capture from the source-side tip failed.
    AnchorFailed(AnchorCaptureError),
    /// Writing the new commit / advancing the ref failed.
    WriteFailed(RefTreeWriteError),
}

impl core::fmt::Display for SealError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RoundRefMissing { slug } => {
                write!(f, "round ref for slug `{slug}` does not exist")
            }
            Self::PhaseMarkerMissing => write!(f, ".phase blob missing from round tree"),
            Self::PhaseMarkerInvalid { raw } => {
                write!(f, "`.phase` blob did not parse as a known phase: {raw:?}")
            }
            Self::WrongPhase { expected, actual } => {
                write!(f, "wrong phase for seal; expected {expected:?}, got {actual:?}")
            }
            Self::AuthoringManifestMissing { path } => {
                write!(f, "authoring manifest missing at `{path}`")
            }
            Self::ManifestNotUtf8 { path } => {
                write!(f, "manifest at `{path}` is not valid UTF-8")
            }
            Self::ManifestParseFailed { path, error } => {
                write!(f, "manifest at `{path}` did not parse: {error}")
            }
            Self::ManifestInvalid(e) => write!(f, "manifest failed structural validation: {e:?}"),
            Self::AlreadyLocked { path } => {
                write!(f, "locked manifest already present at `{path}`")
            }
            Self::ReadFailed(e) => write!(f, "round ref read failed: {e}"),
            Self::AnchorFailed(e) => write!(f, "anchor capture failed: {e}"),
            Self::WriteFailed(e) => write!(f, "round ref write failed: {e}"),
        }
    }
}

impl std::error::Error for SealError {}

impl From<RefTreeReadError> for SealError {
    fn from(e: RefTreeReadError) -> Self {
        Self::ReadFailed(e)
    }
}

impl From<AnchorCaptureError> for SealError {
    fn from(e: AnchorCaptureError) -> Self {
        Self::AnchorFailed(e)
    }
}

impl From<RefTreeWriteError> for SealError {
    fn from(e: RefTreeWriteError) -> Self {
        Self::WriteFailed(e)
    }
}

impl From<ValidationError> for SealError {
    fn from(e: ValidationError) -> Self {
        Self::ManifestInvalid(e)
    }
}

/// Expected starting [`Phase`] for a seal on the given side.
const fn plan_phase_for(side: ManifestSide) -> Phase {
    match side {
        ManifestSide::Doc => Phase::PlanDoc,
        ManifestSide::Src => Phase::PlanSrc,
    }
}

/// Resulting [`Phase`] after a successful seal on the given side.
const fn apply_phase_for(side: ManifestSide) -> Phase {
    match side {
        ManifestSide::Doc => Phase::ApplyDoc,
        ManifestSide::Src => Phase::ApplySrc,
    }
}

impl RepoHandle {
    /// Seal the authoring manifest for `(slug, side)` and advance
    /// the round ref to a new commit carrying the locked form plus
    /// the captured anchor.
    ///
    /// The caller must hold `lock`; the parameter is borrowed so
    /// the lock outlives this call without being consumed.
    /// `source_branch_tip` is the source-side branch tip OID
    /// resolved by the caller per spec §24 step 5.
    pub fn seal_manifest<S: crate::slug::Slug>(
        &self,
        _lock: &FlockTransitionLock,
        slug: &S,
        side: ManifestSide,
        source_branch_tip: gix::ObjectId,
    ) -> Result<SealReport, SealError> {
        let ref_path = DefaultRefPath::round_mock(slug);

        // Step 2: read the current round ref tree and pin its OID
        // for the CAS update later.
        let current_oid = match self.resolve_ref_oid(&ref_path) {
            Ok(oid) => oid,
            Err(RefTreeReadError::RefNotFound { .. }) => {
                return Err(SealError::RoundRefMissing {
                    slug: slug.as_ref().to_owned(),
                });
            }
            Err(other) => return Err(other.into()),
        };
        let current_tree = self.read_ref_tree(&ref_path)?;

        // Validate the .phase marker matches the side we are
        // sealing for. plan_<side> is the only legal starting
        // point; apply_<side> means already sealed, done means
        // archived, topic means no manifest at all.
        let phase_bytes = current_tree
            .get(".phase")
            .ok_or(SealError::PhaseMarkerMissing)?;
        let phase_str = std::str::from_utf8(phase_bytes)
            .map_err(|_| SealError::PhaseMarkerInvalid {
                raw: format!("{phase_bytes:?}"),
            })?
            .trim();
        let actual_phase =
            Phase::from_marker(phase_str).ok_or_else(|| SealError::PhaseMarkerInvalid {
                raw: phase_str.to_owned(),
            })?;
        let expected = plan_phase_for(side);
        if actual_phase != expected {
            return Err(SealError::WrongPhase {
                expected,
                actual: actual_phase,
            });
        }

        // Step 6: validate the authoring manifest.
        let authoring_name = ManifestStage::Authoring.filename(side);
        let locked_name = ManifestStage::Locked.filename(side);
        if current_tree.get(&locked_name).is_some() {
            return Err(SealError::AlreadyLocked { path: locked_name });
        }
        let authoring_bytes = current_tree.get(&authoring_name).ok_or_else(|| {
            SealError::AuthoringManifestMissing {
                path: authoring_name.clone(),
            }
        })?;
        let manifest_text =
            std::str::from_utf8(authoring_bytes).map_err(|_| SealError::ManifestNotUtf8 {
                path: authoring_name.clone(),
            })?;
        let manifest =
            Manifest::from_toml(manifest_text).map_err(|error| SealError::ManifestParseFailed {
                path: authoring_name.clone(),
                error,
            })?;
        validate_structural(&manifest, side)?;

        // Step 8: capture the anchor at the source-side branch tip.
        let anchor_entries = self.capture_anchor(source_branch_tip, side)?;

        // Step 9: build the new tree.
        //
        // Start from the current entries map; drop the authoring
        // manifest path, insert it under the locked name with the
        // same bytes, splice in every anchor entry, and rewrite
        // `.phase` to the apply marker.
        let mut new_entries: BTreeMap<String, Vec<u8>> =
            current_tree.iter().map(|(k, v)| (k.to_owned(), v.to_vec())).collect();
        let manifest_bytes = new_entries
            .remove(&authoring_name)
            .expect("authoring_bytes existed in current_tree");
        new_entries.insert(locked_name.clone(), manifest_bytes);
        let blob_prefix = format!(".anchor.{}.blobs/", side.marker());
        let anchor_blob_count = anchor_entries
            .iter()
            .filter(|(k, _)| k.starts_with(&blob_prefix))
            .count();
        // Anchor namespace wins by design. If a previous failed
        // seal left stale `.anchor.<side>.*` entries in the tree,
        // the fresh capture overwrites them; the seal is the
        // authoritative source for the anchor surface.
        for (path, bytes) in anchor_entries {
            new_entries.insert(path, bytes);
        }
        let apply_marker = format!("{}\n", apply_phase_for(side).marker());
        new_entries.insert(".phase".to_owned(), apply_marker.into_bytes());

        let new_tree = RoundRefTree::from_entries_pub(new_entries);

        // Step 11: write the new commit + CAS-advance the ref.
        let message = format!("seal: PLAN({side}) -> APPLY({side})");
        let new_commit =
            self.write_round_ref(&ref_path, &new_tree, &message, Some(current_oid))?;

        Ok(SealReport {
            new_commit,
            locked_manifest_path: locked_name,
            anchor_blob_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{AcceptanceBlock, ChangeBlock, Manifest, ScopeBlock};
    use crate::verifier::{VerifierCheck, VerifierKind};
    use std::path::Path;
    use std::path::PathBuf;
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

    fn init_source_tip(dir: &Path, files: &[(&str, &str)]) -> gix::ObjectId {
        for (path, content) in files {
            if let Some(parent) = Path::new(path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(dir.join(parent)).unwrap();
                }
            }
            std::fs::write(dir.join(path), content).unwrap();
            let out = run(&["add", path], dir);
            assert!(out.status.success());
        }
        let out = run(&["commit", "--quiet", "-m", "source tip"], dir);
        assert!(out.status.success());
        let out = run(&["rev-parse", "HEAD"], dir);
        let sha = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        gix::ObjectId::from_hex(sha.as_bytes()).unwrap()
    }

    fn doc_manifest_toml(slug: &str) -> String {
        let m = Manifest {
            mockspace_version: "1.0".to_owned(),
            round_slug: slug.to_owned(),
            phase: ManifestSide::Doc,
            scope: ScopeBlock {
                description: "test seal".to_owned(),
                in_scope_tasks: vec![],
                out_of_scope: vec![],
            },
            acceptance: AcceptanceBlock {
                criteria: "passes".to_owned(),
            },
            changes: vec![ChangeBlock {
                task: None,
                file: PathBuf::from("README.md"),
                description: "doc change".to_owned(),
                verify: VerifierCheck::Kind(VerifierKind::PathExists {
                    file: PathBuf::from("README.md"),
                }),
            }],
            deprecated_accounting: vec![],
        };
        m.to_toml().unwrap()
    }

    fn seed_plan_doc_round(repo_dir: &Path, slug: &DefaultSlug) {
        // Write the PLAN(DOC) initial state directly as an orphan
        // mock ref: `.phase = plan_doc`, manifest.doc.toml present.
        // Use the slice E3 writer through RepoHandle since it is
        // the same machinery seal will compose with.
        let handle = RepoHandle::open(repo_dir).expect("open");
        let ref_path = DefaultRefPath::round_mock(slug);
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
        entries.insert(
            "manifest.doc.toml".to_owned(),
            doc_manifest_toml(slug.as_ref()).into_bytes(),
        );
        let tree = RoundRefTree::from_entries_pub(entries);
        handle
            .write_round_ref(&ref_path, &tree, "init PLAN(doc)", None)
            .expect("init writes");
    }

    #[test]
    fn seal_manifest_transitions_plan_doc_to_apply_doc() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let source_tip = init_source_tip(dir.path(), &[("README.md", "hello\n")]);
        let slug = DefaultSlug::new("test-seal").unwrap();
        seed_plan_doc_round(dir.path(), &slug);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire lock");

        let report = handle
            .seal_manifest(&lock, &slug, ManifestSide::Doc, source_tip)
            .expect("seal succeeds");

        assert_eq!(report.locked_manifest_path, "manifest.doc.locked.toml");
        assert_eq!(report.anchor_blob_count, 1);

        // The round ref should now contain the locked manifest at
        // the new path; the authoring path should be gone; .phase
        // should read apply_doc; the anchor TOML and a blob entry
        // for README.md should be present.
        let ref_path = DefaultRefPath::round_mock(&slug);
        let sealed_tree = handle.read_ref_tree(&ref_path).expect("read sealed");
        assert!(sealed_tree.get("manifest.doc.toml").is_none());
        assert!(sealed_tree.get("manifest.doc.locked.toml").is_some());
        let phase_bytes = sealed_tree.get(".phase").unwrap();
        assert_eq!(phase_bytes, b"apply_doc\n");
        assert!(sealed_tree.get(".anchor.doc.toml").is_some());
        let blob_count = sealed_tree
            .iter()
            .filter(|(k, _)| k.starts_with(".anchor.doc.blobs/"))
            .count();
        assert_eq!(blob_count, 1);
    }

    #[test]
    fn seal_manifest_errors_when_round_ref_missing() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let source_tip = init_source_tip(dir.path(), &[("README.md", "x\n")]);
        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let slug = DefaultSlug::new("nothing-there").unwrap();
        let err = handle
            .seal_manifest(&lock, &slug, ManifestSide::Doc, source_tip)
            .unwrap_err();
        assert!(matches!(err, SealError::RoundRefMissing { .. }), "got {err:?}");
    }

    #[test]
    fn seal_manifest_errors_on_wrong_phase() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let source_tip = init_source_tip(dir.path(), &[("README.md", "x\n")]);
        let slug = DefaultSlug::new("wrong-phase").unwrap();

        // Seed with .phase = apply_doc (already sealed) instead of
        // plan_doc. seal_manifest should refuse.
        let handle = RepoHandle::open(dir.path()).expect("open");
        let ref_path = DefaultRefPath::round_mock(&slug);
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"apply_doc\n".to_vec());
        entries.insert(
            "manifest.doc.locked.toml".to_owned(),
            doc_manifest_toml(slug.as_ref()).into_bytes(),
        );
        handle
            .write_round_ref(
                &ref_path,
                &RoundRefTree::from_entries_pub(entries),
                "init APPLY(doc)",
                None,
            )
            .unwrap();

        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let err = handle
            .seal_manifest(&lock, &slug, ManifestSide::Doc, source_tip)
            .unwrap_err();
        assert!(
            matches!(
                err,
                SealError::WrongPhase {
                    expected: Phase::PlanDoc,
                    actual: Phase::ApplyDoc
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn seal_manifest_errors_when_locked_manifest_already_present() {
        // Seed with .phase = plan_doc AND a manifest.doc.locked.toml
        // already in the tree. Sealing twice on the same side is
        // forbidden; the executor refuses.
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let source_tip = init_source_tip(dir.path(), &[("README.md", "x\n")]);
        let slug = DefaultSlug::new("already-locked").unwrap();

        let handle = RepoHandle::open(dir.path()).expect("open");
        let ref_path = DefaultRefPath::round_mock(&slug);
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
        entries.insert(
            "manifest.doc.toml".to_owned(),
            doc_manifest_toml(slug.as_ref()).into_bytes(),
        );
        entries.insert(
            "manifest.doc.locked.toml".to_owned(),
            doc_manifest_toml(slug.as_ref()).into_bytes(),
        );
        handle
            .write_round_ref(
                &ref_path,
                &RoundRefTree::from_entries_pub(entries),
                "init with stray locked",
                None,
            )
            .unwrap();

        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let err = handle
            .seal_manifest(&lock, &slug, ManifestSide::Doc, source_tip)
            .unwrap_err();
        assert!(matches!(err, SealError::AlreadyLocked { .. }), "got {err:?}");
    }

    #[test]
    fn seal_manifest_errors_when_authoring_manifest_missing() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let source_tip = init_source_tip(dir.path(), &[("README.md", "x\n")]);
        let slug = DefaultSlug::new("no-manifest").unwrap();

        // Seed with .phase = plan_doc but no manifest.doc.toml.
        let handle = RepoHandle::open(dir.path()).expect("open");
        let ref_path = DefaultRefPath::round_mock(&slug);
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
        handle
            .write_round_ref(
                &ref_path,
                &RoundRefTree::from_entries_pub(entries),
                "init",
                None,
            )
            .unwrap();

        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let err = handle
            .seal_manifest(&lock, &slug, ManifestSide::Doc, source_tip)
            .unwrap_err();
        assert!(
            matches!(err, SealError::AuthoringManifestMissing { .. }),
            "got {err:?}"
        );
    }
}
