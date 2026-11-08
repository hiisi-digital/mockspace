use std::path::Path;

use mockspace_lint_rules::{DocClass, IndexQuery, RepoIndex};

use super::{Index, Source};

fn built(root: &Path) -> Index {
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

fn put(root: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(root.join("research")).unwrap();
    std::fs::write(root.join("research").join(name), body).unwrap();
}

fn paths(idx: &Index, text: &str, phrase: bool) -> Vec<String> {
    let q = IndexQuery { text, expand: &[], phrase, limit: 10, today: 0 };
    idx.search(&q).hits.into_iter().map(|h| h.path).collect()
}

#[test]
fn a_phrase_matches_only_the_words_said_together_in_order() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "together.md", "The vapour saturation curve of air is Magnus shaped.\n");
    put(t.path(), "scattered.md", "Curve fitting is unrelated. Saturation of a market. Vapour trails.\n");
    put(t.path(), "reversed.md", "The curve saturation vapour order is wrong.\n");
    let idx = built(t.path());

    assert_eq!(paths(&idx, "vapour saturation curve", true), ["research/together.md"]);
    // THE CONTROL: the same words as a question find all three, so the phrase mode is what narrowed it.
    assert_eq!(paths(&idx, "vapour saturation curve", false).len(), 3);
}

#[test]
fn one_common_word_of_a_nonsense_phrase_does_not_make_the_research_carry_it() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "a.md", "The zzz plant needs little water.\n");
    let idx = built(t.path());
    assert!(paths(&idx, "quaternion-nonsense-zzz", true).is_empty());
    // THE CONTROL: as a question, the one word that is there does answer.
    assert_eq!(paths(&idx, "quaternion-nonsense-zzz", false), ["research/a.md"]);
}

#[test]
fn a_phrase_of_common_words_is_still_matched_whole() {
    let t = tempfile::tempdir().unwrap();
    put(t.path(), "a.md", "Who is the owner of the air cell.\n");
    put(t.path(), "b.md", "The owner is who the air cell belongs to.\n");
    let idx = built(t.path());
    assert_eq!(paths(&idx, "who is the owner", true), ["research/a.md"]);
}
