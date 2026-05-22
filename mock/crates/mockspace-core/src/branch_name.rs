//! Git branch-name identifiers (spec §16 task-closure metadata).
//!
//! Per the workspace harness-the-type-system rule, branch-name
//! identity lives as a trait abstraction with a default impl. The
//! canonical mockspace branch-name validator ([`DefaultBranchName`])
//! follows the practical subset of `git-check-ref-format(1)` that
//! mockspace uses for the `closed_branch` field in [`crate::task::TaskClosure`]
//! and similar provenance positions: no leading slash, no trailing
//! slash, no double-slash, no `..`, no `@{`, no leading `-`, no
//! ASCII control chars or shell-special chars.
//!
//! Mockspace does not own git's branch-name semantics; it borrows
//! them. Consumers that want strict semver-style or strict reflog
//! semantics implement [`BranchName`] themselves with the same
//! supertrait bundle.

use core::fmt;
use core::hash::Hash;

/// A validated git branch-name identifier.
///
/// Implementations carry a parser + the supertrait bundle that lets
/// consumers treat any branch-name value uniformly (`AsRef<str>` for
/// git plumbing, `Display` for diagnostics). Mockspace's canonical
/// impl is [`DefaultBranchName`]; alternative impls (stricter
/// validation, different separator conventions) plug in by
/// implementing this trait.
pub trait BranchName: AsRef<str> + fmt::Display + Eq + Hash + Clone + Sized {
    /// Why parsing failed.
    type Error: fmt::Display + fmt::Debug;

    /// Parse a branch name from its string form. Validates the
    /// impl's branch-shape invariants.
    fn parse(s: &str) -> Result<Self, Self::Error>;
}

/// The canonical mockspace branch-name: practical subset of
/// `git-check-ref-format`. Implements [`BranchName`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DefaultBranchName(String);

/// Why a [`DefaultBranchName`] rejected at parse time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefaultBranchNameError {
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

impl fmt::Display for DefaultBranchNameError {
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

impl std::error::Error for DefaultBranchNameError {}

impl DefaultBranchName {
    /// Parse a branch name, validating the canonical mockspace
    /// invariants. Inherent constructor; the [`BranchName`] trait
    /// method delegates here.
    pub fn new(s: &str) -> Result<Self, DefaultBranchNameError> {
        if s.is_empty() {
            return Err(DefaultBranchNameError::Empty);
        }
        if s == "@" {
            return Err(DefaultBranchNameError::BareAt);
        }
        if s.starts_with('-') {
            return Err(DefaultBranchNameError::LeadingDash);
        }
        if s.starts_with('/') {
            return Err(DefaultBranchNameError::LeadingSlash);
        }
        if s.ends_with('/') {
            return Err(DefaultBranchNameError::TrailingSlash);
        }
        if s.ends_with('.') {
            return Err(DefaultBranchNameError::TrailingDot);
        }
        if s.contains("..") {
            return Err(DefaultBranchNameError::DoubleDot);
        }
        if s.contains("//") {
            return Err(DefaultBranchNameError::DoubleSlash);
        }
        if s.contains("@{") {
            return Err(DefaultBranchNameError::ReflogSequence);
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
                return Err(DefaultBranchNameError::ForbiddenChar {
                    position,
                    found: ch,
                });
            }
        }
        for component in s.split('/') {
            if component.starts_with('.') {
                return Err(DefaultBranchNameError::BadComponent {
                    component: component.to_owned(),
                    reason: "component must not begin with `.`",
                });
            }
            if component.ends_with(".lock") {
                return Err(DefaultBranchNameError::BadComponent {
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

impl AsRef<str> for DefaultBranchName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DefaultBranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl BranchName for DefaultBranchName {
    type Error = DefaultBranchNameError;

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
            "main",
            "dev",
            "feat/mock-task-slice-a",
            "fix/type-harness/branch-name-trait",
            "release/0.1.0",
            "user/alice/work",
        ] {
            assert!(DefaultBranchName::new(input).is_ok(), "rejected: {input}");
        }
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(DefaultBranchName::new(""), Err(DefaultBranchNameError::Empty));
    }

    #[test]
    fn rejects_bare_at() {
        assert_eq!(DefaultBranchName::new("@"), Err(DefaultBranchNameError::BareAt));
    }

    #[test]
    fn rejects_leading_dash() {
        assert_eq!(
            DefaultBranchName::new("-foo"),
            Err(DefaultBranchNameError::LeadingDash)
        );
    }

    #[test]
    fn rejects_leading_slash() {
        assert_eq!(
            DefaultBranchName::new("/foo"),
            Err(DefaultBranchNameError::LeadingSlash)
        );
    }

    #[test]
    fn rejects_trailing_slash() {
        assert_eq!(
            DefaultBranchName::new("foo/"),
            Err(DefaultBranchNameError::TrailingSlash)
        );
    }

    #[test]
    fn rejects_trailing_dot() {
        assert_eq!(
            DefaultBranchName::new("foo."),
            Err(DefaultBranchNameError::TrailingDot)
        );
    }

    #[test]
    fn rejects_double_dot() {
        assert_eq!(
            DefaultBranchName::new("foo..bar"),
            Err(DefaultBranchNameError::DoubleDot)
        );
    }

    #[test]
    fn rejects_double_slash() {
        assert_eq!(
            DefaultBranchName::new("foo//bar"),
            Err(DefaultBranchNameError::DoubleSlash)
        );
    }

    #[test]
    fn rejects_reflog_sequence() {
        assert_eq!(
            DefaultBranchName::new("foo@{1}"),
            Err(DefaultBranchNameError::ReflogSequence)
        );
    }

    #[test]
    fn rejects_forbidden_chars() {
        for ch in ['~', '^', ':', '?', '*', '[', '\\', ' '] {
            let input = format!("foo{ch}bar");
            assert!(
                matches!(
                    DefaultBranchName::new(&input),
                    Err(DefaultBranchNameError::ForbiddenChar { .. })
                ),
                "expected ForbiddenChar for {input:?}"
            );
        }
    }

    #[test]
    fn rejects_component_starting_with_dot() {
        assert!(matches!(
            DefaultBranchName::new("foo/.hidden"),
            Err(DefaultBranchNameError::BadComponent { .. })
        ));
    }

    #[test]
    fn rejects_component_ending_with_dot_lock() {
        assert!(matches!(
            DefaultBranchName::new("foo/work.lock"),
            Err(DefaultBranchNameError::BadComponent { .. })
        ));
    }

    #[test]
    fn trait_parse_dispatches_to_default_impl() {
        let b = <DefaultBranchName as BranchName>::parse("feat/foo").expect("parse");
        assert_eq!(b.as_str(), "feat/foo");
    }

    #[test]
    fn trait_bounds_satisfied_by_default_impl() {
        fn takes_branch_name<B: BranchName>(b: B) -> String {
            b.to_string()
        }
        let b = DefaultBranchName::new("alpha").expect("parse");
        assert_eq!(takes_branch_name(b), "alpha");
    }
}
