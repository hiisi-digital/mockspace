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
    // Self-heals by running the launcher, which is the only thing that
    // both writes the validator and points core.hooksPath. It previously
    // ran `cargo check` in the mock workspace, on the reasoning that
    // build.rs re-ran the bootstrap; that bootstrap is gone and its
    // remaining symbol fails the build, so the step healed nothing.
    assert!(hook.contains("cd \"$root\" && cargo mock activate"), "{hook}");
    // The narrow repair is tried before the full run, because a full run
    // regenerates docs and agent rules and this executes while a commit is in
    // flight.
    let narrow = hook.find("cargo mock activate").expect("narrow repair present");
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
                c.stdin.as_mut().expect("stdin").write_all(path.as_bytes())?;
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
    std::fs::write(rounds.join("202601010000_changelist.doc.lock.md"), "locked\n").unwrap();

    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "t@example.com"],
        vec!["config", "user.name", "t"],
        vec!["add", "-A"],
    ] {
        Command::new("git").args(&args).current_dir(&repo).output().unwrap();
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
        child.stdin.as_mut().unwrap().write_all(payload.as_bytes()).unwrap();
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
    assert!(rule.body.contains("mock lock"), "a builtin must appear too:\n{}", rule.body);

    // and the negative: an empty pack's rule says nothing about a tool that
    // was never declared
    let builtins_empty = generate_builtin_templates(&cfg, &LintPack::default());
    let rule_empty = builtins_empty.rules.iter().find(|r| r.name == "tool-catalogue").unwrap();
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
    assert!(rule.body.contains("Ninety-nine"), "the seat cap must be stated:\n{}", rule.body);
    assert!(
        rule.body.contains("do not write canon") || rule.body.contains("never writes canon"),
        "the canon prohibition must be stated in words a reader cannot miss:\n{}",
        rule.body
    );
    assert!(rule.body.contains("mock panel seat"));
    assert!(rule.body.contains("mock panel consolidate"));
}
