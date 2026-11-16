use std::collections::HashMap;
use std::path::Path;

use super::*;
use crate::{AgentMode, LintConfig, check_message_with_extra};

// Every refusal is asked through `check_message_with_extra`, the entry the gate calls, with no
// pack at all: a project that imported nothing must still be refused. Each test names the mutant
// that turns it red.

fn message<'a>(text: &'a str, root: &'a Path) -> MessageContext<'a> {
    MessageContext::new(
        MessageDomain::CommitMessage,
        AgentMode::Assistant,
        text,
        "COMMIT_EDITMSG",
        root,
    )
}

fn kinds(text: &str, overrides: Option<&LintConfig>) -> Vec<&'static str> {
    let root = Path::new("/tmp");
    check_message_with_extra(&message(text, root), overrides, &[])
        .into_iter()
        .filter_map(|e| e.finding_kind)
        .collect()
}

fn configured(pairs: &[(&str, &str)]) -> LintConfig {
    let mut cfg = LintConfig::empty();
    let params: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    cfg.params.insert(LINT_NAME.to_string(), params);
    cfg
}

#[test]
fn a_well_formed_subject_of_every_default_type_passes_with_no_pack_and_no_config() {
    // Mutant: the default set drops a type, or the lint refuses what it should pass.
    for ty in DEFAULT_TYPES {
        assert_eq!(
            kinds(&format!("{ty}: add the thing"), None),
            Vec::<&str>::new(),
            "{ty}"
        );
    }
}

#[test]
fn bench_is_in_the_default_set() {
    // Mutant: `bench` is left out of `DEFAULT_TYPES`; seven such commits once passed unlisted.
    assert!(DEFAULT_TYPES.contains(&"bench"));
    assert!(kinds("bench: land the m1 results", None).is_empty());
}

#[test]
fn a_type_outside_the_set_is_refused_and_the_refusal_says_how_to_add_one() {
    // Mutant: the type set is not consulted when nothing is configured.
    let root = Path::new("/tmp");
    let found = check_message_with_extra(&message("research: sketch a lint", root), None, &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].finding_kind, Some("unknown-type"));
    assert!(
        found[0].message.contains("extra_types"),
        "{}",
        found[0].message
    );
    assert!(
        found[0].message.contains("[lints.commit-subject]"),
        "{}",
        found[0].message
    );
}

#[test]
fn a_subject_with_no_type_is_refused() {
    // Mutant: `require_type` defaults to false, as the pack's lint does.
    assert_eq!(kinds("add the thing", None), vec!["missing-type"]);
}

#[test]
fn a_type_without_the_space_after_the_colon_is_refused() {
    // Mutant: the separator is matched as `:` alone.
    assert_eq!(kinds("feat:add the thing", None), vec!["missing-type"]);
}

#[test]
fn a_colon_inside_the_message_is_not_read_as_an_unknown_type() {
    // Mutant: the head is taken whole, so `fix the loop` is reported as a type.
    assert_eq!(kinds("fix the loop: again", None), vec!["missing-type"]);
}

#[test]
fn a_capitalised_type_is_refused_as_unknown() {
    // Mutant: types are matched case-insensitively.
    assert_eq!(kinds("Feat: add the thing", None), vec!["unknown-type"]);
}

#[test]
fn a_parenthesised_scope_is_refused_by_default() {
    // Mutant: `allow_scope` defaults to true.
    assert_eq!(kinds("feat(gate): add the thing", None), vec![
        "scope-forbidden"
    ]);
}

#[test]
fn a_breaking_marker_is_the_type_and_not_an_unknown_one() {
    // Mutant: the marker is not stripped before the lookup.
    assert!(kinds("feat!: change the trait", None).is_empty());
}

#[test]
fn an_uppercase_message_is_refused() {
    // Mutant: `subject_case` defaults to any.
    assert_eq!(kinds("fix: Handle the empty case", None), vec![
        "subject-case"
    ]);
}

#[test]
fn a_trailing_period_is_refused() {
    // Mutant: `forbid_trailing_period` defaults to false.
    assert_eq!(kinds("fix: handle the empty case.", None), vec![
        "trailing-period"
    ]);
}

#[test]
fn a_subject_of_seventy_two_characters_is_refused_and_seventy_one_passes() {
    // Mutant: the limit is read as inclusive of 72, or is off by default.
    let body = |n: usize| format!("fix: {}", "a".repeat(n - 5));
    assert_eq!(body(71).chars().count(), 71);
    assert!(kinds(&body(71), None).is_empty());
    assert_eq!(kinds(&body(72), None), vec!["subject-length"]);
}

#[test]
fn nothing_after_the_type_is_refused() {
    // Mutant: an empty message after a valid type passes.
    assert_eq!(kinds("fix: ", None), vec!["empty-subject"]);
}

