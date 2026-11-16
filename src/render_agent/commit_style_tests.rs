//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The commit-style skill and the commit gate read one configuration.
//!
//! The documentation said `feat, fix, refactor, docs, test, chore` while nothing enforced it,
//! and seven `bench:` commits passed. These hold the two to each other: what the skill lists is
//! what the gate accepts, an example in the skill passes the gate, and a type added in the one
//! place the project configures reaches both.

use std::path::Path;

use mockspace_lint_rules::commit_subject::SubjectCase;
use mockspace_lint_rules::{AgentMode, LintPack, MessageContext, MessageDomain};

use crate::model::CrateMap;
use crate::render_agent::{
    commit_style,
    compute_template_vars,
    generate_builtin_templates,
    substitute_vars,
};

fn project(lints: &str, agent: &str) -> tempfile::TempDir {
    let guard = mockspace_manifest::scratch::tempdir().unwrap();
    let mock = guard.path().join("mock");
    std::fs::create_dir_all(mock.join("agent")).unwrap();
    std::fs::write(
        guard.path().join("mockspace.toml"),
        format!(
            "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\nmock_dir = \"mock\"\n{lints}"
        ),
    )
    .unwrap();
    std::fs::write(mock.join("agent").join("config.toml"), agent).unwrap();
    guard
}

fn skill_of(lints: &str) -> (crate::config::Config, String) {
    let fx = project(lints, "");
    let cfg = crate::config::Config::from_dir(&fx.path().join("mock"));
    let built = generate_builtin_templates(&cfg, &LintPack::default());
    let body = built
        .skills
        .iter()
        .find(|s| s.dir_name == "commit-style")
        .expect("the commit-style skill is on by default")
        .body
        .clone();
    (cfg, body)
}

fn gate_refuses(cfg: &crate::config::Config, subject: &str) -> bool {
    let root = Path::new("/tmp");
    let ctx = MessageContext::new(
        MessageDomain::CommitMessage,
        AgentMode::Assistant,
        subject,
        "COMMIT_EDITMSG",
        root,
    );
    !mockspace_lint_rules::check_message_with_extra(&ctx, Some(&cfg.lint_overrides), &[]).is_empty()
}

#[test]
fn the_skill_is_generated_by_default_and_lists_bench() {
    // Mutant: the skill is not shipped, or `bench` is left out of the default set.
    let (_, body) = skill_of("");
    assert!(body.contains("`bench`"), "{body}");
    for ty in ["feat", "fix", "refactor", "docs", "test", "chore"] {
        assert!(
            body.contains(&format!("`{ty}`")),
            "{ty} missing from\n{body}"
        );
    }
}

#[test]
fn no_token_is_left_in_the_rendered_skill() {
    // Mutant: a placeholder in the template has no substitution.
    let (_, body) = skill_of("");
    for t in [
        "{separator}",
        "{commit_rules}",
        "{commit_types}",
        "{commit_breaking}",
        "{commit_examples}",
        "{mock_dir}",
    ] {
        assert!(!body.contains(t), "{t} left in\n{body}");
    }
}

#[test]
fn a_type_added_in_the_one_place_reaches_the_skill_and_the_gate() {
    // Mutant: the skill reads a list of its own and the gate another.
    let lints = "[lints.commit-subject]\nextra_types = \"research\"\n";
    let (cfg, body) = skill_of(lints);
    assert!(body.contains("`research`"), "{body}");
    assert!(!gate_refuses(&cfg, "research: sketch a lint"));
    let (plain, plain_body) = skill_of("");
    assert!(!plain_body.contains("`research`"));
    assert!(gate_refuses(&plain, "research: sketch a lint"));
}

#[test]
fn every_type_the_skill_lists_passes_the_gate_and_every_example_does_too() {
    // Mutant: an example or a listed type breaks a rule printed above it.
    for lints in [
        "",
        "[lints.commit-subject]\nextra_types = \"research,deploy\"\n",
        "[lints.commit-subject]\ntypes = \"feat,fix\"\nsubject_case = \"sentence\"\n",
    ] {
        let (cfg, _) = skill_of(lints);
        let policy = commit_style::policy(&cfg);
        let sample = match policy.subject_case() {
            SubjectCase::Sentence => "Add the thing",
            _ => "add the thing",
        };
        for ty in policy.types() {
            assert!(
                !gate_refuses(&cfg, &format!("{ty}: {sample}")),
                "{ty} under {lints:?}"
            );
        }
        for example in commit_style::examples_text(&policy).lines() {
            assert!(!gate_refuses(&cfg, example), "`{example}` under {lints:?}");
        }
    }
}

#[test]
fn a_loosened_knob_is_no_longer_printed_as_a_rule() {
    // Mutant: the skill keeps saying the stricter thing after the gate stops enforcing it.
    let (_, strict) = skill_of("");
    assert!(strict.contains("NO PARENTHESES"), "{strict}");
    assert!(strict.contains("No period at end"), "{strict}");
    assert!(strict.contains("At most 71 characters"), "{strict}");
    let (_, loose) = skill_of(
        "[lints.commit-subject]\nallow_scope = \"true\"\nforbid_trailing_period = \"false\"\nmax_subject = \"0\"\n",
    );
    assert!(!loose.contains("NO PARENTHESES"), "{loose}");
    assert!(!loose.contains("No period at end"), "{loose}");
    assert!(!loose.contains("At most"), "{loose}");
}

#[test]
fn the_skill_can_be_declined() {
    // Mutant: `agent_commit_style_skill = false` is not read.
    let fx = project("", "agent_commit_style_skill = false\n");
    let cfg = crate::config::Config::from_dir(&fx.path().join("mock"));
    let built = generate_builtin_templates(&cfg, &LintPack::default());
    assert!(!built.skills.iter().any(|s| s.dir_name == "commit-style"));
}

#[test]
fn a_consumer_template_can_read_the_same_set() {
    // Mutant: the variables are not computed, so a consumer's own skill keeps a hand-kept list.
    let fx = project("[lints.commit-subject]\nextra_types = \"research\"\n", "");
    let cfg = crate::config::Config::from_dir(&fx.path().join("mock"));
    let vars = compute_template_vars(&CrateMap::new(), &cfg);
    let get = |k: &str| {
        vars.iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert!(get("commit_types").contains("`bench`") && get("commit_types").contains("`research`"));
    assert!(get("commit_rules").contains("1. Type is required"));
    assert!(get("commit_examples").contains("bench: "));
    assert_eq!(
        substitute_vars("Types: {{commit_types}}", &vars),
        format!("Types: {}", get("commit_types"))
    );
}
