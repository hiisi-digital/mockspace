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

#[test]
fn an_overlay_added_after_a_read_is_what_the_next_read_answers() {
    let dir = Dir::new("overlay-late");
    dir.write("a.txt", "on disk").write("d/b.txt", "");
    let reading = Reading::new(&dir.0).with_threads(3);
    assert_eq!(reading.view().text("a.txt"), Some("on disk"), "the control: the disk is read first");
    let names = |r: &Reading| -> Vec<String> {
        r.view().list("d").unwrap().entries().iter().map(|e| e.name().to_string()).collect()
    };
    assert_eq!(names(&reading), ["b.txt"]);

    let reading = reading.with_overlay("a.txt", "the edit").with_overlay("d/c.txt", "x");
    assert_eq!(reading.view().text("a.txt"), Some("the edit"));
    assert_eq!(names(&reading), ["b.txt", "c.txt"], "a listing read before the overlay is not kept");
    assert_eq!(reading.threads(), 3, "the threads are kept");
}
