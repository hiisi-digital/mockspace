//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `deprecate` and `unlock` tell the author the replacement changelist owes the
//! `deprecation-comparison` lint.

use mockspace_lint_rules as lint_rules;

/// What the changelist that replaces a deprecated one owes the `deprecation-comparison` lint, worded
/// for the author, or `None` for a stub that owes nothing. The lint refuses the replacement at the
/// push, and everything it asks follows from the deprecated text, so it is said when the deprecation
/// happens.
///
/// The lint asks this of each deprecated changelist of the kind that is still in `design_rounds`,
/// and the note covers the one deprecated now.
pub(crate) fn owed_note(deprecated_text: &str) -> Option<String> {
    let owed = lint_rules::owed_comparison(deprecated_text)?;
    let mut note = String::from(
        "  the replacement changelist owes the `deprecation-comparison` lint this section, one subheader for each header of the deprecated text:\n",
    );
    for line in owed.section.lines() {
        if !line.is_empty() {
            note.push_str("    ");
            note.push_str(line);
        }
        note.push('\n');
    }
    if !owed.names.is_empty() {
        let names: Vec<String> = owed.names.iter().map(|n| format!("`{n}`")).collect();
        note.push_str(&format!("  and the section must mention each of: {}\n", names.join(", ")));
    }
    if !owed.met_by_the_section.is_empty() {
        let met: Vec<String> = owed.met_by_the_section.iter().map(|n| format!("`{n}`")).collect();
        note.push_str(&format!(
            "  the lint counts {} as mentioned by the subheaders alone, and the comparison still owes them a word\n",
            met.join(", ")
        ));
    }
    Some(note)
}

/// Say what the replacement of the changelist now at `deprecated` owes.
pub(super) fn say_what_the_replacement_owes(deprecated: &std::path::Path) {
    if let Some(note) = std::fs::read_to_string(deprecated).ok().as_deref().and_then(owed_note) {
        eprint!("{note}");
    }
}
