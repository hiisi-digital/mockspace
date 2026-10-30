//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The hooks generated from `[lints.forbidden-imports]` are wired into the
//! settings like every other hook, so the editor calls them.
//!
//! Written but unregistered, they sit in `.claude/hooks/` under no event and
//! nothing runs them, which the workspace's regen then refuses as an orphan.

use std::fs;
use std::path::Path;

use super::*;

fn cfg_with_rule(repo: &Path) -> Config {
    let mock = repo.join("mock");
    fs::create_dir_all(&mock).unwrap();
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = repo.to_path_buf();
    cfg.mock_dir = mock;
    cfg.src_dirs = vec![repo.join("mock/crates")];
    let rule: std::collections::HashMap<String, String> = [
        ("rule.no-dyn.scope", "sim-*"),
        ("rule.no-dyn.forbidden", "dyn"),
        ("rule.no-dyn.reason", "no dispatch"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    cfg.lint_overrides
        .params
        .insert("forbidden-imports".to_string(), rule);
    cfg
}

/// Runs the lint-derived generation the way the renderer does, with no hook
/// directories made beforehand.
fn derive(repo: &Path, cfg: &Config) -> (usize, Vec<HookMeta>) {
    generate_lint_derived_content(
        cfg,
        &repo.join(".claude/rules"),
        &repo.join(".github/instructions"),
        &repo.join(".claude/hooks"),
        &repo.join(".github/hooks"),
        "",
        "",
        "",
    )
}

#[test]
fn a_forbidden_imports_hook_is_returned_with_the_events_it_guards() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    let (_, hooks) = derive(tmp.path(), &cfg);
    let hook = hooks
        .iter()
        .find(|h| h.name == "forbidden-imports.sh")
        .expect("the generated hook is returned for the settings step");
    // It inspects the text being written, so it guards the writing tools, not Bash.
    assert!(
        hook.matchers.iter().any(|m| m == "Write"),
        "{:?}",
        hook.matchers
    );
    assert!(
        hook.matchers.iter().any(|m| m == "Edit"),
        "{:?}",
        hook.matchers
    );
    assert!(
        !hook.matchers.iter().any(|m| m == "Bash"),
        "{:?}",
        hook.matchers
    );
}

#[test]
fn the_generated_settings_register_a_forbidden_imports_hook_under_an_event() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    let (_, hooks) = derive(tmp.path(), &cfg);
    wire::generate_settings(tmp.path(), &hooks);
    let settings: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(tmp.path().join(".claude/settings.json")).unwrap(),
    )
    .unwrap();
    let entries = settings["hooks"]["PreToolUse"]
        .as_array()
        .expect("a PreToolUse list");
    for matcher in ["Write", "Edit"] {
        let script = entries
            .iter()
            .filter(|e| e["matcher"] == matcher)
            .flat_map(|e| e["hooks"].as_array().into_iter().flatten())
            .filter_map(|h| h["command"].as_str())
            .find(|c| c.contains("forbidden-imports.sh"))
            .unwrap_or_else(|| panic!("no {matcher} entry runs the hook:\n{settings}"));
        assert!(
            tmp.path().join(script).is_file(),
            "the {matcher} entry names {script}, which is not on disk"
        );
    }
}

#[test]
fn a_config_without_forbidden_imports_returns_no_hooks() {
    // The control: the new return is not simply every hook that exists.
    let tmp = tempfile::tempdir().unwrap();
    let mock = tmp.path().join("mock");
    fs::create_dir_all(&mock).unwrap();
    let mut cfg = Config::from_dir(&mock);
    cfg.repo_root = tmp.path().to_path_buf();
    let (n, hooks) = derive(tmp.path(), &cfg);
    assert_eq!(n, 0);
    assert!(hooks.is_empty());
}
fn generated_hook(repo: &Path) -> String {
    let cfg = cfg_with_rule(repo);
    let (_, hooks) = derive(repo, &cfg);
    hooks
        .into_iter()
        .find(|h| h.name == "forbidden-imports.sh")
        .expect("the hook is generated, and is named after the lint it asks")
        .name
}

