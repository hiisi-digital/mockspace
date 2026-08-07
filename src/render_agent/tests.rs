#![allow(unused_imports)]
use super::*;

// The agent frontmatter schema belongs to Claude Code. These tests exist to
// fail if someone later "normalises" it into a fixed field list the way the
// skills phase does, which would silently drop whatever upstream adds next.

#[test]
fn agent_frontmatter_passes_through_verbatim() {
    let fm = "name: fabian-giesen\ndescription: A persona.\ntools: Read, Grep\nmodel: fable";
    let out = render_agent_content(fm, "\n# You are Fabian Giesen\n");
    assert_eq!(
        out,
        "---\nname: fabian-giesen\ndescription: A persona.\ntools: Read, Grep\nmodel: fable\n---\n# You are Fabian Giesen\n"
    );
}

#[test]
fn agent_frontmatter_keeps_fields_mockspace_does_not_know() {
    // The whole reason this phase passes frontmatter through instead of
    // re-encoding it: a field mockspace has never heard of must survive.
    let fm = "name: x\ndescription: d\ntools: Read\nmodel: opus\nsome_future_field: value";
    let out = render_agent_content(fm, "body");
    assert!(
        out.contains("some_future_field: value"),
        "unknown frontmatter field was dropped; the agent schema is Claude Code's, \
             not mockspace's, so it must pass through untouched:\n{out}"
    );
}

#[test]
fn agent_body_is_not_wrapped_in_bookends() {
    // A persona is read as a whole character definition. If bookends ever
    // start being applied here, this catches it.
    let out = render_agent_content("name: x\ndescription: d", "\n# You are X\n");
    assert!(
        out.ends_with("---\n# You are X\n"),
        "body was altered: {out}"
    );
}

#[test]
fn agent_render_adds_separator_when_body_lacks_one() {
    // Defensive: a hand-written template whose body does not start with a
    // newline must not weld itself onto the closing `---`.
    let out = render_agent_content("name: x\ndescription: d", "# You are X\n");
    assert!(
        out.ends_with("---\n# You are X\n"),
        "separator missing: {out}"
    );
}

#[test]
fn agent_frontmatter_gets_variable_substitution() {
    // Substitution runs over the whole template before the split, matching
    // the rules phase, so frontmatter expands too. Pass-through is about not
    // renaming or dropping fields, not about refusing to expand them.
    let vars = vec![("project_name".to_string(), "ikiuni-renderer".to_string())];
    let raw = "---\nname: p\ndescription: Reviews {{project_name}} code\n---\n\nBody.\n";
    let raw = substitute_vars(raw, &vars);
    let (fm, body) = split_frontmatter(&raw);
    let out = render_agent_content(&fm, &body);
    assert!(
        out.contains("description: Reviews ikiuni-renderer code"),
        "frontmatter did not get substitution:\n{out}"
    );
    assert!(!out.contains("{{"), "a placeholder survived:\n{out}");
}

#[test]
fn agent_template_without_frontmatter_yields_none_to_skip_on() {
    // The phase skips these with a message rather than emitting an agent
    // Claude Code cannot register. `split_frontmatter` reporting an empty
    // frontmatter is what that skip keys off.
    let (fm, _body) = split_frontmatter("# Just a heading, no frontmatter\n");
    assert!(
        fm.trim().is_empty(),
        "expected no frontmatter to be detected"
    );
}

#[test]
fn agent_template_with_unterminated_frontmatter_is_treated_as_none() {
    // An opening `---` with no closing one is malformed. It lands in the
    // same skip path as "no frontmatter at all", which is the safe read:
    // better a named skip than shipping the whole file as frontmatter.
    let (fm, _body) = split_frontmatter("---\nname: p\ndescription: d\n");
    assert!(
        fm.trim().is_empty(),
        "unterminated frontmatter should not parse as frontmatter"
    );
}

#[test]
fn agent_render_round_trips_a_real_persona_shape() {
    // Exercises the actual path: split_frontmatter then render, the way the
    // agents phase runs it.
    let raw =
        "---\nname: p\ndescription: d\ntools: Read\nmodel: sonnet\n---\n\n# You are P\n\nBody.\n";
    let (fm, body) = split_frontmatter(raw);
    let out = render_agent_content(&fm, &body);
    assert_eq!(out, raw, "round trip changed a well-formed persona");
}

#[test]
fn builtin_preamble_within_budget() {
    let words = count_bookend_words(BUILTIN_PREAMBLE);
    assert!(
        words <= BOOKEND_MAX_WORDS,
        "BUILTIN_PREAMBLE is {words} words; budget is {BOOKEND_MAX_WORDS}. \
             Tight constant reminders only: longer content belongs in MAIN.md.tmpl."
    );
}

