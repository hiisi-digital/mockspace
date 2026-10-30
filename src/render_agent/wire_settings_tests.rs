//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

fn hook(name: &str, matchers: &[&str]) -> HookMeta {
    HookMeta {
        name:     name.into(),
        matchers: matchers.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn claude(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join(".claude/settings.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_first_run_writes_the_wiring_and_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]),
        2
    );

    let v = claude(d.path());
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1);
    assert_eq!(pre[0]["matcher"], "Bash");
    assert_eq!(pre[0]["hooks"][0]["command"], ".claude/hooks/gate.sh");
    assert_eq!(
        v.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["hooks"],
        "a fresh file gains nothing beyond what was asked for"
    );
}

/// A hook script as it lands on disk, generated or not.
fn hook_script(root: &Path, name: &str, generated: bool) -> String {
    let dir = root.join(".claude/hooks");
    fs::create_dir_all(&dir).unwrap();
    let marker = if generated {
        format!("# {}\n", crate::render_design::GENERATED_MARKER)
    } else {
        String::new()
    };
    fs::write(
        dir.join(name),
        format!("#!/usr/bin/env bash\n{marker}exit 0\n"),
    )
    .unwrap();
    format!(".claude/hooks/{name}")
}

#[test]
fn a_hook_somebody_wired_by_hand_survives_even_inside_the_generated_directory() {
    // `.claude/hooks/` is where a hook script goes, so somebody writing
    // their own puts it there, and the entry pointing at it read as ours
    // on nothing but the directory. It was deleted on every run, silently,
    // which is the same class of thing this merge exists to stop.
    let d = tempfile::tempdir().unwrap();
    let theirs = hook_script(d.path(), "my-own-check.sh", false);
    let stale = hook_script(d.path(), "retired-gate.sh", true);
    fs::write(
        d.path().join(".claude/settings.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": { "PreToolUse": [
                { "matcher": "Write", "hooks": [{ "type": "command", "command": theirs }] },
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": stale }] },
            ]}
        }))
        .unwrap(),
    )
    .unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);

    let v = claude(d.path());
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    let commands: Vec<&str> = pre
        .iter()
        .map(|e| e["hooks"][0]["command"].as_str().unwrap())
        .collect();
    assert!(
        commands.contains(&theirs.as_str()),
        "a hand-written hook was deleted for living in the generated \
         directory: {commands:?}"
    );
    assert!(
        !commands.contains(&stale.as_str()),
        "a retired hook of ours stayed wired, so retirement stopped \
         working: {commands:?}"
    );
    assert!(commands.contains(&".claude/hooks/gate.sh"), "{commands:?}");
}

#[test]
fn an_entry_pointing_at_a_script_that_is_gone_is_dropped() {
    // The other half of the same question. A hook of ours retires and its
    // script is swept, and the entry left behind names a file that is not
    // there. Nothing can read a marker off it, so the rule has to say what
    // an absent script means, and inside the directory this tool writes
    // the honest answer is that it was ours.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".claude")).unwrap();
    fs::write(
        d.path().join(".claude/settings.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":".claude/hooks/swept.sh"}]}]}}"#,
    )
    .unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);

    let v = claude(d.path());
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 1, "the dangling entry stayed: {pre:?}");
    assert_eq!(pre[0]["hooks"][0]["command"], ".claude/hooks/gate.sh");
}

#[test]
fn the_file_keeps_its_own_order_and_its_own_shape() {
    // Preserving the contents and reordering them is most of the way to
    // preserving the file and is not the same thing. A settings file is
    // hand-edited and read in diffs, and alphabetising every key at every
    // depth makes the next diff unreadable in a change whose whole claim
    // is that it leaves alone what it does not own.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".claude")).unwrap();
    let theirs = concat!(
        "{\n",
        "  \"$schema\": \"https://json.schemastore.org/claude-code-settings.json\",\n",
        "  \"permissions\": { \"allow\": [\"Bash(git status)\"] },\n",
        "  \"model\": \"opus\",\n",
        "  \"env\": { \"FOO\": \"1\" }\n",
        "}\n"
    );
    fs::write(d.path().join(".claude/settings.json"), theirs).unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);

    let text = fs::read_to_string(d.path().join(".claude/settings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        v.as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["$schema", "permissions", "model", "env", "hooks"],
        "their keys were reordered; ours belongs at the end"
    );
    assert_eq!(
        v["permissions"]["allow"][0], "Bash(git status)",
        "a nested value moved or went missing"
    );
}