#[test]
fn one_hook_is_written_however_many_scopes_the_rules_name() {
    // The lint holds the scopes, so a hook per scope ran the whole lint once for
    // each of them. Two scopes, still one hook, and the legacy per-scope names gone.
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = cfg_with_rule(tmp.path());
    let params = cfg.lint_overrides.params.get_mut("forbidden-imports").unwrap();
    for (k, v) in [
        ("rule.no-box.scope", "ui-*"),
        ("rule.no-box.forbidden", "Box"),
        ("rule.no-box.reason", "no heap"),
    ] {
        params.insert(k.to_string(), v.to_string());
    }
    // An earlier render's per-scope hook, which is neither the lint's name nor this render's.
    let legacy = tmp.path().join(".claude/hooks/lint-forbidden-sim-star.sh");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "#!/bin/sh\n").unwrap();
    let (_, hooks) = derive(tmp.path(), &cfg);
    assert_eq!(
        hooks.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(),
        ["forbidden-imports.sh"]
    );
    assert!(!legacy.exists(), "the per-scope hook an earlier render wrote must go");
}

// --- The hook's verdict is the lint's -----------------------------------------------------------

/// A repository with two crates, one in the rule's scope and one outside it.
fn repo_with_crates(repo: &Path, forbidden: &str) -> Config {
    for krate in ["sim-core", "other"] {
        let dir = repo.join("mock/crates").join(krate);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), format!("[package]\nname = \"{krate}\"\n")).unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    }
    let mut cfg = cfg_with_rule(repo);
    cfg.crate_prefix = "sim".to_string();
    cfg.lint_overrides
        .params
        .get_mut("forbidden-imports")
        .unwrap()
        .insert("rule.no-dyn.forbidden".to_string(), forbidden.to_string());
    cfg
}

/// What the hook asks the engine: the lint's findings for `text` as the content of `file`.
fn edit_verdict(cfg: &Config, file: &Path, text: &str) -> Vec<mockspace_lint_rules::LintError> {
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    crate::lint::lint_edit(
        &crates,
        &cfg.src_dirs,
        &cfg.mock_dir,
        &cfg.proc_macro_crates,
        cfg.lint_proc_macro_source,
        &cfg.crate_prefix,
        &cfg.lint_overrides,
        &cfg.primitive_introductions,
        &LintPack::default(),
        "forbidden-imports",
        file,
        text,
    )
    .expect("the file is in a crate of the repository")
}

/// What the gate says of the same text once it is on disk: the blocking count of
/// the pass every crate goes through, scoped to the crate holding `file`.
fn gate_verdict(cfg: &Config, krate: &str, file: &Path, text: &str) -> usize {
    fs::write(file, text).unwrap();
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    crate::lint::run_lints(
        &crates,
        &cfg.src_dirs,
        &cfg.mock_dir,
        mockspace_lint_rules::LintMode::Commit,
        Some(&[krate.to_string()]),
        false,
        &cfg.proc_macro_crates,
        cfg.lint_proc_macro_source,
        &cfg.crate_prefix,
        &cfg.lint_overrides,
        &mockspace_lint_rules::RegistryView::default(),
        &[],
        &[],
        &cfg.primitive_introductions,
        &LintPack::default(),
    )
}

#[test]
fn an_import_the_lint_refuses_is_refused_by_the_hook_too() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let file = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let text = "use std::fmt;\npub fn f() {}\n";
    assert!(
        !edit_verdict(&cfg, &file, text).is_empty(),
        "the hook let through an import the lint refuses"
    );
    assert!(gate_verdict(&cfg, "sim-core", &file, text) > 0, "the control: the gate refuses it");
}

