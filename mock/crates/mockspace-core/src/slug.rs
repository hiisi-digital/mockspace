//! Slug identifiers for rounds and tasks (spec §16).
//!
//! The canonical mockspace slug: charset `[a-z][a-z0-9-]{0,62}`,
//! max length 63. Same shape for task slugs and round slugs;
//! namespace segments use the same charset.
//!
//! [`Slug`] plays the **human-readable name** role
//! ([`NamedRefTo<Round>`] and [`NamedRefTo<Task>`]) for both Round
//! and Task entities. The same validated string shape names either,
//! and the bound at the call site picks which kind.
//!
//! Slug does NOT impl `RefTo` / `NamedRefTo` for namespace segments
//! despite namespace segments sharing the charset. A namespace
//! segment is structural composition of a TaskId, not an entity that
//! mockspace tracks references to.

use core::fmt;

use crate::entity::{Round, Task};
use crate::identity::{NamedRefTo, RefTo};

/// Maximum slug length, including the leading character.
pub const MAX_SLUG_LEN: usize = 63;

/// The canonical mockspace slug.
///
/// Construct via [`Slug::new`] or `<Slug as NamedRefTo<T>>::parse`.
/// The internal string is guaranteed to match `[a-z][a-z0-9-]{0,62}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(String);

/// Why a [`Slug`] rejected at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugError {
    /// Slug was empty.
    Empty,
    /// Slug exceeded [`MAX_SLUG_LEN`].
    TooLong { len: usize },
    /// First character was not `[a-z]`.
    BadLeadingChar { found: char },
    /// A non-first character was outside `[a-z0-9-]`.
    BadChar { position: usize, found: char },
}

impl Slug {
    /// Parse a slug, validating the charset and length. Inherent
    /// constructor; equivalent to either of the trait-method
    /// parses ([`<Slug as NamedRefTo<Round>>::parse`] or
    /// [`<Slug as NamedRefTo<Task>>::parse`]) since validation is
    /// independent of the named entity.
    pub fn new(s: &str) -> Result<Self, SlugError> {
        if s.is_empty() {
            return Err(SlugError::Empty);
        }
        if s.len() > MAX_SLUG_LEN {
            return Err(SlugError::TooLong { len: s.len() });
        }
        let mut chars = s.chars().enumerate();
        let (_, first) = chars.next().expect("non-empty checked above");
        if !first.is_ascii_lowercase() {
            return Err(SlugError::BadLeadingChar { found: first });
        }
        for (position, ch) in chars {
            let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
            if !ok {
                return Err(SlugError::BadChar {
                    position,
                    found: ch,
                });
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The validated slug as a string slice. Convenience over the
    /// `AsRef<str>` supertrait method.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Slug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl RefTo<Round> for Slug {}

impl NamedRefTo<Round> for Slug {
    type Error = SlugError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl RefTo<Task> for Slug {}

impl NamedRefTo<Task> for Slug {
    type Error = SlugError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl fmt::Display for SlugError {
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

impl std::error::Error for SlugError {}

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
            assert!(Slug::new(input).is_ok(), "rejected: {input}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(Slug::new(""), Err(SlugError::Empty));
    }

    #[test]
    fn rejects_too_long() {
        let long = "a".repeat(MAX_SLUG_LEN + 1);
        match Slug::new(&long) {
            Err(SlugError::TooLong { len }) => assert_eq!(len, MAX_SLUG_LEN + 1),
            other => panic!("expected TooLong, got {other:?}"),
        }
    }

    #[test]
    fn rejects_bad_leading_char() {
        for input in ["1abc", "-abc", "Abc", "_abc"] {
            assert!(matches!(
                Slug::new(input),
                Err(SlugError::BadLeadingChar { .. })
            ));
        }
    }

    #[test]
    fn rejects_bad_inner_char() {
        for (input, want_pos) in [("a_b", 1), ("abc.def", 3), ("abc def", 3), ("abcD", 3)] {
            match Slug::new(input) {
                Err(SlugError::BadChar { position, .. }) => {
                    assert_eq!(position, want_pos, "input {input}");
                }
                other => panic!("expected BadChar for {input}, got {other:?}"),
            }
        }
    }

    #[test]
    fn at_max_length_ok() {
        let s = "a".repeat(MAX_SLUG_LEN);
        assert!(Slug::new(&s).is_ok());
    }

    #[test]
    fn named_ref_to_round_parses() {
        let s = <Slug as NamedRefTo<Round>>::parse("arvo-graph").expect("parse");
        assert_eq!(s.as_str(), "arvo-graph");
    }

    #[test]
    fn named_ref_to_task_parses() {
        let s = <Slug as NamedRefTo<Task>>::parse("migrate-to-codeberg").expect("parse");
        assert_eq!(s.as_str(), "migrate-to-codeberg");
    }

    #[test]
    fn satisfies_named_ref_to_round_bound() {
        fn take<N: NamedRefTo<Round>>(n: N) -> String {
            n.to_string()
        }
        assert_eq!(take(Slug::new("alpha").unwrap()), "alpha");
    }

    #[test]
    fn satisfies_named_ref_to_task_bound() {
        fn take<N: NamedRefTo<Task>>(n: N) -> String {
            n.to_string()
        }
        assert_eq!(take(Slug::new("beta").unwrap()), "beta");
    }
}
