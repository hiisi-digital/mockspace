//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
    let vars = vec![("project_name".to_string(), "widget".to_string())];
    let raw = "---\nname: p\ndescription: Reviews {{project_name}} code\n---\n\nBody.\n";
    let raw = substitute_vars(raw, &vars);
    let (fm, body) = split_frontmatter(&raw);
    let out = render_agent_content(&fm, &body);
    assert!(
        out.contains("description: Reviews widget code"),
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
    // Self-heals by running the launcher, which is the only thing that
    // both writes the validator and points core.hooksPath. It previously
    // ran `cargo check` in the mock workspace, on the reasoning that
    // build.rs re-ran the bootstrap; that bootstrap is gone and its
    // remaining symbol fails the build, so the step healed nothing.
    assert!(
        hook.contains("cd \"$root\" && cargo mock activate"),
        "{hook}"
    );
    // The narrow repair is tried before the full run, because a full run
    // regenerates docs and agent rules and this executes while a commit is in
    // flight.
    let narrow = hook
        .find("cargo mock activate")
        .expect("narrow repair present");
    let full = hook.rfind("&& cargo mock )").expect("full repair present");
    assert!(narrow < full, "the narrow repair must be attempted first");
    assert!(
        !hook.contains("cargo check"),
        "the cargo check self-heal cannot restore the gate: build.rs no longer bootstraps"
    );
    // fails closed via the deny helper
    assert!(hook.contains("deny \"mockspace gate is broken"));
    // scoped to this repo like the other builtins
    assert!(hook.contains("_scope_or_allow"));
}