#[test]
fn text_the_lint_passes_is_passed_by_the_hook_though_the_old_substring_test_refused_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let file = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    // A comment naming the path. The hook used to refuse any text containing `std::`.
    let text = "// no std::fmt in here, by design\npub fn f() {}\n";
    assert!(text.contains("std::"), "the case must be one the substring test refused");
    assert!(
        edit_verdict(&cfg, &file, text).is_empty(),
        "the hook refused text the lint passes"
    );
    assert_eq!(gate_verdict(&cfg, "sim-core", &file, text), 0, "the control: the gate passes it");
}

#[test]
fn a_crate_outside_the_scope_is_the_lints_to_pass_not_the_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let file = tmp.path().join("mock/crates/other/src/lib.rs");
    let text = "use std::fmt;\n";
    assert!(edit_verdict(&cfg, &file, text).is_empty());
    assert_eq!(gate_verdict(&cfg, "other", &file, text), 0);
}

#[test]
fn a_finding_elsewhere_in_the_crate_does_not_block_an_edit_that_adds_none() {
    // Blocking every write to a crate that already has a finding would make the crate
    // unfixable by edits. The gate still counts the one on disk; the edit is judged on its file.
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let lib = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    fs::write(&lib, "use std::io;\n").unwrap();
    let other_file = tmp.path().join("mock/crates/sim-core/src/extra.rs");
    assert!(edit_verdict(&cfg, &other_file, "pub fn g() {}\n").is_empty());
    assert!(
        !edit_verdict(&cfg, &other_file, "use std::fmt;\n").is_empty(),
        "control: a finding in the edited file itself blocks"
    );
    assert!(gate_verdict(&cfg, "sim-core", &other_file, "pub fn g() {}\n") > 0);
}

#[test]
fn a_file_that_is_not_a_crate_source_is_in_no_lints_view() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    let verdict = |file: &Path| {
        crate::lint::lint_edit(
            &crates,
            &cfg.src_dirs,
            &cfg.mock_dir,
            &cfg.proc_macro_crates,
            cfg.lint_proc_macro_source,
            &cfg.crate_prefix,
            &cfg.lint_overrides,
            &cfg.primitive_introductions,
            &LintPack::default(),
            "forbidden-imports",
            file,
            "use std::fmt;\n",
        )
    };
    assert!(verdict(&tmp.path().join("mock/crates/sim-core/Cargo.toml")).is_none());
    assert!(verdict(&tmp.path().join("mock/crates/sim-core/README.md")).is_none());
    assert!(verdict(&tmp.path().join("elsewhere/src/lib.rs")).is_none());
    assert!(verdict(&tmp.path().join("mock/crates/sim-core/src/lib.rs")).is_some(), "control");
}

// --- The generated script hands the lint the edit and carries its verdict back -------------------

/// One run of the hook against a launcher that records what it was asked and answers `rc`.
struct Run {
    stdout: String,
    args:   Option<Vec<String>>,
    stdin:  Option<String>,
    old:    Option<String>,
}

fn run_against_stub(repo: &Path, payload: &serde_json::Value, rc: i32, launcher: bool) -> Run {
    let hook = generated_hook(repo);
    let script = repo.join(".claude/hooks").join(hook);
    let stub = repo.join("stub");
    fs::create_dir_all(stub.join("bin")).unwrap();
    for f in ["args", "stdin", "replace"] {
        let _ = fs::remove_file(stub.join(f));
    }
    if launcher {
        crate::test_exec::write_executable(
            &stub.join("bin/mock"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$STUB_DIR/args\"\ncat > \"$STUB_DIR/stdin\"\nprev=\"\"\nfor a in \"$@\"; do [ \"$prev\" = --replace ] && cp \"$a\" \"$STUB_DIR/replace\"; prev=\"$a\"; done\necho 'the lint says no' >&2\nexit \"$STUB_RC\"\n",
        );
    }
    let mut child = std::process::Command::new("bash")
        .arg(&script)
        .current_dir(repo)
        .env("PATH", format!("{}:/usr/bin:/bin", stub.join("bin").display()))
        .env("STUB_DIR", &stub)
        .env("STUB_RC", rc.to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let read = |f: &str| fs::read_to_string(stub.join(f)).ok();
    Run {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        args:   read("args").map(|a| a.lines().map(String::from).collect()),
        stdin:  read("stdin"),
        old:    read("replace"),
    }
}

fn tool_call(tool: &str, input: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "session_id": "t",
        "transcript_path": "/tmp/t",
        "hook_event_name": "PreToolUse",
        "tool_name": tool,
        "tool_input": input,
    })
}

