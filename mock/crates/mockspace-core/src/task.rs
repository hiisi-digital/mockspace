//! Task data layer (spec §16, §26).
//!
//! A task is a work item with identity, a lifecycle, and content. Identity is
//! `<namespace>#<slug>`; lifecycle moves through open / in-progress / blocked
//! / deferred / closed. Tasks live on `refs/mock/task/<ns-path>/<slug>` until
//! archival, when they move into the unified `refs/mock/task-archive`.
//!
//! This module ships the data types and TOML schema. Git plumbing (creating
//! the task ref, archiving on close) lives elsewhere; the types here flow
//! through it.

use core::fmt;
use core::hash::Hash;

use serde::{Deserialize, Serialize};

use crate::namespace::Namespace;
use crate::slug::{Slug, SlugError};

/// A task's lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskState {
    /// Exists; no one is actively working on it.
    Open,
    /// Being worked on.
    InProgress,
    /// Waiting on something external.
    Blocked,
    /// Intentionally postponed.
    Deferred,
    /// No longer active.
    Closed,
}

impl TaskState {
    /// The kebab-case marker (`open`, `in-progress`, `blocked`, `deferred`, `closed`).
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::InProgress => "in-progress",
            Self::Blocked => "blocked",
            Self::Deferred => "deferred",
            Self::Closed => "closed",
        }
    }

    /// The state-marker filename at the task ref tree root: `.state.<marker>`.
    pub fn marker_filename(self) -> String {
        format!(".state.{}", self.marker())
    }

    /// Inverse of [`Self::marker`].
    pub fn from_marker(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "in-progress" => Self::InProgress,
            "blocked" => Self::Blocked,
            "deferred" => Self::Deferred,
            "closed" => Self::Closed,
            _ => return None,
        })
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.marker())
    }
}

/// Why a closed task closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskResolution {
    /// Work shipped.
    Completed,
    /// Cancelled before ship.
    Cancelled,
    /// Replaced by another task.
    Superseded,
    /// Not going to be done by design choice.
    Wontfix,
}

impl TaskResolution {
    /// The kebab-case marker stored in `[closure].resolution`.
    pub const fn marker(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Superseded => "superseded",
            Self::Wontfix => "wontfix",
        }
    }
}

/// Which manifest side(s) a step operates on.
///
/// Distinct from [`crate::phase::ManifestSide`]: a step may live across both
/// the doc and src manifests, in which case the step's `phase` tag is
/// `doc+src`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepPhase {
    /// Step is claimed only by doc manifests.
    Doc,
    /// Step is claimed only by src manifests.
    Src,
    /// Step may be claimed by either manifest.
    #[serde(rename = "doc+src")]
    DocSrc,
}

/// One sub-task on a parent task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// Short description of what this step covers.
    pub description: String,
    /// Which manifest side(s) this step belongs to.
    pub phase:       StepPhase,
    /// Current state of this step.
    pub state:       TaskState,
}

/// Cross-references between tasks. Each entry is the prose form
/// `<ns-path>#<slug>` (or `mock://task/<ns-path>::<slug>` URI form).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRefs {
    /// Tasks this task blocks.
    #[serde(default)]
    pub blocks:     Vec<String>,
    /// Tasks this task is blocked by.
    #[serde(default)]
    pub blocked_by: Vec<String>,
    /// Tasks loosely related but not blocking.
    #[serde(default)]
    pub relates_to: Vec<String>,
}

impl TaskRefs {
    /// True when every cross-reference list is empty.
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty() && self.blocked_by.is_empty() && self.relates_to.is_empty()
    }
}

/// Closure metadata: what happened when the task closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskClosure {
    /// Why the task closed.
    pub resolution:         TaskResolution,
    /// ISO-8601 close timestamp.
    pub closed_at:          String,
    /// The source-side branch that carried the closing work.
    pub closed_branch:      String,
    /// The phase marker at close time (e.g. `apply_src`).
    pub closing_phase:      String,
    /// The round slug that closed this task.
    pub closing_round_slug: String,
}

