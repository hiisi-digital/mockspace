//! Task IO executors against the v2 orphan-ref storage substrate.
//!
//! Tasks live on `refs/mock/task/<ns-path>/<slug>` (spec §16). The ref
//! is an orphan commit whose tree carries:
//!
//! - `meta.toml`: the serialized [`TaskMeta`] document.
//! - `.state.<marker>`: a zero-byte marker file naming the current
//!   [`TaskState`] (`.state.open`, `.state.in-progress`, etc.).
//!
//! Slice A of #579 ships:
//!
//! - [`RepoHandle::create_task`]: write a new task ref carrying
//!   `meta.toml` + `.state.open`.
//! - [`RepoHandle::list_tasks`]: enumerate every `refs/mock/task/*`
//!   ref and return the parsed [`TaskId`] set.
//! - [`RepoHandle::show_task`]: read a task ref's tree and parse
//!   `meta.toml` into a [`TaskMeta`].
//!
//! Slice B ships the lifecycle verbs:
//!
//! - [`RepoHandle::start_task`] / [`RepoHandle::block_task`] /
//!   [`RepoHandle::defer_task`]: rotate the state marker file.
//! - [`RepoHandle::close_task`]: rotate the state marker AND write
//!   the `[closure]` block into `meta.toml`.
//!
//! Slice C extends with move semantics (redirect markers), archival
//! to `refs/mock/task-archive`, and step tracking (per spec §16's
//! step sub-structure).

use std::collections::BTreeMap;

use crate::branch_name::BranchName;
use crate::io::ref_tree::{RefTreeReadError, RoundRefTree};
use crate::io::ref_write::RefTreeWriteError;
use crate::io::repo::RepoHandle;
use crate::ref_path::RefPath;
use crate::task::{TaskClosure, TaskId, TaskIdError, TaskMeta, TaskResolution, TaskState};

/// The task-ref namespace prefix shared by every task ref. Used by
/// [`RepoHandle::list_tasks`] to filter the global ref iteration.
const TASK_REF_PREFIX: &str = "refs/mock/task/";

/// Outcome of a successful [`RepoHandle::create_task`] call.
#[derive(Debug, Clone)]
pub struct CreateTaskReport {
    /// The ref that now points at the new orphan commit.
    pub ref_path: RefPath,
    /// The commit OID of the freshly-written ref.
    pub commit_oid: gix::ObjectId,
}

/// Failure modes for [`RepoHandle::create_task`].
#[derive(Debug)]
pub enum CreateTaskError {
    /// A ref already exists at the target path. The caller asked to
    /// create a task whose slug is already taken; the existing ref
    /// is left untouched.
    AlreadyExists { ref_path: String },
    /// The provided [`TaskMeta`] does not serialize as TOML. Surface
    /// for the rare structural-invalid case (e.g. fields that
    /// serialize-derive refuses).
    MetaSerialise(toml::ser::Error),
    /// Resolving the existing ref (the "must not exist" precondition)
    /// failed for a non-not-found reason.
    PreconditionRead(RefTreeReadError),
    /// Writing the new ref's commit failed.
    WriteFailed(RefTreeWriteError),
}

impl core::fmt::Display for CreateTaskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AlreadyExists { ref_path } => {
                write!(f, "task ref `{ref_path}` already exists")
            }
            Self::MetaSerialise(e) => write!(f, "TaskMeta serialise failed: {e}"),
            Self::PreconditionRead(e) => write!(f, "pre-create ref read failed: {e}"),
            Self::WriteFailed(e) => write!(f, "task ref write failed: {e}"),
        }
    }
}

impl std::error::Error for CreateTaskError {}

/// Failure modes for [`RepoHandle::show_task`].
#[derive(Debug)]
pub enum ShowTaskError {
    /// Task ref does not exist.
    NotFound { ref_path: String },
    /// Reading the ref tree failed for a non-not-found reason.
    ReadFailed(RefTreeReadError),
    /// The task tree did not carry a `meta.toml` blob.
    MetaMissing { ref_path: String },
    /// `meta.toml` exists but is not valid UTF-8.
    MetaNotUtf8 { ref_path: String },
    /// `meta.toml` is valid UTF-8 but does not parse as a [`TaskMeta`].
    MetaParse {
        ref_path: String,
        source: toml::de::Error,
    },
}

impl core::fmt::Display for ShowTaskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound { ref_path } => {
                write!(f, "task ref `{ref_path}` not found")
            }
            Self::ReadFailed(e) => write!(f, "task ref read failed: {e}"),
            Self::MetaMissing { ref_path } => {
                write!(f, "task ref `{ref_path}` carries no meta.toml")
            }
            Self::MetaNotUtf8 { ref_path } => {
                write!(f, "task ref `{ref_path}` meta.toml is not valid UTF-8")
            }
            Self::MetaParse { ref_path, source } => {
                write!(f, "task ref `{ref_path}` meta.toml parse failed: {source}")
            }
        }
    }
}

impl std::error::Error for ShowTaskError {}

