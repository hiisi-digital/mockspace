//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Lint: a commit subject has the shape its project declared, and a type it listed.
//!
//! The one builtin message lint. It is on in every project, because a commit gate
//! that checks nothing until a pack is imported and configured passes every
//! subject, which is what happened: a `type: message` convention was written down
//! in a skill and nothing enforced the list in it. A project that wants a
//! different convention sets the keys below under `[lints.commit-subject]` in
//! `mockspace.toml`, and one that wants none sets `commit = "off"` there, which
//! alone turns all three gates off.
//!
//! # What it checks
//!
//! The subject is the first authored line of the message that is not blank. It must start with a
//! type from the configured set, then the separator, then a message. By default
//! the shape is `type: lowercase message`, no parenthesised scope, no trailing
//! period, under 72 characters. Those are the rules the workspace's commit-style
//! skill states; the mood of the verb is not checkable and is not checked.
//!
//! # How it is configured
//!
//! | key | default | meaning |
//! |---|---|---|
//! | `types` | [`DEFAULT_TYPES`] | the permitted types, comma separated; replaces the default |
//! | `extra_types` | none | types added to the set, comma separated; how a project adds one |
//! | `separator` | `": "` | what follows the type |
//! | `breaking_marker` | `"!"` | suffix of the type marking a breaking change |
//! | `require_type` | `true` | whether a subject without a type is refused |
//! | `max_subject` | `71` | longest subject in characters, so under 72; `0` turns it off |
//! | `subject_case` | `lower` | `lower`, `sentence` or `any`, for the first letter after the type |
//! | `allow_scope` | `false` | whether `type(scope):` is permitted |
//! | `forbid_trailing_period` | `true` | whether a final `.` is refused |
//! | `require_blank_line` | `false` | whether the subject must be followed by a blank line |
//!
//! A key that is not in the table, and a value that cannot be read (a boolean that is not one,
//! a number that is not, a list naming no type), stop the load with the line that is wrong:
//! [`CommitSubject::problems`] names each, and the config loader refuses on them. The lint
//! never runs on a default the project believes it replaced.
//!
//! The effective set is read by the generated commit-style skill through
//! [`CommitSubject::from_params`], so the documentation an agent reads and the gate
//! it meets are the same configuration.
//!
//! # What it leaves to others
//!
//! A subject git wrote itself, in the shapes it writes (`Merge branch '...'`, `Merge pull request
//! #...`, `Revert "..."`, `Reapply "..."`), is not authored and is not held to the convention.
//! `fixup!`, `squash!` and `amend!` subjects are passed at the commit gate, where they are
//! wanted, and refused at the build and push gates, where they would reach the trunk unfolded.
//! Authorship trailers belong to an attribution lint, which also covers forge bodies; this lint
//! reads commits only. An agent hook hands the lint a shell command rather than a message, and a
//! subject cannot be read out of a command line reliably, so the lint declines there and the
//! `commit-msg` hook, which always follows, reads the real message file.

use std::collections::HashMap;

mod check;
mod policy;

pub use policy::{CommitSubject, DEFAULT_TYPES, SubjectCase};

use crate::{Level, Lint, LintError, MessageContext, MessageDomain, MessageLint, Severity};

const LINT_NAME: &str = "commit-subject";

impl Lint for CommitSubject {
    fn name(&self) -> &'static str {
        LINT_NAME
    }

    fn description(&self) -> &'static str {
        "a commit subject has a configured type and the shape the project declared"
    }

    fn source_only(&self) -> bool {
        false
    }

    fn default_severity(&self) -> Severity {
        Severity::HARD_ERROR
    }

    fn finding_kinds(&self) -> &[&str] {
        &[
            "missing-type",
            "unknown-type",
            "scope-forbidden",
            "malformed-scope",
            "subject-case",
            "subject-length",
            "trailing-period",
            "missing-blank-line",
            "empty-subject",
            "autosquash-leftover",
        ]
    }

    fn config_keys(&self) -> &[&str] {
        policy::KEYS
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        self.apply(params);
    }

    fn check_params(&self, params: &HashMap<String, String>) -> Vec<String> {
        Self::problems(params)
    }
}

impl MessageLint for CommitSubject {
    fn domains(&self) -> &[MessageDomain] {
        &[MessageDomain::CommitMessage]
    }

    fn check_message(&self, ctx: &MessageContext) -> Vec<LintError> {
        let mut out = Vec::new();
        // An invocation means an agent hook, whose text is a command and not a
        // message. See the module docs for why this declines.
        if ctx.invocation.is_some() {
            return out;
        }
        let lines = check::authored_lines(ctx.message);
        let Some(at) = check::subject_at(&lines) else {
            return out;
        };
        let subject = lines[at].trim_end();
        match check::exemption(subject) {
            check::Exempt::Generated => return out,
            check::Exempt::Autosquash => {
                out.push(LintError::with_finding_kind(
                    ctx.origin.to_string(),
                    1,
                    LINT_NAME,
                    "an autosquash commit must be folded into its target before it is pushed: run `git rebase -i --autosquash`"
                        .to_string(),
                    Severity::new(Level::Pass, Level::Error, Level::Error),
                    "autosquash-leftover",
                ));
                return out;
            },
            check::Exempt::No => {},
        }

        let rest = self.check_type(ctx, subject, &mut out);

        let length = subject.chars().count();
        if self.max_subject > 0 && length > self.max_subject {
            out.push(finding(
                ctx,
                "subject-length",
                format!(
                    "the subject is {length} characters; keep it at {} or fewer",
                    self.max_subject
                ),
            ));
        }

        if let Some(first) = rest.trim_start().chars().next() {
            match self.subject_case {
                SubjectCase::Lower if first.is_uppercase() => {
                    out.push(finding(
                        ctx,
                        "subject-case",
                        "the message after the type must start lowercase".to_string(),
                    ));
                },
                SubjectCase::Sentence if first.is_lowercase() => {
                    out.push(finding(
                        ctx,
                        "subject-case",
                        "the message after the type must start uppercase".to_string(),
                    ));
                },
                _ => {},
            }
        }

        if self.forbid_trailing_period && subject.ends_with('.') && !subject.ends_with("..") {
            out.push(finding(
                ctx,
                "trailing-period",
                "drop the trailing period from the subject".to_string(),
            ));
        }

        if self.require_blank_line
            && let Some(second) = lines.get(at + 1)
            && !second.trim().is_empty()
        {
            out.push(finding(
                ctx,
                "missing-blank-line",
                "separate the subject from the body with a blank line".to_string(),
            ));
        }

        out
    }
}

fn finding(ctx: &MessageContext, kind: &'static str, message: String) -> LintError {
    LintError::with_finding_kind(
        ctx.origin.to_string(),
        1,
        LINT_NAME,
        message,
        Severity::HARD_ERROR,
        kind,
    )
}

#[cfg(test)]
#[path = "commit_subject_knob_tests.rs"]
mod knob_tests;
#[cfg(test)]
#[path = "commit_subject_tests.rs"]
pub(self) mod tests;
