//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit subjects the round commands commit with or print as a suggestion.
//!
//! They are all here so one test can hold them to the shape a repository's commit-msg gate
//! enforces: `type: lowercase imperative`, under 72 characters, no parentheses, no trailing
//! period. The tool's own suggestion has to pass the gate it runs beside.

/// A changelist filename reduced to its round id, the part before the first underscore.
fn round_id(filename: &str) -> &str {
    filename.split_once('_').map_or(filename, |(id, _)| id)
}

pub(crate) fn lock_doc(new_name: &str) -> String {
    format!("chore: lock doc changelist {}", round_id(new_name))
}

pub(crate) fn lock_src(new_name: &str) -> String {
    format!("chore: lock src changelist {}", round_id(new_name))
}

pub(crate) fn deprecate_strays(filenames: &[&str]) -> String {
    let ids: Vec<&str> = filenames.iter().map(|f| round_id(f)).collect();
    format!("chore: deprecate stray changelist {}", ids.join(", "))
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

pub(crate) fn rename_round(old: &str, new: &str) -> String {
    format!("chore: rename design round {old} to {new}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design_round::ArchiveKind;

    /// The shape the commit-msg gate holds a subject to.
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
    fn every_subject_the_round_commands_print_passes_the_gate() {
        let long_cl = "202610221529-kx3f_changelist.src.lock.md";
        let id = "202610221529-kx3f-abandoned";
        let subjects = [
            ArchiveKind::Closed.commit_subject(id),
            ArchiveKind::Abandoned.commit_subject(id),
            lock_doc(long_cl),
            lock_src(long_cl),
            deprecate_strays(&[long_cl, "202610221540-kx3f_changelist.doc.md"]),
            deprecate_doc(long_cl),
            deprecate_src_and_unlock(long_cl),
            unlock_round(),
            migrate(12),
            rename_round("202610221529-kx3f", "202610221530-ab12"),
        ];
        for s in &subjects {
            assert_eq!(gate_refusal(s), None, "{s}");
        }
    }
}
