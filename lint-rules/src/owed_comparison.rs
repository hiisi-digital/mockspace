//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the changelist that replaces a deprecated one owes the `deprecation-comparison` lint,
//! worked out from the deprecated text alone.
//!
//! The lint refuses a replacement at the push, and each of its three demands is a function of the
//! deprecated text, so the author is told them when the deprecation happens and need not meet them
//! one refusal at a time.

use crate::changelist_helpers::ClStatus;
use crate::deprecation_comparison::{extract_h2_headers, owes_a_comparison};
use crate::type_scanner;

/// The comparison a replacement owes, as the structure to write under and the names to mention.
///
/// The `section` is only the structure: the header, and one `###` subheader for each `##` header of
/// the deprecated text, written in lower case. The lint compares subheaders in lower case and looks
/// for the names with a case-sensitive search, so a name carried in a header (`## The `Marker`
/// type`) does not turn up in the lower-cased subheader and pasting the section does not satisfy the
/// lint by itself. A name that is already lower case is the one exception, and `met_by_the_section`
/// lists those.
pub struct OwedComparison {
    /// The section to paste into the replacement and write under.
    pub section:            String,
    /// The backtick-wrapped names the section must mention, sorted.
    pub names:              Vec<String>,
    /// The names among `names` that the section's own words already contain, so that the lint counts
    /// them mentioned before anything is written. They are macro names a header carries in lower
    /// case, and the author owes each of them a comparison all the same.
    pub met_by_the_section: Vec<String>,
}

/// The comparison a replacement of `deprecated_text` owes, or `None` for a stub that owes none.
pub fn owed_comparison(deprecated_text: &str) -> Option<OwedComparison> {
    if !owes_a_comparison(deprecated_text, ClStatus::Active) {
        return None;
    }
    let mut section = String::from("## Comparison to deprecated changelist\n");
    for header in extract_h2_headers(deprecated_text) {
        section.push_str("\n### ");
        section.push_str(&header.to_lowercase());
        section.push('\n');
    }
    let names = type_scanner::extract_backtick_names(deprecated_text);
    let met_by_the_section = names.iter().filter(|n| section.contains(n.as_str())).cloned().collect();
    Some(OwedComparison {
        section,
        names,
        met_by_the_section,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changelist_helpers::ClKind;
    use crate::deprecation_comparison::check_comparison;

    fn errors_for(deprecated: &str, replacement_section: &str) -> Vec<String> {
        check_comparison("old.deprecated.md", deprecated, "new.doc.md", replacement_section, ClKind::Doc)
            .into_iter()
            .map(|e| e.message)
            .collect()
    }

    #[test]
    fn a_bare_title_owes_nothing_to_write() {
        assert!(owed_comparison("# src changelist\n\n").is_none());
    }

    #[test]
    fn the_scaffold_has_every_subheader_the_lint_asks_for() {
        let deprecated = "## Overview\nUses `StorageRecord`.\n## Types\nThe `Marker` type.\n";
        let owed = owed_comparison(deprecated).expect("a changelist with content owes one");
        let msgs = errors_for(deprecated, &owed.section);
        assert!(
            !msgs.iter().any(|m| m.contains("is missing a `###")) && !msgs.iter().any(|m| m.contains("missing a `## Comparison")),
            "structure must be satisfied by the scaffold, got: {msgs:?}"
        );
    }

    #[test]
    fn the_scaffold_alone_still_owes_every_name() {
        let deprecated = "## Overview\nUses `StorageRecord` and `Behavior`.\n";
        let owed = owed_comparison(deprecated).unwrap();
        let msgs = errors_for(deprecated, &owed.section);
        for name in ["StorageRecord", "Behavior"] {
            assert!(
                msgs.iter().any(|m| m.contains(&format!("`{name}`"))),
                "pasting the scaffold must not satisfy the check on `{name}`"
            );
        }
        assert_eq!(owed.names, ["Behavior", "StorageRecord"]);
        assert!(owed.met_by_the_section.is_empty());
    }

    /// A name a header carries, and a name that is a substring of a header, were both counted as
    /// mentioned by the verbatim header copy, so the bare section passed the lint.
    #[test]
    fn a_name_in_or_inside_a_header_is_not_met_by_the_scaffold() {
        let deprecated = "## The `Marker` type\nUses `Marker`.\n## Types\nUses `Type`.\n";
        let owed = owed_comparison(deprecated).unwrap();
        assert!(owed.section.contains("### the `marker` type"), "got: {}", owed.section);
        assert!(owed.section.contains("### types"), "got: {}", owed.section);
        let msgs = errors_for(deprecated, &owed.section);
        for name in ["Marker", "Type"] {
            assert!(
                msgs.iter().any(|m| m.contains(&format!("does not mention `{name}`"))),
                "the bare scaffold must still owe `{name}`, got: {msgs:?}"
            );
        }
        assert!(owed.met_by_the_section.is_empty(), "got: {:?}", owed.met_by_the_section);
    }

    /// A lower-case macro name in a header cannot be kept out of the subheader, since the lint
    /// matches the subheader to the header, so it is reported rather than hidden.
    #[test]
    fn a_lower_case_macro_name_in_a_header_is_reported_as_met() {
        let deprecated = "## Uses `vec!`\nCalls `vec!` and `Other`.\n";
        let owed = owed_comparison(deprecated).unwrap();
        assert_eq!(owed.names, ["Other", "vec!"]);
        assert_eq!(owed.met_by_the_section, ["vec!"]);
        let msgs = errors_for(deprecated, &owed.section);
        assert!(
            !msgs.iter().any(|m| m.contains("`vec!`")),
            "the lint counts `vec!` met by the header, which is why it is reported: {msgs:?}"
        );
        assert!(msgs.iter().any(|m| m.contains("`Other`")), "got: {msgs:?}");
    }

    #[test]
    fn the_scaffold_filled_in_passes_the_lint() {
        let deprecated = "## Overview\nUses `StorageRecord`.\n## Types\nThe `Marker` type.\n";
        let owed = owed_comparison(deprecated).unwrap();
        let filled = format!("{}\nChanged `StorageRecord` and the `Marker` type.\n", owed.section);
        let msgs = errors_for(deprecated, &filled);
        assert!(msgs.is_empty(), "got: {msgs:?}");
    }
}
