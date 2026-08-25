//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// Write a single builtin hook to both Claude and Copilot directories.
///
/// Returns the hook metadata and increments count by 2 (one per platform).
pub(crate) fn write_builtin_hook(
    name: &str,
    content: &str,
    repo_root: &str,
    claude_hooks_dir: &Path,
    copilot_hooks_dir: &Path,
    count: &mut usize,
) -> HookMeta {
    let claude_content = content
        .replace("{{HOOK_HELPERS}}", CLAUDE_HOOK_HELPERS)
        .replace("{{REPO_ROOT}}", repo_root);
    let claude_path = claude_hooks_dir.join(name);
    fs::write(&claude_path, &claude_content).expect("failed to write builtin claude hook");
    #[cfg(unix)]
    set_executable(&claude_path);
    eprintln!("  {} (builtin)", claude_path.display());
    *count += 1;

    let copilot_content = content
        .replace("{{HOOK_HELPERS}}", COPILOT_HOOK_HELPERS)
        .replace("{{REPO_ROOT}}", repo_root);
    let copilot_path = copilot_hooks_dir.join(name);
    fs::write(&copilot_path, &copilot_content).expect("failed to write builtin copilot hook");
    #[cfg(unix)]
    set_executable(&copilot_path);
    eprintln!("  {} (builtin)", copilot_path.display());
    *count += 1;

    HookMeta {
        name:     name.to_string(),
        matchers: matchers_from_name(name),
    }
}

/// Generate all builtin agent hooks.
///
/// Writes 5 hooks to both `.claude/hooks/` and `.github/hooks/`:
/// - check-message.sh
/// - mockspace-write-guard.sh
/// - mockspace-reminder.sh
/// - no-yagni-guard.sh
/// - mockspace-gate.sh
pub(crate) fn generate_builtin_hooks(
    cfg: &Config,
    claude_hooks_dir: &Path,
    copilot_hooks_dir: &Path,
    count: &mut usize,
) -> Vec<HookMeta> {
    let mut hooks = Vec::new();

    let repo_root = cfg.repo_root.to_string_lossy().to_string();

    hooks.push(write_builtin_hook(
        "mockspace-gate.sh",
        &builtin_mockspace_gate(),
        &repo_root,
        claude_hooks_dir,
        copilot_hooks_dir,
        count,
    ));

    hooks.push(write_builtin_hook(
        "check-message.sh",
        &builtin_check_message(cfg),
        &repo_root,
        claude_hooks_dir,
        copilot_hooks_dir,
        count,
    ));

    hooks.push(write_builtin_hook(
        "mockspace-write-guard.sh",
        &builtin_write_guard(cfg),
        &repo_root,
        claude_hooks_dir,
        copilot_hooks_dir,
        count,
    ));

    hooks.push(write_builtin_hook(
        "mockspace-reminder.sh",
        &builtin_reminder(cfg),
        &repo_root,
        claude_hooks_dir,
        copilot_hooks_dir,
        count,
    ));

    hooks.push(write_builtin_hook(
        "no-yagni-guard.sh",
        &builtin_no_yagni(),
        &repo_root,
        claude_hooks_dir,
        copilot_hooks_dir,
        count,
    ));

    hooks
}

// ---------------------------------------------------------------------------
// Phase 7: Auto-generated settings
// ---------------------------------------------------------------------------

/// Where a hook this crate generates is written, on each side. The merge below
/// recognises its own entries by this prefix and by nothing else, so a hook
/// somebody wired by hand to a script of their own is not one of ours.
const CLAUDE_HOOK_PREFIX: &str = ".claude/hooks/";
const COPILOT_HOOK_PREFIX: &str = ".github/hooks/";

/// Read a JSON file as an object, and say whether it was readable.
///
/// Three answers, and the third is the one that matters. Absent gives an empty
/// object, which is the right base to build on. Present and parseable gives its
/// contents. Present and unparseable gives `None`, and every caller treats that
/// as "leave it alone": a file this cannot read is a file whose contents it
/// cannot preserve, and overwriting on that basis is how somebody's
/// configuration disappears.
fn read_json_object(path: &Path) -> Option<serde_json::Map<String, serde_json::Value>> {
    match fs::read_to_string(path) {
        Err(_) => Some(serde_json::Map::new()),
        Ok(text) if text.trim().is_empty() => Some(serde_json::Map::new()),
        Ok(text) => {
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Object(m)) => Some(m),
                _ => None,
            }
        },
    }
}

