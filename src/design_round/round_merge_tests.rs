//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

// Each test names the mutant that turns it red.

#[test]
fn identical_text_is_no_collision() {
    // Mutant: `verdict` reports a collision for any add/add.
    assert_eq!(verdict("", "round: x\n", "round: x\n"), Verdict::Same);
}

#[test]
fn added_on_both_sides_with_different_text_is_a_collision() {
    // Mutant: the empty-base test is dropped, so an add/add merges as text.
    assert_eq!(
        verdict("", "closed: aaa\n", "closed: bbb\n"),
        Verdict::Collision
    );
    assert_eq!(
        verdict("  \n", "closed: aaa\n", "closed: bbb\n"),
        Verdict::Collision
    );
}

#[test]
fn a_change_from_a_common_base_is_a_text_merge() {
    // Mutant: every difference is called a collision.
    assert_eq!(
        verdict("closed: 000\n", "closed: aaa\n", "closed: 000\n"),
        Verdict::Text
    );
}

#[test]
fn the_round_is_named_from_the_path() {
    // Mutant: `round_of` reads the wrong component.
    assert_eq!(
        round_of("mock/design_rounds/202610221529/.meta"),
        Some("202610221529")
    );
    assert_eq!(round_of("design_rounds/202610221529-kx3f/.meta"), Some("202610221529-kx3f"));
    assert_eq!(round_of("mock/design_rounds/202610221529/.history"), None);
}

#[test]
fn the_message_names_the_id_and_the_one_command() {
    // Mutant: the message loses the id, or the command.
    let msg = collision_message("mock/design_rounds/202610221529/.meta");
    assert!(msg.contains("`202610221529` was closed on both branches"), "{msg}");
    assert!(msg.contains("cargo mock rename-round 202610221529"), "{msg}");
    assert!(msg.contains("git merge --abort"), "{msg}");
}
