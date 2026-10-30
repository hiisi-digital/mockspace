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
    write_executable(&claude_path, &claude_content).expect("failed to write builtin claude hook");
    eprintln!("  {} (builtin)", claude_path.display());
    *count += 1;

    let copilot_content = with_generated_marker(
        &content
            .replace("{{HOOK_HELPERS}}", COPILOT_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", repo_root),
    );
    let copilot_path = copilot_hooks_dir.join(name);
    write_executable(&copilot_path, &copilot_content).expect("failed to write builtin copilot hook");
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
                        "timeout": HOOK_TIMEOUT_SEC,
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
                return generate_copilot_hooks(repo_root, all_hooks, count);
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

    generate_copilot_hooks(repo_root, all_hooks, count)
}

/// Whether `settings.json` holds a hook shape this cannot merge into.
///
/// `None` means it can. `Some(why)` names what was found, for the warning, and
/// means the file is left byte for byte as it was.
/// The same question for `hooks.json`, whose `hooks` is an object keyed by event
/// with an array under `preToolUse`. A `hooks` of any other shape would be silently
/// replaced, except the flat list of our own entries that earlier renders wrote.
fn unexpected_copilot_shape(
    doc: &serde_json::Map<String, serde_json::Value>,
    repo_root: &Path,
) -> Option<&'static str> {
    match doc.get("hooks") {
        None => None,
        Some(v) if v.is_null() => Some("a null `hooks`"),
        Some(serde_json::Value::Object(events)) => match events.get("preToolUse") {
            Some(v) if !v.is_array() => Some("a `preToolUse` that is not an array"),
            _ => None,
        },
        Some(serde_json::Value::Array(entries)) => {
            if entries.iter().all(|e| is_generated_copilot_entry(e, repo_root)) {
                None
            } else {
                Some("a flat `hooks` list with entries that are not this crate's, a shape Copilot does not read")
            }
        },
        Some(_) => Some("a `hooks` that is neither an object nor a list"),
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

/// Copilot's runtime names for the tools a Claude matcher names.
///
/// Copilot tests a hook's `matcher` against its own tool name, as a regex anchored
/// to the whole name, and its file tools are `create` and `edit` where Claude's are
/// `Write`, `Edit` and `MultiEdit` (hooks reference, "Claude tool name" table). A
/// token with no row is passed through, since it may already be a runtime name or a
/// regex of the author's.
///
/// The reference also lists `str_replace_editor` and `apply_patch` under `Edit`. They are
/// left out on purpose and catalogued as ignored tests in
/// `tests/a_hook_named_after_a_lint_asks_the_lint.rs`: a patch carries no path, and the
/// reference gives no argument names for either, nor for `edit` and `create` (the names the
/// hook reads are `path`, `old_str`, `new_str` and `file_text`, which the reference does
/// not confirm), so wiring them would be a guess.
const COPILOT_TOOL_NAMES: &[(&str, &[&str])] = &[
    ("Bash", &["bash", "powershell"]),
    ("Write", &["create"]),
    ("Edit", &["edit"]),
    ("MultiEdit", &["edit"]),
];

/// A hook's Claude matchers as the one Copilot `matcher` regex, or `None` when the
/// hook fires on every tool (a `*` token, which Copilot spells by omitting the key).
fn copilot_matcher(claude: &[String]) -> Option<String> {
    let mut names: Vec<String> = Vec::new();
    for token in claude.iter().flat_map(|m| m.split('|')) {
        let token = token.trim();
        if token == "*" || token.is_empty() {
            return None;
        }
        let mapped = COPILOT_TOOL_NAMES.iter().find(|(c, _)| *c == token);
        let runtime: Vec<String> = match mapped {
            Some((_, runtime)) => runtime.iter().map(|n| (*n).to_string()).collect(),
            None => vec![token.to_string()],
        };
        for name in runtime {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Some(names.join("|"))
}

/// How long either agent lets a generated hook run, in seconds, written on every
/// entry so the figure is ours rather than each side's default (30 on Copilot, per
/// its hooks reference). A timeout is fail-open on both, so this is what a verdict may
/// take before the edit goes through unjudged. A warm `check-edit` takes a few tens of
/// milliseconds; the rest is room for the engine to load a lint pack that went stale.
const HOOK_TIMEOUT_SEC: u32 = 60;

/// The `preToolUse` entry for one generated hook, in the shape Copilot reads.
fn copilot_entry(hook: &HookMeta) -> serde_json::Value {
    let mut entry = serde_json::json!({
        "type": "command",
        "bash": format!("{COPILOT_HOOK_PREFIX}{}", hook.name),
        "timeoutSec": HOOK_TIMEOUT_SEC,
    });
    if let Some(matcher) = copilot_matcher(&hook.matchers) {
        entry["matcher"] = serde_json::Value::String(matcher);
    }
    entry
}

/// Whether a Copilot hook entry is one this crate wrote: it runs a generated script.
fn is_generated_copilot_entry(entry: &serde_json::Value, repo_root: &Path) -> bool {
    ["bash", "command"].iter().any(|key| {
        entry
            .get(key)
            .and_then(|c| c.as_str())
            .is_some_and(|c| names_a_generated_script(c, repo_root, COPILOT_HOOK_PREFIX))
    })
}

/// The Copilot half: `.github/hooks/hooks.json` as the hooks reference spells it,
/// `{"version":1,"hooks":{"preToolUse":[{"type":"command","bash":..,"matcher":..}]}}`.
///
/// Earlier renders wrote a flat list of `{name, command, event, tool}` that Copilot
/// never read. A list holding only entries of ours is that file and is replaced; one
/// holding anything else is left alone, since there is no telling where the entry
/// belongs in the new shape.
fn generate_copilot_hooks(repo_root: &Path, all_hooks: &[HookMeta], mut count: usize) -> usize {
    let copilot_hooks_dir = repo_root.join(".github").join("hooks");
    let _ = fs::create_dir_all(&copilot_hooks_dir);
    let copilot_hooks_path = copilot_hooks_dir.join("hooks.json");

    let generated: Vec<serde_json::Value> = all_hooks.iter().map(copilot_entry).collect();

    match read_json_object(&copilot_hooks_path) {
        None => {
            eprintln!(
                "  warning: {} is not readable as JSON, so its hook wiring was left alone. Fix \
                 or remove it and run again.",
                copilot_hooks_path.display()
            );
        },
        Some(doc) if unexpected_copilot_shape(&doc, repo_root).is_some() => {
            eprintln!(
                "  warning: {} carries {}, which this does not understand, so its hook wiring \
                 was left alone.",
                copilot_hooks_path.display(),
                unexpected_copilot_shape(&doc, repo_root).unwrap_or("an unexpected shape"),
            );
        },
        Some(mut doc) => {
            let mut events = match doc.remove("hooks") {
                Some(serde_json::Value::Object(events)) => events,
                _ => serde_json::Map::new(),
            };
            // Everything on this event that is not ours, in the order it was
            // written, then ours after it. Other events are not read and not
            // touched.
            let mut kept: Vec<serde_json::Value> = events
                .get("preToolUse")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|e| !is_generated_copilot_entry(e, repo_root))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            kept.extend(generated);
            events.insert("preToolUse".into(), serde_json::Value::Array(kept));
            // A version the file already carries is not ours to change.
            doc.entry("version").or_insert_with(|| serde_json::json!(1));
            doc.insert("hooks".into(), serde_json::Value::Object(events));

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
#[path = "wire_settings_tests.rs"]
mod settings_tests;