/// The task's `meta.toml` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskMeta {
    /// Schema version.
    pub mockspace_version: String,
    /// Leaf identifier.
    pub slug:              String,
    /// Ref-path namespace form (e.g. `compiler/ir/lower-pass`).
    pub namespace:         String,
    /// One-line human-facing title.
    pub title:             String,
    /// ISO-8601 creation timestamp.
    pub created:           String,
    /// Coarse priority label (P0, P1, P2; project-defined).
    #[serde(default)]
    pub priority:          Option<String>,
    /// Optional grouping label.
    #[serde(default)]
    pub group:             Option<String>,
    /// Sub-tasks keyed by step name.
    #[serde(default)]
    pub steps:             std::collections::BTreeMap<String, Step>,
    /// Cross-references to other tasks.
    #[serde(default, rename = "refs", skip_serializing_if = "TaskRefs::is_empty")]
    pub refs:              TaskRefs,
    /// Closure block; present only when state == Closed.
    #[serde(default, rename = "closure")]
    pub closure:           Option<TaskClosure>,
}

impl TaskMeta {
    /// Serialize as TOML.
    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Parse from TOML.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

/// The canonical mockspace task identifier: composite of namespace
/// segments + leaf slug. Plays the **stable identifier** role for
/// [`Task`] entity: impls [`RefTo<Task>`] only, NOT [`NamedRefTo<Task>`].
///
/// The composite structure (segments + leaf) IS TaskId's canonical
/// identity. The `as_uri_form()` and `as_ref_path()` rendered strings
/// are serializations of the composite, not the canonical name.
/// Consumers needing a flat human-readable name reach for [`Slug`]
/// (which impls `NamedRefTo<Task>`). Consumers needing the structural
/// view reach for inherent methods on the concrete `TaskId`.
///
/// Identity is a path of slug-shaped segments where the final
/// segment is the leaf slug and preceding segments (if any) form
/// the namespace. The same shape renders two ways:
///
/// - URI / prose form: segments joined with `::`
///   (e.g. `compiler::ir::lower-pass`)
/// - Ref form: segments joined with `/`
///   (e.g. `compiler/ir/lower-pass`)
///
/// **Single-segment task identifiers are permitted.** A bare
/// `migrate-to-codeberg` is a valid identifier with namespace empty
/// and slug `migrate-to-codeberg`. Spec §16's convention note
/// recommends namespacing for tooling UX but mockspace does not
/// police away the no-namespace case.
///
/// The `#` character is reserved for step references (see
/// [`StepRef`]) and is never part of task identity itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaskId {
    /// Namespace segments. Empty for top-level (no-namespace) tasks.
    namespace_segments: Vec<Slug>,
    /// Leaf slug.
    slug:               Slug,
}

/// Why a [`TaskId`] string rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskIdError {
    /// Identifier contains `#`; that character is reserved for step refs.
    ContainsStepSeparator,
    /// Identifier was empty.
    Empty,
    /// A `::` appears with no content on one side (leading, trailing, or doubled).
    EmptySegment {
        position: usize,
    },
    /// A segment failed slug validation; `index` counts from 0.
    InvalidSegment {
        index: usize,
        error: SlugError,
    },
}

impl TaskId {
    /// Construct from validated parts: a possibly-empty list of namespace
    /// segments plus a leaf slug.
    pub fn new(namespace_segments: Vec<Slug>, slug: Slug) -> Self {
        Self {
            namespace_segments,
            slug,
        }
    }

    /// Construct from a namespace plus slug. Convenience for callers that
    /// already have a [`Namespace`] (which is non-empty by construction).
    pub fn with_namespace(namespace: Namespace, slug: Slug) -> Self {
        Self {
            namespace_segments: namespace.segments().to_vec(),
            slug,
        }
    }

    /// Parse the URI / prose form `<seg>::<seg>::...::<slug>`. The final
    /// segment becomes the slug; any preceding segments form the namespace.
    /// A single segment yields a top-level (no-namespace) task identity.
    /// Rejects any `#` character.
    pub fn parse(input: &str) -> Result<Self, TaskIdError> {
        if input.contains('#') {
            return Err(TaskIdError::ContainsStepSeparator);
        }
        if input.is_empty() {
            return Err(TaskIdError::Empty);
        }
        let mut segments: Vec<Slug> = Vec::new();
        let mut byte_pos: usize = 0;
        for (index, raw) in input.split("::").enumerate() {
            if raw.is_empty() {
                return Err(TaskIdError::EmptySegment {
                    position: byte_pos,
                });
            }
            let slug = Slug::new(raw).map_err(|error| {
                TaskIdError::InvalidSegment {
                    index,
                    error,
                }
            })?;
            segments.push(slug);
            byte_pos += raw.len() + 2;
        }
        let slug = segments.pop().expect("non-empty checked above");
        Ok(Self {
            namespace_segments: segments,
            slug,
        })
    }

