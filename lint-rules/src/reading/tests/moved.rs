//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What the reading keeps of a crate's sources stays good when the reading is moved.
//!
//! A crate's files are kept by the reading the first time they are asked, and a kept file reaches
//! its tree through what it holds. Had it held the reading itself, a move of the reading after that
//! first ask would leave it pointing at the place the reading was, and the tree would be read from
//! whatever was put there since. The reading is moved here between the ask and the tree, and the
//! place it left is written over.

use super::*;

#[inline(never)]
fn read_once_and_hand_back(dir: &Path) -> Reading {
    let reading = Reading::new(dir);
    let files = reading.crate_sources(&dir.join("k"));
    assert_eq!(files.len(), 1, "the control: the crate's one file is found");
    reading
}

/// Write over the stack where the moved reading stood.
#[inline(never)]
fn clobber() -> u64 {
    let mut junk = [0xAAu8; 16 * 1024];
    std::hint::black_box(&mut junk);
    junk.iter().map(|b| u64::from(*b)).sum()
}

#[test]
fn a_kept_source_reaches_its_tree_after_the_reading_moved() {
    let dir = Dir::new("moved");
    dir.write("k/src/lib.rs", "pub fn after_the_move() {}\n");
    let moved = Box::new(read_once_and_hand_back(&dir.0));
    std::hint::black_box(clobber());
    let file = moved.crate_sources(&dir.0.join("k"))[0];
    let tree = file.tree();
    let root = tree.root_node();
    assert_eq!(root.kind(), "source_file");
    let name = root
        .child(0)
        .and_then(|item| item.child_by_field_name("name"))
        .and_then(|n| n.utf8_text(file.text.as_bytes()).ok());
    assert_eq!(name, Some("after_the_move"), "{}", root.to_sexp());
    assert_eq!(moved.counts().parses, 1, "parsed once, into the reading that moved");
}