fn refused(out: &str) -> bool {
    super::write_guard_phase_tests::refused(out)
}

#[test]
fn a_write_hands_the_engine_the_whole_text_and_names_the_lint() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let call = tool_call(
        "Write",
        serde_json::json!({ "file_path": target.display().to_string(), "content": "fn f() {}" }),
    );
    let run = run_against_stub(tmp.path(), &call, 0, true);
    let args = run.args.expect("the engine was asked");
    assert_eq!(&args[..5], ["check-edit", "--lint", "forbidden-imports", "--file", &target.display().to_string()]);
    assert!(!args.iter().any(|a| a == "--replace" || a == "--all"), "{args:?}");
    assert_eq!(run.stdin.as_deref(), Some("fn f() {}"));
    assert!(!refused(&run.stdout), "a pass was refused: {}", run.stdout);
}

#[test]
fn an_edit_hands_the_engine_what_it_replaces_and_what_replaces_it() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let call = tool_call(
        "Edit",
        serde_json::json!({
            "file_path": target.display().to_string(),
            "old_string": "fn f() {}",
            "new_string": "fn f(x: &dyn Tr) {}",
            "replace_all": true,
        }),
    );
    let run = run_against_stub(tmp.path(), &call, 0, true);
    let args = run.args.expect("the engine was asked");
    assert!(args.iter().any(|a| a == "--replace"), "{args:?}");
    assert!(args.iter().any(|a| a == "--all"), "{args:?}");
    assert_eq!(run.old.as_deref(), Some("fn f() {}"));
    assert_eq!(run.stdin.as_deref(), Some("fn f(x: &dyn Tr) {}"));
}

#[test]
fn the_hook_refuses_exactly_when_the_engine_does_and_carries_its_words() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let call = tool_call(
        "Write",
        serde_json::json!({ "file_path": target.display().to_string(), "content": "x" }),
    );
    let run = run_against_stub(tmp.path(), &call, 1, true);
    assert!(refused(&run.stdout), "the engine refused and the hook did not: {}", run.stdout);
    assert!(run.stdout.contains("the lint says no"), "{}", run.stdout);
    let run = run_against_stub(tmp.path(), &call, 2, true);
    assert!(refused(&run.stdout), "a lint that is gone must not pass everything: {}", run.stdout);
}

#[test]
fn a_file_that_is_not_rust_or_not_this_repos_never_reaches_the_engine() {
    let tmp = tempfile::tempdir().unwrap();
    for path in [
        tmp.path().join("mock/crates/sim-core/Cargo.toml"),
        std::path::PathBuf::from("/somewhere/else/src/lib.rs"),
    ] {
        let call = tool_call(
            "Write",
            serde_json::json!({ "file_path": path.display().to_string(), "content": "x" }),
        );
        let run = run_against_stub(tmp.path(), &call, 1, true);
        assert!(run.args.is_none(), "{} reached the engine", path.display());
        assert!(!refused(&run.stdout));
    }
}

#[test]
fn with_no_launcher_the_hook_refuses_rather_than_passing_what_it_cannot_judge() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let call = tool_call(
        "Write",
        serde_json::json!({ "file_path": target.display().to_string(), "content": "x" }),
    );
    let run = run_against_stub(tmp.path(), &call, 0, false);
    assert!(refused(&run.stdout), "{}", run.stdout);
    assert!(run.stdout.contains("launcher"), "{}", run.stdout);
}

