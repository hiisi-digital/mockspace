//! Hierarchical namespace for tasks (spec §16).
//!
//! Tasks live under `<ns-path>#<slug>` where `<ns-path>` is one or more
//! [`Slug`]-shaped segments. Two render forms:
//!
//! - URI form: segments joined with `::` (e.g. `compiler::ir::lower-pass`)
//! - Ref form: segments joined with `/`  (e.g. `compiler/ir/lower-pass`)

use core::fmt;

use crate::slug::{Slug, SlugError};

/// A non-empty list of slug-shaped namespace segments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Namespace {
    segments: Vec<Slug>,
}

/// Why a namespace string rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceError {
    /// Empty input or only separators.
    Empty,
    /// A `::` appears with no content on one side (leading, trailing, or doubled).
    EmptySegment { position: usize },
    /// A segment failed slug validation.
    InvalidSegment { index: usize, error: SlugError },
}

impl Namespace {
    /// Construct from a non-empty list of segments.
    ///
    /// Returns `None` if `segments` is empty (every namespace requires at
    /// least one segment per spec §16).
    pub fn from_segments(segments: Vec<Slug>) -> Option<Self> {
        if segments.is_empty() {
            None
        } else {
            Some(Self { segments })
        }
    }

    /// Parse a URI-form namespace string like `compiler::ir::lower-pass`.
    pub fn parse(input: &str) -> Result<Self, NamespaceError> {
        if input.is_empty() {
            return Err(NamespaceError::Empty);
        }
        let mut segments = Vec::new();
        let mut byte_pos = 0;
        for (index, raw) in input.split("::").enumerate() {
            if raw.is_empty() {
                return Err(NamespaceError::EmptySegment { position: byte_pos });
            }
            let slug =
                Slug::new(raw).map_err(|error| NamespaceError::InvalidSegment { index, error })?;
            segments.push(slug);
            byte_pos += raw.len() + 2; // segment + "::" separator
        }
        Ok(Self { segments })
    }

    /// Borrow the segments in declaration order.
    pub fn segments(&self) -> &[Slug] {
        &self.segments
    }

    /// Render as URI form: segments joined with `::`.
    pub fn as_uri_form(&self) -> String {
        let mut out = String::new();
        for (i, segment) in self.segments.iter().enumerate() {
            if i > 0 {
                out.push_str("::");
            }
            out.push_str(segment.as_str());
        }
        out
    }

    /// Render as ref-path form: segments joined with `/`.
    pub fn as_ref_path(&self) -> String {
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

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_uri_form())
    }
}

impl fmt::Display for NamespaceError {
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

impl std::error::Error for NamespaceError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_segment() {
        let ns = Namespace::parse("workspace").unwrap();
        assert_eq!(ns.segments().len(), 1);
        assert_eq!(ns.as_uri_form(), "workspace");
        assert_eq!(ns.as_ref_path(), "workspace");
    }

    #[test]
    fn parses_three_segments() {
        let ns = Namespace::parse("compiler::ir::lower-pass").unwrap();
        assert_eq!(ns.segments().len(), 3);
        assert_eq!(ns.as_uri_form(), "compiler::ir::lower-pass");
        assert_eq!(ns.as_ref_path(), "compiler/ir/lower-pass");
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(Namespace::parse(""), Err(NamespaceError::Empty));
    }

    #[test]
    fn rejects_leading_separator() {
        assert!(matches!(
            Namespace::parse("::foo"),
            Err(NamespaceError::EmptySegment { .. })
        ));
    }

    #[test]
    fn rejects_trailing_separator() {
        assert!(matches!(
            Namespace::parse("foo::"),
            Err(NamespaceError::EmptySegment { .. })
        ));
    }

    #[test]
    fn rejects_doubled_separator() {
        assert!(matches!(
            Namespace::parse("foo::::bar"),
            Err(NamespaceError::EmptySegment { .. })
        ));
    }

    #[test]
    fn rejects_invalid_segment_charset() {
        match Namespace::parse("compiler::IR::lower") {
            Err(NamespaceError::InvalidSegment { index, .. }) => {
                assert_eq!(index, 1);
            }
            other => panic!("expected InvalidSegment, got {other:?}"),
        }
    }

    #[test]
    fn from_segments_requires_non_empty() {
        assert!(Namespace::from_segments(vec![]).is_none());
        let one = Namespace::from_segments(vec![Slug::new("workspace").unwrap()]);
        assert!(one.is_some());
    }
}
