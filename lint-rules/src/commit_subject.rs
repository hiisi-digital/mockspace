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
//! `mockspace.toml`, and one that wants none sets `commit = "off"` there.
//!
//! # What it checks
//!
//! The subject is the first authored line of the message. It must start with a
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
//! The effective set is read by the generated commit-style skill through
//! [`CommitSubject::from_params`], so the documentation an agent reads and the gate
//! it meets are the same configuration.
//!
//! # What it leaves to others
//!
//! A subject git wrote itself (a merge, a revert, a fixup) is not authored and is
//! not held to the convention. Authorship trailers belong to an attribution lint,
//! which also covers forge bodies; this lint reads commits only. An agent hook
//! hands the lint a shell command rather than a message, and a subject cannot be
//! read out of a command line reliably, so the lint declines there and the
//! `commit-msg` hook, which always follows, reads the real message file.

use std::collections::HashMap;

use crate::{Lint, LintError, MessageContext, MessageDomain, MessageLint, Severity};

const LINT_NAME: &str = "commit-subject";

/// The types a project gets when it names none: the workspace's commit-style set,
/// and `bench` for a commit that lands a measurement.
pub const DEFAULT_TYPES: &[&str] = &["feat", "fix", "refactor", "docs", "test", "chore", "bench"];

/// How the first letter of the message after the type is constrained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectCase {
    /// Must begin lowercase.
    Lower,
    /// Must begin uppercase.
    Sentence,
    /// Unconstrained.
    Any,
}

impl SubjectCase {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lower" | "lowercase" => Some(Self::Lower),
            "sentence" | "upper" | "capital" => Some(Self::Sentence),
            "any" | "off" | "" => Some(Self::Any),
            _ => None,
        }
    }
}

/// The commit subject lint, holding the convention it enforces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSubject {
    types:                  Vec<String>,
    separator:              String,
    breaking_marker:        String,
    require_type:           bool,
    max_subject:            usize,
    subject_case:           SubjectCase,
    allow_scope:            bool,
    forbid_trailing_period: bool,
    require_blank_line:     bool,
}

impl Default for CommitSubject {
    fn default() -> Self {
        Self {
            types:                  DEFAULT_TYPES.iter().map(|t| (*t).to_string()).collect(),
            separator:              ": ".to_string(),
            breaking_marker:        "!".to_string(),
            require_type:           true,
            max_subject:            71,
            subject_case:           SubjectCase::Lower,
            allow_scope:            false,
            forbid_trailing_period: true,
            require_blank_line:     false,
        }
    }
}

impl CommitSubject {
    /// The convention a project's `[lints.commit-subject]` parameters declare,
    /// defaults where it names nothing. `None` is a project with no such table.
    #[must_use]
    pub fn from_params(params: Option<&HashMap<String, String>>) -> Self {
        let mut lint = Self::default();
        if let Some(params) = params {
            lint.configure(params);
        }
        lint
    }

    /// The permitted types, in the order they were declared.
    #[must_use]
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// What follows the type.
    #[must_use]
    pub fn separator(&self) -> &str {
        &self.separator
    }

    /// The suffix of a type that marks a breaking change, empty for none.
    #[must_use]
    pub fn breaking_marker(&self) -> &str {
        &self.breaking_marker
    }

    /// Whether a subject with no type is refused.
    #[must_use]
    pub fn requires_type(&self) -> bool {
        self.require_type
    }

    /// The longest subject allowed, in characters. Zero means unlimited.
    #[must_use]
    pub fn max_subject(&self) -> usize {
        self.max_subject
    }

    /// How the first letter after the type is constrained.
    #[must_use]
    pub fn subject_case(&self) -> SubjectCase {
        self.subject_case
    }

    /// Whether `type(scope):` is permitted.
    #[must_use]
    pub fn allows_scope(&self) -> bool {
        self.allow_scope
    }

    /// Whether a trailing period is refused.
    #[must_use]
    pub fn forbids_trailing_period(&self) -> bool {
        self.forbid_trailing_period
    }

    /// Whether the subject must be followed by a blank line.
    #[must_use]
    pub fn requires_blank_line(&self) -> bool {
        self.require_blank_line
    }

