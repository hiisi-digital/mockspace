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