#[test]
fn builtin_postamble_within_budget() {
    let words = count_bookend_words(BUILTIN_POSTAMBLE);
    assert!(
        words <= BOOKEND_MAX_WORDS,
        "BUILTIN_POSTAMBLE is {words} words; budget is {BOOKEND_MAX_WORDS}."
    );
}

#[test]
fn word_count_strips_html_comments() {
    let text = "real words <!-- this block of ignored words --> more real";
    assert_eq!(count_bookend_words(text), 4);
}

#[test]
fn word_count_ignores_pure_punctuation() {
    let text = "one two -- three --- four";
    assert_eq!(count_bookend_words(text), 4);
}

#[test]
fn phase_labels_lock_down_user_visible_strings() {
    use mockspace_lint_rules::changelist_helpers::Phase;
    // These five strings flow into deny messages, status output,
    // README and USAGE_GUIDE tables, and the generated agent skill
    // text. Treat them as a public contract; #227 renamed
    // SRC-PLAN/SRC/DONE to DRAFT/IMPL/CLOSED so the verb `lock` in
    // `cargo mock lock` reads consistently with file-suffix state.
    assert_eq!(Phase::Topic.label(), "TOPIC");
    assert_eq!(Phase::Doc.label(), "DOC");
    assert_eq!(Phase::SrcPlan.label(), "DRAFT");
    assert_eq!(Phase::Src.label(), "IMPL");
    assert_eq!(Phase::Done.label(), "CLOSED");
}

#[test]
fn write_guard_uses_new_phase_labels() {
    // Regression check for #227: bash hook PHASE values must match
    // the new user-visible labels. If a stray "SRC-PLAN" / "SRC" /
    // "DONE" survives the rename, the rendered deny messages will
    // contradict the docs and Phase::label().
    let tmp = tempfile::tempdir().expect("tempdir");
    let mock_dir = tmp.path().join("mock");
    std::fs::create_dir(&mock_dir).expect("create mock dir");
    let cfg = Config::from_dir(&mock_dir);
    let hook = builtin_write_guard(&cfg);

    for new_label in ["DRAFT", "IMPL", "CLOSED"] {
        assert!(
            hook.contains(new_label),
            "rendered hook must reference new phase label `{new_label}`"
        );
    }
    for old_label in ["SRC-PLAN", "\"SRC\"", "\"DONE\""] {
        assert!(
            !hook.contains(old_label),
            "rendered hook still contains old phase token `{old_label}`"
        );
    }
}

#[test]
fn write_guard_phase_detection_is_cwd_independent() {
    // Regression test for #257: phase-detection git invocations must
    // run with `git -C "$REPO_ROOT"` so they resolve repo-relative
    // pathspecs regardless of the caller's cwd.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mock_dir = tmp.path().join("mock");
    std::fs::create_dir(&mock_dir).expect("create mock dir");
    let cfg = Config::from_dir(&mock_dir);
    let hook = builtin_write_guard(&cfg);

    // Phase-detection ls-files calls (4) and the dirty-docs diff and
    // the per-file --error-unmatch lookup all need the -C form.
    let phase_marker = "git -C \"$REPO_ROOT\" ls-files \"${MOCK_ROOT}/design_rounds/\"";
    let phase_calls = hook.matches(phase_marker).count();
    assert_eq!(
        phase_calls, 4,
        "expected 4 phase-detection ls-files calls with `-C \"$REPO_ROOT\"`, got {phase_calls}"
    );

    assert!(
        hook.contains("git -C \"$REPO_ROOT\" diff --name-only"),
        "DIRTY_DOCS query must use -C \"$REPO_ROOT\" so cwd-relative paths in the diff don't bypass the regex"
    );
    assert!(
        hook.contains("git -C \"$REPO_ROOT\" ls-files --error-unmatch \"$FULL_GIT_PATH\""),
        "FULL_GIT_PATH lookup must use -C \"$REPO_ROOT\" since the path is repo-rooted"
    );

    // Negative: no remaining bare ls-files / diff invocations slipped through.
    assert!(
        !hook.contains("\ngit ls-files "),
        "found bare `git ls-files` (cwd-sensitive); switch to `git -C \"$REPO_ROOT\" ls-files`"
    );
    assert!(
        !hook.contains("\ngit diff "),
        "found bare `git diff` (cwd-sensitive); switch to `git -C \"$REPO_ROOT\" diff`"
    );
}

