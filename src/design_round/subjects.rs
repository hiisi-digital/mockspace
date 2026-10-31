//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit subjects the round commands commit with or print as a suggestion.
//!
//! They are all here so one test can hold them to the shape a repository's commit-msg gate
//! enforces: `type: lowercase imperative`, under 72 characters, no parentheses, no trailing
//! period. The tool's own suggestion has to pass the gate it runs beside.

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

pub(crate) fn migrate(renamed: u32) -> String {
    format!("chore: migrate {renamed} design round files to the new naming")
}

/// Names the new directory only: the old one carries the same tail, and both would not fit.
pub(crate) fn rename_round(new: &str) -> String {
    format!("chore: rename design round to {new}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_round::ArchiveKind;

    /// The shape the commit-msg gate holds a subject to. The gate itself is a message lint a project
    /// imports from its packs, which mockspace does not carry, so this is a model of it.
    /// The model is held against the subject the tool used to print, below.
    fn gate_refusal(s: &str) -> Option<String> {
        let Some((ty, rest)) = s.split_once(": ") else {
            return Some("no `type: ` prefix".into());
        };
        if ty.is_empty() || !ty.chars().all(|c| c.is_ascii_lowercase()) {
            return Some(format!("type `{ty}` is not lowercase"));
        }
        if rest.chars().next().is_none_or(|c| !c.is_lowercase()) {
            return Some("message does not lead lowercase".into());
        }
        if s.chars().count() >= 72 {
            return Some(format!("{} characters, not under 72", s.chars().count()));
        }
        if s.contains('(') || s.contains(')') {
            return Some("contains parentheses".into());
        }
        if s.ends_with('.') {
            return Some("trailing period".into());
        }
        if s.contains('\u{2014}') || s.contains('\u{2013}') {
            return Some("contains a dash glyph".into());
        }
        None
    }

    #[test]
    fn the_gate_model_refuses_what_the_gate_refuses() {
        // the control: the subject `archive` used to print
        assert!(gate_refusal("chore: archive design round 202604191100-abandoned (abandoned)").is_some());
        assert!(gate_refusal("chore: close design round 202604191100").is_none());
        assert!(gate_refusal("Chore: x").is_some());
        assert!(gate_refusal("chore: Close it").is_some());
        assert!(gate_refusal("chore: close it.").is_some());
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
            migrate(u32::MAX),
            rename_round(&format!("202610221530-ab12-abandoned-10")),
            rename_round(id),
        ];
        // one, two and many strays are one shape
        subjects.extend([1usize, 2, 3, 40, usize::MAX].map(deprecate_strays));
        for s in &subjects {
            assert_eq!(gate_refusal(s), None, "{s}");
        }
    }
}