#[test]
fn canon_design_code_chain_is_a_generated_builtin() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = Config::from_dir(&tmp.path().join("mock"));
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
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
    // design_rounds/ is where an agent decides whether what it is writing is
    // canon, design, or code, so this rule applies there too
    assert!(rule.apply_to.iter().any(|g| g.contains("design_rounds")));

    assert!(rule.body.contains("Canon is the theory"));
    assert!(rule.body.contains("Design is the spec"));
    assert!(rule.body.contains("nuked"));
    assert!(rule.body.contains("Canon is never deleted, only demoted"));
    // the reserved directory is named, with its archive/ and examples/
    // subdirectories, and the deliberate inversion of the design_rounds/
    // any-subdir-means-archived rule
    assert!(rule.body.contains("mock/canon/") || rule.body.contains("/canon/"));
    assert!(rule.body.contains("archive/"));
    assert!(rule.body.contains("examples/"));
    assert!(rule.body.contains("only `archive/` does"));
    // demotion moves a whole timestamped directory (parallel to
    // design_rounds/<timestamp>/), not a filename suffix
    assert!(rule.body.contains("canon/archive/<timestamp>/"));
    assert!(rule.body.contains("design_rounds/<timestamp>/"));
    assert!(
        rule.body
            .contains("a directory rather than a filename suffix")
    );
    // canon files are .md, stated as derived from the design_rounds/
    // parallel rather than as a settled decision
    assert!(rule.body.contains("A canon file is `.md`, not `.md.tmpl`"));
    assert!(rule.body.contains("derived rather than decided"));
    // the two rules that hold now, stated firmly rather than as intent
    assert!(rule.body.contains("must declare the canon it relates to"));
    assert!(rule.body.contains("has no reason to exist"));
    assert!(
        rule.body
            .contains("Naming a canon that does not exist is a hard failure")
    );
    // the cascade/relation mechanism is named as anticipated and unspecified,
    // not as something to invent or assume absent
    assert!(
        rule.body.contains("not specified")
            || rule.body.contains("not yet formalised")
            || rule.body.contains("is not formalised yet")
    );
    assert!(rule.body.contains("dogfood"));
    // no automated gate exists yet for any of the three rules above
    assert!(
        rule.body.contains("No tooling enforces any of this yet")
            || rule
                .body
                .contains("has a lint, phase gate, or hook behind it yet")
    );

    // A canon change nukes only its declared dependents, never every design.
    // A guard pinned to one literal sentence ("every design is nuked") missed
    // a second bad phrasing ("every design beneath it") in the unformalised-
    // cascade section: the grep that produced the guard shared its own blind
    // spot. Pin the CLAIM instead of a sentence: enumerate every legitimate
    // "every design" occurrence and assert the substring appears nowhere
    // else. Three are legitimate, not two: the mutation-order clause naming
    // its own scope, the disclaimer following it, and the tier test (whose
    // "every design built on it" is inherently scoped by "on it").
    let every_design_occurrences = rule.body.matches("every design").count();
    assert_eq!(
        every_design_occurrences, 3,
        "expected exactly 3 occurrences of \"every design\" (the mutation-order clause, its \
         disclaimer, and the tier test); found {every_design_occurrences}. A new occurrence is \
         either a fourth legitimate one this assertion needs updating for, or a regression to the \
         unscoped \"nukes every design\" claim the mutation order corrected."
    );
    assert!(
        rule.body
            .contains("every design that declares that file is nuked first")
    );
    assert!(
        rule.body
            .contains("Not every design in the project. Only the declared dependents")
    );
    assert!(
        rule.body
            .contains("every design built on it is wrong, it is canon")
    );
    assert!(!rule.body.contains("every design is nuked"));
    assert!(!rule.body.contains("every design beneath"));
    // the two consequences that fall out of scoping to declared dependents
    assert!(rule.body.contains("Adding a new canon file nukes nothing"));
    assert!(
        rule.body
            .contains("Appending to an existing canon file also nukes nothing")
    );
    assert!(rule.body.contains("trigger is invalidation, not editing"));
    // file granularity: one file is one topic, so splitting is the fix for
    // an over-broad blast radius, not a finer-grained dependency unit
    assert!(rule.body.contains("one file is one topic"));
    assert!(
        rule.body
            .contains("splitting the file, not refining the granularity")
    );

    // "just change it" overstates the leaf tier's freedom into no
    // constraints at all; this is the corrected framing (2026-08-07), and a
    // later editor restoring the old bare phrasing must not slip past
    // unnoticed, since it contradicts the design-governs-code relation the
    // rule states elsewhere
    assert!(!rule.body.contains("just change it. It is the leaf"));
    assert!(
        rule.body
            .contains("nothing has to be nuked first, because code is the leaf")
    );
    assert!(
        rule.body
            .contains("The mockspace round ceremony applies in full")
    );
    assert!(
        rule.body
            .contains("nothing may appear in code that is not in the design")
    );
    assert!(
        rule.body
            .contains("undeclared design change wearing the leaf tier's freedom")
    );
    assert!(
        rule.body
            .contains("unconstrained downward and fully constrained upward")
    );

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
    // Carrying the marker is the load-bearing half. The settings merge asks
    // each script whether it is ours, so a builtin written without one reads
    // as somebody's, its wiring is kept, and the fresh wiring is appended
    // beside it on every run. Deleting the marker call left the whole suite
    // green before this assertion existed.
    for dir in [&claude, &copilot] {
        let body = std::fs::read_to_string(dir.join("mockspace-gate.sh")).unwrap();
        assert!(
            crate::render_design::is_generated(&body),
            "a builtin hook reached {} unmarked:\n{body}",
            dir.display()
        );
        assert!(
            body.starts_with("#!"),
            "the marker was put in front of the shebang:\n{body}"
        );
    }
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
        let builtins = generate_builtin_templates(&cfg, &LintPack::default());
        assert!(
            !builtins.skills.iter().any(|s| s.dir_name == "design-talk"),
            "off by default, and off when declared false ({declared:?})"
        );
    }
}

#[test]
fn design_talk_skill_ships_its_script_executable_and_its_manifest() {
    let cfg = crate::config::Config::from_dir(&skill_fixture(Some(true)));
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());

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
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());

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
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
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
        let builtins = generate_builtin_templates(&cfg, &LintPack::default());
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

