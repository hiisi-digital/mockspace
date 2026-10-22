//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

// Each test names the mutant that turns it red.

fn touch(dir: &Path, name: &str, text: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), text).unwrap();
}

fn listing(dir: &Path) -> Vec<String> {
    names_in(dir)
}

fn closed_round(dr: &Path, dir: &str) {
    let d = dr.join(dir);
    touch(&d, "202610221529_changelist.doc.lock.md", "# doc\nbody\n");
    touch(&d, "202610221529_changelist.src.lock.md", "# src\nbody\n");
    touch(&d, "202610221525_topic.earlier.md", "x");
    touch(&d, ".meta", &format!("round: {dir}\nclosed: abc\nclose_date: 2026-10-22\nabandoned: false\n"));
    touch(&d, ".history", "abc one\n");
}

#[test]
fn a_closed_round_is_renamed_whole() {
    // Mutants: the directory is not moved; the files keep the old id; `.meta` keeps
    // the old `round:`; a file carrying another stamp is renamed too.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    let r = rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f");
    assert!(!dr.join("202610221529").exists());
    let new = dr.join("202610221529-kx3f");
    assert_eq!(
        listing(&new),
        [
            ".history",
            ".meta",
            "202610221525_topic.earlier.md",
            "202610221529-kx3f_changelist.doc.lock.md",
            "202610221529-kx3f_changelist.src.lock.md",
        ]
    );
    let meta = fs::read_to_string(new.join(".meta")).unwrap();
    assert!(meta.starts_with("round: 202610221529-kx3f\n"), "{meta}");
    assert!(meta.contains("closed: abc\n") && meta.contains("abandoned: false\n"), "{meta}");
    assert_eq!(r.rewrote, [new.join(".meta")]);
    // every file that moved is listed, for the commit
    assert_eq!(r.moves.len(), 5);
    assert!(r.moves.iter().all(|(from, to)| !from.exists() && to.exists()));
}

#[test]
fn the_renamed_round_still_reads_as_a_round() {
    // Mutant: the new names are not parsed by the changelist reader.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    let cls = changelist_helpers::find_changelists(&dr.join("202610221529-kx3f"));
    assert_eq!(cls.len(), 2);
    assert!(lint_rules::round_id::is_round_dir("202610221529-kx3f"));
}

#[test]
fn an_abandoned_round_keeps_its_suffix_and_a_count_goes() {
    // Mutant: `kept_tail` keeps `-2`, or drops `-abandoned`.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529-abandoned");
    let r = rename_round_on_disk(dr, "202610221529-abandoned", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f-abandoned");
    closed_round(dr, "202610221530-2");
    let r = rename_round_on_disk(dr, "202610221530-2", "202610221530-aaaa").unwrap();
    assert_eq!(r.new_name, "202610221530-aaaa");
}

#[test]
fn the_active_round_is_renamed_by_its_flat_files() {
    // Mutant: flat files are not looked for, or a topic of another stamp moves.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    touch(dr, "202610221529_changelist.doc.md", "x");
    touch(dr, "202610221529_topic.a.md", "x");
    touch(dr, "202610221500_topic.other.md", "x");
    touch(dr, "README.md", "x");
    let r = rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    assert_eq!(r.new_name, "202610221529-kx3f");
    assert_eq!(
        listing(dr),
        [
            "202610221500_topic.other.md",
            "202610221529-kx3f_changelist.doc.md",
            "202610221529-kx3f_topic.a.md",
            "README.md",
        ]
    );
}

#[test]
fn a_directory_wins_over_flat_files_with_the_same_id() {
    // Mutant: the flat branch runs first.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    touch(dr, "202610221529_topic.next.md", "x");
    rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap();
    assert!(dr.join("202610221529_topic.next.md").exists());
    assert!(dr.join("202610221529-kx3f").is_dir());
}

#[test]
fn a_target_that_exists_is_refused_and_nothing_moves() {
    // Mutant: `rename` overwrites, or is attempted before the check.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    closed_round(dr, "202610221529");
    closed_round(dr, "202610221529-kx3f");
    let err = rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(dr.join("202610221529/202610221529_changelist.doc.lock.md").exists());

    touch(dr, "202610221600_topic.a.md", "x");
    touch(dr, "202610221600-aaaa_topic.a.md", "x");
    let err = rename_round_on_disk(dr, "202610221600", "202610221600-aaaa").unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    assert!(dr.join("202610221600_topic.a.md").exists());
}

#[test]
fn an_unknown_round_or_a_bad_new_id_is_an_error() {
    // Mutant: a missing round is a silent success; a malformed id is taken.
    let tmp = tempfile::tempdir().unwrap();
    let dr = tmp.path();
    assert!(rename_round_on_disk(dr, "202610221529", "202610221529-kx3f").is_err());
    closed_round(dr, "202610221529");
    assert!(rename_round_on_disk(dr, "202610221529", "202610221529-KX3F").is_err());
    assert!(rename_round_on_disk(dr, "202610221529", "2026").is_err());
    assert!(rename_round_on_disk(dr, "README", "202610221529-kx3f").is_err());
}

#[test]
fn only_the_round_line_of_meta_is_rewritten() {
    // Mutant: a blanket replace of the old id in the text.
    let text = "round: 202610221529\nclosed: 202610221529\nnote: round: 202610221529\r\n";
    assert_eq!(
        rewrite_meta(text, "202610221529", "202610221529-kx3f"),
        "round: 202610221529-kx3f\nclosed: 202610221529\nnote: round: 202610221529\r\n"
    );
    assert_eq!(
        rewrite_meta("round: 202610221529\r\nx\r\n", "202610221529", "202610221529-kx3f"),
        "round: 202610221529-kx3f\r\nx\r\n"
    );
}