#[test]
fn an_empty_subject_line_is_refused() {
    // Mutant: a blank first line is skipped instead of reported.
    assert_eq!(kinds("   \n\nbody", None), vec!["empty-subject"]);
}

#[test]
fn only_the_authored_lines_are_judged_and_git_comments_are_not() {
    // Mutant: a `#` comment line is read as the subject.
    assert!(kinds("# please enter a message\nfix: handle it\n\nA body.", None).is_empty());
    assert!(
        kinds(
            "fix: handle it\n# ------------------------ >8\nBad: diff",
            None
        )
        .is_empty()
    );
}

#[test]
fn a_subject_git_wrote_is_not_held_to_the_convention() {
    // Mutant: merge, revert and fixup subjects are judged as authored text.
    for s in [
        "Merge pull request #251 from a/b",
        "Revert \"feat: x\"",
        "fixup! fix: x",
        "squash! fix: x",
    ] {
        assert!(kinds(s, None).is_empty(), "{s}");
    }
}

#[test]
fn an_authored_lowercase_merge_is_still_judged() {
    // Mutant: the generated-subject skip is case-insensitive, so `merge:` slips through.
    assert_eq!(kinds("merge: join the branches", None), vec![
        "unknown-type"
    ]);
}

#[test]
fn an_agent_hook_command_is_declined_and_never_read_as_a_subject() {
    // Mutant: the serialised tool input is judged as a subject.
    let root = Path::new("/tmp");
    let ctx = message("{\"command\":\"git commit -m 'Bad'\"}", root).with_invocation(Some(
        crate::Invocation {
            command:   Some("git commit -m 'Bad'"),
            tool_name: Some("Bash"),
        },
    ));
    assert!(check_message_with_extra(&ctx, None, &[]).is_empty());
}

#[test]
fn a_forge_body_is_never_judged_as_a_commit() {
    // Mutant: the lint runs on every message domain.
    let root = Path::new("/tmp");
    let ctx = MessageContext::new(
        MessageDomain::PullRequestBody,
        AgentMode::Assistant,
        "Anything at all.",
        "body",
        root,
    );
    assert!(check_message_with_extra(&ctx, None, &[]).is_empty());
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
fn an_empty_types_list_does_not_turn_the_check_off() {
    // Mutant: an empty `types` clears the set, and an empty set accepts anything.
    let cfg = configured(&[("types", " , ")]);
    assert_eq!(kinds("research: a thing", Some(&cfg)), vec!["unknown-type"]);
}

#[test]
fn every_knob_can_loosen_what_it_tightens() {
    // Mutant: a knob is declared and not read.
    let cfg = configured(&[
        ("allow_scope", "true"),
        ("subject_case", "any"),
        ("forbid_trailing_period", "false"),
        ("max_subject", "0"),
    ]);
    let long = format!("feat(gate): Add {}.", "a".repeat(100));
    assert!(kinds(&long, Some(&cfg)).is_empty());
}

#[test]
fn require_blank_line_is_off_until_asked_for() {
    // Mutant: the blank-line rule is on by default, which no skill states.
    assert!(kinds("fix: a thing\nbody right away", None).is_empty());
    let cfg = configured(&[("require_blank_line", "true")]);
    assert_eq!(kinds("fix: a thing\nbody right away", Some(&cfg)), vec![
        "missing-blank-line"
    ]);
}

#[test]
fn the_severity_knob_turns_the_lint_off_for_a_project_that_wants_no_convention() {
    // Mutant: the builtin ignores `commit = "off"`.
    let mut cfg = LintConfig::empty();
    cfg.base
        .insert(LINT_NAME.to_string(), crate::parse_severity("off").unwrap());
    assert!(kinds("anything goes", Some(&cfg)).is_empty());
    assert_eq!(kinds("anything goes", None), vec!["missing-type"]);
}

#[test]
fn from_params_reads_what_the_gate_reads() {
    // Mutant: the skill's view of the set differs from the gate's.
    let cfg = configured(&[("extra_types", "research")]);
    let policy = CommitSubject::from_params(cfg.params.get(LINT_NAME));
    assert!(policy.types().iter().any(|t| t == "research"));
    assert!(policy.types().iter().any(|t| t == "bench"));
    assert_eq!(CommitSubject::from_params(None), CommitSubject::default());
}

#[test]
fn the_builtin_is_registered_once_and_in_no_pack() {
    // Mutant: the lint is listed twice, or the duplicate check cannot see it.
    let names: Vec<_> = crate::all_message_lints()
        .iter()
        .map(|l| l.name())
        .collect();
    assert_eq!(names, vec![LINT_NAME]);
    assert!(crate::duplicate_lint_names(&crate::LintPack::default()).is_empty());
}
