//! `advance_phase` executor: dispatch the four phase verbs.
//!
//! Slice E7 of the Phase 5 IO plan. Composes the building blocks
//! from E1-E5 (plus the seal executor from E6) into a single
//! verb-dispatching entry point for the four transition verbs per
//! spec §14 + §15:
//!
//! - [`AdvanceVerb::Plan`]: `TOPIC` -> `PLAN(doc)`. Rewrite
//!   `.phase`. The PLAN-phase manifest authoring happens via
//!   separate ref-tree writes by the CLI; advance_phase opens the
//!   phase, it does not scaffold.
//! - [`AdvanceVerb::Apply { source_branch_tip }`]: `PLAN(side)` ->
//!   `APPLY(side)`. Delegates to [`RepoHandle::seal_manifest`].
//! - [`AdvanceVerb::Finish`]: `APPLY(doc)` -> `PLAN(src)`, or
//!   `APPLY(src)` -> `DONE`. Rewrites `.phase`; on the doc->src
//!   transition leaves the src-side manifest authoring to the
//!   PLAN(src) phase.
//! - [`AdvanceVerb::Replan(mode)`]: `APPLY(side)` -> `PLAN(side)`.
//!   Renames `manifest.<side>.locked.toml` to
//!   `manifest.<side>.deprecated.<n>.toml` (picking the next
//!   1-indexed `n`), rewrites `.phase`. The [`ReplanMode`]
//!   handling for post-APPLY work touching claimed source-side
//!   files lives in higher-level orchestration; this slice ships
//!   the local-ref portion.
//!
//! The caller holds the [`FlockTransitionLock`] across the full
//! sequence and pre-resolves any source-side input.

use std::collections::BTreeMap;

use crate::io::lock::FlockTransitionLock;
use crate::io::ref_tree::{RefTreeReadError, RoundRefTree};
use crate::io::ref_write::RefTreeWriteError;
use crate::io::repo::RepoHandle;
use crate::io::seal::{SealError, SealReport};
use crate::phase::{ManifestSide, Phase};
use crate::ref_path::RefPath;
use crate::round::ManifestStage;
use crate::slug::Slug;
use crate::transition::{ReplanMode, TransitionVerb};

/// The four phase verbs packaged with the per-verb data needed to
/// execute them against the round ref tree.
///
/// Mirrors [`crate::transition::Transition`] but extends Apply
/// with the resolved source-side tip OID (spec §24 step 5). The
/// transition module's enum captures the verb shape for validation;
/// the IO layer's enum captures what the executor needs at runtime.
///
/// Kept in sync with [`crate::transition::Transition`] by hand.
/// If a fifth verb lands in spec §14, both enums change together,
/// plus [`crate::transition::TransitionVerb`]. The duplication is
/// deliberate: the transition module is contract-layer with no IO
/// dependency; this layer carries the runtime payloads.
#[derive(Debug)]
pub enum AdvanceVerb {
    /// Open a planning surface. TOPIC -> PLAN(doc).
    Plan,
    /// Seal the current authoring manifest and transition to APPLY.
    /// PLAN(side) -> APPLY(side). Delegates to [`RepoHandle::seal_manifest`].
    Apply {
        /// Source-side branch tip OID at APPLY entry. Captured by
        /// the anchor.
        source_branch_tip: gix::ObjectId,
    },
    /// Advance bookkeeping past APPLY. APPLY(doc) -> PLAN(src), or
    /// APPLY(src) -> DONE.
    Finish,
    /// Deprecate the current sealed manifest and return to PLAN.
    /// APPLY(side) -> PLAN(side). Renames the locked manifest to
    /// `manifest.<side>.deprecated.<n>.toml`.
    ///
    /// The [`ReplanMode`] is taken as input for API stability so
    /// callers compile against the final signature, but this slice
    /// ships only the local-ref portion of replan (rename + phase
    /// flip), which is independent of the mode. The destructive /
    /// additive / accept-loss handling for post-APPLY commits
    /// touching claimed source-side files lives in higher-level
    /// orchestration that has access to the source-side branch
    /// state.
    Replan(ReplanMode),
}

impl AdvanceVerb {
    /// The [`TransitionVerb`] tag for this verb.
    pub fn verb(&self) -> TransitionVerb {
        match self {
            Self::Plan => TransitionVerb::Plan,
            Self::Apply { .. } => TransitionVerb::Apply,
            Self::Finish => TransitionVerb::Finish,
            Self::Replan(_) => TransitionVerb::Replan,
        }
    }
}

