//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the commit-style documentation says, read from what the commit gate enforces.
//!
//! The gate is the builtin `commit-subject` lint, configured under
//! `[lints.commit-subject]` in `mockspace.toml`. The generated commit-style skill and the
//! `{{commit_types}}` and `{{commit_rules}}` variables a consumer's own template may use are
//! built from the same parameters through the lint's own parsing, so a type a project adds is
//! in the gate and in the documentation by the one edit, and the two cannot list different sets.

use std::path::Path;

use mockspace_lint_rules::commit_subject::{CommitSubject, SubjectCase};
use mockspace_lint_rules::{AgentMode, MessageContext, MessageDomain, MessageLint};

use crate::config::Config;

/// The convention the gate enforces for this project.
pub(crate) fn policy(cfg: &Config) -> CommitSubject {
    CommitSubject::from_params(cfg.lint_overrides.params.get("commit-subject"))
}

/// The permitted types as a markdown list: `` `feat`, `fix` ``.
pub(crate) fn types_text(policy: &CommitSubject) -> String {
    policy
        .types()
        .iter()
        .map(|t| format!("`{t}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The rules the gate enforces, one numbered line each, in the order a subject is read.
///
/// A rule appears only when the gate enforces it, so a project that loosens a knob does not
/// have the skill keep telling agents the stricter thing.
pub(crate) fn rules_text(policy: &CommitSubject) -> String {
    let sep = policy.separator();
    let mut lines: Vec<String> = Vec::new();
    if policy.requires_type() {
        lines.push("Type is required".to_string());
    }
    lines.push(format!("Exactly `{sep}` after the type"));
    if policy.allows_scope() {
        lines.push("A scope in parentheses after the type is allowed".to_string());
    } else {
        lines.push("NO PARENTHESES after the type, never `type(scope):`".to_string());
    }
    match policy.subject_case() {
        SubjectCase::Lower => lines.push("Lowercase message".to_string()),
        SubjectCase::Sentence => lines.push("Message starts uppercase".to_string()),
        SubjectCase::Any => {},
    }
    if policy.forbids_trailing_period() {
        lines.push("No period at end".to_string());
    }
    if policy.requires_blank_line() {
        lines.push("A blank line between the subject and the body".to_string());
    }
    lines.push(
        "Imperative mood (\"add\" not \"added\"); the gate cannot check this one".to_string(),
    );
    if policy.max_subject() > 0 {
        lines.push(format!(
            "At most {} characters in the subject",
            policy.max_subject()
        ));
    }
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}. {l}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

/// How a breaking change is written, or nothing when the project has no marker.
pub(crate) fn breaking_text(policy: &CommitSubject) -> String {
    let mark = policy.breaking_marker();
    if mark.is_empty() {
        return String::new();
    }
    format!(
        "Breaking: `feat{mark}{sep}message` or `refactor{mark}{sep}message`",
        sep = policy.separator()
    )
}

/// One example subject per permitted type, each passing the gate under this policy.
///
/// The messages for the default types are fixed and the rest say what to write, since the skill
/// cannot know what a project's own type means. Each candidate is built from the policy's
/// separator and case and then read by the lint itself, so an example never breaks the rules
/// printed above it: where the project's length limit is too short for the message, a shorter
/// candidate stands in, and a type with no candidate that fits has no example.
pub(crate) fn examples_text(policy: &CommitSubject) -> String {
    let sep = policy.separator();
    let upper = policy.subject_case() == SubjectCase::Sentence;
    let root = Path::new(".");
    policy
        .types()
        .iter()
        .filter_map(|ty| {
            let first = match ty.as_str() {
                "feat" => "add catalogue entry validation",
                "fix" => "handle missing loadout in project load",
                "refactor" => "extract platform detection to shared crate",
                "docs" => "update data store design",
                "test" => "add pipeline manifest tests",
                "chore" => "bump clap to 4.5",
                "bench" => "land the linux results of the wide matrix",
                "release" => "0.4.0",
                _ => "describe the change in one line",
            };
            [first, "describe the change", "change it", "x"]
                .into_iter()
                .map(|message| {
                    let message = if upper { capitalised(message) } else { message.to_string() };
                    format!("{ty}{sep}{message}")
                })
                .find(|subject| {
                    let ctx = MessageContext::new(
                        MessageDomain::CommitMessage,
                        AgentMode::Assistant,
                        subject,
                        "example",
                        root,
                    );
                    policy.check_message(&ctx).is_empty()
                })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// The skill body: the template with every token filled from the gate's configuration.
pub(crate) fn skill_body(template: &str, policy: &CommitSubject, mock_dir: &str) -> String {
    template
        .replace("{separator}", policy.separator())
        .replace("{commit_rules}", &rules_text(policy))
        .replace("{commit_types}", &types_text(policy))
        .replace("{commit_breaking}", &breaking_text(policy))
        .replace("{commit_examples}", &examples_text(policy))
        .replace("{mock_dir}", mock_dir)
}

#[cfg(test)]
#[path = "commit_style_tests.rs"]
mod tests;
