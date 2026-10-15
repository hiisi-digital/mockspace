//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Files in the documents directory that are not generated, and the image that
//! follows its diagram.
//!
//! The attribute covers the directory, so the driver is what tells the files
//! apart: a hand-written file merges as git merges it, and a rendered image,
//! which has no header to read, is generated when the diagram of the same stem
//! is.

mod common;

use std::fs;

use common::{Repo, write};

#[test]
fn a_hand_written_file_in_the_documents_directory_still_conflicts() {
    // `docs/` holds files nobody generates (`sweep_retired` leaves them), and
    // the attribute covers the directory, so the driver is what tells the two
    // apart. Keeping ours here would drop somebody's edit without a word.
    let r = Repo::new();
    write(&r.root.join("docs/CONTRIBUTING.md"), "one\ntwo\nthree\n");
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: add contributing"]);
    r.git(&["switch", "-q", "-c", "a"]);
    write(
        &r.root.join("docs/CONTRIBUTING.md"),
        "one\ntwo, on a\nthree\n",
    );
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: edit on a"]);
    r.git(&["switch", "-q", "dev"]);
    write(
        &r.root.join("docs/CONTRIBUTING.md"),
        "one\ntwo, on dev\nthree\n",
    );
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: edit on dev"]);
    r.generate();

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(!merged.status.success(), "{}", Repo::failure(&merged));
    assert_eq!(r.unmerged(), ["docs/CONTRIBUTING.md"]);
    let text = fs::read_to_string(r.root.join("docs/CONTRIBUTING.md")).unwrap();
    assert!(
        text.contains("two, on a") && text.contains("two, on dev"),
        "{text}"
    );
}

#[test]
fn a_hand_written_file_in_the_documents_directory_merges_when_the_edits_are_apart() {
    let r = Repo::new();
    write(
        &r.root.join("docs/CONTRIBUTING.md"),
        "one\ntwo\nthree\nfour\nfive\n",
    );
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "--no-verify", "-m", "docs: add contributing"]);
    r.git(&["switch", "-q", "-c", "a"]);
    write(
        &r.root.join("docs/CONTRIBUTING.md"),
        "one, on a\ntwo\nthree\nfour\nfive\n",
    );
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: edit on a"]);
    r.git(&["switch", "-q", "dev"]);
    write(
        &r.root.join("docs/CONTRIBUTING.md"),
        "one\ntwo\nthree\nfour\nfive, on dev\n",
    );
    r.git(&["commit", "-q", "--no-verify", "-am", "docs: edit on dev"]);

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    let text = fs::read_to_string(r.root.join("docs/CONTRIBUTING.md")).unwrap();
    assert_eq!(text, "one, on a\ntwo\nthree\nfour\nfive, on dev\n");
}

#[test]
fn a_rendered_image_follows_the_diagram_beside_it() {
    // The png carries no header to read, so it is the generated diagram of the
    // same stem that says the image is generated. Two sides that each changed
    // the bytes are not a conflict, same as the document.
    let r = Repo::new();
    r.git(&["switch", "-q", "-c", "a"]);
    r.side("a", "one", "one, edited on a");
    r.git(&["switch", "-q", "dev"]);
    r.side("dev", "five", "five, edited on dev");
    let png = "docs/STRUCTURE.GRAPH.png";
    assert!(
        r.root.join(png).exists(),
        "the fixture renders the image (graphviz)"
    );
    write(&r.root.join(png), "dev bytes\n");
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: png on dev"]);
    r.git(&["switch", "-q", "a"]);
    write(&r.root.join(png), "a bytes\n");
    r.git(&["commit", "-q", "--no-verify", "-am", "chore: png on a"]);
    r.git(&["switch", "-q", "dev"]);

    let merged = r.git_out(&["merge", "a", "-m", "chore: merge a"]);
    assert!(merged.status.success(), "{}", Repo::failure(&merged));
    assert!(r.unmerged().is_empty());
}