/// Outcome of a successful [`RepoHandle::advance_phase`] call.
#[derive(Debug, Clone)]
pub struct AdvanceReport {
    /// The verb that was executed.
    pub verb: TransitionVerb,
    /// Phase the round landed in.
    pub landed_in: Phase,
    /// New commit OID on `refs/mock/round/<slug>` after the
    /// transition.
    pub new_commit: gix::ObjectId,
    /// For [`AdvanceVerb::Apply`]: the underlying seal report
    /// (locked manifest path + anchor blob count). `None` for the
    /// other verbs.
    pub seal: Option<SealReport>,
    /// For [`AdvanceVerb::Replan`]: the deprecated manifest's
    /// iteration number. `None` for the other verbs.
    pub deprecated_iteration: Option<u32>,
}

/// Failure modes for [`RepoHandle::advance_phase`].
#[derive(Debug)]
pub enum AdvanceError {
    /// The round ref does not exist.
    RoundRefMissing { slug: String },
    /// The round tree has no `.phase` blob.
    PhaseMarkerMissing,
    /// The `.phase` blob did not parse as a known phase.
    PhaseMarkerInvalid { raw: String },
    /// The verb is not valid from the current phase.
    InvalidFromPhase {
        verb: TransitionVerb,
        current: Phase,
        allowed_from: &'static [Phase],
    },
    /// Round ref read failed.
    ReadFailed(RefTreeReadError),
    /// Round ref write failed.
    WriteFailed(RefTreeWriteError),
    /// The underlying seal executor failed (Apply path only).
    SealFailed(SealError),
    /// Replan was requested but the locked manifest for the
    /// current side is missing from the tree. APPLY-phase
    /// invariant requires it; absence is a corruption signal.
    LockedManifestMissing { path: String },
}

impl core::fmt::Display for AdvanceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RoundRefMissing { slug } => {
                write!(f, "round ref for slug `{slug}` does not exist")
            }
            Self::PhaseMarkerMissing => write!(f, ".phase blob missing from round tree"),
            Self::PhaseMarkerInvalid { raw } => {
                write!(f, "`.phase` blob did not parse: {raw:?}")
            }
            Self::InvalidFromPhase {
                verb,
                current,
                allowed_from,
            } => write!(
                f,
                "verb {verb:?} not valid from {current:?}; allowed from {allowed_from:?}"
            ),
            Self::ReadFailed(e) => write!(f, "round ref read failed: {e}"),
            Self::WriteFailed(e) => write!(f, "round ref write failed: {e}"),
            Self::SealFailed(e) => write!(f, "seal executor failed: {e}"),
            Self::LockedManifestMissing { path } => {
                write!(f, "locked manifest absent at `{path}`")
            }
        }
    }
}

impl std::error::Error for AdvanceError {}

impl From<RefTreeReadError> for AdvanceError {
    fn from(e: RefTreeReadError) -> Self {
        Self::ReadFailed(e)
    }
}

impl From<RefTreeWriteError> for AdvanceError {
    fn from(e: RefTreeWriteError) -> Self {
        Self::WriteFailed(e)
    }
}

impl From<SealError> for AdvanceError {
    fn from(e: SealError) -> Self {
        Self::SealFailed(e)
    }
}

impl RepoHandle {
    /// Execute one of the four phase verbs against the round
    /// `slug`. The caller must hold `lock`; the parameter is
    /// borrowed so the lock outlives this call.
    pub fn advance_phase(
        &self,
        lock: &FlockTransitionLock,
        slug: &Slug,
        verb: AdvanceVerb,
    ) -> Result<AdvanceReport, AdvanceError> {
        let ref_path = RefPath::round_mock(slug);
        let current_oid = match self.resolve_ref_oid(&ref_path) {
            Ok(oid) => oid,
            Err(RefTreeReadError::RefNotFound { .. }) => {
                return Err(AdvanceError::RoundRefMissing {
                    slug: slug.as_ref().to_owned(),
                });
            }
            Err(other) => return Err(other.into()),
        };
        let current_tree = self.read_ref_tree(&ref_path)?;
        let current_phase = read_phase(&current_tree)?;

        let verb_tag = verb.verb();
        let allowed_from = verb_tag.allowed_from();
        if !allowed_from.contains(&current_phase) {
            return Err(AdvanceError::InvalidFromPhase {
                verb: verb_tag,
                current: current_phase,
                allowed_from,
            });
        }

        match verb {
            AdvanceVerb::Plan => {
                exec_plan(self, lock, slug, &ref_path, current_oid, &current_tree)
            }
            AdvanceVerb::Apply { source_branch_tip } => exec_apply(
                self,
                lock,
                slug,
                current_phase,
                source_branch_tip,
            ),
            AdvanceVerb::Finish => exec_finish(
                self,
                lock,
                slug,
                &ref_path,
                current_oid,
                &current_tree,
                current_phase,
            ),
            AdvanceVerb::Replan(_mode) => exec_replan(
                self,
                lock,
                slug,
                &ref_path,
                current_oid,
                &current_tree,
                current_phase,
            ),
        }
    }
}