/// Whether a Claude hook entry is one this crate wrote.
///
/// True when every command in it points into the generated hooks directory. An
/// entry mixing one of ours with one of theirs is theirs, because dropping it
/// would take the hand-written half with it.
fn is_generated_claude_entry(entry: &serde_json::Value) -> bool {
    let hooks = match entry.get("hooks").and_then(|h| h.as_array()) {
        Some(h) if !h.is_empty() => h,
        _ => return false,
    };
    hooks.iter().all(|h| {
        h.get("command")
            .and_then(|c| c.as_str())
            .is_some_and(|c| c.starts_with(CLAUDE_HOOK_PREFIX))
    })
}

/// Generate `settings.json` (Claude) and `hooks.json` (Copilot) from hook metadata.
///
/// Both files are merged rather than replaced. They are the repository's own
/// agent configuration and hold far more than hook wiring: permissions,
/// environment, model selection, hooks on events this does not generate, and
/// whatever else the tooling grows. Replacing the file wholesale destroyed all
/// of it on every run, which is a particularly bad thing to do to a file whose
/// job includes recording what somebody has agreed an agent may do.
pub(crate) fn generate_settings(repo_root: &Path, all_hooks: &[HookMeta]) -> usize {
    let mut count = 0;

    // Group hooks by matcher
    let mut matcher_to_hooks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for hook in all_hooks {
        for matcher in &hook.matchers {
            matcher_to_hooks
                .entry(matcher.clone())
                .or_default()
                .push(hook.name.clone());
        }
    }

    // --- Claude settings.json ---
    let claude_dir = repo_root.join(".claude");
    let _ = fs::create_dir_all(&claude_dir);
    let claude_settings_path = claude_dir.join("settings.json");

    let generated: Vec<serde_json::Value> = matcher_to_hooks
        .iter()
        .flat_map(|(matcher, names)| {
            names.iter().map(move |name| {
                serde_json::json!({
                    "matcher": matcher,
                    "hooks": [{
                        "type": "command",
                        "command": format!("{CLAUDE_HOOK_PREFIX}{name}"),
                    }],
                })
            })
        })
        .collect();

    match read_json_object(&claude_settings_path) {
        None => {
            eprintln!(
                "  warning: {} is not readable as JSON, so its hook wiring was left alone. Fix \
                 or remove it and run again.",
                claude_settings_path.display()
            );
        },
        Some(mut settings) => {
            let hooks = settings
                .entry("hooks")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
            if !hooks.is_object() {
                *hooks = serde_json::Value::Object(serde_json::Map::new());
            }
            let hooks = hooks.as_object_mut().expect("just made it an object");

            // Everything on this event that is not ours, in the order it was
            // written, then ours after it. Other events are not read and not
            // touched.
            let mut kept: Vec<serde_json::Value> = hooks
                .get("PreToolUse")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|e| !is_generated_claude_entry(e))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            kept.extend(generated);
            hooks.insert("PreToolUse".into(), serde_json::Value::Array(kept));

            let body = serde_json::to_string_pretty(&serde_json::Value::Object(settings))
                .expect("a map of JSON values serialises");
            fs::write(&claude_settings_path, format!("{body}\n"))
                .expect("failed to write claude settings.json");
            eprintln!("  {} (hook wiring merged)", claude_settings_path.display());
            count += 1;
        },
    }

    // --- Copilot hooks.json ---
    let copilot_hooks_dir = repo_root.join(".github").join("hooks");
    let _ = fs::create_dir_all(&copilot_hooks_dir);
    let copilot_hooks_path = copilot_hooks_dir.join("hooks.json");

    let generated: Vec<serde_json::Value> = matcher_to_hooks
        .iter()
        .flat_map(|(matcher, names)| {
            names.iter().map(move |name| {
                serde_json::json!({
                    "name": name,
                    "command": format!("{COPILOT_HOOK_PREFIX}{name}"),
                    "event": "pre_tool_use",
                    "tool": matcher,
                })
            })
        })
        .collect();

    match read_json_object(&copilot_hooks_path) {
        None => {
            eprintln!(
                "  warning: {} is not readable as JSON, so its hook wiring was left alone. Fix \
                 or remove it and run again.",
                copilot_hooks_path.display()
            );
        },
        Some(mut doc) => {
            // Flat list here rather than keyed by event, so ours are the ones
            // whose command points into the generated directory.
            let mut kept: Vec<serde_json::Value> = doc
                .get("hooks")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|e| {
                            !e.get("command")
                                .and_then(|c| c.as_str())
                                .is_some_and(|c| c.starts_with(COPILOT_HOOK_PREFIX))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            kept.extend(generated);
            doc.insert("hooks".into(), serde_json::Value::Array(kept));

            let body = serde_json::to_string_pretty(&serde_json::Value::Object(doc))
                .expect("a map of JSON values serialises");
            fs::write(&copilot_hooks_path, format!("{body}\n"))
                .expect("failed to write copilot hooks.json");
            eprintln!("  {} (hook wiring merged)", copilot_hooks_path.display());
            count += 1;
        },
    }

    count
}