/// Failure modes for [`RepoHandle::list_tasks`].
#[derive(Debug)]
pub enum ListTasksError {
    /// gix's reference iterator failed at startup or mid-walk.
    GixIter { message: String },
    /// A ref name under `refs/mock/task/` did not parse back to a
    /// valid [`TaskId`]. Indicates external mutation (a hand-pushed
    /// ref with an invalid slug); the listing surfaces the offender
    /// rather than skipping it silently.
    InvalidRef {
        ref_name: String,
        source: TaskIdError,
    },
}

impl core::fmt::Display for ListTasksError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::GixIter { message } => write!(f, "ref iteration failed: {message}"),
            Self::InvalidRef { ref_name, source } => {
                write!(f, "ref `{ref_name}` is not a valid task identifier: {source}")
            }
        }
    }
}

impl std::error::Error for ListTasksError {}

impl RepoHandle {
    /// Create a new task by writing an orphan commit to
    /// `refs/mock/task/<ns-path>/<slug>` carrying `meta.toml` plus
    /// a `.state.open` marker. Refuses if the ref already exists
    /// (use slice B's lifecycle verbs to update existing tasks).
    ///
    /// The caller passes a [`TaskMeta`] that names the same task
    /// identity as `task_id`. Mockspace does not cross-check them
    /// at this layer; that contract is upheld at the CLI / authoring
    /// site.
    ///
    /// Atomicity: the precondition `resolve_ref_oid` is a UX nicety
    /// that maps the common non-racy case to the friendlier
    /// [`CreateTaskError::AlreadyExists`]. The load-bearing atomicity
    /// comes from gix at ref-edit time via `MustNotExist` (encoded as
    /// `expected_current = None` in [`Self::write_round_ref`]). A true
    /// concurrent create where two processes both pass the precondition
    /// surfaces on the loser as
    /// [`CreateTaskError::WriteFailed`] wrapping a non-fast-forward
    /// error, not as `AlreadyExists`.
    pub fn create_task(
        &self,
        task_id: &TaskId,
        meta: &TaskMeta,
    ) -> Result<CreateTaskReport, CreateTaskError> {
        let ref_path = RefPath::task_from_id(task_id);

        // Precondition: the ref must not already exist. Resolve and
        // refuse on success; map RefNotFound to "ok, proceed".
        match self.resolve_ref_oid(&ref_path) {
            Ok(_) => {
                return Err(CreateTaskError::AlreadyExists {
                    ref_path: ref_path.as_str().to_owned(),
                });
            }
            Err(RefTreeReadError::RefNotFound { .. }) => {}
            Err(other) => return Err(CreateTaskError::PreconditionRead(other)),
        }

        let meta_toml = meta
            .to_toml()
            .map_err(CreateTaskError::MetaSerialise)?;

        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        entries.insert("meta.toml".to_owned(), meta_toml.into_bytes());
        entries.insert(TaskState::Open.marker_filename(), Vec::new());

        let tree = RoundRefTree::from_entries(entries);
        let message = format!("task: create `{}`", task_id.as_uri_form());
        // expected_current = None: this is a ref CREATE, no prior
        // commit to CAS against. The precondition check above
        // covers the existence guard.
        let commit_oid = self
            .write_round_ref(&ref_path, &tree, &message, None)
            .map_err(CreateTaskError::WriteFailed)?;

        Ok(CreateTaskReport { ref_path, commit_oid })
    }

    /// Enumerate every `refs/mock/task/*` ref and parse its name into
    /// a [`TaskId`]. Returns the IDs in stable lexicographic ref-name
    /// order (the gix iterator yields refs in that order). Excludes
    /// `refs/mock/task-archive` (a sibling ref, not under the
    /// per-task prefix).
    pub fn list_tasks(&self) -> Result<Vec<TaskId>, ListTasksError> {
        let repo = self.repo();
        let platform = repo
            .references()
            .map_err(|e| ListTasksError::GixIter {
                message: e.to_string(),
            })?;
        let iter = platform
            .prefixed(TASK_REF_PREFIX)
            .map_err(|e| ListTasksError::GixIter {
                message: e.to_string(),
            })?;
        let mut out: Vec<TaskId> = Vec::new();
        for r in iter {
            let r = r.map_err(|e| ListTasksError::GixIter {
                message: e.to_string(),
            })?;
            let name = r.name().as_bstr().to_string();
            // Strip the prefix to get the dotted-path form, then
            // convert path separators back to `::` for TaskId::parse.
            let suffix = match name.strip_prefix(TASK_REF_PREFIX) {
                Some(s) => s,
                None => continue,
            };
            let uri_form = suffix.replace('/', "::");
            let id = TaskId::parse(&uri_form).map_err(|source| ListTasksError::InvalidRef {
                ref_name: name.clone(),
                source,
            })?;
            out.push(id);
        }
        Ok(out)
    }

