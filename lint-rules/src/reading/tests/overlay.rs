//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! An overlaid reading judges the text it was given, and a run over it saves nothing.

use std::sync::atomic::Ordering;

use super::*;

#[test]
fn an_overlay_judges_the_replaced_text_and_writes_nothing() {
    let dir = Dir::new("overlay");
    dir.write("a.txt", "on disk");
    let cache = Mem::default();
    let (probe, judged) = Probe::new(|v| v.text("a.txt").unwrap_or("absent").to_string());
    let pack = pack_of(probe);

    // A saved unit for the text on disk, which the overlay must not replay.
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["on disk"]);
    let saved = cache.remembered();
    assert!(saved > 0, "the control: a plain run does save");

    let overlaid = Reading::new(&dir.0).with_overlay("a.txt", "the edit");
    let ran = run_repo_over(overlaid, &dir, &cache, &pack, None);
    assert_eq!(messages(&ran.found), ["the edit"]);
    assert_eq!(cache.remembered(), saved, "an overlaid run writes nothing to the store");
    assert_eq!(std::fs::read_to_string(dir.0.join("a.txt")).unwrap(), "on disk");
    assert_eq!(judged.load(Ordering::SeqCst), 2);

    // And the next plain run replays what the disk says, untouched by the edit.
    assert_eq!(messages(&run_repo(&dir, &cache, &pack, None).found), ["on disk"]);
    assert_eq!(judged.load(Ordering::SeqCst), 2);
}

#[test]
fn an_overlay_adds_a_file_to_the_listing_it_sits_in() {
    let dir = Dir::new("overlay-list");
    dir.write("d/a.txt", "");
    let reading = Reading::new(&dir.0).with_overlay("d/new.txt", "x");
    let names: Vec<String> = reading
        .view()
        .list("d")
        .unwrap()
        .entries()
        .iter()
        .map(|e| e.name().to_string())
        .collect();
    assert_eq!(names, ["a.txt", "new.txt"]);
}
