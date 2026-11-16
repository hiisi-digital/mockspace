use std::collections::HashMap;

use super::tests::{configured, kinds};
use super::*;

// One arm per knob in each direction, and one per refusal of a bad parameter. Each names the
// mutant that turns it red.

fn problems(pairs: &[(&str, &str)]) -> Vec<String> {
    let params: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    CommitSubject::problems(&params)
}

#[test]
fn extra_types_add_to_the_set_without_restating_it() {
    // Mutant: `extra_types` replaces the set, or is not read.
    let cfg = configured(&[("extra_types", "research, deploy")]);
    assert!(kinds("research: sketch a lint", Some(&cfg)).is_empty());
    assert!(kinds("deploy: ship it", Some(&cfg)).is_empty());
    assert!(kinds("feat: still here", Some(&cfg)).is_empty());
    assert_eq!(kinds("perf: not listed", Some(&cfg)), vec!["unknown-type"]);
}

#[test]
fn types_replace_the_default_set() {
    // Mutant: `types` appends to the default instead of replacing it.
    let cfg = configured(&[("types", "feat,fix")]);
    assert!(kinds("fix: a thing", Some(&cfg)).is_empty());
    assert_eq!(kinds("bench: a thing", Some(&cfg)), vec!["unknown-type"]);
}

#[test]
fn separator_is_read() {
    // Mutant: the separator is hard-coded to `: `.
    let cfg = configured(&[("separator", " - ")]);
    assert!(kinds("fix - add the thing", Some(&cfg)).is_empty());
    assert_eq!(kinds("fix: add the thing", Some(&cfg)), vec![
        "missing-type"
    ]);
}

#[test]
fn breaking_marker_is_read() {
    // Mutant: the marker is hard-coded to `!`.
    let cfg = configured(&[("breaking_marker", "~")]);
    assert!(kinds("feat~: change the trait", Some(&cfg)).is_empty());
    assert_eq!(kinds("feat!: change the trait", Some(&cfg)), vec![
        "unknown-type"
    ]);
    let none = configured(&[("breaking_marker", "")]);
    assert_eq!(kinds("feat!: change the trait", Some(&none)), vec![
        "unknown-type"
    ]);
}

#[test]
fn require_type_false_lets_a_subject_with_no_type_through() {
    // Mutant: `require_type` is not read, or the case check still fires on a missing type.
    let cfg = configured(&[("require_type", "false")]);
    assert!(kinds("add the thing", Some(&cfg)).is_empty());
    assert_eq!(kinds("Add the thing", Some(&cfg)), vec!["subject-case"]);
    assert_eq!(kinds("add the thing", None), vec!["missing-type"]);
}

#[test]
fn a_missing_type_is_reported_once_and_not_also_as_a_case_fault() {
    // Mutant: the case check runs over the whole subject when no type was found.
    assert_eq!(kinds("Add the thing", None), vec!["missing-type"]);
}

#[test]
fn max_subject_is_read_and_zero_turns_it_off() {
    // Mutant: the limit is hard-coded, or zero means a limit of zero.
    let cfg = configured(&[("max_subject", "12")]);
    assert!(kinds("fix: a thing", Some(&cfg)).is_empty());
    assert_eq!(kinds("fix: a thing!", Some(&cfg)), vec!["subject-length"]);
    let off = configured(&[("max_subject", "0")]);
    assert!(kinds(&format!("fix: {}", "a".repeat(500)), Some(&off)).is_empty());
}

#[test]
fn the_default_limit_is_seventy_one_characters() {
    // Mutant: the default is 72, or the check is `>=`.
    let at = |n: usize| format!("fix: {}", "a".repeat(n - 5));
    assert!(kinds(&at(71), None).is_empty());
    assert_eq!(kinds(&at(72), None), vec!["subject-length"]);
}

#[test]
fn length_counts_characters_and_not_bytes() {
    // Mutant: `chars().count()` is replaced by `len()`, which counts the two bytes of each `ä`.
    let at = |n: usize| format!("fix: {}", "ä".repeat(n - 5));
    assert_eq!(at(71).len(), 5 + 66 * 2);
    assert!(kinds(&at(71), None).is_empty());
    assert_eq!(kinds(&at(72), None), vec!["subject-length"]);
}

