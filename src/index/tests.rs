//! The index against a small tree with a known right answer.

use std::path::Path;

use mockspace_lint_rules::{DocClass, IndexQuery, RepoIndex};

use super::stamp::days_from_civil;
use super::{Index, Refreshed, Source, index_path};

fn put(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn sources(root: &Path) -> Vec<Source> {
    vec![
        Source {
            dir:   root.join("research"),
            shown: "research".into(),
            class: DocClass::Research,
            about: None,
        },
        Source {
            dir:   root.join("design_rounds"),
            shown: "design_rounds".into(),
            class: DocClass::Design,
            about: None,
        },
        Source {
            dir:   root.join("captures"),
            shown: "op-responses".into(),
            class: DocClass::Human,
            about: Some("kaski".into()),
        },
    ]
}

/// A long note that mentions the subject many times, as a copied research directory does.
fn long_note() -> String {
    let mut s = String::from("# A renderer library survey\n\n");
    for i in 0 .. 40 {
        s.push_str(&format!(
            "Section {i} of the survey lists how a renderer library might model the air, its \
             saturation and a curve of some kind, among {i} other things the library does.\n\n"
        ));
    }
    s
}

/// Documents about other things, so a word that few documents carry is rare, as it is in a real
/// tree. Without them every document holds every query word and no word discriminates.
fn filler(root: &Path, n: usize) {
    for i in 0 .. n {
        put(
            root,
            &format!("research/2025010{}0000_filler{i}/note.md", i % 9 + 1),
            &format!("# Topic number{i}\n\nnotes on topic{i} and its sibling subject{i}, nothing else\n"),
        );
    }
}

/// The air case, rebuilt small: five copies of a long survey under research, and the design round
/// that holds the answer, short, with the subject in its heading.
fn the_air_tree(root: &Path) {
    filler(root, 40);
    for n in 0 .. 5 {
        put(
            root,
            &format!("research/202611010500_ikiuni-renderer-library/copy{n}/survey.md"),
            &long_note(),
        );
    }
    put(
        root,
        "design_rounds/202610040050/202610040042_topic.the-air-and-heat-lattice.md",
        "# The air's saturation curve\n\nThe air holds water up to a saturation that rises with \
         heat; past it the water condenses. The curve is fixed in the lattice, one value per \
         temperature.\n",
    );
}

const TODAY: (i64, i64, i64) = (2026, 11, 8);

fn today() -> i64 {
    days_from_civil(TODAY.0, TODAY.1, TODAY.2)
}

fn ask<'a>(text: &'a str, limit: usize) -> IndexQuery<'a> {
    IndexQuery {
        text,
        expand: &[],
        limit,
        today: today(),
    }
}

fn built(root: &Path) -> Index {
    let mut idx = Index::in_memory().unwrap();
    idx.refresh(&sources(root)).unwrap();
    idx
}

#[test]
fn the_design_round_that_holds_the_answer_outranks_five_copies_of_a_long_survey() {
    let t = tempfile::tempdir().unwrap();
    the_air_tree(t.path());
    let idx = built(t.path());

    let r = idx.search(&ask("the air's saturation curve", 5));
    let first = &r.hits[0];
    assert!(
        first.path.starts_with("design_rounds/202610040050/202610040042_topic"),
        "the round with the answer is first, not a copy of the survey: {:?}",
        r.hits.iter().map(|h| &h.path).collect::<Vec<_>>()
    );
    assert_eq!(first.class, DocClass::Design);

    // The five copies are one hit with the other four folded behind it.
    let survey: Vec<_> = r.hits.iter().filter(|h| h.path.starts_with("research/")).collect();
    assert_eq!(survey.len(), 1, "five copies are one hit");
    assert_eq!(survey[0].copies.len(), 4);
}

#[test]
fn a_hit_carries_its_date_its_age_and_the_line_to_open() {
    let t = tempfile::tempdir().unwrap();
    the_air_tree(t.path());
    let idx = built(t.path());

    let hit = &idx.search(&ask("the air's saturation curve", 5)).hits[0];
    let wrote = days_from_civil(2026, 10, 4);
    assert_eq!(hit.day, Some(wrote));
    assert_eq!(hit.age_days, Some(today() - wrote));
    assert_eq!(hit.line, 1, "the heading names the subject");
    assert!(hit.excerpt.contains("saturation"), "{}", hit.excerpt);
}

#[test]
fn a_document_with_no_stamp_has_no_date_rather_than_a_guessed_one() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "research/NOTES.md", "# Moisture\n\nsaturation of the air\n");
    let hit = &built(t.path()).search(&ask("saturation", 3)).hits[0];
    assert_eq!(hit.day, None);
    assert_eq!(hit.age_days, None);
}