#[test]
fn write_guard_shame_carve_out_matches_a_whole_path_component() {
    // The carve-out is for the file named `SHAME.md.tmpl`. Written as a
    // bare suffix regex it also allows `NOT_SHAME.md.tmpl` and friends
    // straight through the phase gate. This runs the pattern the hook
    // actually ships rather than asserting on its text.
    let tmp = tempfile::tempdir().expect("tempdir");
    let mock_dir = tmp.path().join("mock");
    std::fs::create_dir(&mock_dir).expect("create mock dir");
    let cfg = Config::from_dir(&mock_dir);
    let hook = builtin_write_guard(&cfg);

    let line = hook
        .lines()
        .find(|l| l.contains("SHAME") && l.contains("grep -qE"))
        .expect("the write guard carves SHAME out with a grep -qE");
    let open = line.find('\'').expect("pattern is single-quoted");
    let close = line[open + 1 ..].find('\'').expect("pattern is closed") + open + 1;
    let pattern = &line[open + 1 .. close];

    // The oracle is the Rust predicate the two lints call, not a list of
    // cases someone thought of. Any divergence between the shell regex and
    // `is_shame_template` fails here, which is the only thing keeping one
    // rule expressed in two languages in step.
    let cases = [
        "crates/foo/SHAME.md.tmpl",
        "crates/SHAME.md.tmpl",
        "SHAME.md.tmpl",
        "crates/foo/NOT_SHAME.md.tmpl",
        "crates/foo/DESIGN_SHAME.md.tmpl",
        "crates/foo/DESIGN.md.tmpl",
        "crates/foo/SHAME.md.tmpl.bak",
        "crates/SHAME.md.tmpl/inner.md.tmpl",
    ];
    for path in cases {
        let expected = mockspace_lint_rules::is_shame_template(path);
        let out = std::process::Command::new("grep")
            .args(["-qE", pattern])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                c.stdin
                    .as_mut()
                    .expect("stdin")
                    .write_all(path.as_bytes())?;
                c.wait()
            })
            .expect("run grep");
        assert_eq!(
            out.success(),
            expected,
            "the hook regex `{pattern}` and is_shame_template disagree about `{path}`"
        );
    }
}

/// Run the generated write guard against one target path in a repo whose
/// round is in DRAFT (doc changelist locked, so crate docs are frozen).
/// Returns true when the hook allowed the write.
#[cfg(unix)]
fn write_guard_allows(target_rel: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};

    let repo = std::env::temp_dir().join(format!(
        "wg-{}-{}",
        std::process::id(),
        target_rel.replace('/', "_")
    ));
    let _ = std::fs::remove_dir_all(&repo);
    let rounds = repo.join("mock/design_rounds");
    std::fs::create_dir_all(&rounds).unwrap();
    std::fs::create_dir_all(repo.join("mock/crates/foo")).unwrap();
    // A locked doc changelist and no src changelist is DRAFT: crate doc
    // templates are frozen, which is the state the carve-out exists for.
    std::fs::write(
        rounds.join("202601010000_changelist.doc.lock.md"),
        "locked\n",
    )
    .unwrap();

    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "t@example.com"],
        vec!["config", "user.name", "t"],
        vec!["add", "-A"],
    ] {
        Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .output()
            .unwrap();
    }

    let cfg = Config::from_dir(&repo.join("mock"));
    let script = repo.join("guard.sh");
    let body = builtin_write_guard(&cfg)
        .replace("{{HOOK_HELPERS}}", crate::render_agent::CLAUDE_HOOK_HELPERS)
        .replace("{{REPO_ROOT}}", &repo.display().to_string());
    std::fs::write(&script, body).unwrap();
    let mut perm = std::fs::metadata(&script).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&script, perm).unwrap();

    let payload = format!(
        r#"{{"tool_name":"Write","tool_input":{{"file_path":"{}/mock/{}"}}}}"#,
        repo.display(),
        target_rel
    );
    let mut child = Command::new("bash")
        .arg(&script)
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(payload.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let _ = std::fs::remove_dir_all(&repo);

    // Fail closed on anything that is not a verdict. Testing
    // `!stdout.contains("deny")` reads a script that errored out as an allow,
    // which is the opposite of what a gate must do under uncertainty.
    assert!(
        out.status.success(),
        "the guard exited {:?} for `{target_rel}`: {stderr}",
        out.status.code()
    );
    let denied = stdout.contains(r#""permissionDecision":"deny""#);
    // The allow helper emits a hookSpecificOutput carrying no decision key at
    // all, so an allow is the absence of a deny inside a well-formed verdict
    // rather than an explicit "allow". Requiring the envelope is what keeps a
    // crashed script from reading as permission.
    assert!(
        stdout.contains("hookSpecificOutput"),
        "the guard produced no verdict envelope for `{target_rel}`: stdout={stdout} stderr={stderr}"
    );
    !denied
}

#[test]
#[cfg(unix)]
fn the_shame_escape_hatch_stays_writable_while_a_lookalike_is_gated() {
    // The behaviour, not the regex text. Neutering the carve-out's `allow`
    // leaves every text assertion green, so this runs the hook.
    assert!(
        write_guard_allows("crates/foo/SHAME.md.tmpl"),
        "SHAME.md.tmpl is the escape hatch and must be writable in every phase"
    );
    assert!(
        !write_guard_allows("crates/foo/DESIGN.md.tmpl"),
        "an ordinary crate doc template is frozen in DRAFT"
    );
    assert!(
        !write_guard_allows("crates/foo/NOT_SHAME.md.tmpl"),
        "a lookalike is not the escape hatch and stays gated"
    );
}

// ---------------------------------------------------------------------------
// The tool-catalogue and panel-discipline builtin rules
// ---------------------------------------------------------------------------

struct StubTool;
impl mockspace_lint_rules::tool::Tool for StubTool {
    fn name(&self) -> &'static str {
        "phrase-search"
    }

    fn description(&self) -> &'static str {
        "find a phrase across hard-wrapped lines"
    }

    fn not_a_lint(&self) -> mockspace_lint_rules::tool::NotALint {
        mockspace_lint_rules::tool::NotALint::TakesAQuestion
    }

    fn args(&self) -> &[mockspace_lint_rules::tool::ArgSpec] {
        &[mockspace_lint_rules::tool::ArgSpec {
            name:        "phrase",
            required:    true,
            description: "the phrase to look for",
        }]
    }

    fn run(
        &self,
        _ctx: &mockspace_lint_rules::tool::ToolContext<'_>,
    ) -> mockspace_lint_rules::tool::ToolReport {
        mockspace_lint_rules::tool::ToolReport::reported("", 1)
    }
}

