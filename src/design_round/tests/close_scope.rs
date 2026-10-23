//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What `close` takes into the round's directory, and what it leaves flat.
//!
//! An active round is the flat files of `design_rounds/`, and a topic filed
//! while the round was in flight, or parked for a later one, sits beside them.
//! A close that moved every flat file filed that topic under the closed round,
//! where it read as finished.

use std::path::Path;
use std::process::ExitCode;

use crate::config::Config;
use crate::design_round::*;

const OPTS: SubcmdOpts = SubcmdOpts {
    auto_commit: false,
};

const DOC_LOCK: &str = "202610230520_changelist.doc.lock.md";
const SRC_LOCK: &str = "202610230535_changelist.src.lock.md";
const OWN_TOPIC: &str = "202610230520_topic.the-round-answers-this.md";
const OWN_SRC_TOPIC: &str = "202610230535_topic.filed-with-the-src-changelist.md";
const PARKED_OLDER: &str = "202610221816_topic.parked-before-the-round.md";
const PARKED_LATER: &str = "202610230600_topic.filed-while-the-round-was-open.md";

fn mock(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("design_rounds")).unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join("design_rounds").join(name), text).unwrap();
    }
    dir
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.file_type().unwrap().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| !n.starts_with(".meta") && !n.starts_with(".history"))
        .collect();
    v.sort();
    v
}

fn sorted(names: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    v.sort();
    v
}

/// A closable round: both changelists locked and saying something.
fn closable<'a>(extra: &[(&'a str, &'a str)]) -> tempfile::TempDir {
    let mut files: Vec<(&str, &str)> =
        vec![(DOC_LOCK, "# doc changelist: x\n"), (SRC_LOCK, "# src changelist: x\n")];
    files.extend_from_slice(extra);
    mock(&files)
}

#[test]
fn close_moves_its_own_files_and_leaves_an_unrelated_open_topic_flat() {
    let dir =
        closable(&[(OWN_TOPIC, "own\n"), (PARKED_OLDER, "parked\n"), (PARKED_LATER, "later\n")]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        names_in(&dr.join("202610230520")),
        sorted(&[DOC_LOCK, SRC_LOCK, OWN_TOPIC]),
        "the round's own files are what the archive holds"
    );
    assert_eq!(
        names_in(&dr),
        sorted(&[PARKED_OLDER, PARKED_LATER]),
        "a topic of no part of this round stays flat and open"
    );
}

#[test]
fn close_takes_the_topic_filed_under_the_src_changelists_own_id() {
    // The two changelists of one round carry different stamps; the topic
    // beside the second one is as much the round's as one beside the first.
    let dir = closable(&[(OWN_SRC_TOPIC, "own\n"), (PARKED_OLDER, "parked\n")]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        names_in(&dr.join("202610230520")),
        sorted(&[DOC_LOCK, SRC_LOCK, OWN_SRC_TOPIC])
    );
    assert_eq!(names_in(&dr), sorted(&[PARKED_OLDER]));
}

#[test]
fn close_takes_a_topic_a_changelist_names_though_its_id_differs() {
    let named = "202610230330_topic.named-by-the-changelist.md";
    let dir = mock(&[
        (
            DOC_LOCK,
            "# doc changelist: x\n\nAnswers `202610230330_topic.named-by-the-changelist.md`.\n",
        ),
        (SRC_LOCK, "# src changelist: x\n"),
        (named, "named\n"),
        (PARKED_OLDER, "parked\n"),
    ]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        names_in(&dr.join("202610230520")),
        sorted(&[DOC_LOCK, SRC_LOCK, named])
    );
    assert_eq!(names_in(&dr), sorted(&[PARKED_OLDER]));
}

#[test]
fn close_takes_a_deprecated_changelist_of_its_round() {
    let deprecated = "202610230525_changelist.src.deprecated.md";
    let dir = closable(&[(deprecated, "# src changelist: first try\n")]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        names_in(&dr.join("202610230520")),
        sorted(&[DOC_LOCK, SRC_LOCK, deprecated])
    );
    assert_eq!(names_in(&dr), Vec::<String>::new());
}

