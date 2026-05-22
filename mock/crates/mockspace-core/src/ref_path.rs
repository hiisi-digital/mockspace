//! Type-safe ref-path composition (spec §19).
//!
//! Per the workspace harness-the-type-system rule, ref-path identity
//! lives as a trait abstraction with a default impl. Function
//! signatures parameterise over `R: RefPath` so future swaps
//! (alternative storage layouts, refs/heads/-rooted rather than
//! refs/mock/-rooted naming, different prefix conventions) land as
//! new impls rather than codebase-wide rewrites.
//!
//! The mockspace canonical layout lives in [`DefaultRefPath`]:
//! `refs/mock/round/<slug>` for round refs, `refs/mock/task/<ns>/<slug>`
//! for task refs, `refs/mock/{round,task}-archive` for the unified
//! archives. Constructors are inherent to [`DefaultRefPath`] because
//! the layout is impl-specific; the trait carries only the abstraction
//! contract (parse + the `AsRef<str>` / `Display` supertrait bundle).

use core::fmt;
use core::hash::Hash;

use crate::namespace::{DefaultNamespace, Namespace};
use crate::slug::DefaultSlug;

/// A fully-qualified git ref path identifier.
///
/// Implementations carry a parser + the supertrait bundle that lets
/// consumers treat any ref-path value uniformly (`AsRef<str>` for git
/// plumbing, `Display` for diagnostics). Construction lives on the
/// impl-side because the layout (prefix, segment separators, archive
/// conventions) is impl-specific. [`DefaultRefPath`] ships the mockspace
/// canonical layout.
pub trait RefPath: AsRef<str> + fmt::Display + Eq + Hash + Clone + Sized {
    /// Why parsing failed.
    type Error: fmt::Display + fmt::Debug;

    /// Parse a ref-path from its string form. Validates the impl's
    /// layout invariants (prefix, segment shape, charset).
    fn parse(s: &str) -> Result<Self, Self::Error>;
}

/// The canonical mockspace ref-path: `refs/mock/<...>` prefix family
/// per spec §19. Implements [`RefPath`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefaultRefPath(String);

/// Why a [`DefaultRefPath`] rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultRefPathError {
    /// Empty input.
    Empty,
    /// Did not start with a recognised mockspace prefix
    /// (`refs/mock/`, `refs/heads/round/`).
    InvalidPrefix,
}

impl fmt::Display for DefaultRefPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("ref-path is empty"),
            Self::InvalidPrefix => f.write_str(
                "ref-path does not carry a recognised mockspace prefix (`refs/mock/` or `refs/heads/round/`)",
            ),
        }
    }
}

impl std::error::Error for DefaultRefPathError {}

impl DefaultRefPath {
    /// `refs/mock/round/<slug>` — per-round orphan mock-side ref (spec §21).
    /// Generic over any [`Slug`] impl; the slug's [`fmt::Display`]
    /// supertrait is the load-bearing contract here.
    pub fn round_mock<S: crate::slug::Slug>(slug: &S) -> Self {
        Self(format!("refs/mock/round/{slug}"))
    }

    /// `refs/heads/round/<slug>` — per-round source-side feature branch (spec §21).
    pub fn round_source<S: crate::slug::Slug>(slug: &S) -> Self {
        Self(format!("refs/heads/round/{slug}"))
    }

    /// `refs/mock/round/<slug>-conflict-<host>-<ts>` — side branch
    /// preserving a lost-race commit (spec §19, §24).
    pub fn round_conflict<S: crate::slug::Slug>(slug: &S, host: &str, timestamp: &str) -> Self {
        Self(format!(
            "refs/mock/round/{slug}-conflict-{host}-{timestamp}"
        ))
    }

    /// `refs/mock/harness` — the project's configuration ref (spec §22).
    pub fn harness() -> Self {
        Self("refs/mock/harness".to_owned())
    }

    /// `refs/mock/task/<ns-path>/<slug>` — per-active-task orphan ref (spec §16).
    /// Generic over both identity traits; the slug's [`fmt::Display`]
    /// supertrait + the namespace's `as_ref_path()` trait method are
    /// the load-bearing contracts here.
    pub fn task<N: Namespace, S: crate::slug::Slug>(ns: &N, slug: &S) -> Self {
        Self(format!("refs/mock/task/{}/{}", ns.as_ref_path(), slug))
    }