    fn how_to_add_a_type() -> &'static str {
        "to allow another type, add it to `extra_types` under `[lints.commit-subject]` in mockspace.toml"
    }
}

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
            "subject-case",
            "subject-length",
            "trailing-period",
            "missing-blank-line",
            "empty-subject",
        ]
    }

    fn config_keys(&self) -> &[&str] {
        &[
            "types",
            "extra_types",
            "separator",
            "breaking_marker",
            "require_type",
            "max_subject",
            "subject_case",
            "allow_scope",
            "forbid_trailing_period",
            "require_blank_line",
        ]
    }

    fn configure(&mut self, params: &HashMap<String, String>) {
        if let Some(v) = params.get("types") {
            let listed = split_list(v);
            // An empty list would turn the type check off by accident, and
            // "no types" is not a convention anyone means to declare.
            if !listed.is_empty() {
                self.types = listed;
            }
        }
        if let Some(v) = params.get("extra_types") {
            for t in split_list(v) {
                if !self.types.contains(&t) {
                    self.types.push(t);
                }
            }
        }
        if let Some(v) = params.get("separator")
            && !v.is_empty()
        {
            self.separator = v.clone();
        }
        if let Some(v) = params.get("breaking_marker") {
            self.breaking_marker = v.clone();
        }
        if let Some(v) = params.get("require_type") {
            self.require_type = truthy(v);
        }
        if let Some(v) = params.get("max_subject")
            && let Ok(n) = v.trim().parse::<usize>()
        {
            self.max_subject = n;
        }
        if let Some(v) = params.get("subject_case")
            && let Some(c) = SubjectCase::parse(v)
        {
            self.subject_case = c;
        }
        if let Some(v) = params.get("allow_scope") {
            self.allow_scope = truthy(v);
        }
        if let Some(v) = params.get("forbid_trailing_period") {
            self.forbid_trailing_period = truthy(v);
        }
        if let Some(v) = params.get("require_blank_line") {
            self.require_blank_line = truthy(v);
        }
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
        let body = authored_body(ctx.message);
        let Some(subject) = body.lines().next() else {
            return out;
        };
        let subject = subject.trim_end();
        if is_generated_subject(subject) {
            return out;
        }
        if subject.trim().is_empty() {
            out.push(finding(
                ctx,
                "empty-subject",
                "the commit subject is empty".to_string(),
            ));
            return out;
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
            && let Some(second) = body.lines().nth(1)
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

impl CommitSubject {
    /// Check the type prefix and return the message after it.
    fn check_type<'m>(
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

        // The head is what precedes the first separator, and it is a type only if
        // it is one word. `fix the loop: again` has a separator in its message,
        // not after a type, and reading `fix the loop` as an unknown type would
        // send the author hunting for a type they did write.
        //
        // A subject cut off right after the type (`fix:`, the trailing space trimmed
        // away) is a type with nothing after it, and says so rather than missing one.
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
        let head_and_rest =
            split.filter(|(head, _)| !head.is_empty() && !head.chars().any(char::is_whitespace));
        let Some((head, rest)) = head_and_rest else {
            if self.require_type {
                out.push(finding(ctx, "missing-type", shape));
            }
            return subject;
        };

        // Strip the breaking marker first so `feat!` is the `feat` type.
        let mut ty = head;
        if !self.breaking_marker.is_empty() {
            ty = ty.strip_suffix(self.breaking_marker.as_str()).unwrap_or(ty);
        }

        let base = match ty.split_once('(') {
            Some((base, tail)) => {
                if !self.allow_scope {
                    out.push(finding(
                        ctx,
                        "scope-forbidden",
                        "drop the parenthesised scope: write `type: message`, not `type(scope): message`"
                            .to_string(),
                    ));
                } else if !tail.ends_with(')') {
                    out.push(finding(
                        ctx,
                        "unknown-type",
                        "the scope parenthesis is not closed".to_string(),
                    ));
                }
                base
            },
            None => ty,
        };

        if !self.types.iter().any(|t| t == base) {
            out.push(finding(
                ctx,
                "unknown-type",
                format!(
                    "unknown type `{base}`; types: {listed}; {}",
                    Self::how_to_add_a_type()
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
}

/// The authored part of a commit message: comment lines dropped, and everything
/// from git's verbose-diff scissors marker, so text git added cannot trip the lint.
fn authored_body(message: &str) -> String {
    message
        .split("# ------------------------ >8")
        .next()
        .unwrap_or(message)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether git wrote this subject rather than a person. These are git's own
/// spellings, capital `Merge` and `Revert "` included, so an authored `merge: x`
/// is still judged.
fn is_generated_subject(subject: &str) -> bool {
    const GENERATED: &[&str] = &["Merge ", "Revert \"", "fixup! ", "squash! ", "amend! "];
    GENERATED.iter().any(|p| subject.starts_with(p))
}

fn split_list(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn truthy(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "true" | "yes" | "on" | "1"
    )
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
#[path = "commit_subject_tests.rs"]
mod tests;
