//! Git branch-name identifiers (spec §16 task-closure metadata).
//!
//! [`BranchName`] plays the **human-readable name** role for the
//! [`Branch`] entity: a single-token validated string that names a
//! git branch in the consumer's repository.
//!
//! The canonical validator follows a practical subset of
//! `git-check-ref-format(1)`: no leading slash, no trailing slash,
//! no double-slash, no `..`, no `@{`, no leading `-`, no ASCII
//! control or shell-special chars, no `/`-component starting with
//! `.` or ending with `.lock`.

use core::fmt;

use crate::entity::Branch;
use crate::identity::{NamedRefTo, RefTo};

/// The canonical mockspace branch-name. Implements
/// [`NamedRefTo<Branch>`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BranchName(String);

/// Why a [`BranchName`] rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchNameError {
    /// Empty input.
    Empty,
    /// Branch name began with `-` (would look like a CLI flag).
    LeadingDash,
    /// Branch name began with `/`.
    LeadingSlash,
    /// Branch name ended with `/`.
    TrailingSlash,
    /// Branch name ended with `.`.
    TrailingDot,
    /// Branch name contained `..` (forbidden by git ref format).
    DoubleDot,
    /// Branch name contained `//` (consecutive slashes).
    DoubleSlash,
    /// Branch name contained `@{` (reflog syntax).
    ReflogSequence,
    /// Branch name was a bare `@`.
    BareAt,
    /// Branch name contained a forbidden character: ASCII control,
    /// space, or one of `~ ^ : ? * [ \`.
    ForbiddenChar { position: usize, found: char },
    /// A `/`-separated component began with `.` or ended with
    /// `.lock`.
    BadComponent { component: String, reason: &'static str },
}

impl fmt::Display for BranchNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("branch name is empty"),
            Self::LeadingDash => f.write_str("branch name must not begin with `-`"),
            Self::LeadingSlash => f.write_str("branch name must not begin with `/`"),
            Self::TrailingSlash => f.write_str("branch name must not end with `/`"),
            Self::TrailingDot => f.write_str("branch name must not end with `.`"),
            Self::DoubleDot => f.write_str("branch name must not contain `..`"),
            Self::DoubleSlash => f.write_str("branch name must not contain `//`"),
            Self::ReflogSequence => {
                f.write_str("branch name must not contain `@{` (reflog syntax)")
            }
            Self::BareAt => f.write_str("branch name must not be a bare `@`"),
            Self::ForbiddenChar { position, found } => write!(
                f,
                "branch name contains forbidden character {found:?} at byte position {position}"
            ),
            Self::BadComponent { component, reason } => {
                write!(f, "branch name component `{component}` rejected: {reason}")
            }
        }
    }
}

impl std::error::Error for BranchNameError {}

impl BranchName {
    /// Parse a branch name, validating the canonical mockspace
    /// invariants. Inherent constructor; equivalent to
    /// `<BranchName as NamedRefTo<Branch>>::parse`.
    pub fn new(s: &str) -> Result<Self, BranchNameError> {
        if s.is_empty() {
            return Err(BranchNameError::Empty);
        }
        if s == "@" {
            return Err(BranchNameError::BareAt);
        }
        if s.starts_with('-') {
            return Err(BranchNameError::LeadingDash);
        }
        if s.starts_with('/') {
            return Err(BranchNameError::LeadingSlash);
        }
        if s.ends_with('/') {
            return Err(BranchNameError::TrailingSlash);
        }
        if s.ends_with('.') {
            return Err(BranchNameError::TrailingDot);
        }
        if s.contains("..") {
            return Err(BranchNameError::DoubleDot);
        }
        if s.contains("//") {
            return Err(BranchNameError::DoubleSlash);
        }
        if s.contains("@{") {
            return Err(BranchNameError::ReflogSequence);
        }
        for (position, ch) in s.char_indices() {
            let forbidden = ch.is_ascii_control()
                || ch == ' '
                || ch == '~'
                || ch == '^'
                || ch == ':'
                || ch == '?'
                || ch == '*'
                || ch == '['
                || ch == '\\';
            if forbidden {
                return Err(BranchNameError::ForbiddenChar {
                    position,
                    found: ch,
                });
            }
        }
        for component in s.split('/') {
            if component.starts_with('.') {
                return Err(BranchNameError::BadComponent {
                    component: component.to_owned(),
                    reason: "component must not begin with `.`",
                });
            }
            if component.ends_with(".lock") {
                return Err(BranchNameError::BadComponent {
                    component: component.to_owned(),
                    reason: "component must not end with `.lock`",
                });
            }
        }
        Ok(Self(s.to_owned()))
    }

    /// The validated branch name as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for BranchName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl RefTo<Branch> for BranchName {}

impl NamedRefTo<Branch> for BranchName {
    type Error = BranchNameError;

    fn parse(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_shapes() {
        for input in [
            "main", "dev", "feat/mock-task-slice-a",
            "fix/type-harness/branch-name", "release/0.1.0", "user/alice/work",
        ] {
            assert!(BranchName::new(input).is_ok(), "rejected: {input}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(BranchName::new(""), Err(BranchNameError::Empty));
    }

    #[test]
    fn rejects_bare_at() {
        assert_eq!(BranchName::new("@"), Err(BranchNameError::BareAt));
    }

    #[test]
    fn rejects_leading_dash_slash() {
        assert_eq!(BranchName::new("-foo"), Err(BranchNameError::LeadingDash));
        assert_eq!(BranchName::new("/foo"), Err(BranchNameError::LeadingSlash));
    }

    #[test]
    fn rejects_trailing_slash_dot() {
        assert_eq!(BranchName::new("foo/"), Err(BranchNameError::TrailingSlash));
        assert_eq!(BranchName::new("foo."), Err(BranchNameError::TrailingDot));
    }

    #[test]
    fn rejects_double_dot_slash() {
        assert_eq!(BranchName::new("foo..bar"), Err(BranchNameError::DoubleDot));
        assert_eq!(BranchName::new("foo//bar"), Err(BranchNameError::DoubleSlash));
    }

    #[test]
    fn rejects_reflog_sequence() {
        assert_eq!(BranchName::new("foo@{1}"), Err(BranchNameError::ReflogSequence));
    }

    #[test]
    fn rejects_forbidden_chars() {
        for ch in ['~', '^', ':', '?', '*', '[', '\\', ' '] {
            let input = format!("foo{ch}bar");
            assert!(
                matches!(BranchName::new(&input), Err(BranchNameError::ForbiddenChar { .. })),
                "expected ForbiddenChar for {input:?}"
            );
        }
    }

    #[test]
    fn rejects_bad_components() {
        assert!(matches!(
            BranchName::new("foo/.hidden"),
            Err(BranchNameError::BadComponent { .. })
        ));
        assert!(matches!(
            BranchName::new("foo/work.lock"),
            Err(BranchNameError::BadComponent { .. })
        ));
    }

    #[test]
    fn named_ref_to_branch_parses() {
        let b = <BranchName as NamedRefTo<Branch>>::parse("feat/foo").expect("parse");
        assert_eq!(b.as_str(), "feat/foo");
    }

    #[test]
    fn satisfies_named_ref_to_branch_bound() {
        fn take<N: NamedRefTo<Branch>>(n: N) -> String {
            n.to_string()
        }
        assert_eq!(take(BranchName::new("alpha").unwrap()), "alpha");
    }
}
