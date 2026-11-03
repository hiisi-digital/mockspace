//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `deprecate` and `unlock` say the replacement changelist owes the `deprecation-comparison`
//! lint.

use crate::design_round::owed::owed_note;

#[test]
fn a_deprecation_tells_the_author_what_the_replacement_owes() {
    let note = owed_note("## Overview\nUses `StorageRecord` and `Behavior`.\n## Types\nThe `Marker` type.\n")
        .expect("a changelist with content owes a comparison");
    for want in [
        "## Comparison to deprecated changelist",
        "### overview",
        "### types",
        "and the section must mention each of: `Behavior`, `Marker`, `StorageRecord`\n",
    ] {
        assert!(note.contains(want), "the note must carry {want:?}, got: {note}");
    }
    assert!(!note.contains("mentioned by the subheaders alone"), "nothing is met by the section here, got: {note}");
}

#[test]
fn a_stub_deprecation_says_nothing_about_a_comparison() {
    assert!(owed_note("# src changelist\n\n").is_none());
}

#[test]
fn a_macro_name_in_a_header_is_said_to_be_met_by_the_subheaders() {
    let note = owed_note("## Uses `vec!`\nCalls `vec!` and `Other`.\n").unwrap();
    assert!(
        note.contains("the lint counts `vec!` as mentioned by the subheaders alone"),
        "got: {note}"
    );
}
