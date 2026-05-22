//! Hierarchical namespace for tasks (spec §16).
//!
//! Per the workspace harness-the-type-system rule, namespace identity
//! lives as a trait abstraction with a default impl. Function signatures
//! parameterise over `N: Namespace` so future swaps (different segment
//! charset, alternative separator convention, dotted vs colon-double
//! style) land as new impls rather than codebase-wide rewrites.
//!
//! Tasks live under `<ns-path>#<slug>` where `<ns-path>` is one or more
//! [`Slug`]-shaped segments. The default impl [`DefaultNamespace`]
//! carries mockspace's canonical separator (`::` for URI form, `/` for
//! ref-path form).

use core::fmt;
use core::hash::Hash;

use crate::slug::{DefaultSlug, DefaultSlugError, Slug};

/// A hierarchical namespace identifier.
///
/// Implementations carry a parser + segment accessor + the two render
/// forms mockspace cares about (URI form for human-facing UI, ref-path
/// form for git-ref construction). The associated `Slug` type lets each
/// impl declare which slug shape its segments take.
pub trait Namespace: fmt::Display + Eq + Hash + Clone + Sized {
    /// The slug type each segment takes.
    type Slug: Slug;
    /// Why parsing failed.
    type Error: fmt::Display + fmt::Debug;

    /// Parse a namespace from its string form. The shape and separator
    /// are impl-defined; the default impl uses `::` per spec §16.
    fn parse(s: &str) -> Result<Self, Self::Error>;

    /// Borrow the namespace's segments in declaration order.
    ///
    /// **Storage constraint**: returning a slice requires the impl to
    /// back its segments with contiguous memory (e.g. `Vec<Self::Slug>`
    /// in [`DefaultNamespace`]). Impls that need non-contiguous storage
    /// (linked list, tree, segment table) cannot satisfy this trait as
    /// written. Lift to `impl Iterator<Item = &Self::Slug>` if a future
    /// impl genuinely needs non-contiguous backing; until then the
    /// slice return is the minimum viable shape for [`RefPath::task`]
    /// and the other consumers that walk segments tightly.
    fn segments(&self) -> &[Self::Slug];

    /// Render as URI form (the human-facing rendering, e.g.
    /// `compiler::ir::lower-pass` in the default impl).
    fn as_uri_form(&self) -> String;

    /// Render as ref-path form (the git-ref-safe rendering, e.g.
    /// `compiler/ir/lower-pass` in the default impl).
    fn as_ref_path(&self) -> String;
}

/// The canonical mockspace namespace: [`DefaultSlug`]-segmented, `::`
/// for URI form, `/` for ref-path form. Implements [`Namespace`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefaultNamespace {
    segments: Vec<DefaultSlug>,
}

/// Why a [`DefaultNamespace`] string rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultNamespaceError {
    /// Empty input or only separators.
    Empty,
    /// A `::` appears with no content on one side (leading, trailing, or doubled).
    EmptySegment { position: usize },
    /// A segment failed slug validation.
    InvalidSegment { index: usize, error: DefaultSlugError },
}

impl DefaultNamespace {
    /// Construct from a non-empty list of segments.
    ///
    /// Returns `None` if `segments` is empty (every namespace requires at
    /// least one segment per spec §16).
    pub fn from_segments(segments: Vec<DefaultSlug>) -> Option<Self> {
        if segments.is_empty() {
            None
        } else {
            Some(Self { segments })
        }
    }
}

impl Namespace for DefaultNamespace {
    type Slug = DefaultSlug;
    type Error = DefaultNamespaceError;

    /// Parse a URI-form namespace string like `compiler::ir::lower-pass`.
    fn parse(input: &str) -> Result<Self, Self::Error> {
        if input.is_empty() {
            return Err(DefaultNamespaceError::Empty);
        }
        let mut segments = Vec::new();
        let mut byte_pos = 0;
        for (index, raw) in input.split("::").enumerate() {
            if raw.is_empty() {
                return Err(DefaultNamespaceError::EmptySegment { position: byte_pos });
            }
            let slug = DefaultSlug::new(raw)
                .map_err(|error| DefaultNamespaceError::InvalidSegment { index, error })?;
            segments.push(slug);
            byte_pos += raw.len() + 2; // segment + "::" separator
        }
        Ok(Self { segments })
    }

    fn segments(&self) -> &[Self::Slug] {
        &self.segments
    }

    fn as_uri_form(&self) -> String {
        let mut out = String::new();
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                out.push_str("::");
            }
            out.push_str(segment.as_str());
        }
        out
    }

    fn as_ref_path(&self) -> String {
        let mut out = String::new();
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            out.push_str(segment.as_str());
        }
        out
    }
}

impl fmt::Display for DefaultNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_uri_form())
    }
}

impl fmt::Display for DefaultNamespaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("namespace is empty"),
            Self::EmptySegment { position } => {
                write!(f, "empty namespace segment at byte position {position}")
            }
            Self::InvalidSegment { index, error } => {
                write!(f, "segment {index} is not a valid slug: {error}")
            }
        }
    }
}

impl std::error::Error for DefaultNamespaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_segment() {
        let ns = DefaultNamespace::parse("workspace").unwrap();
        assert_eq!(ns.segments().len(), 1);
        assert_eq!(ns.as_uri_form(), "workspace");
        assert_eq!(ns.as_ref_path(), "workspace");
    }

    #[test]
    fn parses_three_segments() {
        let ns = DefaultNamespace::parse("compiler::ir::lower-pass").unwrap();
        assert_eq!(ns.segments().len(), 3);
        assert_eq!(ns.as_uri_form(), "compiler::ir::lower-pass");
        assert_eq!(ns.as_ref_path(), "compiler/ir/lower-pass");
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(
            DefaultNamespace::parse(""),
            Err(DefaultNamespaceError::Empty)
        );
    }

    #[test]
    fn rejects_leading_separator() {
        assert!(matches!(
            DefaultNamespace::parse("::foo"),
            Err(DefaultNamespaceError::EmptySegment { .. })
        ));
    }

    #[test]
    fn rejects_trailing_separator() {
        assert!(matches!(
            DefaultNamespace::parse("foo::"),
            Err(DefaultNamespaceError::EmptySegment { .. })
        ));
    }

    #[test]
    fn rejects_doubled_separator() {
        assert!(matches!(
            DefaultNamespace::parse("foo::::bar"),
            Err(DefaultNamespaceError::EmptySegment { .. })
        ));
    }

    #[test]
    fn rejects_invalid_segment_charset() {
        match DefaultNamespace::parse("compiler::IR::lower") {
            Err(DefaultNamespaceError::InvalidSegment { index, .. }) => {
                assert_eq!(index, 1);
            }
            other => panic!("expected InvalidSegment, got {other:?}"),
        }
    }

    #[test]
    fn from_segments_requires_non_empty() {
        assert!(DefaultNamespace::from_segments(vec![]).is_none());
        let one =
            DefaultNamespace::from_segments(vec![DefaultSlug::new("workspace").unwrap()]);
        assert!(one.is_some());
    }

    #[test]
    fn trait_parse_dispatches_to_default_impl() {
        let ns = <DefaultNamespace as Namespace>::parse("compiler::ir").expect("parse");
        assert_eq!(ns.as_uri_form(), "compiler::ir");
    }

    #[test]
    fn trait_bounds_satisfied_by_default_impl() {
        fn takes_namespace<N: Namespace>(n: N) -> String {
            n.to_string()
        }
        let ns = DefaultNamespace::parse("alpha::beta").expect("parse");
        assert_eq!(takes_namespace(ns), "alpha::beta");
    }
}