#[test]
fn agent_gate_hook_self_heals_then_blocks() {
    let hook = builtin_mockspace_gate();
    // gates only git commit/push, anchored to the subcommand verb so
    // read-only commands are not caught
    assert!(hook.contains(r"\bgit[[:space:]]+(commit|push)\b"));
    assert!(!hook.contains(r"\bgit\b.*\b(commit|push)\b"));
    // checks both the validator and that core.hooksPath is wired
    assert!(hook.contains("target/hooks/pre-commit"));
    assert!(hook.contains("core.hooksPath"));
    assert!(hook.contains("mockspace.mockdir"));
    // self-heals with a locked check so it cannot dirty Cargo.lock
    assert!(hook.contains("cargo check --quiet --locked"));
    // fails closed via the deny helper
    assert!(hook.contains("deny \"mockspace gate is broken"));
    // scoped to this repo like the other builtins
    assert!(hook.contains("_scope_or_allow"));
}

#[test]
fn canon_design_code_chain_is_a_generated_builtin() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = Config::from_dir(&tmp.path().join("mock"));
    let builtins = generate_builtin_templates(&cfg);
    let rule = builtins
        .rules
        .iter()
        .find(|r| r.name == "canon-design-code-chain")
        .expect("canon-design-code-chain builtin rule must be generated");

    // covers design templates, crate source, and the research area, since
    // the rule governs which tier a document belongs to across all three
    assert!(
        rule.apply_to
            .iter()
            .any(|g| g.ends_with("*.md.tmpl") && !g.contains("crates"))
    );
    assert!(
        rule.apply_to
            .iter()
            .any(|g| g.contains("crates") && g.ends_with("*.md.tmpl"))
    );
    assert!(
        rule.apply_to
            .iter()
            .any(|g| g.contains("crates") && g.ends_with("*.rs"))
    );
    assert!(rule.apply_to.iter().any(|g| g.contains("research")));
    assert!(rule.apply_to.iter().any(|g| g.contains("canon")));

    assert!(rule.body.contains("Canon is the theory"));
    assert!(rule.body.contains("Design is the spec"));
    assert!(rule.body.contains("nuked"));
    assert!(rule.body.contains("Canon is never deleted, only demoted"));
    // the reserved directory is named, and the future guard is stated as
    // intent rather than as an already-working gate
    assert!(rule.body.contains("mock/canon/") || rule.body.contains("/canon/"));
    assert!(rule.body.contains("does not exist yet"));
    assert!(
        !rule.body.contains('\u{2014}'),
        "no em-dashes in a generated rule"
    );
}

#[test]
fn agent_gate_is_a_generated_builtin() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let claude = tmp.path().join(".claude/hooks");
    let copilot = tmp.path().join(".github/hooks");
    std::fs::create_dir_all(&claude).unwrap();
    std::fs::create_dir_all(&copilot).unwrap();
    let cfg = Config::from_dir(&tmp.path().join("mock"));
    let mut count = 0;
    let metas = generate_builtin_hooks(&cfg, &claude, &copilot, &mut count);
    assert!(metas.iter().any(|m| m.name == "mockspace-gate.sh"));
    assert!(claude.join("mockspace-gate.sh").exists());
    // matched on Bash so it fires on the agent's git commands
    let gate = metas
        .iter()
        .find(|m| m.name == "mockspace-gate.sh")
        .unwrap();
    assert!(gate.matchers.iter().any(|s| s == "Bash"));
}

// ---------------------------------------------------------------------------
// The design-talk skill and the files it ships
// ---------------------------------------------------------------------------
//
// A skill that only tells an agent what to do is a rule with extra steps. This
// one carries a script, so the shipping of files is part of the contract, and
// the gate is opt-in because the flow presumes a human answering in the loop.

/// A throwaway repository with a mockspace.toml and an agent config, so the
/// gate is exercised through the config that actually declares it rather than
/// through a hand-built struct that could drift from it.
fn skill_fixture(design_talks: Option<bool>) -> std::path::PathBuf {
    // A sequence number besides the pid and the flag: two tests passing the
    // same flag otherwise share one directory and remove_dir_all it under
    // each other when the suite runs in parallel.
    static FIXTURE_SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "mockspace-skilltest-{}-{}-{:?}",
        std::process::id(),
        FIXTURE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        design_talks
    ));
    let _ = std::fs::remove_dir_all(&root);
    let mock = root.join("mock");
    std::fs::create_dir_all(mock.join("agent")).unwrap();
    std::fs::write(
        root.join("mockspace.toml"),
        "project_name = \"fixture\"\ncrate_prefix = \"fixture\"\nmock_dir = \"mock\"\n",
    )
    .unwrap();
    if let Some(on) = design_talks {
        std::fs::write(
            mock.join("agent").join("config.toml"),
            format!("agent_accelerated_interactive_design_talks = {on}\n"),
        )
        .unwrap();
    }
    mock
}

