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

/// Generate `settings.json` (Claude) and `hooks.json` (Copilot) from hook metadata.
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

    let mut claude_hooks_entries = Vec::new();
    for (matcher, hook_names) in &matcher_to_hooks {
        for hook_name in hook_names {
            claude_hooks_entries.push(format!(
                r#"      {{
        "matcher": "{matcher}",
        "hooks": [
          {{
            "type": "command",
            "command": ".claude/hooks/{hook_name}"
          }}
        ]
      }}"#
            ));
        }
    }

    let hooks_json_inner = claude_hooks_entries.join(",\n");

    let claude_settings = format!(
        r#"{{
    "hooks": {{
      "PreToolUse": [
{hooks_json_inner}
      ]
    }}
}}"#
    );

    let claude_settings_path = claude_dir.join("settings.json");
    fs::write(&claude_settings_path, &claude_settings)
        .expect("failed to write claude settings.json");
    eprintln!("  {} (auto-generated)", claude_settings_path.display());
    count += 1;

    // --- Copilot hooks.json ---
    let copilot_hooks_dir = repo_root.join(".github").join("hooks");
    let _ = fs::create_dir_all(&copilot_hooks_dir);

    let mut copilot_entries = Vec::new();
    for (matcher, hook_names) in &matcher_to_hooks {
        for hook_name in hook_names {
            copilot_entries.push(format!(
                r#"    {{
      "name": "{hook_name}",
      "command": ".github/hooks/{hook_name}",
      "event": "pre_tool_use",
      "tool": "{matcher}"
    }}"#
            ));
        }
    }

    let copilot_hooks_inner = copilot_entries.join(",\n");
    let copilot_hooks_json = format!(
        r#"{{
  "hooks": [
{copilot_hooks_inner}
  ]
}}"#
    );

    let copilot_hooks_path = copilot_hooks_dir.join("hooks.json");
    fs::write(&copilot_hooks_path, &copilot_hooks_json)
        .expect("failed to write copilot hooks.json");
    eprintln!("  {} (auto-generated)", copilot_hooks_path.display());
    count += 1;

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
