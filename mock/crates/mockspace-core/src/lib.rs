//! Ref-based storage foundation for mockspace v2.
//!
//! Implements the storage layer from Part III of the v2 spec at
//! `mock/research/202605181400_mockspace-v2-spec.md`. Concerns split:
//!
//! - [`phase`]: the six-phase state machine and manifest-side vocabulary (§14).
//! - [`slug`]: identifier shapes for rounds, tasks, and refs (§16).
//! - [`namespace`]: hierarchical task namespacing (§16).
//! - [`ref_path`]: type-safe construction of `refs/...` paths (§19).
//! - [`bookkeeping`]: catalog of marker files filtered out of `.mock/` (§20, §25).
//! - [`anchor`]: content-addressed per-file snapshots captured at APPLY entry (§23).
//! - [`task`]: task identity, state, meta document (§16, §26).
//! - [`round`]: round mock-ref tree layout. Manifest filenames, topic and comment composers, `round.toml` (§25).
//! - [`manifest`]: per-side manifest contract sealed at APPLY entry (§17, §53).
//! - [`verifier`]: closed catalog of manifest verifier kinds + composition (§54).
//! - [`transition`]: phase transition verbs + validity matrix (§14, §15). Contract surface; impl in Phase 5.
//! - [`atomicity`]: transition atomicity contract (lock + race conflict shapes) (§24). Contract surface; impl in Phase 5.
//! - [`typestate`]: compile-time enforcement of phase / verb / side / stage / task-state invariants via sealed marker traits + zero-sized markers.
//! - [`lint`]: minimal lint engine substrate (§5). Vocabulary (Severity / Impact / Category / Gate / Finding / Span), suppression model, [`LintEngine`] swap point. Concrete engines (Rust, viola) live in separate crates.

pub mod anchor;
pub mod atomicity;
pub mod bookkeeping;
pub mod io;
pub mod lint;
pub mod manifest;
pub mod namespace;
pub mod phase;
pub mod ref_path;
pub mod round;
pub mod slug;
pub mod task;
pub mod transition;
pub mod typestate;
pub mod verifier;

pub use anchor::{Anchor, BlobSha, BlobShaError, FileEntry};
pub use atomicity::{
    AtomicityFinding, LockHolder, OnPhaseRaceAction, PhaseRaceConflict, ResolveStrategy,
    TransitionLock,
};
pub use bookkeeping::{classify_root_entry, BookkeepingFile, RootEntry};
pub use io::{
    FlockTransitionLock, LockError, RefTreeReadError, RefTreeWriteError, RepoError, RepoHandle,
    RoundRefTree,
};
pub use lint::{
    matches_pattern, Category, ContentHash, Directive, DirectiveRecord, Document, FileDisableEntry,
    FileDisableSet, FileOp, Finding, Fix, Gate, GateSeverity, HashAlgorithm, Impact, Language,
    LintCfgStore, LintContext, LintEngine, LintError, MetadataBlob, Project, PropEntry, PropMap,
    PropValue, RelatedSpan, RunSurface, ScopeAddEntry, ScopeAddMap, ScopeAxis, Severity, Span,
    Suggestion, SuppressionKind, SuppressionMap, SuppressionScope, LINT_CONTRACT_VERSION,
};
pub use manifest::{
    parse_task_uri, validate_deprecated_accounting, validate_structural, AcceptanceBlock,
    ChangeBlock, DeprecatedAccounting, Manifest, ScopeBlock, TaskUriError, ValidationError,
    SCHEMA_MAJOR, TASK_URI_PREFIX,
};
pub use namespace::{Namespace, NamespaceError};
pub use phase::{ManifestSide, Phase};
pub use ref_path::RefPath;
pub use round::{comment_filename, topic_filename, ClosedMeta, ManifestStage, PrMeta, RoundMeta};
pub use slug::{Slug, SlugError};
pub use task::{
    Step, StepPhase, StepRef, StepRefError, TaskClosure, TaskId, TaskIdError, TaskMeta, TaskRefs,
    TaskResolution, TaskState,
};
pub use transition::{ReplanMode, Transition, TransitionValidity, TransitionVerb};
pub use typestate::{
    AdvanceVia, ApplyDocState, ApplySrcState, ApplyVerb, AuthoringStage, BlockedTaskState,
    ClosedTaskState, DeferredTaskState, DeprecatedStage, DocSide, DoneState, FinishVerb,
    InProgressTaskState, LockProof, LockedStage, OpenTaskState, PhaseMarker, PhaseMismatch,
    PlanDocState, PlanSrcState, PlanVerb, ReplanVerb, Side, SideMismatch, SrcSide, Stage,
    TaskStateMarker, TaskTransitionsTo, TopicState, TypedManifest, TypedRound, VerbMarker,
};
pub use verifier::{VerifierAllOf, VerifierAnyOf, VerifierCheck, VerifierKind, VerifierNot};