    /// Read a task ref's `meta.toml` and parse it into a [`TaskMeta`].
    /// Returns [`ShowTaskError::NotFound`] when the ref does not
    /// exist; returns [`ShowTaskError::MetaMissing`] when the ref
    /// exists but its tree lacks `meta.toml` (drift mode, treated as
    /// an error rather than a default-meta fallback).
    pub fn show_task(&self, task_id: &TaskId) -> Result<TaskMeta, ShowTaskError> {
        let ref_path = RefPath::task_from_id(task_id);
        let tree = match self.read_ref_tree(&ref_path) {
            Ok(t) => t,
            Err(RefTreeReadError::RefNotFound { .. }) => {
                return Err(ShowTaskError::NotFound {
                    ref_path: ref_path.as_str().to_owned(),
                });
            }
            Err(other) => return Err(ShowTaskError::ReadFailed(other)),
        };
        let meta_bytes = tree.get("meta.toml").ok_or_else(|| ShowTaskError::MetaMissing {
            ref_path: ref_path.as_str().to_owned(),
        })?;
        let meta_text =
            core::str::from_utf8(meta_bytes).map_err(|_| ShowTaskError::MetaNotUtf8 {
                ref_path: ref_path.as_str().to_owned(),
            })?;
        TaskMeta::from_toml(meta_text).map_err(|source| ShowTaskError::MetaParse {
            ref_path: ref_path.as_str().to_owned(),
            source,
        })
    }
}

// ---------------------------------------------------------------------------
// Slice B: lifecycle verbs (start / block / defer / close).

/// Outcome of a successful state-transition verb.
#[derive(Debug, Clone)]
pub struct TaskTransitionReport {
    /// The ref that now points at the new orphan commit.
    pub ref_path: RefPath,
    /// The commit OID of the freshly-written ref.
    pub commit_oid: gix::ObjectId,
    /// The state observed before the transition.
    pub previous_state: TaskState,
    /// The state the task moved to.
    pub new_state: TaskState,
}

/// Metadata fields the caller supplies when closing a task. The
/// `closed_at` timestamp is filled by the executor itself so
/// concurrent callers cannot drift on it.
///
/// The `closed_branch` field is typed as `Option<BranchName>`: `None`
/// means "no branch named at close time" (e.g. closing a task from
/// outside an active round). The two remaining `String` fields
/// (`closing_phase`, `closing_round_slug`) stay as `String` here
/// pending #595's IO carrier audit; they hold serde-shaped values
/// that the executor collapses into the wire-format `TaskClosure`.
#[derive(Debug, Clone)]
pub struct CloseMetadata {
    /// Why the task closed.
    pub resolution: TaskResolution,
    /// Source-side branch that carried the closing work, or `None`.
    pub closed_branch: Option<BranchName>,
    /// Phase marker at close time (e.g. `apply_src`), or empty.
    pub closing_phase: String,
    /// Round slug that closed this task, or empty.
    pub closing_round_slug: String,
}

/// Failure modes for the four lifecycle verbs.
#[derive(Debug)]
pub enum TaskTransitionError {
    /// Task ref does not exist.
    NotFound { ref_path: String },
    /// Reading the existing ref tree failed for a non-not-found reason.
    ReadFailed(RefTreeReadError),
    /// The task tree did not carry a `meta.toml` blob.
    MetaMissing { ref_path: String },
    /// `meta.toml` exists but is not valid UTF-8.
    MetaNotUtf8 { ref_path: String },
    /// `meta.toml` parse failed.
    MetaParse {
        ref_path: String,
        source: toml::de::Error,
    },
    /// The task tree did not carry a recognisable `.state.<marker>`
    /// file at its root. Indicates external corruption (e.g. a
    /// hand-pushed ref missing the marker).
    StateMarkerMissing { ref_path: String },
    /// Cannot transition out of `Closed`. Lifecycle verbs treat the
    /// closed state as terminal; re-opening a closed task requires
    /// a new task identity.
    Terminal { current: TaskState },
    /// The task is already in the requested state. Surfaces as a
    /// no-op for the caller to render distinctly from a success.
    NoOp { state: TaskState },
    /// Re-serialising `meta.toml` failed (only relevant when the
    /// verb mutates meta, i.e. `close_task`).
    MetaSerialise(toml::ser::Error),
    /// Writing the new ref's commit failed.
    WriteFailed(RefTreeWriteError),
}

impl core::fmt::Display for TaskTransitionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound { ref_path } => write!(f, "task ref `{ref_path}` not found"),
            Self::ReadFailed(e) => write!(f, "task ref read failed: {e}"),
            Self::MetaMissing { ref_path } => {
                write!(f, "task ref `{ref_path}` carries no meta.toml")
            }
            Self::MetaNotUtf8 { ref_path } => {
                write!(f, "task ref `{ref_path}` meta.toml is not valid UTF-8")
            }
            Self::MetaParse { ref_path, source } => {
                write!(f, "task ref `{ref_path}` meta.toml parse failed: {source}")
            }
            Self::StateMarkerMissing { ref_path } => {
                write!(
                    f,
                    "task ref `{ref_path}` carries no `.state.<marker>` file"
                )
            }
            Self::Terminal { current } => {
                write!(f, "task is in terminal state `{current}`; cannot transition")
            }
            Self::NoOp { state } => write!(f, "task already in state `{state}`"),
            Self::MetaSerialise(e) => write!(f, "TaskMeta serialise failed: {e}"),
            Self::WriteFailed(e) => write!(f, "task ref write failed: {e}"),
        }
    }
}

impl std::error::Error for TaskTransitionError {}