#[test]
fn a_shape_this_does_not_understand_is_left_where_it_is() {
    // The same principle the top level already follows, one and two levels
    // in. A `hooks` that is not an object, or a `PreToolUse` that is not an
    // array, is somebody's mistake or somebody's extension; either way it
    // is not this tool's to throw away without a word.
    for body in [
        r#"{"hooks": null}"#,
        r#"{"hooks": []}"#,
        r#"{"hooks": {"PreToolUse": "everything"}}"#,
    ] {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join(".claude")).unwrap();
        let path = d.path().join(".claude/settings.json");
        fs::write(&path, body).unwrap();

        generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            body,
            "{body} was rewritten rather than left alone"
        );
    }
}

#[test]
fn everything_the_file_already_held_survives_a_run() {
    // The defect this replaces. `settings.json` is where a repository
    // records what an agent may do, and the whole file was rewritten from
    // hook metadata on every run, so a permission list, an environment, a
    // model choice and every hook on an event this does not generate were
    // all destroyed without a word.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".claude")).unwrap();
    fs::write(
        d.path().join(".claude/settings.json"),
        r#"{
  "permissions": { "allow": ["Bash(git status)"] },
  "env": { "RUST_LOG": "debug" },
  "hooks": {
"PreToolUse": [
  { "matcher": "Write", "hooks": [{ "type": "command", "command": "scripts/mine.sh" }] },
  { "matcher": "Bash", "hooks": [{ "type": "command", "command": ".claude/hooks/stale.sh" }] }
],
"Stop": [
  { "matcher": "*", "hooks": [{ "type": "command", "command": "scripts/done.sh" }] }
]
  }
}"#,
    )
    .unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    let v = claude(d.path());

    assert_eq!(v["permissions"]["allow"][0], "Bash(git status)");
    assert_eq!(v["env"]["RUST_LOG"], "debug");
    assert_eq!(
        v["hooks"]["Stop"][0]["hooks"][0]["command"],
        "scripts/done.sh"
    );

    // Their own PreToolUse entry stays. The stale generated one goes,
    // which is the half that makes this a merge rather than an append.
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    let commands: Vec<&str> = pre
        .iter()
        .map(|e| e["hooks"][0]["command"].as_str().unwrap())
        .collect();
    assert_eq!(commands, vec!["scripts/mine.sh", ".claude/hooks/gate.sh"]);
}

#[test]
fn running_twice_leaves_the_same_file() {
    // A merge that appends rather than replacing its own entries grows the
    // list on every invocation, and every case above passes while it does.
    let d = tempfile::tempdir().unwrap();
    generate_settings(d.path(), &[hook("gate.sh", &["Bash", "Write"])]);
    let once = fs::read_to_string(d.path().join(".claude/settings.json")).unwrap();
    generate_settings(d.path(), &[hook("gate.sh", &["Bash", "Write"])]);
    let twice = fs::read_to_string(d.path().join(".claude/settings.json")).unwrap();
    assert_eq!(once, twice);
}

#[test]
fn an_entry_mixing_one_of_ours_with_one_of_theirs_is_theirs() {
    // Dropping it would take the hand-written half with it, so the rule is
    // that an entry is ours only when every command in it is.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".claude")).unwrap();
    fs::write(
        d.path().join(".claude/settings.json"),
        r#"{"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[
            {"type":"command","command":".claude/hooks/old.sh"},
            {"type":"command","command":"scripts/theirs.sh"}]}]}}"#,
    )
    .unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    let v = claude(d.path());
    let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2, "the mixed entry stays, ours is added");
    assert_eq!(pre[0]["hooks"][1]["command"], "scripts/theirs.sh");
}