fn pack_with_stub_tool() -> LintPack {
    LintPack {
        tools: vec![Box::new(StubTool)],
        ..LintPack::default()
    }
}

#[test]
fn tool_catalogue_rule_is_on_by_default_and_off_when_declared_false() {
    let cfg = crate::config::Config::from_dir(&skill_fixture(None));
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
    assert!(
        builtins.rules.iter().any(|r| r.name == "tool-catalogue"),
        "on by default, no config.toml at all"
    );

    let mock = skill_fixture_config("agent_tool_catalogue = false\n");
    let cfg = crate::config::Config::from_dir(&mock);
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
    assert!(
        !builtins.rules.iter().any(|r| r.name == "tool-catalogue"),
        "declared false must turn it off"
    );
}

#[test]
fn tool_catalogue_rule_embeds_a_live_snapshot_not_a_fixed_string() {
    // The claim this rule makes about itself ("never drifts, computed from
    // what actually exists") is only true if a project tool passed into the
    // generator actually shows up in the generated body. Without this, the
    // rule could just as well be static prose that happens to also print a
    // command name.
    let cfg = crate::config::Config::from_dir(&skill_fixture(None));
    let builtins = generate_builtin_templates(&cfg, &pack_with_stub_tool());
    let rule = builtins
        .rules
        .iter()
        .find(|r| r.name == "tool-catalogue")
        .expect("on by default");
    assert!(
        rule.body.contains("phrase-search"),
        "the declared project tool must appear in the generated snapshot:\n{}",
        rule.body
    );
    assert!(
        rule.body.contains("mock lock"),
        "a builtin must appear too:\n{}",
        rule.body
    );

    // and the negative: an empty pack's rule says nothing about a tool that
    // was never declared
    let builtins_empty = generate_builtin_templates(&cfg, &LintPack::default());
    let rule_empty = builtins_empty
        .rules
        .iter()
        .find(|r| r.name == "tool-catalogue")
        .unwrap();
    assert!(!rule_empty.body.contains("phrase-search"));
}

#[test]
fn panel_discipline_rule_is_off_by_default_and_on_when_opted_in() {
    let cfg = crate::config::Config::from_dir(&skill_fixture(None));
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
    assert!(
        !builtins.rules.iter().any(|r| r.name == "panel-discipline"),
        "off by default"
    );

    let mock = skill_fixture_config("agent_panel_discipline = true\n");
    let cfg = crate::config::Config::from_dir(&mock);
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
    assert!(
        builtins.rules.iter().any(|r| r.name == "panel-discipline"),
        "declared true must turn it on"
    );
}

