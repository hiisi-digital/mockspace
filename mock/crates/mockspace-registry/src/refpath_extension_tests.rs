//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A citation naming its file's extension, as a shape on the step naming the file, where two files
//! differ by nothing else (letkajenkka `SPEC.md` 9.1).

use super::*;

fn tree(files: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let p = dir.path().join(f);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "# One\n\ntext\n").unwrap();
    }
    dir
}

const PROBES: &[&str] = &[
    "probes/unblock_value.out",
    "probes/unblock_value.sh",
    "probes/alone.md",
    "probes/run",
    "probes/run.sh",
    "round/changelist.doc.lock.md",
    "round/changelist.src.md",
    "case/Foo.md",
    "case/foo.sh",
    "a-b/x.md",
    "a_b/x.sh",
];

// Mutation: reading the shape onto `names` loses it from `extension` and `render`; refusing every
// constraint, as the reader before this did, refuses all three.
#[test]
fn a_citation_reads_and_writes_back_the_extension_on_the_step_naming_its_file() {
    let line = FileRef::read("panel::probes::unblock_value<out>::22").unwrap();
    assert_eq!(line.names, ["probes", "unblock_value"]);
    assert_eq!(line.extension.as_deref(), Some("out"));
    assert_eq!(line.lines, Some((22, 22)));
    assert_eq!(line.render(), "panel::probes::unblock_value<out>::22");
    assert_eq!(line.path_text(), "probes/unblock_value<out>");

    let head = FileRef::read("panel::round::changelist<doc_lock_md>::one").unwrap();
    assert_eq!(head.extension.as_deref(), Some("doc_lock_md"));
    assert_eq!(head.anchor(), Anchor::Heading("one".to_string()));
    assert_eq!(head.render(), "panel::round::changelist<doc_lock_md>::one");

    let span = FileRef::read("panel::probes::run<sh>::2..3").unwrap();
    assert_eq!(span.render(), "panel::probes::run<sh>::2..3");
}

// Mutation: taking the shape from whichever step carries one reads the first three as citations
// of some file; taking the first of several shapes reads the fourth.
#[test]
fn a_shape_anywhere_but_on_the_file_or_more_than_one_there_is_refused_as_misplaced() {
    for text in [
        "panel::probes<out>::unblock_value::1",
        "panel::probes::unblock_value::one<md>",
        "panel::probes::unblock_value::1<out>",
        "panel::probes::unblock_value<out, sh>::1",
        "panel::probes::unblock_value<out>::run<sh>::1",
    ] {
        assert_eq!(FileRef::read(text), Err(Refusal::MisplacedShape), "{text}");
        assert!(
            Refusal::MisplacedShape.say(text).contains("extension"),
            "{text}"
        );
    }
}

// Mutation: matching the extension by its last part (`md`) reaches both changelists; dropping the
// filter in `walk` leaves `unblock_value<out>` ambiguous.
#[test]
fn an_extension_narrows_the_walk_to_the_one_file_carrying_it_whole() {
    let dir = tree(PROBES);
    let at = |text: &str| FileRef::read(text).unwrap().locate(dir.path());
    assert_eq!(
        at("panel::probes::unblock_value::1"),
        Location::Ambiguous(vec![
            "unblock_value.out".to_string(),
            "unblock_value.sh".to_string()
        ])
    );
    assert_eq!(
        at("panel::probes::unblock_value<out>::1"),
        Location::Found(dir.path().join("probes/unblock_value.out"))
    );
    assert_eq!(
        at("panel::probes::unblock_value<sh>::1"),
        Location::Found(dir.path().join("probes/unblock_value.sh"))
    );
    assert_eq!(at("panel::probes::unblock_value<rs>::1"), Location::Missing);
    assert_eq!(
        at("panel::round::changelist<doc_lock_md>::1"),
        Location::Found(dir.path().join("round/changelist.doc.lock.md"))
    );
    assert_eq!(at("panel::round::changelist<md>::1"), Location::Missing);
}

// A file keeps one spelling: an extension on a name that alone reaches one file is refused, and
// the file it reached is said.
//
// Mutation: answering `Found` wherever the narrowed walk finds one file lets `alone<md>` stand
// beside `alone`.
#[test]
fn an_extension_the_name_alone_does_not_need_is_refused() {
    let dir = tree(PROBES);
    let at = |text: &str| FileRef::read(text).unwrap().locate(dir.path());
    assert_eq!(
        at("panel::probes::alone::1"),
        Location::Found(dir.path().join("probes/alone.md"))
    );
    assert_eq!(
        at("panel::probes::alone<md>::1"),
        Location::NeedlessExtension(dir.path().join("probes/alone.md"))
    );
}

// An extension tells apart only files whose names are the same up to the first dot, in one
// directory: files folding to one name by case or a hyphen, or lying in two directories that fold
// to one path, stay ambiguous whatever extension is named.
//
// Mutation: filtering by the extension alone, as a walk that narrows each match, reaches `Foo.md`
// for `foo<md>` and `a-b/x.md` for `a_b::x<md>`.
#[test]
fn an_extension_tells_apart_no_files_that_differ_before_the_first_dot() {
    let dir = tree(PROBES);
    let at = |text: &str| FileRef::read(text).unwrap().locate(dir.path());
    assert_eq!(at("panel::case::foo<md>::1"), Location::Ambiguous(vec!["Foo.md".to_string(), "foo.sh".to_string()]));
    assert_eq!(at("panel::a_b::x<md>::1"), Location::Ambiguous(vec!["x.md".to_string(), "x.sh".to_string()]));
}
