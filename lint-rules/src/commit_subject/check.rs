//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Reading a commit message down to its subject, and the type in front of it.

use super::{CommitSubject, finding};
use crate::{LintError, MessageContext};

/// What git or the workflow did to a subject, as against what an author wrote.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Exempt {
    /// Authored text, judged by the convention.
    No,
    /// A subject git writes itself, in the exact shapes it writes them.
    Generated,
    /// `fixup!`, `squash!` or `amend!`: a commit that exists to be folded into another.
    Autosquash,
}

/// The shapes git and the forge write for a merge, a revert and a reapply.
///
/// Prefixes and not a bare `Merge `, since an author's `Merge the two lists` is authored text
/// and a gate that skipped everything starting that way would skip it too.
const GENERATED: &[&str] = &[
    "Merge branch ",
    "Merge branches ",
    "Merge remote-tracking branch ",
    "Merge remote-tracking branches ",
    "Merge tag ",
    "Merge commit ",
    "Merge pull request #",
    "Revert \"",
    "Reapply \"",
];

const AUTOSQUASH: &[&str] = &["fixup! ", "squash! ", "amend! "];

pub(super) fn exemption(subject: &str) -> Exempt {
    if GENERATED.iter().any(|p| subject.starts_with(p)) {
        Exempt::Generated
    } else if AUTOSQUASH.iter().any(|p| subject.starts_with(p)) {
        Exempt::Autosquash
    } else {
        Exempt::No
    }
}

/// The authored part of a commit message: comment lines dropped, and everything from git's
/// verbose-diff scissors marker, so text git added cannot trip the lint.
///
/// Only text still in the editor file has comment lines. A `recorded` message is what git kept,
/// where a line starting with `#` is part of the message (`-m` cleans whitespace only), so it is
/// read whole: `#42 fix it` is a subject with no type, not a comment.
pub(super) fn authored_lines(message: &str, recorded: bool) -> Vec<&str> {
    if recorded {
        return message.lines().collect();
    }
    message
        .split("# ------------------------ >8")
        .next()
        .unwrap_or(message)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect()
}

/// Where the subject is among `lines`: the first that is not blank. Git strips leading blank
/// lines before it records a message, and the `commit-msg` hook reads the file before it does.
pub(super) fn subject_at(lines: &[&str]) -> Option<usize> {
    lines.iter().position(|l| !l.trim().is_empty())
}

impl CommitSubject {
    /// Check the type prefix and return the message after it.
    pub(super) fn check_type<'m>(
        &self,
        ctx: &MessageContext,
        subject: &'m str,
        out: &mut Vec<LintError>,
    ) -> &'m str {
        let listed = self.types.join(", ");
        let shape = format!(
            "start the subject with a type and `{}`; types: {listed}",
            self.separator
        );

        // A subject cut off right after the type (`fix:`, the trailing space trimmed away) is a
        // type with nothing after it, and says so rather than missing one.
        let bare = self.separator.trim_end();
        let split = subject
            .find(&self.separator)
            .map(|at| (&subject[.. at], &subject[at + self.separator.len() ..]))
            .or_else(|| {
                (!bare.is_empty())
                    .then(|| subject.strip_suffix(bare))
                    .flatten()
                    .map(|head| (head, ""))
            });
        // The head is what precedes the first separator, and it is a type only if its name is one
        // word. `fix the loop: again` has a separator in its message, not after a type, and
        // reading `fix the loop` as an unknown type would send the author hunting for a type they
        // did write. A scope may hold spaces; the name before it may not.
        let Some((head, rest)) = split.map(|(head, rest)| (self.without_marker(head), rest)) else {
            return self.without_type(ctx, subject, shape, out);
        };
        let (name, scope) = match head.split_once('(') {
            Some((name, tail)) => (name, Some(tail)),
            None => (head, None),
        };
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return self.without_type(ctx, subject, shape, out);
        }

        if let Some(tail) = scope {
            if !self.allow_scope {
                out.push(finding(
                    ctx,
                    "scope-forbidden",
                    "drop the parenthesised scope: write `type: message`, not `type(scope): message`"
                        .to_string(),
                ));
            } else if tail
                .strip_suffix(')')
                .is_none_or(|s| s.is_empty() || s.contains(['(', ')']))
            {
                out.push(finding(
                    ctx,
                    "malformed-scope",
                    "the scope is empty or its parentheses do not close: write `type(scope): message`"
                        .to_string(),
                ));
            }
        }

        if !self.types.iter().any(|t| t == name) {
            out.push(finding(
                ctx,
                "unknown-type",
                format!(
                    "unknown type `{name}`; types: {listed}; to allow another type, add it to \
                     `extra_types` under `[lints.commit-subject]` in mockspace.toml"
                ),
            ));
        }

        if rest.trim().is_empty() {
            out.push(finding(
                ctx,
                "empty-subject",
                "nothing follows the type".to_string(),
            ));
        }
        rest
    }

    /// A subject with no type in it. Refused when a type is required, and then there is no message
    /// to hold to a case, so nothing is returned to judge; where none is required the whole
    /// subject is the message.
    fn without_type<'m>(
        &self,
        ctx: &MessageContext,
        subject: &'m str,
        shape: String,
        out: &mut Vec<LintError>,
    ) -> &'m str {
        if self.require_type {
            out.push(finding(ctx, "missing-type", shape));
            ""
        } else {
            subject
        }
    }

    /// `head` with the breaking marker taken off the end, so `feat!` reads as `feat`.
    fn without_marker<'h>(&self, head: &'h str) -> &'h str {
        if self.breaking_marker.is_empty() {
            head
        } else {
            head.strip_suffix(self.breaking_marker.as_str())
                .unwrap_or(head)
        }
    }
}