/// Outcome of a successful [`RepoHandle::move_task`] call.
#[derive(Debug, Clone)]
pub struct MoveTaskReport {
    /// The ref path that was deleted (source).
    pub from_ref_path: RefPath,
    /// The ref path that now points at the moved task (destination).
    pub to_ref_path: RefPath,
    /// The commit OID of the newly-written destination ref.
    pub commit_oid: gix::ObjectId,
}

/// Failure modes for [`RepoHandle::move_task`].
#[derive(Debug)]
pub enum MoveTaskError {
    /// Source ref does not exist.
    SourceNotFound { ref_path: String },
    /// Destination ref already exists. Move refuses to clobber.
    DestinationExists { ref_path: String },
    /// Source and destination are the same task identifier; no-op moves
    /// are rejected so callers do not mistake them for a real rename.
    SameTaskId,
    /// Reading the source ref tree failed for a non-not-found reason.
    SourceReadFailed(RefTreeReadError),
    /// Resolving the destination ref (the "must not exist" check)
    /// failed for a non-not-found reason.
    DestinationCheckFailed(RefTreeReadError),
    /// The source tree did not carry a `meta.toml` blob.
    MetaMissing { ref_path: String },
    /// `meta.toml` was not valid UTF-8.
    MetaNotUtf8 { ref_path: String },
    /// `meta.toml` did not parse as a [`TaskMeta`].
    MetaParse {
        ref_path: String,
        source: toml::de::Error,
    },
    /// The updated `meta.toml` did not serialize.
    MetaSerialise(toml::ser::Error),
    /// Writing the destination ref failed.
    WriteFailed(RefTreeWriteError),
    /// Deleting the source ref failed after the destination was
    /// successfully written. This leaves both refs present and
    /// pointing at equivalent trees; the caller can retry or repair
    /// manually.
    SourceDeleteFailed { message: String },
}

impl core::fmt::Display for MoveTaskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SourceNotFound { ref_path } => {
                write!(f, "source task ref `{ref_path}` not found")
            }
            Self::DestinationExists { ref_path } => {
                write!(f, "destination task ref `{ref_path}` already exists")
            }
            Self::SameTaskId => f.write_str("source and destination task ids are identical"),
            Self::SourceReadFailed(e) => write!(f, "source ref read failed: {e}"),
            Self::DestinationCheckFailed(e) => {
                write!(f, "destination ref existence check failed: {e}")
            }
            Self::MetaMissing { ref_path } => {
                write!(f, "source ref `{ref_path}` carries no meta.toml")
            }
            Self::MetaNotUtf8 { ref_path } => {
                write!(f, "source ref `{ref_path}` meta.toml is not valid UTF-8")
            }
            Self::MetaParse { ref_path, source } => {
                write!(f, "source ref `{ref_path}` meta.toml parse failed: {source}")
            }
            Self::MetaSerialise(e) => write!(f, "updated meta.toml serialise failed: {e}"),
            Self::WriteFailed(e) => write!(f, "destination ref write failed: {e}"),
            Self::SourceDeleteFailed { message } => {
                write!(f, "source ref delete failed after destination write: {message}")
            }
        }
    }
}

impl std::error::Error for MoveTaskError {}

impl RepoHandle {
    /// Transition a task to `InProgress`.
    pub fn start_task(
        &self,
        task_id: &TaskId,
    ) -> Result<TaskTransitionReport, TaskTransitionError> {
        self.transition_task(task_id, TaskState::InProgress, None)
    }

    /// Transition a task to `Blocked`.
    pub fn block_task(
        &self,
        task_id: &TaskId,
    ) -> Result<TaskTransitionReport, TaskTransitionError> {
        self.transition_task(task_id, TaskState::Blocked, None)
    }

    /// Transition a task to `Deferred`.
    pub fn defer_task(
        &self,
        task_id: &TaskId,
    ) -> Result<TaskTransitionReport, TaskTransitionError> {
        self.transition_task(task_id, TaskState::Deferred, None)
    }

    /// Close a task. Rotates the state marker to `Closed` AND writes
    /// the `[closure]` block into `meta.toml` per spec §16. The
    /// `closed_at` field is filled by the executor with the current
    /// wall-clock time formatted as ISO-8601 UTC.
    pub fn close_task(
        &self,
        task_id: &TaskId,
        metadata: CloseMetadata,
    ) -> Result<TaskTransitionReport, TaskTransitionError> {
        self.transition_task(task_id, TaskState::Closed, Some(metadata))
    }