    /// The namespace segments (possibly empty for top-level tasks).
    pub fn namespace_segments(&self) -> &[Slug] {
        &self.namespace_segments
    }

    /// The namespace as a [`Namespace`] value, if any. Returns `None`
    /// for top-level (no-namespace) tasks.
    pub fn namespace(&self) -> Option<Namespace> {
        Namespace::from_segments(self.namespace_segments.clone())
    }

    /// The leaf slug.
    pub fn slug(&self) -> &Slug {
        &self.slug
    }

    /// True for single-segment task identifiers (no namespace).
    pub fn is_top_level(&self) -> bool {
        self.namespace_segments.is_empty()
    }

    /// URI / prose form: segments joined with `::`.
    pub fn as_uri_form(&self) -> String {
        if self.namespace_segments.is_empty() {
            self.slug.as_str().to_owned()
        } else {
            let mut out = String::new();
            for seg in &self.namespace_segments {
                out.push_str(seg.as_str());
                out.push_str("::");
            }
            out.push_str(self.slug.as_str());
            out
        }
    }

    /// Ref-path form: segments joined with `/`.
    pub fn as_ref_path(&self) -> String {
        if self.namespace_segments.is_empty() {
            self.slug.as_str().to_owned()
        } else {
            let mut out = String::new();
            for seg in &self.namespace_segments {
                out.push_str(seg.as_str());
                out.push('/');
            }
            out.push_str(self.slug.as_str());
            out
        }
    }
}

impl crate::identity::RefTo<crate::entity::Task> for TaskId {}

impl fmt::Display for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_uri_form())
    }
}

impl fmt::Display for TaskIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContainsStepSeparator => f.write_str(
                "task identifier must not contain `#`; that separator is reserved for step refs",
            ),
            Self::Empty => f.write_str("task identifier is empty"),
            Self::EmptySegment {
                position,
            } => {
                write!(
                    f,
                    "empty segment in task identifier at byte position {position}"
                )
            },
            Self::InvalidSegment {
                index,
                error,
            } => {
                write!(f, "segment {index} is not a valid slug: {error}")
            },
        }
    }
}

impl std::error::Error for TaskIdError {}

/// A reference to a specific step within a task.
///
/// Renders as `<task>#<step>` where `<task>` is the task's URI form and
/// `<step>` is the step's key from `meta.toml`'s `[steps.<key>]` table.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StepRef {
    task: TaskId,
    step: String,
}

/// Why a step-ref string rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepRefError {
    /// No `#` separator between task path and step key.
    MissingSeparator,
    /// More than one `#` separator.
    DuplicateSeparator,
    /// Step key was empty.
    EmptyStep,
    /// Task half rejected.
    InvalidTask(TaskIdError),
}

impl StepRef {
    /// Construct from validated parts. `step` is stored verbatim; the
    /// canonical step-key shape comes from `meta.toml`'s `[steps.<key>]`
    /// table and is project-defined (typically snake_case).
    pub fn new(task: TaskId, step: String) -> Self {
        Self {
            task,
            step,
        }
    }

    /// Parse `<task>#<step>`.
    pub fn parse(input: &str) -> Result<Self, StepRefError> {
        let mut parts = input.splitn(2, '#');
        let task_part = parts.next().ok_or(StepRefError::MissingSeparator)?;
        let step_part = parts.next().ok_or(StepRefError::MissingSeparator)?;
        if step_part.contains('#') {
            return Err(StepRefError::DuplicateSeparator);
        }
        if step_part.is_empty() {
            return Err(StepRefError::EmptyStep);
        }
        let task = TaskId::parse(task_part).map_err(StepRefError::InvalidTask)?;
        Ok(Self {
            task,
            step: step_part.to_owned(),
        })
    }

    /// The task half.
    pub fn task(&self) -> &TaskId {
        &self.task
    }

    /// The step key.
    pub fn step(&self) -> &str {
        &self.step
    }
}

impl fmt::Display for StepRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.task, self.step)
    }
}