#[test]
#[should_panic(expected = "failed to create a hooks or rules directory")]
fn a_hooks_directory_that_cannot_be_made_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    fs::create_dir_all(tmp.path().join(".claude")).unwrap();
    // a regular file where the directory must go
    fs::write(tmp.path().join(".claude/hooks"), "").unwrap();
    let cfg = cfg_with_rule(tmp.path());
    derive(tmp.path(), &cfg);
}

#[test]
#[should_panic(expected = "failed to write forbidden-imports claude hook")]
fn a_hook_that_cannot_be_written_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    derive(tmp.path(), &cfg);
    // the second run meets a directory where the script goes
    let hook = generated_hook(tmp.path());
    let path = tmp.path().join(".claude/hooks").join(&hook);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    derive(tmp.path(), &cfg);
}

#[test]
fn a_full_render_registers_the_forbidden_imports_hooks_under_write_and_edit() {
    // Through `generate_agent_rules`, where the derived hooks join the builtin ones: the hand-wired
    // test above would stay green if that join were dropped.
    let mock = super::tests::skill_fixture(None);
    let root = mock.parent().unwrap().to_path_buf();
    let mut cfg = Config::from_dir(&mock);
    let rule = cfg_with_rule(&root);
    cfg.lint_overrides.params = rule.lint_overrides.params;
    cfg.src_dirs = vec![root.join("mock/crates")];
    generate_agent_rules(
        &CrateMap::default(),
        &cfg,
        &crate::registry::Registry::default(),
        &super::tests::pack_with_stub_tool(),
    );
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    let entries = settings["hooks"]["PreToolUse"].as_array().expect("a PreToolUse list");
    for matcher in ["Write", "Edit"] {
        let registered = entries
            .iter()
            .filter(|e| e["matcher"] == matcher)
            .flat_map(|e| e["hooks"].as_array().into_iter().flatten())
            .filter_map(|h| h["command"].as_str())
            .any(|c| c.contains("forbidden-imports.sh"));
        assert!(registered, "the full render left no {matcher} entry for the hook:\n{settings}");
    }
}

#[test]
#[should_panic(expected = "failed to write forbidden-imports copilot hook")]
fn a_copilot_hook_that_cannot_be_written_fails_loudly() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    derive(tmp.path(), &cfg);
    let hook = generated_hook(tmp.path());
    let path = tmp.path().join(".github/hooks").join(&hook);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    derive(tmp.path(), &cfg);
}

// --- An edit is judged on what it adds ----------------------------------------------------------

#[test]
fn a_file_holding_two_violations_can_be_mended_one_edit_at_a_time() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let file = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    fs::write(&file, "use std::fmt;\nuse std::io;\npub fn f() {}\n").unwrap();
    // Removing one leaves one, which the file had already.
    assert!(edit_verdict(&cfg, &file, "use std::fmt;\npub fn f() {}\n").is_empty());
    // An edit beside them adds nothing.
    assert!(
        edit_verdict(&cfg, &file, "use std::fmt;\nuse std::io;\npub fn g() {}\n").is_empty()
    );
    // The controls: an edit that adds a third is refused, and so is one that adds a duplicate
    // of a message the file already holds (every `std` import shares one message, so
    // it is the count that rises).
    assert_eq!(
        edit_verdict(&cfg, &file, "use std::fmt;\nuse std::io;\nuse std::fs;\npub fn f() {}\n")
            .len(),
        1
    );
    assert_eq!(edit_verdict(&cfg, &file, "use std::fmt;\nuse std::io;\nuse std::io;\n").len(), 1);
}

#[test]
fn a_violation_the_file_holds_twice_is_allowed_twice_and_no_more() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let file = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    fs::write(&file, "use std::fmt;\nuse std::fmt;\n").unwrap();
    assert!(edit_verdict(&cfg, &file, "use std::fmt;\nuse std::fmt;\n// x\n").is_empty());
    assert_eq!(
        edit_verdict(&cfg, &file, "use std::fmt;\nuse std::fmt;\nuse std::fmt;\n").len(),
        1
    );
}