    /// Move a task from one identifier to another. Reads the source
    /// ref tree, updates `meta.toml` to reflect the destination's
    /// namespace + leaf slug, writes the destination ref, then
    /// deletes the source ref.
    ///
    /// Operates on active task refs (under `refs/mock/task/...`).
    /// Archived tasks are not movable; archive lookup is left to a
    /// future verb if a use case emerges.
    ///
    /// Atomicity: the write + delete is NOT a single git transaction.
    /// On a crash between the destination write and the source delete,
    /// both refs end up present with equivalent trees. The retry path
    /// re-resolves the source (still pointing at the old commit),
    /// observes the destination exists, and surfaces a
    /// [`MoveTaskError::DestinationExists`]. The repair is manual:
    /// delete the duplicate at one end.
    pub fn move_task(
        &self,
        from: &TaskId,
        to: &TaskId,
    ) -> Result<MoveTaskReport, MoveTaskError> {
        if from == to {
            return Err(MoveTaskError::SameTaskId);
        }
        let from_ref = RefPath::task_from_id(from);
        let to_ref = RefPath::task_from_id(to);

        // Source must exist.
        let from_oid = match self.resolve_ref_oid(&from_ref) {
            Ok(oid) => oid,
            Err(RefTreeReadError::RefNotFound { .. }) => {
                return Err(MoveTaskError::SourceNotFound {
                    ref_path: from_ref.as_str().to_owned(),
                });
            }
            Err(other) => return Err(MoveTaskError::SourceReadFailed(other)),
        };

        // Destination must not exist. This pre-check is a UX nicety
        // that surfaces a typed `DestinationExists` error; the actual
        // race-safe guard is `write_round_ref(..., None)` below,
        // which translates to `PreviousValue::MustNotExist` at the
        // gix transaction layer and rejects clobbers atomically. A
        // concurrent move that wins the gix transaction here will
        // surface as `WriteFailed` rather than `DestinationExists`.
        match self.resolve_ref_oid(&to_ref) {
            Ok(_) => {
                return Err(MoveTaskError::DestinationExists {
                    ref_path: to_ref.as_str().to_owned(),
                });
            }
            Err(RefTreeReadError::RefNotFound { .. }) => {}
            Err(other) => return Err(MoveTaskError::DestinationCheckFailed(other)),
        }

        // Read source tree.
        let tree = self
            .read_ref_tree(&from_ref)
            .map_err(MoveTaskError::SourceReadFailed)?;
        let meta_bytes = tree
            .get("meta.toml")
            .ok_or_else(|| MoveTaskError::MetaMissing {
                ref_path: from_ref.as_str().to_owned(),
            })?;
        let meta_text =
            core::str::from_utf8(meta_bytes).map_err(|_| MoveTaskError::MetaNotUtf8 {
                ref_path: from_ref.as_str().to_owned(),
            })?;
        let mut meta: TaskMeta =
            TaskMeta::from_toml(meta_text).map_err(|source| MoveTaskError::MetaParse {
                ref_path: from_ref.as_str().to_owned(),
                source,
            })?;

        // Update meta.toml's slug + namespace to match destination.
        meta.slug = to.slug().as_str().to_owned();
        meta.namespace = to
            .namespace()
            .map(|ns| ns.as_ref_path())
            .unwrap_or_default();
        let new_meta_toml = meta.to_toml().map_err(MoveTaskError::MetaSerialise)?;

        // Build the destination tree by cloning the source and
        // replacing meta.toml.
        let mut entries: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (name, bytes) in tree.iter() {
            if name == "meta.toml" {
                continue;
            }
            entries.insert(name.to_owned(), bytes.to_vec());
        }
        entries.insert("meta.toml".to_owned(), new_meta_toml.into_bytes());
        let new_tree = RoundRefTree::from_entries(entries);

        // Write destination ref (no prior commit; this is a create).
        let message = format!(
            "task: move `{}` -> `{}`",
            from.as_uri_form(),
            to.as_uri_form()
        );
        let commit_oid = self
            .write_round_ref(&to_ref, &new_tree, &message, None)
            .map_err(MoveTaskError::WriteFailed)?;

        // Delete source ref. If this fails, the destination already
        // landed; surface the error so the caller can repair manually.
        self.delete_task_ref(&from_ref, from_oid)
            .map_err(|message| MoveTaskError::SourceDeleteFailed { message })?;

        Ok(MoveTaskReport {
            from_ref_path: from_ref,
            to_ref_path: to_ref,
            commit_oid,
        })
    }