fn read_phase(tree: &RoundRefTree) -> Result<Phase, AdvanceError> {
    let bytes = tree
        .get(".phase")
        .ok_or(AdvanceError::PhaseMarkerMissing)?;
    let s = core::str::from_utf8(bytes)
        .map_err(|_| AdvanceError::PhaseMarkerInvalid {
            raw: format!("{bytes:?}"),
        })?
        .trim();
    Phase::from_marker(s).ok_or_else(|| AdvanceError::PhaseMarkerInvalid {
        raw: s.to_owned(),
    })
}

fn exec_plan(
    handle: &RepoHandle,
    _lock: &FlockTransitionLock,
    _slug: &Slug,
    ref_path: &RefPath,
    current_oid: gix::ObjectId,
    current_tree: &RoundRefTree,
) -> Result<AdvanceReport, AdvanceError> {
    let new_entries = rewrite_phase(current_tree, Phase::PlanDoc);
    let new_tree = RoundRefTree::from_entries_pub(new_entries);
    let new_commit = handle.write_round_ref(
        ref_path,
        &new_tree,
        "plan: TOPIC -> PLAN(doc)",
        Some(current_oid),
    )?;
    Ok(AdvanceReport {
        verb: TransitionVerb::Plan,
        landed_in: Phase::PlanDoc,
        new_commit,
        seal: None,
        deprecated_iteration: None,
    })
}

fn exec_apply(
    handle: &RepoHandle,
    lock: &FlockTransitionLock,
    slug: &Slug,
    current_phase: Phase,
    source_branch_tip: gix::ObjectId,
) -> Result<AdvanceReport, AdvanceError> {
    let side = match current_phase {
        Phase::PlanDoc => ManifestSide::Doc,
        Phase::PlanSrc => ManifestSide::Src,
        // The pre-dispatch validation guarantees we are in a PLAN
        // phase here; the match is exhaustive on that subset.
        _ => unreachable!("validate guard verified PLAN phase"),
    };
    let landed_in = match side {
        ManifestSide::Doc => Phase::ApplyDoc,
        ManifestSide::Src => Phase::ApplySrc,
    };
    let report = handle.seal_manifest(lock, slug, side, source_branch_tip)?;
    Ok(AdvanceReport {
        verb: TransitionVerb::Apply,
        landed_in,
        new_commit: report.new_commit,
        seal: Some(report),
        deprecated_iteration: None,
    })
}

fn exec_finish(
    handle: &RepoHandle,
    _lock: &FlockTransitionLock,
    _slug: &Slug,
    ref_path: &RefPath,
    current_oid: gix::ObjectId,
    current_tree: &RoundRefTree,
    current_phase: Phase,
) -> Result<AdvanceReport, AdvanceError> {
    let landed_in = match current_phase {
        Phase::ApplyDoc => Phase::PlanSrc,
        Phase::ApplySrc => Phase::Done,
        _ => unreachable!("validate guard verified APPLY phase"),
    };
    let new_entries = rewrite_phase(current_tree, landed_in);
    let new_tree = RoundRefTree::from_entries_pub(new_entries);
    let message = match landed_in {
        Phase::PlanSrc => "finish: APPLY(doc) -> PLAN(src)".to_owned(),
        Phase::Done => "finish: APPLY(src) -> DONE".to_owned(),
        _ => unreachable!(),
    };
    let new_commit = handle.write_round_ref(ref_path, &new_tree, &message, Some(current_oid))?;
    Ok(AdvanceReport {
        verb: TransitionVerb::Finish,
        landed_in,
        new_commit,
        seal: None,
        deprecated_iteration: None,
    })
}

