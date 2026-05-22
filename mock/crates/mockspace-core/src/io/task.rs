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
//! Slices B + C extend with lifecycle verbs (start/block/defer/close),
//! move semantics with redirect markers, archival to
//! `refs/mock/task-archive`, and step tracking (per spec §16's step
//! sub-structure).

use std::collections::BTreeMap;

use crate::io::ref_tree::{RefTreeReadError, RoundRefTree};
use crate::io::ref_write::RefTreeWriteError;
use crate::io::repo::RepoHandle;
use crate::ref_path::RefPath;
use crate::task::{TaskId, TaskIdError, TaskMeta, TaskState};

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

        let tree = RoundRefTree::from_entries_pub(entries);
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

}