#[test]
fn subject_case_is_read_in_all_three_directions() {
    // Mutant: the case is hard-coded to lower, or `sentence` and `any` are not read.
    let sentence = configured(&[("subject_case", "sentence")]);
    assert!(kinds("fix: Add the thing", Some(&sentence)).is_empty());
    assert_eq!(kinds("fix: add the thing", Some(&sentence)), vec![
        "subject-case"
    ]);
    let any = configured(&[("subject_case", "any")]);
    assert!(kinds("fix: Add the thing", Some(&any)).is_empty());
    assert!(kinds("fix: add the thing", Some(&any)).is_empty());
    assert_eq!(kinds("fix: Add the thing", None), vec!["subject-case"]);
}

#[test]
fn the_case_check_reads_a_letter_and_not_an_ascii_letter() {
    // Mutant: `is_uppercase` becomes `is_ascii_uppercase`, so `Ä` passes as no case at all.
    assert_eq!(kinds("fix: Ärgern the build", None), vec!["subject-case"]);
    assert!(kinds("fix: ärgern the build", None).is_empty());
    assert!(kinds("fix: 3 things", None).is_empty());
    let sentence = configured(&[("subject_case", "sentence")]);
    assert_eq!(kinds("fix: ärgern the build", Some(&sentence)), vec![
        "subject-case"
    ]);
    assert!(kinds("fix: Ärgern the build", Some(&sentence)).is_empty());
}

#[test]
fn allow_scope_is_read() {
    // Mutant: `allow_scope` is not read, so a scope is refused whatever the project says.
    let cfg = configured(&[("allow_scope", "true")]);
    assert!(kinds("feat(gate): add the thing", Some(&cfg)).is_empty());
    assert_eq!(kinds("feat(gate): add the thing", None), vec![
        "scope-forbidden"
    ]);
}

#[test]
fn a_trailing_period_is_refused_and_an_ellipsis_is_not() {
    // Mutant: the `..` exemption is dropped, or the check never fires.
    assert_eq!(kinds("fix: handle the empty case.", None), vec![
        "trailing-period"
    ]);
    assert!(kinds("fix: wait for it...", None).is_empty());
    assert!(kinds("fix: wait for it..", None).is_empty());
}

#[test]
fn forbid_trailing_period_is_read() {
    // Mutant: `forbid_trailing_period` is not read.
    let cfg = configured(&[("forbid_trailing_period", "false")]);
    assert!(kinds("fix: handle the empty case.", Some(&cfg)).is_empty());
}

#[test]
fn require_blank_line_is_off_until_asked_for() {
    // Mutant: the blank-line rule is on by default, which no skill states.
    assert!(kinds("fix: a thing\nbody right away", None).is_empty());
    let cfg = configured(&[("require_blank_line", "true")]);
    assert_eq!(kinds("fix: a thing\nbody right away", Some(&cfg)), vec![
        "missing-blank-line"
    ]);
    assert!(kinds("fix: a thing\n\nbody", Some(&cfg)).is_empty());
    assert!(kinds("\n\nfix: a thing\n\nbody", Some(&cfg)).is_empty());
}

// --- a parameter the lint cannot read is refused, not defaulted --------------------------

#[test]
fn a_full_valid_configuration_has_no_problem() {
    // Mutant: a valid key or value is reported.
    assert!(
        problems(&[
            ("types", "feat,fix"),
            ("extra_types", "research"),
            ("separator", ": "),
            ("breaking_marker", ""),
            ("require_type", "true"),
            ("max_subject", "0"),
            ("subject_case", "any"),
            ("allow_scope", "yes"),
            ("forbid_trailing_period", "off"),
            ("require_blank_line", "1"),
        ])
        .is_empty()
    );
}

#[test]
fn a_misspelled_key_is_refused_and_names_the_key_it_was_close_to() {
    // Mutant: an unknown key is ignored, so the author believes `extra_type` took effect.
    let found = problems(&[("extra_type", "research")]);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("`extra_type`") && found[0].contains("did you mean `extra_types`"),
        "{found:?}"
    );
}