    /// Delete a task ref with CAS on the current OID. Inline gix call
    /// because the typed error from [`move_task`] needs more structure
    /// than the archive-side helper's `Box<dyn Error>` return.
    fn delete_task_ref(
        &self,
        ref_path: &RefPath,
        expected_oid: gix::ObjectId,
    ) -> Result<(), String> {
        use gix::refs::transaction::{Change, PreviousValue, RefEdit, RefLog};
        let repo = self.repo();
        let name = ref_path
            .as_str()
            .try_into()
            .map_err(|e: gix::refs::name::Error| e.to_string())?;
        let edit = RefEdit {
            change: Change::Delete {
                expected: PreviousValue::ExistingMustMatch(gix::refs::Target::Object(
                    expected_oid,
                )),
                log: RefLog::AndReference,
            },
            name,
            deref: false,
        };
        repo.edit_reference(edit)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Shared executor for the four lifecycle verbs. Reads the task
    /// tree, validates the transition, mutates the tree (rotate
    /// state marker; for `close`, also splice in the closure block),
    /// writes back with CAS against the current commit OID.
    fn transition_task(
        &self,
        task_id: &TaskId,
        new_state: TaskState,
        close_metadata: Option<CloseMetadata>,
    ) -> Result<TaskTransitionReport, TaskTransitionError> {
        let ref_path = RefPath::task_from_id(task_id);
        let current_oid = match self.resolve_ref_oid(&ref_path) {
            Ok(oid) => oid,
            Err(RefTreeReadError::RefNotFound { .. }) => {
                return Err(TaskTransitionError::NotFound {
                    ref_path: ref_path.as_str().to_owned(),
                });
            }
            Err(other) => return Err(TaskTransitionError::ReadFailed(other)),
        };
        let tree = self.read_ref_tree(&ref_path).map_err(|e| match e {
            RefTreeReadError::RefNotFound { .. } => TaskTransitionError::NotFound {
                ref_path: ref_path.as_str().to_owned(),
            },
            other => TaskTransitionError::ReadFailed(other),
        })?;

        // Locate the current state marker. Any prefix `.state.` file
        // counts; we strip exactly one such entry and replace it with
        // the new marker. Drift (multiple state markers) is treated
        // as "first match wins" for the previous-state inference,
        // and ALL prefix matches get stripped on rewrite so the new
        // tree carries exactly one.
        let mut new_entries: BTreeMap<String, Vec<u8>> = tree
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_vec()))
            .collect();
        let prior_state = infer_state_from_entries(&new_entries).ok_or_else(|| {
            TaskTransitionError::StateMarkerMissing {
                ref_path: ref_path.as_str().to_owned(),
            }
        })?;
        if matches!(prior_state, TaskState::Closed) {
            return Err(TaskTransitionError::Terminal { current: prior_state });
        }
        if prior_state == new_state {
            return Err(TaskTransitionError::NoOp { state: prior_state });
        }
        // Strip every `.state.*` entry at the tree root; rewrite a
        // single fresh marker for the target state. Subdir entries
        // beginning with `.state.` are not at the root and are
        // preserved (defensive; the on-disk shape does not put state
        // markers under subdirs today).
        new_entries.retain(|k, _| !is_root_state_marker(k));
        new_entries.insert(new_state.marker_filename(), Vec::new());

        // For `close`, also splice the closure block into meta.toml.
        if let Some(meta) = close_metadata.clone() {
            let meta_bytes = new_entries.get("meta.toml").ok_or_else(|| {
                TaskTransitionError::MetaMissing {
                    ref_path: ref_path.as_str().to_owned(),
                }
            })?;
            let meta_text = core::str::from_utf8(meta_bytes).map_err(|_| {
                TaskTransitionError::MetaNotUtf8 {
                    ref_path: ref_path.as_str().to_owned(),
                }
            })?;
            let mut parsed: TaskMeta = TaskMeta::from_toml(meta_text).map_err(|source| {
                TaskTransitionError::MetaParse {
                    ref_path: ref_path.as_str().to_owned(),
                    source,
                }
            })?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            parsed.closure = Some(TaskClosure {
                resolution: meta.resolution,
                closed_at: format_iso8601(now),
                // TaskClosure's wire-format field stays String per
                // #595's pending IO carrier retyping. Collapse the
                // typed input at the boundary.
                closed_branch: meta
                    .closed_branch
                    .map(|b| b.to_string())
                    .unwrap_or_default(),
                closing_phase: meta.closing_phase,
                closing_round_slug: meta.closing_round_slug,
            });
            let new_toml = parsed
                .to_toml()
                .map_err(TaskTransitionError::MetaSerialise)?;
            new_entries.insert("meta.toml".to_owned(), new_toml.into_bytes());
        }

        let new_tree = RoundRefTree::from_entries(new_entries);
        let message = match (close_metadata.is_some(), new_state) {
            (true, TaskState::Closed) => {
                format!("task: close `{}`", task_id.as_uri_form())
            }
            (_, target) => format!(
                "task: transition `{}` to `{target}`",
                task_id.as_uri_form()
            ),
        };
        let commit_oid = self
            .write_round_ref(&ref_path, &new_tree, &message, Some(current_oid))
            .map_err(TaskTransitionError::WriteFailed)?;
        Ok(TaskTransitionReport {
            ref_path,
            commit_oid,
            previous_state: prior_state,
            new_state,
        })
    }
}

/// Return the first recognised `.state.<marker>` filename at the
/// tree root, decoded into a [`TaskState`]. Returns `None` when no
/// such marker file exists.
fn infer_state_from_entries(entries: &BTreeMap<String, Vec<u8>>) -> Option<TaskState> {
    for key in entries.keys() {
        if let Some(marker) = key.strip_prefix(".state.") {
            // Only consider root-level markers, not subdir entries
            // like `subdir/.state.foo`.
            if key == &format!(".state.{marker}") {
                if let Some(state) = TaskState::from_marker(marker) {
                    return Some(state);
                }
            }
        }
    }
    None
}

/// True when `name` is a root-level `.state.<marker>` file.
fn is_root_state_marker(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix(".state.") {
        !rest.is_empty() && !rest.contains('/')
    } else {
        false
    }
}

