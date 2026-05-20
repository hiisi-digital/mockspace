//! Type-safe ref-path composition (spec §19).
//!
//! Centralises the literal strings that name git refs. The constructors here
//! are the only path through which mockspace builds ref names; anything else
//! risks drift from the canonical layout.

use core::fmt;

use crate::namespace::Namespace;
use crate::slug::Slug;

/// A fully-qualified git ref path (e.g. `refs/mock/round/foo`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RefPath(String);

impl RefPath {
    /// `refs/mock/round/<slug>` — per-round orphan mock-side ref (spec §21).
    pub fn round_mock(slug: &Slug) -> Self {
        Self(format!("refs/mock/round/{slug}"))
    }

    /// `refs/heads/round/<slug>` — per-round source-side feature branch (spec §21).
    pub fn round_source(slug: &Slug) -> Self {
        Self(format!("refs/heads/round/{slug}"))
    }

    /// `refs/mock/round/<slug>-conflict-<host>-<ts>` — side branch
    /// preserving a lost-race commit (spec §19, §24).
    pub fn round_conflict(slug: &Slug, host: &str, timestamp: &str) -> Self {
        Self(format!("refs/mock/round/{slug}-conflict-{host}-{timestamp}"))
    }

    /// `refs/mock/harness` — the project's configuration ref (spec §22).
    pub fn harness() -> Self {
        Self("refs/mock/harness".to_owned())
    }

    /// `refs/mock/task/<ns-path>/<slug>` — per-active-task orphan ref (spec §16).
    pub fn task(ns: &Namespace, slug: &Slug) -> Self {
        Self(format!(
            "refs/mock/task/{}/{}",
            ns.as_ref_path(),
            slug
        ))
    }

    /// `refs/mock/task-archive` — unified closed-tasks archive (spec §26).
    pub fn task_archive() -> Self {
        Self("refs/mock/task-archive".to_owned())
    }

    /// `refs/mock/round-archive` — unified closed-rounds archive (spec §26).
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

impl fmt::Display for RefPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
    fn task_path_nested_namespace() {
        assert_eq!(
            RefPath::task(
                &ns("compiler::ir::lower-pass"),
                &s("define-grammar")
            )
            .as_str(),
            "refs/mock/task/compiler/ir/lower-pass/define-grammar"
        );
    }

    #[test]
    fn archive_paths() {
        assert_eq!(RefPath::task_archive().as_str(), "refs/mock/task-archive");
        assert_eq!(RefPath::round_archive().as_str(), "refs/mock/round-archive");
    }
}