#[test]
fn an_unreadable_file_is_left_exactly_as_it_was() {
    // The only honest thing to do with bytes that cannot be parsed: what
    // cannot be read cannot be preserved, and overwriting on that basis is
    // the defect one size larger.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".claude")).unwrap();
    let broken = "{ not json at all";
    fs::write(d.path().join(".claude/settings.json"), broken).unwrap();

    let count = generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    assert_eq!(
        fs::read_to_string(d.path().join(".claude/settings.json")).unwrap(),
        broken
    );
    assert_eq!(count, 1, "the copilot side still wrote; this one did not");
}

#[test]
fn the_copilot_side_leaves_an_unreadable_file_alone_as_well() {
    // The law governs two files and was asserted on one. The other test
    // proves the copilot side wrote, which is the case where nothing was
    // in its way.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".github/hooks")).unwrap();
    let broken = "{ not json at all";
    let path = d.path().join(".github/hooks/hooks.json");
    fs::write(&path, broken).unwrap();

    let count = generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    assert_eq!(fs::read_to_string(&path).unwrap(), broken);
    assert_eq!(count, 1, "the claude side still wrote; this one did not");
}

#[test]
fn the_copilot_side_leaves_a_shape_it_does_not_understand_alone_as_well() {
    // The shape check governs two files and was written for one. The
    // copilot side read `hooks` through `.and_then(|v| v.as_array())` and
    // fell to an empty default for anything else, so an object or a null
    // there was silently replaced rather than left.
    for odd in [
        "{\n  \"hooks\": null\n}",
        "{\n  \"hooks\": 5\n}",
        "{\n  \"hooks\": {\"preToolUse\": {}}\n}",
        "{\n  \"hooks\": [{\"name\":\"mine\",\"command\":\"scripts/mine.sh\"}]\n}",
    ] {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join(".github/hooks")).unwrap();
        let path = d.path().join(".github/hooks/hooks.json");
        fs::write(&path, odd).unwrap();

        let count = generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            odd,
            "a `hooks` this does not understand was replaced"
        );
        assert_eq!(count, 1, "the claude side still wrote; this one did not");
    }
}

#[test]
fn the_marker_goes_after_the_shebang_and_only_once() {
    // A one-line script has no newline to split on, and the arm that
    // handled that put the marker in front of the shebang, which stops the
    // kernel reading the file as a script at all.
    let one_line = with_generated_marker("#!/usr/bin/env bash");
    assert!(one_line.starts_with("#!/usr/bin/env bash\n"), "{one_line}");
    assert!(crate::render_design::is_generated(&one_line), "{one_line}");

    // And a body that already carries one keeps exactly one.
    let once = with_generated_marker("#!/bin/sh\nexit 0\n");
    let twice = with_generated_marker(&once);
    assert_eq!(once, twice, "the marker was written a second time");

    // A script with no shebang still gets marked, at the top.
    let bare = with_generated_marker("exit 0\n");
    assert!(crate::render_design::is_generated(&bare), "{bare}");
}

#[test]
fn a_hand_written_script_whose_name_carries_a_space_is_still_theirs() {
    // The command is split on whitespace to drop any arguments after the
    // script, which truncates a name with a space in it. The read then
    // fails, and the arm that reads an absent script as a retired one of
    // ours drops the wiring for a hand-written hook sitting right there.
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join(".claude/hooks");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("my check.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    assert!(
        !names_a_generated_script(".claude/hooks/my check.sh", d.path(), CLAUDE_HOOK_PREFIX),
        "a script that is present and unmarked was read as ours"
    );

    // The argument case the split exists for still works.
    fs::write(
        dir.join("gate.sh"),
        format!("#!/bin/sh\n# {}\n", crate::render_design::GENERATED_MARKER),
    )
    .unwrap();
    assert!(names_a_generated_script(
        ".claude/hooks/gate.sh --strict",
        d.path(),
        CLAUDE_HOOK_PREFIX
    ));
}

#[test]
fn a_json_document_that_is_not_an_object_is_left_alone_too() {
    // `[]` parses and is not something a key can be inserted into, so a
    // check that only asked "does this parse" would clobber it.
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".claude")).unwrap();
    fs::write(d.path().join(".claude/settings.json"), "[1, 2, 3]").unwrap();
    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    assert_eq!(
        fs::read_to_string(d.path().join(".claude/settings.json")).unwrap(),
        "[1, 2, 3]"
    );
}

