//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The convention the commit subject lint holds a project to: its knobs, their defaults, how
//! they are read and how a bad one is refused.

use std::collections::HashMap;

/// The types a project gets when it names none: the workspace's commit-style set,
/// `bench` for a commit that lands a measurement, and `release` for the merge commit
/// `homma release run` writes in every repository that releases through it.
pub const DEFAULT_TYPES: &[&str] = &[
    "feat", "fix", "refactor", "docs", "test", "chore", "bench", "release",
];

/// Every key the lint reads under `[lints.commit-subject]`.
pub(super) const KEYS: &[&str] = &[
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
];

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
            "any" | "off" => Some(Self::Any),
            _ => None,
        }
    }
}

/// The convention a project declared, defaults where it named nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSubject {
    pub(super) types:                  Vec<String>,
    pub(super) separator:              String,
    pub(super) breaking_marker:        String,
    pub(super) require_type:           bool,
    pub(super) max_subject:            usize,
    pub(super) subject_case:           SubjectCase,
    pub(super) allow_scope:            bool,
    pub(super) forbid_trailing_period: bool,
    pub(super) require_blank_line:     bool,
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
            lint.apply(params);
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

    /// Read what can be read. A value that cannot be read leaves the default, which
    /// [`CommitSubject::problems`] reports at load, so a run never gets here with one.
    pub(super) fn apply(&mut self, params: &HashMap<String, String>) {
        if let Some(v) = params.get("types") {
            let listed = split_list(v);
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
        if let Some(b) = params.get("require_type").and_then(|v| parse_bool(v)) {
            self.require_type = b;
        }
        if let Some(n) = params
            .get("max_subject")
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            self.max_subject = n;
        }
        if let Some(c) = params
            .get("subject_case")
            .and_then(|v| SubjectCase::parse(v))
        {
            self.subject_case = c;
        }
        if let Some(b) = params.get("allow_scope").and_then(|v| parse_bool(v)) {
            self.allow_scope = b;
        }
        if let Some(b) = params
            .get("forbid_trailing_period")
            .and_then(|v| parse_bool(v))
        {
            self.forbid_trailing_period = b;
        }
        if let Some(b) = params.get("require_blank_line").and_then(|v| parse_bool(v)) {
            self.require_blank_line = b;
        }
    }

    /// Everything wrong with a project's parameters, one line each, in key order.
    ///
    /// An unknown key (named with its nearest known one), a boolean that is not a boolean, a
    /// number that is not one, a case that is none of the three, a list naming no type and an
    /// empty separator. Each would otherwise leave the gate on a default while the project
    /// believes it set something.
    #[must_use]
    pub fn problems(params: &HashMap<String, String>) -> Vec<String> {
        let mut keys: Vec<&String> = params.keys().collect();
        keys.sort();
        let mut out = Vec::new();
        for key in keys {
            let value = params[key].as_str();
            let bad = |what: &str| format!("`{key}` is `{value}`, which {what}");
            match key.as_str() {
                "types" | "extra_types" if split_list(value).is_empty() => {
                    out.push(bad("names no type"));
                },
                "types" | "extra_types" => {
                    let sep = params
                        .get("separator")
                        .filter(|s| !s.is_empty())
                        .map_or(": ", String::as_str);
                    for t in split_list(value) {
                        if let Some(why) = never_matches(&t, sep) {
                            out.push(format!(
                                "`{key}` lists `{t}`, which can never match a subject: {why}; \
                                 separate types with commas"
                            ));
                        }
                    }
                },
                "separator" if value.is_empty() => out.push(bad("is empty")),
                "require_type"
                | "allow_scope"
                | "forbid_trailing_period"
                | "require_blank_line"
                    if parse_bool(value).is_none() =>
                {
                    out.push(bad("is not true or false"));
                },
                "max_subject" if value.trim().parse::<usize>().is_err() => {
                    out.push(bad("is not a whole number (0 turns the check off)"));
                },
                "subject_case" if SubjectCase::parse(value).is_none() => {
                    out.push(bad("is not `lower`, `sentence` or `any`"));
                },
                _ if !KEYS.contains(&key.as_str()) => {
                    let hint = nearest(key, KEYS)
                        .map_or_else(String::new, |k| format!("; did you mean `{k}`"));
                    out.push(format!(
                        "`{key}` is not a key of this lint{hint} (keys: {})",
                        KEYS.join(", ")
                    ));
                },
                _ => {},
            }
        }
        out
    }
}

/// Why a listed type can never be the type of a subject, when it cannot.
///
/// The subject's type is read as the one word before the first separator or `(`, so a name with
/// whitespace, the separator or a parenthesis in it is never what is read. Such an entry would
/// refuse every subject that spells it, with a message naming a type that is on the list.
fn never_matches(ty: &str, separator: &str) -> Option<&'static str> {
    if ty.chars().any(char::is_whitespace) {
        Some("a type is one word, with no whitespace in it")
    } else if ty.contains(separator) {
        Some("the separator ends the type, so it cannot be inside one")
    } else if ty.contains('(') {
        Some("a parenthesis opens a scope, so it cannot be inside a type")
    } else {
        None
    }
}

fn split_list(v: &str) -> Vec<String> {
    v.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `true` and `false` with their usual spellings, and nothing else: a boolean read
/// loosely is how `ture` came to mean false.
fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// The known key closest to `key` by edit distance, when one is within two edits.
fn nearest<'k>(key: &str, known: &[&'k str]) -> Option<&'k str> {
    known
        .iter()
        .copied()
        .map(|k| (edits(key, k), k))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, k)| k)
}

fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0 ..= b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let held = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(held + 1);
            prev = held;
        }
    }
    row[b.len()]
}
