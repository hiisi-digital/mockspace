use std::collections::HashMap;
use std::path::Path;

use super::*;
use crate::{AgentMode, LintConfig, LintMode, check_message_with_extra};

// Every refusal is asked through `check_message_with_extra`, the entry the gate calls, with no
// pack at all: a project that imported nothing must still be refused. Each test names the mutant
// that turns it red.

pub(super) fn message<'a>(text: &'a str, root: &'a Path) -> MessageContext<'a> {
    MessageContext::new(
        MessageDomain::CommitMessage,
        AgentMode::Assistant,
        text,
        "COMMIT_EDITMSG",
        root,
    )
}

pub(super) fn kinds(text: &str, overrides: Option<&LintConfig>) -> Vec<&'static str> {
    let root = Path::new("/tmp");
    check_message_with_extra(&message(text, root), overrides, &[])
        .into_iter()
        .filter_map(|e| e.finding_kind)
        .collect()
}

pub(super) fn configured(pairs: &[(&str, &str)]) -> LintConfig {
    let mut cfg = LintConfig::empty();
    let params: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    cfg.params.insert(LINT_NAME.to_string(), params);
    cfg
}

/// The gates a finding blocks at, for the autosquash arm.
fn blocks_at(text: &str, gate: LintMode) -> bool {
    let root = Path::new("/tmp");
    check_message_with_extra(&message(text, root), None, &[])
        .iter()
        .any(|e| e.severity.effective(gate) == crate::Level::Error)
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
fn a_breaking_marker_is_the_type_and_not_an_unknown_one() {
    // Mutant: the marker is not stripped before the lookup.
    assert!(kinds("feat!: change the trait", None).is_empty());
}

#[test]
fn nothing_after_the_type_is_refused() {
    // Mutant: an empty message after a valid type passes.
    assert_eq!(kinds("fix: ", None), vec!["empty-subject"]);
}

#[test]
fn leading_blank_lines_are_skipped_before_the_subject_is_taken() {
    // Mutant: the first line is the subject, so a message git would record as `fix: x` is refused.
    assert!(kinds("\nfix: x", None).is_empty());
    assert!(kinds("  \n\t\n\nfix: x\n\nbody", None).is_empty());
    assert_eq!(kinds("\n\nWip", None), vec!["missing-type"]);
}

#[test]
fn a_message_with_no_subject_at_all_is_left_to_git_to_abort() {
    // Mutant: an all-blank message is reported as a subject fault; git refuses an empty message
    // before any hook speaks.
    assert!(kinds("   \n\n  \n", None).is_empty());
    assert!(kinds("# only a comment\n", None).is_empty());
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
    // Mutant: a merge, revert or reapply subject is judged as authored text.
    for s in [
        "Merge branch 'dev' into work/x",
        "Merge branches 'a' and 'b'",
        "Merge remote-tracking branch 'origin/dev' into work/x",
        "Merge tag 'v1.0'",
        "Merge commit 'abc123'",
        "Merge pull request #251 from a/b",
        "Revert \"feat: x\"",
        "Reapply \"feat: x\"",
    ] {
        assert!(kinds(s, None).is_empty(), "{s}");
    }
}

#[test]
fn an_authored_subject_that_only_starts_like_a_merge_is_still_judged() {
    // Mutant: any subject starting `Merge ` skips the gate, as the first version did.
    assert_eq!(kinds("Merge the two lists", None), vec!["missing-type"]);
    assert_eq!(kinds("merge: join the branches", None), vec![
        "unknown-type"
    ]);
    assert_eq!(kinds("Revert the change", None), vec!["missing-type"]);
}

#[test]
fn an_autosquash_commit_passes_the_commit_gate_and_is_refused_at_build_and_push() {
    // Mutant: `fixup!`, `squash!` and `amend!` pass every gate, so they reach the trunk unfolded.
    for s in ["fixup! fix: x", "squash! fix: x", "amend! fix: x"] {
        assert!(!blocks_at(s, LintMode::Commit), "{s} at commit");
        assert!(blocks_at(s, LintMode::Build), "{s} at build");
        assert!(blocks_at(s, LintMode::Push), "{s} at push");
        assert_eq!(kinds(s, None), vec!["autosquash-leftover"], "{s}");
    }
    assert!(!blocks_at("fix: x", LintMode::Push));
}

#[test]
fn a_scope_with_a_space_is_a_scope_and_not_a_missing_type() {
    // Mutant: the one-word test runs over the whole head, so `feat(a b)` reads as no type.
    assert_eq!(kinds("feat(a b): x", None), vec!["scope-forbidden"]);
    let cfg = configured(&[("allow_scope", "true")]);
    assert!(kinds("feat(a b): x", Some(&cfg)).is_empty());
}

#[test]
fn an_unclosed_or_empty_scope_is_a_malformed_scope_when_scopes_are_allowed() {
    // Mutant: the unclosed scope is reported as an unknown type.
    let cfg = configured(&[("allow_scope", "true")]);
    assert_eq!(kinds("feat(a: x", Some(&cfg)), vec!["malformed-scope"]);
    assert_eq!(kinds("feat(): x", Some(&cfg)), vec!["malformed-scope"]);
    assert_eq!(kinds("feat(a)(b): x", Some(&cfg)), vec!["malformed-scope"]);
    assert_eq!(kinds("nope(a): x", Some(&cfg)), vec!["unknown-type"]);
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

#[test]
fn release_is_in_the_default_set_for_the_merge_commit_the_release_tool_writes() {
    // Mutant: `release` is left out of `DEFAULT_TYPES`; `homma release run` merges with
    // `release: <version>` and would be refused at the commit gate and again at the push.
    assert!(kinds("release: 0.4.0", None).is_empty());
    assert!(kinds("release: 0.4.0-rc.1", None).is_empty());
}

#[test]
fn a_pound_line_is_a_comment_in_the_editor_file_and_a_subject_once_recorded() {
    // Mutant: comment lines are dropped from a recorded message too, so `#42 fix it`, which
    // `git commit -m` keeps as the subject, is read as nothing and skips the gate at the push.
    let root = Path::new("/tmp");
    let editor = message("#42 fix it\n", root);
    assert!(check_message_with_extra(&editor, None, &[]).is_empty());
    let recorded = message("#42 fix it\n", root).recorded();
    let found: Vec<_> = check_message_with_extra(&recorded, None, &[])
        .into_iter()
        .filter_map(|e| e.finding_kind)
        .collect();
    assert_eq!(found, vec!["missing-type"]);
    // a recorded body line starting with a pound sign is no subject, and stays unjudged
    let body = message("fix: x\n\n# a heading in the body\n", root).recorded();
    assert!(check_message_with_extra(&body, None, &[]).is_empty());
}

#[test]
fn a_base_severity_cannot_raise_the_commit_gate_for_an_autosquash_commit() {
    // Mutant: the table's per-gate severity replaces the finding's own, so `commit = "error"`
    // (which muisti and riimu set) refuses `fixup!` at the commit gate where it is wanted.
    let mut cfg = LintConfig::empty();
    cfg.base.insert(
        LINT_NAME.to_string(),
        crate::Severity::new(crate::Level::Error, crate::Level::Error, crate::Level::Error),
    );
    let root = Path::new("/tmp");
    let found = check_message_with_extra(&message("fixup! fix: x", root), Some(&cfg), &[]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].severity.effective(LintMode::Commit), crate::Level::Pass);
    assert_eq!(found[0].severity.effective(LintMode::Build), crate::Level::Error);
    assert_eq!(found[0].severity.effective(LintMode::Push), crate::Level::Error);
    // the table lowering a gate still lowers it: riimu runs the build gate off
    let mut lowered = LintConfig::empty();
    lowered.base.insert(
        LINT_NAME.to_string(),
        crate::Severity::new(crate::Level::Error, crate::Level::Pass, crate::Level::Error),
    );
    let found = check_message_with_extra(&message("fixup! fix: x", root), Some(&lowered), &[]);
    assert_eq!(found[0].severity.effective(LintMode::Build), crate::Level::Pass);
    assert_eq!(found[0].severity.effective(LintMode::Push), crate::Level::Error);
    // an ordinary finding takes the table's severity whole, as every lint's does
    let found = check_message_with_extra(&message("research: x", root), Some(&cfg), &[]);
    assert_eq!(found[0].severity.effective(LintMode::Commit), crate::Level::Error);
}

#[test]
fn a_finding_named_in_the_table_is_set_by_it_even_for_an_autosquash_commit() {
    // Mutant: the autosquash carve-out also ignores `[lints.commit-subject.findings]`, so a
    // project cannot choose to refuse `fixup!` at the commit gate.
    let mut cfg = LintConfig::empty();
    cfg.findings.insert(
        LINT_NAME.to_string(),
        [(
            "autosquash-leftover".to_string(),
            crate::Severity::new(crate::Level::Error, crate::Level::Error, crate::Level::Error),
        )]
        .into(),
    );
    let root = Path::new("/tmp");
    let found = check_message_with_extra(&message("fixup! fix: x", root), Some(&cfg), &[]);
    assert_eq!(found[0].severity.effective(LintMode::Commit), crate::Level::Error);
}