fn exec_replan(
    handle: &RepoHandle,
    _lock: &FlockTransitionLock,
    _slug: &Slug,
    ref_path: &RefPath,
    current_oid: gix::ObjectId,
    current_tree: &RoundRefTree,
    current_phase: Phase,
) -> Result<AdvanceReport, AdvanceError> {
    let side = match current_phase {
        Phase::ApplyDoc => ManifestSide::Doc,
        Phase::ApplySrc => ManifestSide::Src,
        _ => unreachable!("validate guard verified APPLY phase"),
    };
    let landed_in = match side {
        ManifestSide::Doc => Phase::PlanDoc,
        ManifestSide::Src => Phase::PlanSrc,
    };

    // Rename the currently-locked manifest to the next available
    // deprecated.N slot.
    let locked_name = ManifestStage::Locked.filename(side);
    let locked_bytes = current_tree
        .get(&locked_name)
        .ok_or_else(|| AdvanceError::LockedManifestMissing {
            path: locked_name.clone(),
        })?
        .to_vec();
    let next_n = next_deprecation_iteration(current_tree, side);
    let deprecated_name = ManifestStage::Deprecated(next_n).filename(side);

    let mut entries: BTreeMap<String, Vec<u8>> = current_tree
        .iter()
        .map(|(k, v)| (k.to_owned(), v.to_vec()))
        .collect();
    entries.remove(&locked_name);
    entries.insert(deprecated_name, locked_bytes);
    let phase_blob = format!("{}\n", landed_in.marker());
    entries.insert(".phase".to_owned(), phase_blob.into_bytes());

    let new_tree = RoundRefTree::from_entries_pub(entries);
    let message = format!("replan: APPLY({side}) -> PLAN({side}) [iter {next_n}]");
    let new_commit = handle.write_round_ref(ref_path, &new_tree, &message, Some(current_oid))?;

    Ok(AdvanceReport {
        verb: TransitionVerb::Replan,
        landed_in,
        new_commit,
        seal: None,
        deprecated_iteration: Some(next_n),
    })
}

/// Find the next 1-indexed deprecation iteration `n` such that
/// `manifest.<side>.deprecated.<n>.toml` is not already present
/// in the tree.
///
/// Malformed deprecation paths (e.g. `manifest.doc.deprecated.foo.toml`
/// where the `<n>` segment does not parse as `u32`) are silently
/// skipped. A corrupt tree mixing well-formed `deprecated.1.toml`
/// with malformed `deprecated.foo.toml` will pick `2`, leaving the
/// malformed entry stranded. Skipping rather than failing matches
/// the spec's "anchor namespace wins" stance on stale entries: the
/// executor advances regardless of pre-existing junk.
fn next_deprecation_iteration(tree: &RoundRefTree, side: ManifestSide) -> u32 {
    let prefix = format!("manifest.{}.deprecated.", side.marker());
    let suffix = ".toml";
    let mut max_seen: u32 = 0;
    for (path, _) in tree.iter() {
        if let Some(rest) = path.strip_prefix(&prefix) {
            if let Some(num_str) = rest.strip_suffix(suffix) {
                if let Ok(n) = num_str.parse::<u32>() {
                    if n > max_seen {
                        max_seen = n;
                    }
                }
            }
        }
    }
    max_seen + 1
}