    /// `refs/mock/task/<ns-path>/<slug>` constructor that accepts the
    /// full DefaultTaskId shape, including top-level (namespace-less) tasks.
    /// A top-level task `migrate-to-codeberg` resolves to
    /// `refs/mock/task/migrate-to-codeberg`. A namespaced task
    /// `compiler::ir::lower-pass::define-grammar` resolves to
    /// `refs/mock/task/compiler/ir/lower-pass/define-grammar`.
    /// Generic over any [`crate::task::TaskId`] impl; the trait's
    /// `as_ref_path()` method is the load-bearing contract here.
    pub fn task_from_id<T: crate::task::TaskId>(id: &T) -> Self {
        Self(format!("refs/mock/task/{}", id.as_ref_path()))
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

impl AsRef<str> for DefaultRefPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DefaultRefPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl RefPath for DefaultRefPath {
    type Error = DefaultRefPathError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        if s.is_empty() {
            return Err(DefaultRefPathError::Empty);
        }
        // Recognise the mockspace-canonical prefixes; anything else
        // rejects. Deeper structural validation (segment shape per
        // ref family) lives in the constructors that build the
        // refs in the first place.
        if s.starts_with("refs/mock/") || s.starts_with("refs/heads/round/") {
            Ok(Self(s.to_owned()))
        } else {
            Err(DefaultRefPathError::InvalidPrefix)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(name: &str) -> DefaultSlug {
        DefaultSlug::new(name).expect("test slug")
    }

    fn ns(s: &str) -> DefaultNamespace {
        DefaultNamespace::parse(s).expect("test namespace")
    }

    #[test]
    fn round_mock_path() {
        assert_eq!(
            DefaultRefPath::round_mock(&s("arvo-graph-csr")).as_str(),
            "refs/mock/round/arvo-graph-csr"
        );
    }

    #[test]
    fn round_source_path() {
        assert_eq!(
            DefaultRefPath::round_source(&s("arvo-graph-csr")).as_str(),
            "refs/heads/round/arvo-graph-csr"
        );
    }

    #[test]
    fn round_conflict_path() {
        assert_eq!(
            DefaultRefPath::round_conflict(&s("foo"), "host1", "202605181400").as_str(),
            "refs/mock/round/foo-conflict-host1-202605181400"
        );
    }

    #[test]
    fn harness_path() {
        assert_eq!(DefaultRefPath::harness().as_str(), "refs/mock/harness");
    }

    #[test]
    fn task_path_single_segment() {
        assert_eq!(
            DefaultRefPath::task(&ns("workspace"), &s("migrate-to-codeberg")).as_str(),
            "refs/mock/task/workspace/migrate-to-codeberg"
        );
    }

    #[test]
    fn task_path_nested_namespace() {
        assert_eq!(
            DefaultRefPath::task(&ns("compiler::ir::lower-pass"), &s("define-grammar")).as_str(),
            "refs/mock/task/compiler/ir/lower-pass/define-grammar"
        );
    }

    #[test]
    fn archive_paths() {
        assert_eq!(DefaultRefPath::task_archive().as_str(), "refs/mock/task-archive");
        assert_eq!(DefaultRefPath::round_archive().as_str(), "refs/mock/round-archive");
    }

    #[test]
    fn task_from_id_top_level() {
        let id = crate::task::DefaultTaskId::parse("migrate-to-codeberg").expect("parse");
        assert_eq!(
            DefaultRefPath::task_from_id(&id).as_str(),
            "refs/mock/task/migrate-to-codeberg"
        );
    }

    #[test]
    fn task_from_id_namespaced() {
        let id =
            crate::task::DefaultTaskId::parse("compiler::ir::lower-pass::define-grammar").expect("parse");
        assert_eq!(
            DefaultRefPath::task_from_id(&id).as_str(),
            "refs/mock/task/compiler/ir/lower-pass/define-grammar"
        );
    }

    #[test]
    fn trait_parse_round_trip_via_default_impl() {
        let path = <DefaultRefPath as RefPath>::parse("refs/mock/round/foo").expect("parse");
        assert_eq!(path.as_ref(), "refs/mock/round/foo");
    }

    #[test]
    fn trait_parse_rejects_unknown_prefix() {
        let err = <DefaultRefPath as RefPath>::parse("refs/heads/main").expect_err("rejects");
        assert!(matches!(err, DefaultRefPathError::InvalidPrefix));
    }

    #[test]
    fn trait_bounds_satisfied_by_default_impl() {
        fn takes_ref_path<R: RefPath>(p: R) -> String {
            p.to_string()
        }
        let p = DefaultRefPath::round_mock(&s("alpha"));
        assert_eq!(takes_ref_path(p), "refs/mock/round/alpha");
    }
}