#[test]
fn a_human_s_words_outrank_an_agent_note_that_says_the_same_and_one_about_another_project_is_held_out() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "research/202610010000_note/n.md", "saturation of kaski air in a curve\n");
    put(t.path(), "captures/202610020000_op.md", "kaski: the saturation of air is a curve\n");
    put(t.path(), "captures/202610030000_other.md", "elsewhere: saturation of air, a curve\n");
    let mut idx = Index::in_memory().unwrap();
    let done = idx.refresh(&sources(t.path())).unwrap();
    assert_eq!(done.held_out, 1);

    let r = idx.search(&ask("saturation of air curve", 5));
    assert_eq!(r.hits[0].class, DocClass::Human, "{:?}", r.hits);
    assert!(
        r.hits.iter().all(|h| !h.path.contains("other")),
        "a capture about another project is not a hit"
    );
    assert!(
        r.caveats.iter().any(|c| c.contains("held out")),
        "and the report says it held one out: {:?}",
        r.caveats
    );
}

#[test]
fn a_warm_refresh_opens_nothing_and_an_edit_reads_only_what_changed() {
    let t = tempfile::tempdir().unwrap();
    the_air_tree(t.path());
    let mut idx = Index::in_memory().unwrap();
    let cold = idx.refresh(&sources(t.path())).unwrap();
    assert_eq!(cold.read, 46);

    let warm = idx.refresh(&sources(t.path())).unwrap();
    assert_eq!(warm, Refreshed { unchanged: 46, ..Refreshed::default() });

    // One edit: only that file is read, and the new text is found.
    put(
        t.path(),
        "research/NOTES.md",
        "# Aerosols\n\nzeppelin condensation nuclei are what the saturation needs\n",
    );
    let after = idx.refresh(&sources(t.path())).unwrap();
    assert_eq!((after.read, after.unchanged), (1, 46));
    assert_eq!(idx.search(&ask("zeppelin", 3)).hits[0].path, "research/NOTES.md");
}

#[test]
fn a_file_that_is_gone_is_no_longer_found() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "research/NOTES.md", "zeppelin\n");
    let mut idx = Index::in_memory().unwrap();
    idx.refresh(&sources(t.path())).unwrap();
    assert_eq!(idx.search(&ask("zeppelin", 3)).hits.len(), 1);

    std::fs::remove_file(t.path().join("research/NOTES.md")).unwrap();
    let after = idx.refresh(&sources(t.path())).unwrap();
    assert_eq!(after.removed, 1);
    assert!(idx.search(&ask("zeppelin", 3)).hits.is_empty());
    assert!(idx.is_empty());
}

#[test]
fn a_word_no_document_has_is_named_and_a_missing_root_is_said() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "research/NOTES.md", "saturation of the air\n");
    let r = built(t.path()).search(&ask("saturation zeppelin", 3));
    assert_eq!(r.dead_terms, ["zeppelin"], "the live word is not named dead");
    assert!(
        r.caveats.iter().any(|c| c.contains("design_rounds")),
        "the absent root is a caveat: {:?}",
        r.caveats
    );
}

#[test]
fn expansion_words_find_what_the_question_did_not_use() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "research/NOTES.md", "the broker has two backends\n");
    let idx = built(t.path());
    assert!(idx.search(&ask("two surfaces data store", 3)).hits.is_empty());

    let expand = ["broker".to_string()];
    let q = IndexQuery { text: "two surfaces data store", expand: &expand, limit: 3, today: today() };
    assert_eq!(idx.search(&q).hits[0].path, "research/NOTES.md");
}

#[test]
fn an_index_file_of_another_schema_is_dropped_and_rebuilt_not_trusted() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "research/NOTES.md", "saturation\n");
    let file = t.path().join("cache/idx.sqlite");
    {
        let mut idx = Index::open(&file).unwrap();
        idx.refresh(&sources(t.path())).unwrap();
        assert_eq!(idx.len(), 1);
    }
    // Reopened as it was, it still holds its document.
    assert_eq!(Index::open(&file).unwrap().len(), 1);

    // A file that is not a database at all is a cache that is gone.
    std::fs::write(&file, b"not a database, just text that is long enough to be read as a header")
        .unwrap();
    let again = Index::open(&file).unwrap();
    assert!(again.is_empty());
}

#[test]
fn two_clones_of_one_name_get_two_index_files() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let cache = Path::new("/cache");
    let (pa, pb) = (index_path(cache, a.path()), index_path(cache, b.path()));
    assert_ne!(pa, pb);
    assert_eq!(index_path(cache, a.path()), pa, "and the same clone always gets the same one");
    assert!(pa.starts_with("/cache/mockspace/index"));
}
