//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit subjects the round commands commit with or print as a suggestion.
//!
//! They are all here so one test can hold them to what the builtin `commit-subject` lint
//! enforces by default: a listed type, `: `, a lowercase message, no parenthesised scope, no
//! trailing period, under 72 characters. The tool's own suggestion has to pass the gate it runs
//! beside, and the test asks that lint itself.

/// A changelist filename reduced to its round id, which the id grammar owns.
fn round_id(filename: &str) -> &str {
    mockspace_lint_rules::round_id::base_id(filename).unwrap_or(filename)
}

pub(crate) fn lock_doc(new_name: &str) -> String {
    format!("chore: lock doc changelist {}", round_id(new_name))
}

pub(crate) fn lock_src(new_name: &str) -> String {
    format!("chore: lock src changelist {}", round_id(new_name))
}

/// However many strays there are, the subject does not grow with them.
pub(crate) fn deprecate_strays(count: usize) -> String {
    let noun = if count == 1 { "changelist" } else { "changelists" };
    format!("chore: deprecate {count} stray {noun}")
}

pub(crate) fn deprecate_doc(filename: &str) -> String {
    format!("chore: deprecate doc changelist {}", round_id(filename))
}

pub(crate) fn deprecate_src_and_unlock(filename: &str) -> String {
    format!("chore: deprecate src changelist {} and unlock doc", round_id(filename))
}

pub(crate) fn unlock_round() -> String {
    "chore: unlock design round, deprecate src and unlock doc".to_string()
}

/// Names the new directory only: the old one carries the same tail, and both would not fit.
pub(crate) fn rename_round(new: &str) -> String {
    format!("chore: rename design round to {new}")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mockspace_lint_rules::{AgentMode, MessageContext, MessageDomain, check_message_with_extra};

    use super::*;
    use crate::design_round::ArchiveKind;

    /// What the builtin `commit-subject` lint finds in `s` under its defaults, which is what the
    /// commit-msg gate holds a repository with no `[lints.commit-subject]` table to. The lint is
    /// asked itself and nothing models it.
    fn gate_findings(s: &str) -> Vec<&'static str> {
        let root = Path::new("/tmp");
        let ctx = MessageContext::new(
            MessageDomain::CommitMessage,
            AgentMode::Assistant,
            s,
            "COMMIT_EDITMSG",
            root,
        );
        check_message_with_extra(&ctx, None, &[])
            .into_iter()
            .filter_map(|e| e.finding_kind)
            .collect()
    }

    #[test]
    fn the_gate_refuses_what_these_subjects_must_not_look_like() {
        // the control: a subject that was refused, and each shape the gate refuses
        assert_eq!(gate_findings("chore: close design round 202604191100"), Vec::<&str>::new());
        assert_eq!(gate_findings("Chore: x"), vec!["unknown-type"]);
        assert_eq!(gate_findings("chore: Close it"), vec!["subject-case"]);
        assert_eq!(gate_findings("chore: close it."), vec!["trailing-period"]);
        assert_eq!(gate_findings("close the round"), vec!["missing-type"]);
        assert_eq!(gate_findings("chore(round): close it"), vec!["scope-forbidden"]);
        assert_eq!(gate_findings(&format!("chore: {}", "x".repeat(70))), vec!["subject-length"]);
    }

    #[test]
    fn every_subject_the_round_commands_print_passes_the_gate_at_the_longest_the_grammar_allows() {
        // the longest id: the stamp and a four-character tag; the longest tail: `-abandoned` with a
        // two-digit collision count
        let id = "202610221529-kx3f";
        let dir = format!("{id}-abandoned-10");
        let cl = |kind: &str| format!("{id}_changelist.{kind}.lock.md");
        let mut subjects = vec![
            ArchiveKind::Closed.commit_subject(&dir),
            ArchiveKind::Abandoned.commit_subject(&dir),
            lock_doc(&cl("doc")),
            lock_src(&cl("src")),
            deprecate_doc(&cl("doc")),
            deprecate_src_and_unlock(&cl("src")),
            unlock_round(),
            rename_round(&format!("202610221530-ab12-abandoned-10")),
            rename_round(id),
            // the two the generated-files follow-up commits with, fixed text
            crate::bootstrap::FOLLOW_UP_SUBJECT.to_string(),
            crate::bootstrap::FOLLOW_UP_SUBJECT_REBASE.to_string(),
        ];
        // one, two and many strays are one shape
        subjects.extend([1usize, 2, 3, 40, usize::MAX].map(deprecate_strays));
        for s in &subjects {
            assert_eq!(gate_findings(s), Vec::<&str>::new(), "{s}");
        }
    }
}