#[test]
fn an_unknown_key_with_no_near_match_is_refused_without_a_guess() {
    // Mutant: the nearest key is always offered, however far.
    let found = problems(&[("colour", "red")]);
    assert_eq!(found.len(), 1);
    assert!(!found[0].contains("did you mean"), "{found:?}");
}

#[test]
fn a_boolean_that_is_not_one_is_refused() {
    // Mutant: `ture` and `enabled` read as false and switch the requirement off.
    for key in ["require_type", "allow_scope", "forbid_trailing_period", "require_blank_line"] {
        for bad in ["ture", "enabled", ""] {
            assert_eq!(problems(&[(key, bad)]).len(), 1, "{key} = {bad:?}");
        }
    }
}

#[test]
fn a_max_subject_that_is_not_a_whole_number_is_refused() {
    // Mutant: a bad number keeps the default without a word.
    for bad in ["abc", "-1", "7.5", ""] {
        assert_eq!(problems(&[("max_subject", bad)]).len(), 1, "{bad:?}");
    }
}

#[test]
fn a_subject_case_that_is_none_of_the_three_is_refused() {
    // Mutant: a bad case keeps the default without a word.
    assert_eq!(problems(&[("subject_case", "mixed")]).len(), 1);
    assert_eq!(problems(&[("subject_case", "")]).len(), 1);
}

#[test]
fn a_list_that_names_no_type_is_refused() {
    // Mutant: an empty `types` is skipped, which left the default set while the project meant to replace it.
    assert_eq!(problems(&[("types", " , ")]).len(), 1);
    assert_eq!(problems(&[("extra_types", "")]).len(), 1);
}

#[test]
fn an_empty_separator_is_refused() {
    // Mutant: an empty separator is skipped and the default stays.
    assert_eq!(problems(&[("separator", "")]).len(), 1);
}

#[test]
fn every_problem_is_reported_and_not_only_the_first() {
    // Mutant: the check stops at the first bad parameter.
    assert_eq!(
        problems(&[("extra_type", "x"), ("max_subject", "abc"), ("allow_scope", "ture")]).len(),
        3
    );
}

#[test]
fn the_lint_exposes_the_check_through_the_trait_the_loader_calls() {
    // Mutant: `check_params` is left at the trait's empty default, so the loader sees nothing.
    let params: HashMap<String, String> = [("extra_type".to_string(), "x".to_string())].into();
    assert_eq!(
        Lint::check_params(&CommitSubject::default(), &params).len(),
        1
    );
    let names: Vec<_> = crate::all_message_lints()
        .iter()
        .map(|l| l.check_params(&params).len())
        .collect();
    assert_eq!(names, vec![1]);
}

#[test]
fn a_bad_value_never_changes_what_is_applied() {
    // Mutant: a value that cannot be read is applied as some other value.
    let cfg =
        configured(&[("require_type", "ture"), ("max_subject", "abc"), ("subject_case", "mixed")]);
    let policy = CommitSubject::from_params(cfg.params.get(LINT_NAME));
    assert_eq!(policy, CommitSubject::default());
}

#[test]
fn a_type_entry_that_can_never_match_is_refused_at_load() {
    // Mutant: the list is read without checking each entry, so `feat fix` is accepted and then
    // refuses every subject with a message naming a type that is on the list.
    for list in ["feat fix", "feat, fix up", "fix(x)", "a: b"] {
        for key in ["types", "extra_types"] {
            let found = problems(&[(key, list)]);
            assert_eq!(found.len(), 1, "{key} = {list:?}: {found:?}");
            assert!(found[0].contains("can never match"), "{}", found[0]);
        }
    }
    // the entry that holds the separator is judged against the separator the project set
    assert_eq!(problems(&[("types", "feat-x"), ("separator", "-")]).len(), 1);
    assert!(problems(&[("types", "feat-x")]).is_empty());
    // a list of entries that can match stays clean
    assert!(problems(&[("types", "feat, fix"), ("extra_types", "research,deploy")]).is_empty());
}

#[test]
fn a_refused_type_entry_names_the_entry_and_not_the_whole_list() {
    // Mutant: the whole value is reported, so a long list hides which entry is wrong.
    let found = problems(&[("extra_types", "research, two words, deploy")]);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("`two words`"), "{}", found[0]);
}
