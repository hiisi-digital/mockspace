//! Which line of a hit the report points at.

use std::path::Path;

use mockspace_lint_rules::{DocClass, IndexQuery, RepoIndex};

use super::{Index, Source};

fn one(root: &Path, body: &str) -> Index {
    let p = root.join("research/n.md");
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, body).unwrap();
    let mut idx = Index::in_memory().unwrap();
    idx.refresh(&[Source {
        dir:   root.join("research"),
        shown: "research".into(),
        class: DocClass::Research,
        about: None,
    }])
    .unwrap();
    idx
}

fn line_of(idx: &Index, q: &str) -> usize {
    idx.search(&IndexQuery { text: q, expand: &[], phrase: false, limit: 1, today: 0 }).hits[0].line
}

#[test]
fn the_line_that_says_most_of_the_question_wins_over_one_that_repeats_a_word() {
    let t = tempfile::tempdir().unwrap();
    let idx = one(
        t.path(),
        "# Notes\nair air air air in a chair on a fair day\nsomething else\nthe air has a \
         saturation curve\n",
    );
    assert_eq!(line_of(&idx, "air saturation curve"), 4);
}

#[test]
fn a_word_inside_another_word_does_not_count() {
    let t = tempfile::tempdir().unwrap();
    // THE CONTROL: line 3 has the word, line 2 only has it inside others.
    let idx = one(t.path(), "# T\na chair and a fairground\nthe air is thin\n");
    assert_eq!(line_of(&idx, "air"), 3);
}