fn copilot(root: &Path) -> serde_json::Value {
    let text = fs::read_to_string(root.join(".github/hooks/hooks.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// The shape GitHub's hooks reference gives
/// (docs.github.com/en/copilot/reference/hooks-reference, read 30 October 2026):
/// `version` 1, `hooks` keyed by the camelCase event, each entry a `command` hook
/// with `bash`, an optional `matcher` regex over the runtime tool name, and
/// `timeoutSec`.
#[test]
fn the_copilot_file_has_the_shape_the_hooks_reference_gives() {
    let d = tempfile::tempdir().unwrap();
    generate_settings(d.path(), &[hook("gate.sh", &["Bash"]), hook("imports.sh", &["Write", "Edit", "MultiEdit"])]);
    let v = copilot(d.path());

    assert_eq!(v["version"], 1);
    let entries = v["hooks"]["preToolUse"].as_array().expect("hooks.preToolUse is an array");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["type"], "command");
    assert_eq!(entries[0]["bash"], ".github/hooks/gate.sh");
    assert_eq!(entries[0]["matcher"], "bash|powershell");
    assert_eq!(entries[1]["bash"], ".github/hooks/imports.sh");
    assert_eq!(
        entries[1]["matcher"], "create|edit",
        "the file tools are `create` and `edit` there, not Write and Edit"
    );
    assert_eq!(entries[1]["timeoutSec"], HOOK_TIMEOUT_SEC);
    assert!(
        entries.iter().all(|e| e.get("event").is_none() && e.get("tool").is_none() && e.get("command").is_none()),
        "the keys of the flat list Copilot never read are gone"
    );
    assert!(v["hooks"].is_object(), "the event list is not a flat array");
}

#[test]
fn a_hook_for_every_tool_carries_no_matcher() {
    let d = tempfile::tempdir().unwrap();
    generate_settings(d.path(), &[hook("all.sh", &["*"])]);
    let v = copilot(d.path());
    let entry = &v["hooks"]["preToolUse"][0];
    assert_eq!(entry["bash"], ".github/hooks/all.sh");
    assert!(entry.get("matcher").is_none(), "{entry}");
}

#[test]
fn a_matcher_with_no_runtime_row_passes_through() {
    assert_eq!(copilot_matcher(&["Bash|Glob".to_string()]).as_deref(), Some("bash|powershell|Glob"));
    assert_eq!(copilot_matcher(&["Edit".to_string(), "MultiEdit".to_string()]).as_deref(), Some("edit"));
    assert_eq!(copilot_matcher(&["Write".to_string(), "*".to_string()]), None);
}

#[test]
fn the_flat_list_earlier_renders_wrote_is_replaced_by_the_reference_shape() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".github/hooks")).unwrap();
    fs::write(
        d.path().join(".github/hooks/hooks.json"),
        r#"{"version":1,"hooks":[
            {"name":"stale","command":".github/hooks/stale.sh","event":"pre_tool_use","tool":"Bash"}]}"#,
    )
    .unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    let v = copilot(d.path());
    let entries = v["hooks"]["preToolUse"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{v}");
    assert_eq!(entries[0]["bash"], ".github/hooks/gate.sh");
}

#[test]
fn the_copilot_side_merges_on_the_same_terms() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir_all(d.path().join(".github/hooks")).unwrap();
    fs::write(
        d.path().join(".github/hooks/hooks.json"),
        r#"{"version":1,"hooks":{
            "sessionStart":[{"type":"command","bash":"scripts/start.sh"}],
            "preToolUse":[
            {"type":"command","bash":"scripts/mine.sh","matcher":"create"},
            {"type":"command","bash":".github/hooks/stale.sh","matcher":"bash"}]}}"#,
    )
    .unwrap();

    generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
    let v = copilot(d.path());

    assert_eq!(v["version"], 1);
    assert_eq!(
        v["hooks"]["sessionStart"][0]["bash"], "scripts/start.sh",
        "an event this does not write is not its to drop"
    );
    let bashes: Vec<&str> = v["hooks"]["preToolUse"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["bash"].as_str().unwrap())
        .collect();
    assert_eq!(bashes, vec!["scripts/mine.sh", ".github/hooks/gate.sh"]);
}