// The same fixture with an arbitrary agent config, for knobs beyond the
// design-talk one.
fn skill_fixture_config(config: &str) -> std::path::PathBuf {
    let mock = skill_fixture(None);
    std::fs::write(mock.join("agent").join("config.toml"), config).unwrap();
    mock
}

#[test]
fn design_talk_skill_is_absent_unless_opted_in() {
    for declared in [None, Some(false)] {
        let cfg = crate::config::Config::from_dir(&skill_fixture(declared));
        let builtins = generate_builtin_templates(&cfg);
        assert!(
            !builtins.skills.iter().any(|s| s.dir_name == "design-talk"),
            "off by default, and off when declared false ({declared:?})"
        );
    }
}

#[test]
fn design_talk_skill_ships_its_script_executable_and_its_manifest() {
    let cfg = crate::config::Config::from_dir(&skill_fixture(Some(true)));
    let builtins = generate_builtin_templates(&cfg);

    let skill = builtins
        .skills
        .iter()
        .find(|s| s.dir_name == "design-talk")
        .expect("opted in, so the skill is generated");

    let script = skill
        .files
        .iter()
        .find(|f| f.rel_path == "scripts/consume-strays")
        .expect("the skill ships its script");
    assert!(
        script.executable,
        "a script without the executable bit cannot be run, which is the whole point"
    );
    assert!(
        script.contents.starts_with("#!/usr/bin/env nutshell"),
        "shipped verbatim, so the shebang is still the first line"
    );

    let manifest = skill
        .files
        .iter()
        .find(|f| f.rel_path == "nut.toml")
        .expect("the script's dependencies are declared beside it");
    assert!(
        manifest.contents.contains("[deps.mockspace]"),
        "the script uses mockspace::mock, so the unit declares it"
    );
}

#[test]
fn the_skill_catalogue_names_every_builtin_and_nothing_else() {
    // The sweep in the renderer deletes stale directories against
    // BUILTIN_SKILL_DIRS. A generated skill missing from the catalogue would
    // never be swept when its knob turns off; a catalogue name nothing
    // generates would sweep a directory no knob can bring back.
    let cfg = crate::config::Config::from_dir(&skill_fixture(Some(true)));
    let builtins = generate_builtin_templates(&cfg);

    for skill in &builtins.skills {
        assert!(
            BUILTIN_SKILL_DIRS.contains(&skill.dir_name.as_str()),
            "{} is generated but absent from BUILTIN_SKILL_DIRS, so disabling \
             it would leave its directory behind forever",
            skill.dir_name
        );
    }
    for dir in BUILTIN_SKILL_DIRS {
        assert!(
            builtins.skills.iter().any(|s| s.dir_name == *dir),
            "{dir} is in BUILTIN_SKILL_DIRS but nothing generates it with \
             every knob on, so the sweep could delete what no knob restores"
        );
    }
}

#[test]
fn sketching_and_benchmarking_can_be_declined() {
    // Default on, but a knob: no builtin prose lands in a consumer's context
    // without one, and declining must actually decline.
    let cfg = crate::config::Config::from_dir(&skill_fixture_config(
        "agent_sketching_skill = false\nagent_benchmarking_skill = false\n",
    ));
    let builtins = generate_builtin_templates(&cfg);
    for name in ["sketching", "benchmarking"] {
        assert!(
            !builtins.skills.iter().any(|s| s.dir_name == name),
            "{name} was declined in config and generated anyway"
        );
    }
}

#[test]
fn sketching_and_benchmarking_are_on_by_default() {
    // Unlike design-talk, these default on: the mistake they prevent is
    // made in the first thirty seconds of a task, by every consumer, whether
    // or not a human is in the loop. An absent key or file means on.
    for declared in [None, Some(false), Some(true)] {
        let cfg = crate::config::Config::from_dir(&skill_fixture(declared));
        let builtins = generate_builtin_templates(&cfg);
        for want in ["sketching", "benchmarking"] {
            let skill = builtins
                .skills
                .iter()
                .find(|s| s.dir_name == want)
                .unwrap_or_else(|| {
                    panic!("{want} defaults on whatever the design-talk knob says ({declared:?})")
                });
            assert!(
                skill.skill_description.contains("BEFORE"),
                "{want}'s description must say when to invoke it, since that is what makes a \
                 skill discoverable at the moment it is needed"
            );
            assert!(!skill.body.is_empty());
        }
    }
}
