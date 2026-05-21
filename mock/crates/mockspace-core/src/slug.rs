//! Slug identifiers for rounds and tasks (spec §16).
//!
//! A slug is the leaf-name portion of a round or task identifier. The
//! charset is intentionally narrow: `[a-z][a-z0-9-]{0,62}`. Same shape for
//! task slugs and round slugs; namespace segments use the same charset.

use core::fmt;

/// Maximum slug length, including the leading character.
pub const MAX_SLUG_LEN: usize = 63;

/// A validated slug.
///
/// Construct via [`Slug::new`]. The internal string is guaranteed to match
/// the `[a-z][a-z0-9-]{0,62}` pattern.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(String);

/// Why a slug rejected at construction.
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
    /// Parse a slug, validating the charset and length.
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

    /// The validated slug as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
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
}