#[test]
fn close_leaves_the_readme_and_a_file_without_a_round_id_where_they_are() {
    let dir = closable(&[("README.md", "readme\n"), (".gitkeep", "")]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(names_in(&dr), sorted(&[".gitkeep", "README.md"]));
}

#[test]
fn an_abandoned_round_with_changelists_takes_only_its_own_files_like_close() {
    // `archive` runs at any phase. Here the round is in IMPL: the doc changelist is locked and the
    // src one is not, and a topic parked before the round sits beside them. The archive is named
    // by the changelists' id and the parked topic stays flat and open.
    let src_open = "202610230535_changelist.src.md";
    let dir = mock(&[
        (DOC_LOCK, "# doc changelist: x\n"),
        (src_open, "# src changelist: x\n"),
        (OWN_TOPIC, "own\n"),
        (PARKED_OLDER, "parked\n"),
        ("README.md", "r\n"),
    ]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_archive(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        names_in(&dr.join("202610230520-abandoned")),
        sorted(&[DOC_LOCK, src_open, OWN_TOPIC]),
        "the abandoned round holds its changelists and what they say are its own"
    );
    assert_eq!(
        names_in(&dr),
        sorted(&[PARKED_OLDER, "README.md"]),
        "a topic of no part of the round was swept into its archive"
    );
}

#[test]
fn an_abandoned_round_with_no_changelist_takes_every_flat_file() {
    // The TOPIC phase has no changelists to tell the round's files by, so `archive` takes every
    // flat file and names the archive by the earliest file of any kind. This is the behaviour as
    // it stands, pinned so a change to it is a decision.
    let dir = mock(&[(OWN_TOPIC, "a\n"), (PARKED_OLDER, "b\n"), ("README.md", "r\n")]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_archive(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(names_in(&dr), sorted(&["README.md"]));
    assert_eq!(
        names_in(&dr.join("202610221816-abandoned")),
        sorted(&[OWN_TOPIC, PARKED_OLDER])
    );
}

#[test]
fn a_changelist_naming_a_longer_file_does_not_take_a_shorter_one_inside_its_name() {
    // the changelist names `..named.md.orig`; the file `..named.md` is a different file whose
    // name only occurs inside that one
    let shorter = "202610230330_topic.named.md";
    let longer = "202610230330_topic.named.md.orig";
    let names = owned(&[DOC_LOCK, SRC_LOCK, shorter, longer]);
    let files = files_of_round(&names, |n| {
        Some(if n == DOC_LOCK {
            format!("# doc changelist: x\n\nAnswers `{longer}`.\n")
        } else {
            "# src changelist: x\n".to_string()
        })
    });
    assert_eq!(files.left, owned(&[shorter]));
    assert_eq!(files.taken, owned(&[longer, DOC_LOCK, SRC_LOCK]));
}

#[test]
fn a_changelist_naming_a_file_inside_a_longer_name_before_it_does_not_take_the_shorter() {
    // the character before an occurrence decides it: a name character or `-` continues a longer
    // name, as in `old-<name>`, and a full stop joins one, as in `x.<name>`, so neither names it
    let shorter = "202610230330_topic.named.md";
    let names = owned(&[DOC_LOCK, SRC_LOCK, shorter]);
    let files = files_of_round(&names, |n| {
        Some(if n == DOC_LOCK {
            format!("# doc changelist: x\n\nAnswers `old-{shorter}`.\n")
        } else {
            format!("# src changelist: x\n\nAnswers `x.{shorter}`.\n")
        })
    });
    assert_eq!(files.left, owned(&[shorter]));
    assert_eq!(files.taken, owned(&[DOC_LOCK, SRC_LOCK]));
}

#[test]
fn a_changelist_naming_a_file_at_the_end_of_a_sentence_still_takes_it() {
    // the control for the whole-name match: a name followed by a full stop, a comma, a bracket or
    // the end of the text is named
    let named = "202610230330_topic.named.md";
    for text in [
        format!("Answers {named}."),
        format!("Answers {named}, and more."),
        format!("Answers ({named})"),
        format!("Answers {named}"),
        format!("{named}\nnext line"),
    ] {
        let names = owned(&[DOC_LOCK, SRC_LOCK, named]);
        let files = files_of_round(&names, |n| {
            Some(if n == DOC_LOCK { text.clone() } else { String::new() })
        });
        assert!(files.taken.contains(&named.to_string()), "not taken for `{text}`");
    }
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

#[test]
fn the_files_close_reports_it_left_are_exactly_the_flat_ones_it_did_not_take_in_name_order() {
    // given in reverse so that a list kept in the order a directory happens to be read in
    // differs from the sorted one
    let names = owned(&[PARKED_LATER, PARKED_OLDER, OWN_TOPIC, SRC_LOCK, DOC_LOCK]);
    let files = files_of_round(&names, |_| Some("# changelist\n".to_string()));
    assert_eq!(files.left, owned(&[PARKED_OLDER, PARKED_LATER]));
    assert_eq!(files.taken, owned(&[DOC_LOCK, OWN_TOPIC, SRC_LOCK]));
}

#[test]
fn the_notice_names_every_file_left_one_to_a_line_and_says_nothing_when_none_is() {
    let notice = left_notice(&owned(&[PARKED_OLDER, PARKED_LATER])).expect("two files were left");
    let lines: Vec<&str> = notice.lines().collect();
    assert!(
        lines.contains(&format!("  {PARKED_OLDER}").as_str()),
        "{notice}"
    );
    assert!(
        lines.contains(&format!("  {PARKED_LATER}").as_str()),
        "{notice}"
    );
    assert!(
        lines.iter().position(|l| l.contains(PARKED_OLDER))
            < lines.iter().position(|l| l.contains(PARKED_LATER)),
        "the files are not listed in the order given:\n{notice}"
    );
    assert_eq!(
        left_notice(&[]),
        None,
        "a close that left nothing has nothing to list"
    );
}

#[test]
fn close_with_tagged_ids_leaves_a_same_minute_round_of_another_tag_flat() {
    // Two rounds opened in the same minute on different branches: same stamp, different tag.
    // Only the files carrying this round's whole id are its own.
    let doc = "202610230520-ab12_changelist.doc.lock.md";
    let src = "202610230520-ab12_changelist.src.lock.md";
    let own = "202610230520-ab12_topic.ours.md";
    let other = "202610230520-cd34_topic.another-branchs.md";
    let dir = mock(&[
        (doc, "# doc changelist: x\n"),
        (src, "# src changelist: x\n"),
        (own, "own\n"),
        (other, "theirs\n"),
    ]);
    let dr = dir.path().join("design_rounds");
    assert_eq!(
        cmd_close(&Config::from_dir(dir.path()), &OPTS),
        ExitCode::SUCCESS
    );
    assert_eq!(
        names_in(&dr.join("202610230520-ab12")),
        sorted(&[doc, src, own]),
        "the archive is named by the whole id and holds only that id's files"
    );
    assert_eq!(
        names_in(&dr),
        sorted(&[other]),
        "a same-minute topic of another tag was taken"
    );
}
