//! Type-safe ref-path composition (spec §19).
//!
//! [`RefPath`] plays the **human-readable name** role for the
//! [`GitRef`] entity: a single-token validated string that names a
//! fully-qualified git ref. Construction lives as inherent methods
//! on `RefPath` (round_mock, round_source, task, etc.) because the
//! layout (prefix family, segment shape) is impl-specific.

use core::fmt;

use crate::entity::GitRef;
use crate::identity::{NamedRefTo, RefTo};
use crate::namespace::Namespace;
use crate::slug::Slug;

/// A fully-qualified git ref path. Implements [`NamedRefTo<GitRef>`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefPath(String);

/// Why a [`RefPath`] rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefPathError {
    /// Empty input.
    Empty,
    /// Did not start with a recognised mockspace prefix
    /// (`refs/mock/`, `refs/heads/round/`).
    InvalidPrefix,
}

impl fmt::Display for RefPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("ref-path is empty"),
            Self::InvalidPrefix => f.write_str(
                "ref-path does not carry a recognised mockspace prefix (`refs/mock/` or `refs/heads/round/`)",
            ),
        }
    }
}

impl std::error::Error for RefPathError {}

impl RefPath {
    /// `refs/mock/round/<slug>`, per-round orphan mock-side ref (spec §21).
    pub fn round_mock(slug: &Slug) -> Self {
        Self(format!("refs/mock/round/{slug}"))
    }

    /// `refs/heads/round/<slug>`, per-round source-side feature branch (spec §21).
    pub fn round_source(slug: &Slug) -> Self {
        Self(format!("refs/heads/round/{slug}"))
    }

    /// `refs/mock/round/<slug>-conflict-<host>-<ts>`, side branch
    /// preserving a lost-race commit (spec §19, §24).
    pub fn round_conflict(slug: &Slug, host: &str, timestamp: &str) -> Self {
        Self(format!(
            "refs/mock/round/{slug}-conflict-{host}-{timestamp}"
        ))
    }

    /// `refs/mock/harness`, the project's configuration ref (spec §22).
    pub fn harness() -> Self {
        Self("refs/mock/harness".to_owned())
    }

    /// `refs/mock/task/<ns-path>/<slug>`, per-active-task orphan ref (spec §16).
    pub fn task(ns: &Namespace, slug: &Slug) -> Self {
        Self(format!("refs/mock/task/{}/{}", ns.as_ref_path(), slug))
    }

    /// `refs/mock/task/<ns-path>/<slug>` constructor that accepts the
    /// full TaskId shape, including top-level (namespace-less) tasks.
    pub fn task_from_id(id: &crate::task::TaskId) -> Self {
        Self(format!("refs/mock/task/{}", id.as_ref_path()))
    }

    /// `refs/mock/task-archive`, unified closed-tasks archive (spec §26).
    pub fn task_archive() -> Self {
        Self("refs/mock/task-archive".to_owned())
    }

    /// `refs/mock/round-archive`, unified closed-rounds archive (spec §26).
    pub fn round_archive() -> Self {
        Self("refs/mock/round-archive".to_owned())
    }

    /// Borrow the path as a string slice for git plumbing calls.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume into the owned string.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl AsRef<str> for RefPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RefPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl RefTo<GitRef> for RefPath {}

impl NamedRefTo<GitRef> for RefPath {
    type Error = RefPathError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(RefPathError::Empty);
        }
        if s.starts_with("refs/mock/") || s.starts_with("refs/heads/round/") {
            Ok(Self(s.to_owned()))
        } else {
            Err(RefPathError::InvalidPrefix)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str) -> Slug {
        Slug::new(name).expect("test slug")
    }

    fn ns(s: &str) -> Namespace {
        Namespace::parse(s).expect("test namespace")
    }

    #[test]
    fn round_mock_path() {
        assert_eq!(
            RefPath::round_mock(&s("arvo-graph-csr")).as_str(),
            "refs/mock/round/arvo-graph-csr"
        );
    }

    #[test]
    fn round_source_path() {
        assert_eq!(
            RefPath::round_source(&s("arvo-graph-csr")).as_str(),
            "refs/heads/round/arvo-graph-csr"
        );
    }

    #[test]
    fn round_conflict_path() {
        assert_eq!(
            RefPath::round_conflict(&s("foo"), "host1", "202605181400").as_str(),
            "refs/mock/round/foo-conflict-host1-202605181400"
        );
    }

    #[test]
    fn harness_path() {
        assert_eq!(RefPath::harness().as_str(), "refs/mock/harness");
    }

    #[test]
    fn task_path_single_segment() {
        assert_eq!(
            RefPath::task(&ns("workspace"), &s("migrate-to-codeberg")).as_str(),
            "refs/mock/task/workspace/migrate-to-codeberg"
        );
    }

    #[test]
    fn task_path_three_segments() {
        assert_eq!(
            RefPath::task(&ns("compiler::ir::lower-pass"), &s("define-grammar")).as_str(),
            "refs/mock/task/compiler/ir/lower-pass/define-grammar"
        );
    }

    #[test]
    fn archive_paths() {
        assert_eq!(RefPath::task_archive().as_str(), "refs/mock/task-archive");
        assert_eq!(RefPath::round_archive().as_str(), "refs/mock/round-archive");
    }

    #[test]
    fn task_from_id_top_level() {
        let id = crate::task::TaskId::parse("migrate-to-codeberg").expect("parse");
        assert_eq!(
            RefPath::task_from_id(&id).as_str(),
            "refs/mock/task/migrate-to-codeberg"
        );
    }

    #[test]
    fn task_from_id_namespaced() {
        let id =
            crate::task::TaskId::parse("compiler::ir::lower-pass::define-grammar").expect("parse");
        assert_eq!(
            RefPath::task_from_id(&id).as_str(),
            "refs/mock/task/compiler/ir/lower-pass/define-grammar"
        );
    }

    #[test]
    fn named_ref_to_gitref_parses() {
        let p = <RefPath as NamedRefTo<GitRef>>::parse("refs/mock/round/foo").expect("parse");
        assert_eq!(p.as_ref(), "refs/mock/round/foo");
    }

    #[test]
    fn named_ref_to_gitref_rejects_unknown_prefix() {
        let err = <RefPath as NamedRefTo<GitRef>>::parse("refs/heads/main").expect_err("reject");
        assert!(matches!(err, RefPathError::InvalidPrefix));
    }
}