impl fmt::Display for StepRefError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator => {
                f.write_str("step reference missing `#` between task path and step key")
            },
            Self::DuplicateSeparator => f.write_str("step reference contains more than one `#`"),
            Self::EmptyStep => f.write_str("step key is empty after `#`"),
            Self::InvalidTask(e) => write!(f, "task half invalid: {e}"),
        }
    }
}

impl std::error::Error for StepRefError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_marker_roundtrips() {
        for state in [
            TaskState::Open,
            TaskState::InProgress,
            TaskState::Blocked,
            TaskState::Deferred,
            TaskState::Closed,
        ] {
            assert_eq!(TaskState::from_marker(state.marker()), Some(state));
        }
    }

    #[test]
    fn state_marker_filename_shape() {
        assert_eq!(TaskState::Open.marker_filename(), ".state.open");
        assert_eq!(
            TaskState::InProgress.marker_filename(),
            ".state.in-progress"
        );
        assert_eq!(TaskState::Closed.marker_filename(), ".state.closed");
    }

    #[test]
    fn state_from_marker_rejects_unknown() {
        assert_eq!(TaskState::from_marker(""), None);
        assert_eq!(TaskState::from_marker("OPEN"), None);
        assert_eq!(TaskState::from_marker("in_progress"), None);
    }

    #[test]
    fn task_id_parse_single_segment_top_level() {
        let id = TaskId::parse("migrate-to-codeberg").unwrap();
        assert!(id.is_top_level());
        assert!(id.namespace().is_none());
        assert_eq!(id.namespace_segments().len(), 0);
        assert_eq!(id.slug().as_str(), "migrate-to-codeberg");
        assert_eq!(id.as_uri_form(), "migrate-to-codeberg");
        assert_eq!(id.as_ref_path(), "migrate-to-codeberg");
    }

    #[test]
    fn task_id_parse_two_segments() {
        let id = TaskId::parse("workspace::migrate-to-codeberg").unwrap();
        assert!(!id.is_top_level());
        assert_eq!(id.namespace().unwrap().as_uri_form(), "workspace");
        assert_eq!(id.slug().as_str(), "migrate-to-codeberg");
        assert_eq!(id.as_uri_form(), "workspace::migrate-to-codeberg");
        assert_eq!(id.as_ref_path(), "workspace/migrate-to-codeberg");
    }

    #[test]
    fn task_id_parse_nested() {
        let id = TaskId::parse("compiler::ir::structural-robust-ir").unwrap();
        assert_eq!(id.namespace().unwrap().as_ref_path(), "compiler/ir");
        assert_eq!(id.slug().as_str(), "structural-robust-ir");
        assert_eq!(id.as_ref_path(), "compiler/ir/structural-robust-ir");
    }

    #[test]
    fn task_id_parse_deeper() {
        let id = TaskId::parse("compiler::ir::lower-pass::define-grammar").unwrap();
        assert_eq!(
            id.namespace().unwrap().as_ref_path(),
            "compiler/ir/lower-pass"
        );
        assert_eq!(id.slug().as_str(), "define-grammar");
    }

    #[test]
    fn task_id_rejects_step_separator() {
        assert_eq!(
            TaskId::parse("workspace#migrate"),
            Err(TaskIdError::ContainsStepSeparator)
        );
        assert_eq!(
            TaskId::parse("compiler::ir::lower-pass#define-grammar"),
            Err(TaskIdError::ContainsStepSeparator)
        );
    }

    #[test]
    fn task_id_rejects_empty() {
        assert_eq!(TaskId::parse(""), Err(TaskIdError::Empty));
    }

    #[test]
    fn task_id_rejects_empty_segment() {
        match TaskId::parse("::foo") {
            Err(TaskIdError::EmptySegment {
                ..
            }) => {},
            other => panic!("expected EmptySegment, got {other:?}"),
        }
        match TaskId::parse("foo::") {
            Err(TaskIdError::EmptySegment {
                ..
            }) => {},
            other => panic!("expected EmptySegment, got {other:?}"),
        }
    }

    #[test]
    fn task_id_rejects_invalid_segment() {
        match TaskId::parse("Bad::ns::slug") {
            Err(TaskIdError::InvalidSegment {
                index: 0,
                ..
            }) => {},
            other => panic!("expected InvalidSegment(0), got {other:?}"),
        }
    }

    #[test]
    fn step_ref_parse_simple() {
        let r = StepRef::parse("compiler::ir::structural-robust-ir#define-grammar").unwrap();
        assert_eq!(r.task().as_uri_form(), "compiler::ir::structural-robust-ir");
        assert_eq!(r.step(), "define-grammar");
        assert_eq!(
            r.to_string(),
            "compiler::ir::structural-robust-ir#define-grammar"
        );
    }

    #[test]
    fn step_ref_parse_snake_case_step() {
        let r = StepRef::parse("workspace::migrate-to-codeberg#define_grammar").unwrap();
        assert_eq!(r.step(), "define_grammar");
    }

    #[test]
    fn step_ref_rejects_missing_separator() {
        assert_eq!(
            StepRef::parse("workspace::migrate-to-codeberg"),
            Err(StepRefError::MissingSeparator)
        );
    }

    #[test]
    fn step_ref_rejects_empty_step() {
        assert_eq!(
            StepRef::parse("workspace::migrate-to-codeberg#"),
            Err(StepRefError::EmptyStep)
        );
    }

    #[test]
    fn step_ref_rejects_doubled_separator() {
        assert_eq!(
            StepRef::parse("workspace::migrate-to-codeberg#define#extra"),
            Err(StepRefError::DuplicateSeparator)
        );
    }

    #[test]
    fn step_ref_with_top_level_task() {
        // Top-level tasks are valid; step refs against them work too.
        let r = StepRef::parse("migrate-to-codeberg#initial").unwrap();
        assert!(r.task().is_top_level());
        assert_eq!(r.step(), "initial");
    }

    #[test]
    fn task_meta_round_trip() {
        let mut steps = std::collections::BTreeMap::new();
        steps.insert("define_grammar".to_owned(), Step {
            description: "Specify the IR grammar in DESIGN.md.".to_owned(),
            phase:       StepPhase::Doc,
            state:       TaskState::Closed,
        });
        steps.insert("implement_parser".to_owned(), Step {
            description: "Implement the IR parser.".to_owned(),
            phase:       StepPhase::Src,
            state:       TaskState::Open,
        });

        let meta = TaskMeta {
            mockspace_version: "1.0".to_owned(),
            slug: "structural-robust-ir".to_owned(),
            namespace: "compiler/ir".to_owned(),
            title: "Define structural robust IR shape".to_owned(),
            created: "2026-05-18T10:00:00Z".to_owned(),
            priority: Some("P1".to_owned()),
            group: Some("ref-based-redesign".to_owned()),
            steps,
            refs: TaskRefs {
                blocks:     vec!["compiler::ir#lower-pass".to_owned()],
                blocked_by: vec![],
                relates_to: vec!["compiler::ir::lower-pass#define-grammar".to_owned()],
            },
            closure: None,
        };

        let serialized = meta.to_toml().unwrap();
        let parsed = TaskMeta::from_toml(&serialized).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn task_meta_with_closure() {
        let meta = TaskMeta {
            mockspace_version: "1.0".to_owned(),
            slug:              "structural-robust-ir".to_owned(),
            namespace:         "compiler/ir".to_owned(),
            title:             "Define structural robust IR shape".to_owned(),
            created:           "2026-05-18T10:00:00Z".to_owned(),
            priority:          None,
            group:             None,
            steps:             std::collections::BTreeMap::new(),
            refs:              TaskRefs::default(),
            closure:           Some(TaskClosure {
                resolution:         TaskResolution::Completed,
                closed_at:          "2026-05-18T14:30:00Z".to_owned(),
                closed_branch:      "round/202605181400-arvo-graph-csr".to_owned(),
                closing_phase:      "apply_src".to_owned(),
                closing_round_slug: "202605181400-arvo-graph-csr".to_owned(),
            }),
        };

        let serialized = meta.to_toml().unwrap();
        assert!(serialized.contains("[closure]"));
        assert!(serialized.contains("resolution = \"completed\""));
        let parsed = TaskMeta::from_toml(&serialized).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn step_phase_serializes_doc_plus_src() {
        let step = Step {
            description: "cross-cutting work".to_owned(),
            phase:       StepPhase::DocSrc,
            state:       TaskState::Open,
        };
        let serialized = toml::to_string(&step).unwrap();
        assert!(serialized.contains("phase = \"doc+src\""));
        let parsed: Step = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.phase, StepPhase::DocSrc);
    }
}