// ---------------------------------------------------------------------------
// Phase 9: Builtin agent templates
// ---------------------------------------------------------------------------

/// A builtin rule with its filename, apply_to patterns, and body content.
pub(crate) struct BuiltinRule {
    /// Output filename without extension (e.g. "generated-agent-rules").
    pub(crate) name:     String,
    /// Glob patterns for frontmatter.
    pub(crate) apply_to: Vec<String>,
    /// Body content (already substituted).
    pub(crate) body:     String,
}

/// A file a skill ships alongside its SKILL.md.
///
/// A skill that only tells an agent what to do is a rule with extra steps. The
/// useful ones carry something runnable, and a script needs its own manifest
/// next to it to resolve its dependencies, so both travel with the skill.
pub(crate) struct SkillFile {
    /// Path relative to the skill directory (e.g. "scripts/consume-strays").
    pub(crate) rel_path:   String,
    /// Verbatim contents. Not template-substituted: a script is code, and a
    /// brace in it is a brace.
    pub(crate) contents:   String,
    /// Whether to set the executable bit. Scripts are useless without it.
    pub(crate) executable: bool,
}

/// A builtin skill with its directory name, metadata, and body content.
pub(crate) struct BuiltinSkill {
    /// Directory name (e.g. "design-round").
    pub(crate) dir_name:          String,
    /// Skill display name.
    pub(crate) skill_name:        String,
    /// Skill description.
    pub(crate) skill_description: String,
    /// Body content (already substituted).
    pub(crate) body:              String,
    /// Anything runnable the skill ships. Empty for a prose-only skill.
    pub(crate) files:             Vec<SkillFile>,
}

/// All builtin templates generated by Phase 9.
pub(crate) struct BuiltinTemplates {
    pub(crate) rules:     Vec<BuiltinRule>,
    pub(crate) skills:    Vec<BuiltinSkill>,
    pub(crate) preamble:  String,
    pub(crate) postamble: String,
}

/// Replace `{var}` single-brace placeholders in builtin content.
///
/// Uses single braces to distinguish from the `{{double_brace}}` template
/// system used by consumer templates.
pub(crate) fn substitute_builtin_vars(
    text: &str,
    mock_dir: &str,
    project_name: &str,
    crate_prefix: &str,
) -> String {
    text.replace("{mock_dir}", mock_dir)
        .replace("{project_name}", project_name)
        .replace("{crate_prefix}", crate_prefix)
}

#[cfg(test)]
mod settings_tests {
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

    #[test]
    fn the_copilot_side_merges_on_the_same_terms() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join(".github/hooks")).unwrap();
        fs::write(
            d.path().join(".github/hooks/hooks.json"),
            r#"{"version":1,"hooks":[
                {"name":"mine","command":"scripts/mine.sh","event":"pre_tool_use","tool":"Write"},
                {"name":"stale","command":".github/hooks/stale.sh","event":"pre_tool_use","tool":"Bash"}]}"#,
        )
        .unwrap();

        generate_settings(d.path(), &[hook("gate.sh", &["Bash"])]);
        let text = fs::read_to_string(d.path().join(".github/hooks/hooks.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(
            v["version"], 1,
            "a key this does not write is not its to drop"
        );
        let names: Vec<&str> = v["hooks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| h["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["mine", "gate.sh"]);
    }
}