/// Format a unix epoch as a minimal ISO-8601 UTC string. Delegates
/// to [`crate::iso8601::Iso8601Utc::from_unix_secs`]; kept as a thin
/// wrapper here because the close-task body writes into
/// `TaskClosure.closed_at: String` (still a wire-format String per
/// #595's deferred IO carrier retyping). Once #595 lands, the
/// `TaskClosure` field becomes typed and this shim collapses.
fn format_iso8601(unix_secs: u64) -> String {
    crate::iso8601::Iso8601Utc::from_unix_secs(unix_secs)
        .as_str()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_repo(dir: &std::path::Path) {
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir)
            .status()
            .expect("git init runs");
        assert!(status.success());
    }

    fn meta(slug: &str, ns: &str) -> TaskMeta {
        TaskMeta {
            mockspace_version: "v2".to_owned(),
            slug: slug.to_owned(),
            namespace: ns.to_owned(),
            title: format!("Task {slug}"),
            created: "2026-05-22T12:00:00Z".to_owned(),
            priority: None,
            group: None,
            steps: Default::default(),
            refs: Default::default(),
            closure: None,
        }
    }

    #[test]
    fn create_then_show_top_level_task() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = TaskId::parse("migrate-to-codeberg").expect("parse");

        let report = handle
            .create_task(&id, &meta("migrate-to-codeberg", ""))
            .expect("create");
        assert_eq!(
            report.ref_path.as_str(),
            "refs/mock/task/migrate-to-codeberg"
        );

        let loaded = handle.show_task(&id).expect("show");
        assert_eq!(loaded.slug, "migrate-to-codeberg");
        assert_eq!(loaded.title, "Task migrate-to-codeberg");
    }

    #[test]
    fn create_then_show_namespaced_task() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = TaskId::parse("compiler::ir::lower-pass").expect("parse");

        let report = handle
            .create_task(&id, &meta("lower-pass", "compiler/ir"))
            .expect("create");
        assert_eq!(
            report.ref_path.as_str(),
            "refs/mock/task/compiler/ir/lower-pass"
        );

        let loaded = handle.show_task(&id).expect("show");
        assert_eq!(loaded.slug, "lower-pass");
    }

    #[test]
    fn create_refuses_when_ref_exists() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = TaskId::parse("duplicate-slug").expect("parse");

        handle
            .create_task(&id, &meta("duplicate-slug", ""))
            .expect("first create");
        let err = handle
            .create_task(&id, &meta("duplicate-slug", ""))
            .expect_err("second create must refuse");
        assert!(matches!(err, CreateTaskError::AlreadyExists { .. }));
    }

    #[test]
    fn show_errors_when_ref_missing() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = TaskId::parse("nonexistent").expect("parse");
        let err = handle.show_task(&id).expect_err("show must refuse");
        assert!(matches!(err, ShowTaskError::NotFound { .. }));
    }

    #[test]
    fn list_returns_empty_on_fresh_repo() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let tasks = handle.list_tasks().expect("list");
        assert!(tasks.is_empty());
    }

    #[test]
    fn list_returns_created_tasks_in_ref_name_order() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");

        // Insert in a non-sorted order; iterator yields lexicographic.
        for id_str in ["zeta", "alpha", "compiler::lower", "beta"] {
            let id = TaskId::parse(id_str).expect("parse");
            handle
                .create_task(&id, &meta(id.slug().as_str(), ""))
                .expect("create");
        }

        let tasks = handle.list_tasks().expect("list");
        let names: Vec<String> = tasks.iter().map(|t| t.as_uri_form()).collect();
        assert_eq!(names, vec!["alpha", "beta", "compiler::lower", "zeta"]);
    }

    // -----------------------------------------------------------------
    // Slice B: lifecycle verbs.

    fn setup_open_task(handle: &RepoHandle, id_str: &str) -> TaskId {
        let id = TaskId::parse(id_str).expect("parse");
        handle
            .create_task(&id, &meta(id.slug().as_str(), ""))
            .expect("create");
        id
    }

    fn close_meta() -> CloseMetadata {
        CloseMetadata {
            resolution: TaskResolution::Completed,
            closed_branch: Some(BranchName::new("feat/test").expect("valid branch")),
            closing_phase: "apply_src".to_owned(),
            closing_round_slug: "test-round".to_owned(),
        }
    }

    #[test]
    fn start_transitions_open_to_in_progress() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "alpha");

        let report = handle.start_task(&id).expect("start");
        assert_eq!(report.previous_state, TaskState::Open);
        assert_eq!(report.new_state, TaskState::InProgress);

        // Show confirms the new state is persisted via the marker
        // file rotation. (Inferring from tree, not from meta, since
        // meta does not carry state.)
        let read_tree = handle.read_ref_tree(&report.ref_path).expect("read");
        let entries: BTreeMap<String, Vec<u8>> = read_tree
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_vec()))
            .collect();
        assert_eq!(infer_state_from_entries(&entries), Some(TaskState::InProgress));
    }

    #[test]
    fn block_transitions_open_to_blocked() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "beta");

        let report = handle.block_task(&id).expect("block");
        assert_eq!(report.new_state, TaskState::Blocked);
    }

    #[test]
    fn defer_transitions_open_to_deferred() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "gamma");

        let report = handle.defer_task(&id).expect("defer");
        assert_eq!(report.new_state, TaskState::Deferred);
    }

    #[test]
    fn close_writes_closure_block_into_meta() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "delta");

        let report = handle.close_task(&id, close_meta()).expect("close");
        assert_eq!(report.new_state, TaskState::Closed);

        let loaded = handle.show_task(&id).expect("show");
        let closure = loaded.closure.expect("closure block written");
        assert_eq!(closure.resolution, TaskResolution::Completed);
        assert_eq!(closure.closed_branch, "feat/test");
        assert_eq!(closure.closing_phase, "apply_src");
        assert_eq!(closure.closing_round_slug, "test-round");
        // The executor fills closed_at; assert it parses as ISO-8601
        // shape rather than pinning a specific value (depends on
        // wall-clock at test time).
        assert!(closure.closed_at.contains('T') && closure.closed_at.ends_with('Z'));
    }

    #[test]
    fn start_then_close_chains_cleanly() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "epsilon");

        handle.start_task(&id).expect("start");
        let report = handle.close_task(&id, close_meta()).expect("close");
        assert_eq!(report.previous_state, TaskState::InProgress);
        assert_eq!(report.new_state, TaskState::Closed);
    }

    #[test]
    fn transition_refuses_when_task_missing() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = TaskId::parse("nonexistent").expect("parse");
        let err = handle.start_task(&id).expect_err("start");
        assert!(matches!(err, TaskTransitionError::NotFound { .. }));
    }

    #[test]
    fn transition_refuses_no_op_when_state_already_target() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "zeta");

        handle.start_task(&id).expect("start");
        // Already in InProgress; second start is a no-op error.
        let err = handle.start_task(&id).expect_err("second start");
        assert!(matches!(
            err,
            TaskTransitionError::NoOp {
                state: TaskState::InProgress
            }
        ));
    }

    #[test]
    fn close_is_terminal() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "eta");

        handle.close_task(&id, close_meta()).expect("close");
        // Cannot transition out of Closed.
        let err = handle.start_task(&id).expect_err("start after close");
        assert!(matches!(
            err,
            TaskTransitionError::Terminal {
                current: TaskState::Closed
            }
        ));
    }

    #[test]
    fn transition_strips_old_marker_and_writes_exactly_one() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "theta");

        let report = handle.block_task(&id).expect("block");
        let tree = handle.read_ref_tree(&report.ref_path).expect("read");
        let marker_count = tree
            .iter()
            .filter(|(k, _)| is_root_state_marker(k))
            .count();
        assert_eq!(marker_count, 1, "exactly one .state.<marker> must remain");
        let entries: BTreeMap<String, Vec<u8>> = tree
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_vec()))
            .collect();
        assert_eq!(infer_state_from_entries(&entries), Some(TaskState::Blocked));
    }

    // -----------------------------------------------------------------
    // Slice B: move_task.

    #[test]
    fn move_top_level_to_namespaced() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let from = setup_open_task(&handle, "migrate");
        let to = TaskId::parse("workspace::migrate").expect("parse");

        let report = handle.move_task(&from, &to).expect("move");

        // Source ref gone.
        assert!(matches!(
            handle.resolve_ref_oid(&report.from_ref_path),
            Err(RefTreeReadError::RefNotFound { .. })
        ));
        // Destination ref present and carries updated meta.
        let dest_meta = handle.show_task(&to).expect("show");
        assert_eq!(dest_meta.slug, "migrate");
        assert_eq!(dest_meta.namespace, "workspace");
    }

    #[test]
    fn move_namespaced_to_top_level() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let from = TaskId::parse("compiler::lower").expect("parse");
        handle
            .create_task(&from, &meta("lower", "compiler"))
            .expect("create");
        let to = TaskId::parse("lower").expect("parse");

        handle.move_task(&from, &to).expect("move");

        let dest_meta = handle.show_task(&to).expect("show");
        assert_eq!(dest_meta.slug, "lower");
        assert_eq!(dest_meta.namespace, "");
    }

    #[test]
    fn move_refuses_same_id() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let id = setup_open_task(&handle, "same");

        assert!(matches!(
            handle.move_task(&id, &id),
            Err(MoveTaskError::SameTaskId)
        ));
    }

    #[test]
    fn move_refuses_missing_source() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let from = TaskId::parse("never-existed").expect("parse");
        let to = TaskId::parse("destination").expect("parse");

        assert!(matches!(
            handle.move_task(&from, &to),
            Err(MoveTaskError::SourceNotFound { .. })
        ));
    }

    #[test]
    fn move_refuses_existing_destination() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let from = setup_open_task(&handle, "from-task");
        let to = setup_open_task(&handle, "to-task");

        assert!(matches!(
            handle.move_task(&from, &to),
            Err(MoveTaskError::DestinationExists { .. })
        ));
    }

    #[test]
    fn move_preserves_state_marker() {
        let dir = TempDir::new().unwrap();
        init_repo(dir.path());
        let handle = RepoHandle::open(dir.path()).expect("open");
        let from = setup_open_task(&handle, "movable");
        handle.block_task(&from).expect("block");

        let to = TaskId::parse("archived::movable").expect("parse");
        handle.move_task(&from, &to).expect("move");

        let tree = handle
            .read_ref_tree(&RefPath::task_from_id(&to))
            .expect("read");
        let entries: BTreeMap<String, Vec<u8>> = tree
            .iter()
            .map(|(k, v)| (k.to_owned(), v.to_vec()))
            .collect();
        assert_eq!(infer_state_from_entries(&entries), Some(TaskState::Blocked));
    }
}