#[test]
fn panel_discipline_rule_states_the_seat_cap_and_the_canon_prohibition() {
    let mock = skill_fixture_config("agent_panel_discipline = true\n");
    let cfg = crate::config::Config::from_dir(&mock);
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());
    let rule = builtins
        .rules
        .iter()
        .find(|r| r.name == "panel-discipline")
        .expect("opted in");
    assert!(
        rule.body.contains("Ninety-nine"),
        "the seat cap must be stated:\n{}",
        rule.body
    );
    assert!(
        rule.body.contains("do not write canon") || rule.body.contains("never writes canon"),
        "the canon prohibition must be stated in words a reader cannot miss:\n{}",
        rule.body
    );
    assert!(rule.body.contains("mock panel seat"));
    assert!(rule.body.contains("mock panel consolidate"));
}

// ---------------------------------------------------------------------------
// Every writer into a hooks directory goes through the marker
// ---------------------------------------------------------------------------
//
// The settings merge asks each script on disk whether it carries the
// generation marker, and keeps the entry when it does not. So a writer that
// puts a script into a hooks directory without one produces wiring that is
// kept as somebody's and appended to on every run, without bound.
//
// Three of the four writers called `with_generated_marker`. The fourth, the
// one that renders a repository's own `mock/agent/hooks/`, did not, and
// nothing said so: the suite was green, because every settings test builds its
// scripts with a local helper that writes the marker itself and therefore
// exercises the reader and never the writer.
//
// `agent_gate_is_a_generated_builtin` closes the builtin writer by reading
// what it wrote. This closes the other three by reading the source, which is
// the only instrument that reaches a writer no test drives.

/// A module's source with any inline `#[cfg(test)]` block removed, and a
/// `#[cfg(test)] mod tests;` declaration left where it is.
fn only_the_shipping_half(src: &str) -> String {
    let mut out = src;
    let mut cut = 0usize;
    while let Some(at) = out[cut ..].find("\n#[cfg(test)]") {
        let at = cut + at + 1;
        let after = &out[at ..];
        let declares_a_sibling = after
            .lines()
            .nth(1)
            .is_some_and(|l| l.trim_end().ends_with(';'));
        if declares_a_sibling {
            cut = at + 1;
            continue;
        }
        return out[.. at].to_string();
    }
    out = &out[..];
    out.to_string()
}

/// Every `fs::write` of a hook script in this module, as the file it sits in,
/// its line, and the expression whose content it writes.
///
/// A hook script is identified by where it lands rather than by what anything
/// is called: the path being written was bound from a `*hooks_dir.join(...)`,
/// which is the one thing all four writers have in common and the one that
/// cannot be renamed out from under this. `hooks.json` is joined the same way
/// and is excluded by name, because it is wiring rather than a script and JSON
/// has nowhere to put a shell comment.
fn hook_script_writes() -> Vec<(String, usize, String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render_agent");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir)
        .expect("the module directory is there")
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).expect("a module reads");
        // Fixtures inside an inline test module write whatever the case under
        // test needs, so they are cut. A `#[cfg(test)] mod tests;` declaration
        // is not one: it sits near the top of a `mod.rs`, above everything,
        // and cutting there hides every writer in the file. This reported
        // clean over a deliberately unmarked write twice before the two were
        // told apart.
        let body = only_the_shipping_half(&src);

        // The bindings that name a hook script on disk.
        let mut script_paths: Vec<String> = Vec::new();
        for line in body.lines() {
            let Some((lhs, rhs)) = line.split_once(" = ") else {
                continue;
            };
            let Some(binding) = lhs.trim().strip_prefix("let ") else {
                continue;
            };
            if rhs.contains("hooks_dir.join(") && !rhs.contains("\"hooks.json\"") {
                script_paths.push(binding.trim().to_string());
            }
        }

        for (i, line) in body.lines().enumerate() {
            let Some(rest) = line.split_once("fs::write(").map(|(_, r)| r) else {
                continue;
            };
            let mut parts = rest.splitn(2, ", ");
            let (Some(target), Some(content)) = (parts.next(), parts.next()) else {
                continue;
            };
            let target = target.trim().trim_start_matches('&').to_string();
            if script_paths.contains(&target) {
                found.push((name.clone(), i + 1, target, content.to_string()));
            }
        }
    }
    found
}

