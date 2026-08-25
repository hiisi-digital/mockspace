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
    let claude_content = with_generated_marker(
        &content
            .replace("{{HOOK_HELPERS}}", CLAUDE_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", repo_root),
    );
    let claude_path = claude_hooks_dir.join(name);
    fs::write(&claude_path, &claude_content).expect("failed to write builtin claude hook");
    #[cfg(unix)]
    set_executable(&claude_path);
    eprintln!("  {} (builtin)", claude_path.display());
    *count += 1;

    let copilot_content = with_generated_marker(
        &content
            .replace("{{HOOK_HELPERS}}", COPILOT_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", repo_root),
    );
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

/// Where a hook this crate generates is written, on each side.
///
/// The prefix says where to look and does not say who wrote it. Only some of
/// what lands in these directories is generated: the orphan sweep is scoped to
/// one family of hooks, so a builtin that retires and anything somebody adds
/// themselves both stay. So the directory is shared, and the merge asks the
/// script itself, through [`generated_hook_marker`].
const CLAUDE_HOOK_PREFIX: &str = ".claude/hooks/";
const COPILOT_HOOK_PREFIX: &str = ".github/hooks/";

/// Put the generation marker into a hook script, after the shebang.
///
/// A shell script has nowhere structural to record who wrote it, so it says so
/// in a comment on its second line, inside the window
/// [`crate::render_design::is_generated`] reads. That is what lets the settings
/// merge drop its own wiring and leave a hand-written hook alone even though
/// both live in the same directory, and it tells anybody opening the file that
/// editing it is pointless.
pub(crate) fn with_generated_marker(content: &str) -> String {
    // Already marked stays as it is. Two markers would be one more than the
    // reader needs and would push the rest of the file a line further from the
    // window it reads.
    if crate::render_design::is_generated(content) {
        return content.to_string();
    }
    let marker = format!("# {}", crate::render_design::GENERATED_MARKER);
    // The first line, whether or not a newline follows it. Splitting on the
    // newline alone misses a one-line script, and prepending the marker to one
    // of those puts it in front of the shebang, which stops the kernel reading
    // the file as a script at all.
    let (first, rest) = match content.split_once('\n') {
        Some((first, rest)) => (first, Some(rest)),
        None => (content, None),
    };
    if !first.starts_with("#!") {
        return format!("{marker}\n{content}");
    }
    match rest {
        Some(rest) => format!("{first}\n{marker}\n{rest}"),
        None => format!("{first}\n{marker}\n"),
    }
}

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

/// Whether one command in a settings entry names a script this crate wrote.
///
/// Two conditions. It points into the hooks directory, **and** the script
/// there carries the generation marker. A script without one is somebody's,
/// wherever it sits.
///
/// A command naming a file that is not there counts as ours. It is inside the
/// directory this tool writes and there is no script left to ask, so the entry
/// is wiring for a hook that retired and its script went with it. Leaving it
/// would mean an event permanently wired to nothing.
fn names_a_generated_script(command: &str, repo_root: &Path, prefix: &str) -> bool {
    let Some(rest) = command.strip_prefix(prefix) else {
        return false;
    };
    // The whole rest first, then the first word. A command may carry arguments
    // after the script, so the first word is the usual answer; but a script
    // whose own name has a space in it truncates to nonsense that way, the
    // read then fails, and the arm below reads a present hand-written script as
    // an absent generated one and drops its wiring.
    let read = |script: &str| fs::read_to_string(repo_root.join(prefix).join(script)).ok();
    let first_word = rest.split_whitespace().next().unwrap_or(rest);
    match read(rest).or_else(|| read(first_word)) {
        Some(text) => crate::render_design::is_generated(&text),
        None => true,
    }
}

/// Whether a Claude hook entry is one this crate wrote.
///
/// True when every command in it names a generated script. An entry mixing one
/// of ours with one of theirs is theirs, because dropping it would take the
/// hand-written half with it.
fn is_generated_claude_entry(entry: &serde_json::Value, repo_root: &Path) -> bool {
    let hooks = match entry.get("hooks").and_then(|h| h.as_array()) {
        Some(h) if !h.is_empty() => h,
        _ => return false,
    };
    hooks.iter().all(|h| {
        h.get("command")
            .and_then(|c| c.as_str())
            .is_some_and(|c| names_a_generated_script(c, repo_root, CLAUDE_HOOK_PREFIX))
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
            // The same reading the unparseable arm takes, one and two levels
            // in. A `hooks` that is not an object, or a `PreToolUse` that is
            // not an array, is a shape this does not understand, and replacing
            // it would be discarding somebody's file on the strength of not
            // recognising it.
            if let Some(why) = unexpected_shape(&settings) {
                eprintln!(
                    "  warning: {} has {why}, so its hook wiring was left alone. Fix it and run \
                     again.",
                    claude_settings_path.display()
                );
                return generate_copilot_hooks(repo_root, &matcher_to_hooks, count);
            }
            let hooks = settings
                .entry("hooks")
                .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()))
                .as_object_mut()
                .expect("the shape check above admitted only an object");

            // Everything on this event that is not ours, in the order it was
            // written, then ours after it. Other events are not read and not
            // touched.
            let mut kept: Vec<serde_json::Value> = hooks
                .get("PreToolUse")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|e| !is_generated_claude_entry(e, repo_root))
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

    generate_copilot_hooks(repo_root, &matcher_to_hooks, count)
}

/// Whether `settings.json` holds a hook shape this cannot merge into.
///
/// `None` means it can. `Some(why)` names what was found, for the warning, and
/// means the file is left byte for byte as it was.
/// The same question for `hooks.json`, whose `hooks` is a flat array rather
/// than an object keyed by event. Without this a `hooks` of any other shape
/// falls to `unwrap_or_default()` and is silently replaced.
fn unexpected_copilot_shape(
    doc: &serde_json::Map<String, serde_json::Value>,
) -> Option<&'static str> {
    match doc.get("hooks") {
        None => None,
        Some(v) if v.is_null() => Some("a null `hooks`"),
        Some(v) if !v.is_array() => Some("a `hooks` that is not an array"),
        Some(_) => None,
    }
}