#[test]
fn a_lint_that_judges_the_whole_crate_has_no_verdict_on_an_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = repo_with_crates(tmp.path(), "std::*");
    let whole = mockspace_lint_rules::all_lints()
        .into_iter()
        .find(|l| !l.per_file())
        .expect("the engine ships a lint that judges a crate as a whole");
    let out = crate::entry::edit::run_for_test(&cfg, whole.name(), &tmp.path().join("mock/crates/sim-core/src/lib.rs"));
    assert_eq!(out, 2);
    // The control: a per-file lint gets a verdict.
    assert_eq!(
        crate::entry::edit::run_for_test(&cfg, "forbidden-imports", &tmp.path().join("mock/crates/sim-core/src/lib.rs")),
        0
    );
}

#[test]
fn the_hook_hands_the_engine_exactly_the_bytes_of_the_edit() {
    // `$(...)` strips trailing newlines, which made an old_string of only newlines empty and
    // the edit read as a whole-file write.
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("mock/crates/sim-core/src/lib.rs");
    let call = tool_call(
        "Edit",
        serde_json::json!({
            "file_path": target.display().to_string(),
            "old_string": "\n\n\n",
            "new_string": "a\n\n",
        }),
    );
    let run = run_against_stub(tmp.path(), &call, 0, true);
    assert_eq!(run.old.as_deref(), Some("\n\n\n"));
    assert_eq!(run.stdin.as_deref(), Some("a\n\n"));
    let call = tool_call(
        "Write",
        serde_json::json!({ "file_path": target.display().to_string(), "content": "fn f() {}\n\n" }),
    );
    let run = run_against_stub(tmp.path(), &call, 0, true);
    assert_eq!(run.stdin.as_deref(), Some("fn f() {}\n\n"));
    assert!(run.old.is_none());
}

#[test]
fn a_render_with_no_rules_left_takes_the_hook_it_wrote_before_with_it() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = cfg_with_rule(tmp.path());
    derive(tmp.path(), &cfg);
    let hook = tmp.path().join(".claude/hooks/forbidden-imports.sh");
    assert!(hook.is_file(), "control: the first render wrote it");
    // Not ours: a hook of that name some other hand wrote stays.
    let theirs = tmp.path().join(".github/hooks/forbidden-imports.sh");
    fs::write(&theirs, "#!/bin/sh\n# written by hand\n").unwrap();
    let mut empty = cfg_with_rule(tmp.path());
    empty.lint_overrides.params.clear();
    derive(tmp.path(), &empty);
    assert!(!hook.exists(), "the generated hook outlived its rules");
    assert!(theirs.is_file(), "a hook this renderer did not write is not its to remove");
}

#[test]
fn a_rule_naming_no_scope_gets_the_hook_because_the_lint_reads_it_as_every_crate() {
    let tmp = tempfile::tempdir().unwrap();
    let mut cfg = cfg_with_rule(tmp.path());
    let rule: std::collections::HashMap<String, String> =
        [("rule.no-fmt.forbidden", "std::fmt"), ("rule.no-fmt.reason", "no fmt")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
    cfg.lint_overrides.params.insert("forbidden-imports".to_string(), rule);
    let (_, hooks) = derive(tmp.path(), &cfg);
    assert!(hooks.iter().any(|h| h.name == "forbidden-imports.sh"));
    // The control: with the rule disabled there is nothing to ask about.
    cfg.lint_overrides
        .params
        .get_mut("forbidden-imports")
        .unwrap()
        .insert("rule.no-fmt.enabled".to_string(), "false".to_string());
    let fresh = tempfile::tempdir().unwrap();
    let (_, none) = derive(fresh.path(), &cfg);
    assert!(none.iter().all(|h| h.name != "forbidden-imports.sh"));
}
