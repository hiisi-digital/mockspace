//! Slug identifiers for rounds and tasks (spec §16).
//!
//! Per the workspace harness-the-type-system rule, slug identity lives
//! as a trait abstraction with a default impl. Function signatures
//! parameterise over `S: Slug` so future swaps (different validation
//! policy, alternative consumer's variant) land as new impls rather
//! than codebase-wide rewrites of a hardcoded concrete type.
//!
//! [`DefaultSlug`] is the canonical mockspace slug: charset
//! `[a-z][a-z0-9-]{0,62}`, max length 63. Same shape for task slugs
//! and round slugs; namespace segments use the same charset.

use core::fmt;
use core::hash::Hash;

/// Maximum default-slug length, including the leading character.
pub const MAX_SLUG_LEN: usize = 63;

/// A validated slug identifier.
///
/// Implementations carry a constructor + validated string view + an
/// owning error type. The trait is the abstraction consumers code
/// against; concrete impls plug in. [`DefaultSlug`] is the mockspace
/// canonical shape; alternative impls land here when other charsets
/// or length budgets are needed.
pub trait Slug: AsRef<str> + fmt::Display + Eq + Hash + Clone + Sized {
    /// Why a slug rejected at construction.
    type Error: fmt::Display + fmt::Debug;

    /// Parse a slug from its string form, validating the impl's
    /// invariants. Returns the impl's [`Self::Error`] type on
    /// rejection.
    fn parse(s: &str) -> Result<Self, Self::Error>;
}

/// The canonical mockspace slug: charset `[a-z][a-z0-9-]{0,62}`,
/// max length [`MAX_SLUG_LEN`]. Implements [`Slug`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefaultSlug(String);

/// Why a [`DefaultSlug`] rejected at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultSlugError {
    /// Slug was empty.
    Empty,
    /// Slug exceeded [`MAX_SLUG_LEN`].
    TooLong { len: usize },
    /// First character was not `[a-z]`.
    BadLeadingChar { found: char },
    /// A non-first character was outside `[a-z0-9-]`.
    BadChar { position: usize, found: char },
}

impl DefaultSlug {
    /// Parse a default-slug, validating the charset and length.
    /// Equivalent to `<DefaultSlug as Slug>::parse`; kept as a
    /// direct constructor for sites that genuinely want the default
    /// impl and value concise call syntax.
    pub fn new(s: &str) -> Result<Self, DefaultSlugError> {
        if s.is_empty() {
            return Err(DefaultSlugError::Empty);
        }
        if s.len() > MAX_SLUG_LEN {
            return Err(DefaultSlugError::TooLong { len: s.len() });
        }
        let mut chars = s.chars().enumerate();
        let (_, first) = chars.next().expect("non-empty checked above");
        if !first.is_ascii_lowercase() {
            return Err(DefaultSlugError::BadLeadingChar { found: first });
        }
        for (position, ch) in chars {
            let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
            if !ok {
                return Err(DefaultSlugError::BadChar {
                    position,
                    found: ch,
                });
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The validated slug as a string slice. Convenience over the
    /// `AsRef<str>` supertrait method for sites that already hold a
    /// concrete [`DefaultSlug`].
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for DefaultSlug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DefaultSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Slug for DefaultSlug {
    type Error = DefaultSlugError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl fmt::Display for DefaultSlugError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("slug is empty"),
            Self::TooLong { len } => write!(f, "slug length {len} exceeds maximum {MAX_SLUG_LEN}"),
            Self::BadLeadingChar { found } => write!(f, "leading character {found:?} is not [a-z]"),
            Self::BadChar { position, found } => write!(
                f,
                "character {found:?} at position {position} is not [a-z0-9-]"
            ),
        }
    }
}

impl std::error::Error for DefaultSlugError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_shapes() {
        for input in [
            "arvo-graph-csr",
            "a",
            "structural-robust-ir",
            "quickstart",
            "round-202605181400",
        ] {
            assert!(DefaultSlug::new(input).is_ok(), "rejected: {input}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(DefaultSlug::new(""), Err(DefaultSlugError::Empty));
    }

    #[test]
    fn rejects_too_long() {
        let long = "a".repeat(MAX_SLUG_LEN + 1);
        match DefaultSlug::new(&long) {
            Err(DefaultSlugError::TooLong { len }) => assert_eq!(len, MAX_SLUG_LEN + 1),
            other => panic!("expected TooLong, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_leading_char() {
        for input in ["1abc", "-abc", "Abc", "_abc"] {
            assert!(matches!(
                DefaultSlug::new(input),
                Err(DefaultSlugError::BadLeadingChar { .. })
            ));
        }
    }

    #[test]
    fn rejects_bad_inner_char() {
        for (input, want_pos) in [("a_b", 1), ("abc.def", 3), ("abc def", 3), ("abcD", 3)] {
            match DefaultSlug::new(input) {
                Err(DefaultSlugError::BadChar { position, .. }) => {
                    assert_eq!(position, want_pos, "input {input}");
                }
                other => panic!("expected BadChar for {input}, got {other:?}"),
            }
        }
    }

    #[test]
    fn at_max_length_ok() {
        let s = "a".repeat(MAX_SLUG_LEN);
        assert!(DefaultSlug::new(&s).is_ok());
    }

    #[test]
    fn trait_parse_dispatches_to_default_impl() {
        let s = <DefaultSlug as Slug>::parse("arvo-graph").expect("parse via trait");
        assert_eq!(s.as_str(), "arvo-graph");
    }

    #[test]
    fn trait_bounds_are_satisfied_by_default_impl() {
        // Static assertion via a helper that requires the full
        // supertrait bundle. If DefaultSlug ever loses one of the
        // supertraits, this fails to compile.
        fn takes_slug<S: Slug>(s: S) -> String {
            s.to_string()
        }
        let s = DefaultSlug::new("alpha").expect("parse");
        assert_eq!(takes_slug(s), "alpha");
    }
}