fn unexpected_shape(settings: &serde_json::Map<String, serde_json::Value>) -> Option<&'static str> {
    let hooks = settings.get("hooks")?;
    if hooks.is_null() {
        return Some("a null `hooks`");
    }
    let Some(hooks) = hooks.as_object() else {
        return Some("a `hooks` that is not an object");
    };
    match hooks.get("PreToolUse") {
        Some(v) if !v.is_array() => Some("a `PreToolUse` that is not an array"),
        _ => None,
    }
}

/// The Copilot half, which keys its hooks in a flat list rather than by event.
fn generate_copilot_hooks(
    repo_root: &Path,
    matcher_to_hooks: &BTreeMap<String, Vec<String>>,
    mut count: usize,
) -> usize {
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
        Some(doc) if unexpected_copilot_shape(&doc).is_some() => {
            eprintln!(
                "  warning: {} carries {}, which this does not understand, so its hook wiring \
                 was left alone.",
                copilot_hooks_path.display(),
                unexpected_copilot_shape(&doc).unwrap_or("an unexpected shape"),
            );
        },
        Some(mut doc) => {
            // Flat list here rather than keyed by event, so each entry answers
            // on its own script rather than on an entry's worth of them.
            let mut kept: Vec<serde_json::Value> = doc
                .get("hooks")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|e| {
                            !e.get("command").and_then(|c| c.as_str()).is_some_and(|c| {
                                names_a_generated_script(c, repo_root, COPILOT_HOOK_PREFIX)
                            })
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
        for odd in ["{\n  \"hooks\": {}\n}", "{\n  \"hooks\": null\n}"] {
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