#[test]
fn every_hook_script_this_module_writes_carries_the_marker() {
    let writes = hook_script_writes();
    assert!(
        writes.len() >= 4,
        "the reader found {} hook writers, so it has stopped matching them",
        writes.len()
    );
    let mut unmarked = Vec::new();
    for (file, line, target, content) in &writes {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src/render_agent")
                .join(file),
        )
        .unwrap();
        // Either the call is right there, or it built the binding being written.
        // The write's second argument carries the rest of the statement after
        // it, so the binding is its leading identifier and nothing else.
        let binding: String = content
            .trim()
            .trim_start_matches('&')
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        let binding = binding.as_str();
        let marked_at_the_write = content.contains("with_generated_marker");
        let marked_at_the_binding = src.contains(&format!("let {binding} = "))
            && src
                .split(&format!("let {binding} = "))
                .skip(1)
                .any(|after| {
                    after
                        .split_once(';')
                        .is_some_and(|(rhs, _)| rhs.contains("with_generated_marker"))
                });
        if !(marked_at_the_write || marked_at_the_binding) {
            unmarked.push(format!("{file}:{line} writes {target} unmarked"));
        }
    }
    assert!(unmarked.is_empty(), "{}", unmarked.join("\n"));
}

#[test]
fn the_writer_reader_can_actually_fail() {
    // The check above reports nothing, which a reader matching nothing would
    // do just as well. So the reader is run over a writer built to trip it.
    let writes = hook_script_writes();
    for module in ["wire.rs", "mod.rs", "content.rs"] {
        assert!(
            writes.iter().any(|(f, ..)| f == module),
            "the reader reached no writer in {module}, which has one. A cut in \
             the wrong place hides a whole file and reports clean."
        );
    }
    assert!(
        writes.iter().all(|(_, _, t, _)| !t.contains("hooks.json")),
        "the wiring file was read as a script"
    );
}

/// Every builtin body, with a label naming where it came from.
///
/// Rules and skills together, because the two are read by the same agent in the
/// same session and a defect in one is a defect in the other.
fn every_builtin_body(cfg: &crate::config::Config) -> Vec<(String, String)> {
    let builtins = generate_builtin_templates(cfg, &LintPack::default());
    let mut out: Vec<(String, String)> = builtins
        .rules
        .iter()
        .map(|r| (format!("rule {}", r.name), r.body.clone()))
        .collect();
    out.extend(
        builtins
            .skills
            .iter()
            .map(|s| (format!("skill {}", s.dir_name), s.body.clone())),
    );
    out.push(("preamble".into(), builtins.preamble.clone()));
    out.push(("postamble".into(), builtins.postamble.clone()));
    out
}

/// The documents a builtin's see-also section tells a reader to go and open.
///
/// Only that section, because it is the one place a backticked `.md` is an
/// instruction rather than a path being described. Elsewhere a `.md` is a
/// round's filename, a file this tool writes, or a note the reader is told to
/// create, and none of those has to exist for the text to be right.
fn documents_named(body: &str) -> Vec<&str> {
    let Some(see_also) = body.split("## See also").nth(1) else {
        return Vec::new();
    };
    see_also
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|c| c.ends_with(".md") && !c.contains('*'))
        .map(|c| c.trim_start_matches("./"))
        .collect()
}

#[test]
fn a_builtin_points_only_at_rules_and_skills_that_ship() {
    // A `.md` named in a see-also is an instruction to go and read it, and an
    // agent that cannot find it has no way to tell a file that was never
    // generated from one this repository chose not to opt into. The design-talk
    // skill named four that no consumer has ever had: three rules that live in
    // one workspace's own `.claude/` and a sibling skill nobody wrote.
    let cfg = crate::config::Config::from_dir(&skill_fixture(Some(true)));
    let builtins = generate_builtin_templates(&cfg, &LintPack::default());

    let shipped: std::collections::BTreeSet<String> = builtins
        .rules
        .iter()
        .map(|r| format!("{}.md", r.name))
        .chain(
            builtins
                .skills
                .iter()
                .map(|s| format!("{}/SKILL.md", s.dir_name)),
        )
        .collect();

    // The whole name, not a suffix of it. A suffix test says
    // `panel-discipline.md` ships because `discipline.md` does, and says every
    // skill ships because one of them is spelled `SKILL.md`, which is the shape
    // of the write-guard carve-out defect pinned further up this file.
    let mut complaints = Vec::new();
    for (label, body) in every_builtin_body(&cfg) {
        for named in documents_named(&body) {
            if shipped.contains(named) {
                continue;
            }
            complaints.push(format!("{label} points at `{named}`"));
        }
    }
    assert!(
        complaints.is_empty(),
        "these name a document no consumer receives:\n{}\nshipped: {:?}",
        complaints.join("\n"),
        shipped
    );
}