/// Produce a fresh entries map identical to `current_tree` except
/// for the `.phase` blob, which is rewritten to `<phase_marker>\n`.
fn rewrite_phase(current_tree: &RoundRefTree, new_phase: Phase) -> BTreeMap<String, Vec<u8>> {
    let mut entries: BTreeMap<String, Vec<u8>> = current_tree
        .iter()
        .map(|(k, v)| (k.to_owned(), v.to_vec()))
        .collect();
    let phase_blob = format!("{}\n", new_phase.marker());
    entries.insert(".phase".to_owned(), phase_blob.into_bytes());
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{AcceptanceBlock, ChangeBlock, Manifest, ScopeBlock};
    use crate::verifier::{VerifierCheck, VerifierKind};
    use std::path::{Path, PathBuf};
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
                description: "test advance".to_owned(),
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

    fn seed_round(repo_dir: &Path, slug: &Slug, entries: BTreeMap<String, Vec<u8>>) {
        let handle = RepoHandle::open(repo_dir).expect("open");
        let ref_path = RefPath::round_mock(slug);
        let tree = RoundRefTree::from_entries_pub(entries);
        handle
            .write_round_ref(&ref_path, &tree, "seed", None)
            .expect("seed writes");
    }

    #[test]
    fn plan_verb_transitions_topic_to_plan_doc() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("plan-verb").unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"topic\n".to_vec());
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Plan)
            .expect("plan");
        assert_eq!(report.landed_in, Phase::PlanDoc);
        assert_eq!(report.verb, TransitionVerb::Plan);

        let sealed = handle
            .read_ref_tree(&RefPath::round_mock(&slug))
            .expect("read");
        assert_eq!(sealed.get(".phase").unwrap(), b"plan_doc\n");
    }

    #[test]
    fn apply_verb_delegates_to_seal_manifest() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let source_tip = init_source_tip(dir.path(), &[("README.md", "hello\n")]);
        let slug = Slug::new("apply-verb").unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"plan_doc\n".to_vec());
        entries.insert(
            "manifest.doc.toml".to_owned(),
            doc_manifest_toml(slug.as_str()).into_bytes(),
        );
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle
            .advance_phase(
                &lock,
                &slug,
                AdvanceVerb::Apply {
                    source_branch_tip: source_tip,
                },
            )
            .expect("apply");
        assert_eq!(report.landed_in, Phase::ApplyDoc);
        assert_eq!(report.verb, TransitionVerb::Apply);
        assert!(report.seal.is_some(), "Apply should carry the seal report");
        assert_eq!(
            report.seal.as_ref().unwrap().locked_manifest_path,
            "manifest.doc.locked.toml"
        );
    }

    #[test]
    fn finish_verb_doc_to_plan_src() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("finish-doc").unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"apply_doc\n".to_vec());
        entries.insert(
            "manifest.doc.locked.toml".to_owned(),
            doc_manifest_toml(slug.as_str()).into_bytes(),
        );
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Finish)
            .expect("finish");
        assert_eq!(report.landed_in, Phase::PlanSrc);

        let sealed = handle
            .read_ref_tree(&RefPath::round_mock(&slug))
            .expect("read");
        assert_eq!(sealed.get(".phase").unwrap(), b"plan_src\n");
        // Doc-side locked manifest is preserved across the finish.
        assert!(sealed.get("manifest.doc.locked.toml").is_some());
    }

    #[test]
    fn finish_verb_src_to_done() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("finish-src").unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"apply_src\n".to_vec());
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Finish)
            .expect("finish");
        assert_eq!(report.landed_in, Phase::Done);
    }

    #[test]
    fn replan_renames_locked_to_deprecated_one() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("replan-once").unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"apply_doc\n".to_vec());
        entries.insert(
            "manifest.doc.locked.toml".to_owned(),
            doc_manifest_toml(slug.as_str()).into_bytes(),
        );
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Replan(ReplanMode::Destructive))
            .expect("replan");
        assert_eq!(report.landed_in, Phase::PlanDoc);
        assert_eq!(report.deprecated_iteration, Some(1));

        let sealed = handle
            .read_ref_tree(&RefPath::round_mock(&slug))
            .expect("read");
        assert_eq!(sealed.get(".phase").unwrap(), b"plan_doc\n");
        assert!(sealed.get("manifest.doc.locked.toml").is_none());
        assert!(sealed.get("manifest.doc.deprecated.1.toml").is_some());
    }

    #[test]
    fn replan_picks_next_iteration_after_existing_deprecations() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("replan-iter").unwrap();
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"apply_doc\n".to_vec());
        entries.insert(
            "manifest.doc.locked.toml".to_owned(),
            doc_manifest_toml(slug.as_str()).into_bytes(),
        );
        // Two prior deprecations already present.
        entries.insert(
            "manifest.doc.deprecated.1.toml".to_owned(),
            b"prior-1\n".to_vec(),
        );
        entries.insert(
            "manifest.doc.deprecated.2.toml".to_owned(),
            b"prior-2\n".to_vec(),
        );
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let report = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Replan(ReplanMode::Destructive))
            .expect("replan");
        assert_eq!(report.deprecated_iteration, Some(3));

        let sealed = handle
            .read_ref_tree(&RefPath::round_mock(&slug))
            .expect("read");
        assert!(sealed.get("manifest.doc.deprecated.3.toml").is_some());
    }

    #[test]
    fn advance_phase_errors_on_invalid_verb_from_phase() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let slug = Slug::new("invalid-verb").unwrap();
        // .phase = topic, but caller tries Finish.
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert(".phase".to_owned(), b"topic\n".to_vec());
        seed_round(dir.path(), &slug, entries);

        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let err = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Finish)
            .unwrap_err();
        assert!(
            matches!(
                err,
                AdvanceError::InvalidFromPhase {
                    verb: TransitionVerb::Finish,
                    current: Phase::Topic,
                    ..
                }
            ),
            "got {err:?}"
        );
    }

    #[test]
    fn advance_phase_errors_when_round_ref_missing() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let lock = FlockTransitionLock::acquire(dir.path()).expect("acquire");
        let slug = Slug::new("nothing-there").unwrap();
        let err = handle
            .advance_phase(&lock, &slug, AdvanceVerb::Plan)
            .unwrap_err();
        assert!(
            matches!(err, AdvanceError::RoundRefMissing { .. }),
            "got {err:?}"
        );
    }
}