#[test]
fn no_builtin_assumes_a_workspace_or_one_operator() {
    // Most repositories using this tool are one repository, on their own, with
    // whatever trunk they picked and nobody called op. A builtin written from
    // inside a multi-repo workspace reads as instructions somewhere else and
    // sends the agent looking for siblings that are not there.
    //
    // Each pattern here is positive evidence of an assumption, never the
    // absence of evidence against one, because a false positive fails the build
    // over prose that was fine.
    let cfg = crate::config::Config::from_dir(&skill_fixture(Some(true)));
    let complaints = assumptions_in(&every_builtin_body(&cfg));
    assert!(complaints.is_empty(), "{}", complaints.join("\n"));
}

/// Positive evidence of an assumption, one line per body per pattern.
///
/// A function rather than a loop inside the check, so the control below runs
/// the reader itself over text built to trip it. A control that matched a
/// needle against a string it had just interpolated the needle into proved
/// nothing about this reader at all.
fn assumptions_in(bodies: &[(String, String)]) -> Vec<String> {
    // Every pattern is positive evidence, never the absence of evidence
    // against one, because a false positive fails the build over prose that
    // was fine.
    let forbidden: &[(&str, &str)] = &[
        (" op ", "names one operator by handle"),
        ("Op ", "names one operator by handle"),
        ("op's ", "names one operator by handle"),
        ("each touched repo", "assumes several repositories"),
        ("cross-repo", "assumes several repositories"),
        ("across several repos", "assumes several repositories"),
        ("the repos the work", "assumes several repositories"),
        ("branch off dev", "assumes a trunk this tool does not pick"),
        ("off dev,", "assumes a trunk this tool does not pick"),
        ("clause-dev", "names one workspace"),
    ];
    let mut complaints = Vec::new();
    for (label, body) in bodies {
        // Runs of whitespace flattened first. Every builtin is hard-wrapped
        // near eighty columns, so a needle carrying spaces sits across a line
        // break as often as not, and a raw `contains` walks past it. Six of
        // these ten patterns have that shape.
        let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
        for (needle, why) in forbidden {
            if flat.contains(needle) {
                complaints.push(format!("{label} {why}: {needle:?}"));
            }
        }
    }
    complaints
}

#[test]
fn the_two_builtin_checks_can_actually_fail() {
    // Both checks above read a corpus and report nothing, which is what they
    // would do just as well if their readers matched nothing at all. So each
    // reader is run over text built to trip it.
    // The real reader, over text built to trip it, so a reader that matched
    // nothing would fail here rather than reporting a clean corpus above.
    assert_eq!(
        documents_named("## See also\n\n`nowhere-at-all.md` and `design-round/SKILL.md`"),
        vec!["nowhere-at-all.md", "design-round/SKILL.md"],
    );
    assert!(
        documents_named("writes `.claude/rules/*.md`\n\n## See also\n\n`a/SKILL.md`")
            == vec!["a/SKILL.md"],
        "either a glob was read as a document, or prose above the see-also was"
    );
    assert!(
        documents_named("a round makes `{ts}_topic.md`").is_empty(),
        "a body with no see-also section named something anyway"
    );

    // The reader itself, over a body carrying three assumptions, one of them
    // split across a line break exactly as the wrapped builtins carry theirs.
    let planted = vec![(
        "planted".to_string(),
        "a line ending in\nop and one saying cross-repo, and a third saying branch\noff dev."
            .to_string(),
    )];
    let found = assumptions_in(&planted);
    for needle in ["\" op \"", "\"cross-repo\"", "\"branch off dev\""] {
        assert!(
            found.iter().any(|c| c.contains(needle)),
            "the reader walked past {needle}: {found:?}"
        );
    }
    assert!(
        assumptions_in(&[(
            "clean".to_string(),
            "a round opens a topic and the trunk is whatever this repository picked.".to_string(),
        )])
        .is_empty(),
        "the reader complains about prose that assumes nothing"
    );
}
