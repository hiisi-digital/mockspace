use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use mockspace_lint_rules::LintPack;

use crate::config::Config;
use crate::model::*;
use crate::render_design;

mod helpers;
pub(crate) use helpers::*;
mod builtins;
pub(crate) use builtins::*;
mod wire;
pub(crate) use wire::*;
mod content;
pub(crate) use content::*;
mod templates;
pub(crate) use templates::*;
mod vars;
pub(crate) use vars::*;
mod bookend;
pub(crate) use bookend::*;
#[cfg(test)]
mod tests;

/// Generate agent rules, skills, hooks, and settings from templates in agent/ directory.
///
/// Reads templates from `mock_dir/agent/`, substitutes template variables,
/// and writes dual outputs:
/// - Claude: CLAUDE.md, .claude/rules/*.md, .claude/skills/*/SKILL.md,
///           .claude/agents/*.md, .claude/hooks/*.sh, .claude/settings.json
/// - Copilot: .github/copilot-instructions.md, .github/instructions/*.instructions.md,
///            .github/skills/*/SKILL.md, .github/hooks/*.sh, .github/hooks/hooks.json
pub fn generate_agent_rules(
    crates: &CrateMap,
    cfg: &Config,
    registry: &crate::registry::Registry,
    pack: &LintPack,
) -> usize {
    let repo_root = &cfg.repo_root;
    let agent_dir = cfg.mock_dir.join("agent");

    // Phase 9: generate builtin templates (always, even without agent/ directory)
    let builtins = generate_builtin_templates(cfg, pack);

    // Collect consumer rule/skill names to determine overrides
    let consumer_rule_names = collect_consumer_rule_names(&agent_dir);
    let consumer_skill_names = collect_consumer_skill_names(&agent_dir);

    let mut vars = compute_template_vars(crates, cfg);
    let header_md = render_design::generation_header_md(cfg);

    // Phase 9: Merge preamble: builtin + consumer (concatenated)
    let consumer_preamble = read_optional_template(&agent_dir.join("PREAMBLE.md.tmpl"));
    validate_bookend_size(&consumer_preamble, "PREAMBLE.md.tmpl");
    let preamble = if consumer_preamble.is_empty() {
        builtins.preamble.clone()
    } else {
        format!("{}\n\n{}", builtins.preamble, consumer_preamble)
    };

    // Phase 9: Merge postamble: builtin + consumer (concatenated)
    let consumer_postamble = read_optional_template(&agent_dir.join("POSTAMBLE.md.tmpl"));
    validate_bookend_size(&consumer_postamble, "POSTAMBLE.md.tmpl");
    let postamble = if consumer_postamble.is_empty() {
        builtins.postamble.clone()
    } else {
        format!("{}\n\n{}", builtins.postamble, consumer_postamble)
    };

    vars.push(("PREAMBLE".to_string(), preamble.clone()));
    vars.push(("POSTAMBLE".to_string(), postamble.clone()));
    let mut count = 0;

    // --- MAIN.md.tmpl -> .claude/CLAUDE.md + .github/copilot-instructions.md ---
    let main_tmpl = agent_dir.join("MAIN.md.tmpl");
    if main_tmpl.is_file() {
        let body = read_and_substitute(&main_tmpl, &vars);

        // Claude: .claude/CLAUDE.md (discovered from .claude/ directory)
        let claude_dir = repo_root.join(".claude");
        let _ = fs::create_dir_all(&claude_dir);
        let claude_content = format_with_bookends(&header_md, &preamble, &body, &postamble);
        let claude_path = claude_dir.join("CLAUDE.md");
        render_design::write_generated(&claude_path, &claude_content);
        eprintln!("  {}", claude_path.display());
        count += 1;

        // Clean up the renamed byline hook. It matched Bash only, so leaving it
        // behind would keep a hook that cannot see an MCP tool while the
        // replacement can, and a reader would not know which was authoritative.
        for dir in [claude_dir.join("hooks"), repo_root.join(".github/hooks")] {
            let stale = dir.join("check-byline.sh");
            if stale.is_file() {
                let _ = fs::remove_file(&stale);
                eprintln!("  removed superseded {}", stale.display());
            }
        }

        // Clean up legacy root CLAUDE.md if it exists
        let legacy_path = repo_root.join("CLAUDE.md");
        if legacy_path.is_file() {
            let _ = fs::remove_file(&legacy_path);
            eprintln!("  removed legacy {}", legacy_path.display());
        }

        // Copilot: .github/copilot-instructions.md
        let copilot_dir = repo_root.join(".github");
        let _ = fs::create_dir_all(&copilot_dir);
        let copilot_content = format_with_bookends(&header_md, &preamble, &body, &postamble);
        let copilot_path = copilot_dir.join("copilot-instructions.md");
        render_design::write_generated(&copilot_path, &copilot_content);
        eprintln!("  {}", copilot_path.display());
        count += 1;
    }

    // --- Phase 9: Builtin rules (written first, consumer overrides) ---
    let claude_rules_dir = repo_root.join(".claude").join("rules");
    let copilot_instructions_dir = repo_root.join(".github").join("instructions");
    let _ = fs::create_dir_all(&claude_rules_dir);
    let _ = fs::create_dir_all(&copilot_instructions_dir);

    for builtin_rule in &builtins.rules {
        // Skip if consumer has a template with the same name
        if consumer_rule_names.contains(&builtin_rule.name) {
            eprintln!("  {} (builtin, overridden by consumer)", builtin_rule.name);
            continue;
        }

        // Claude: .claude/rules/<name>.md with paths: frontmatter
        let claude_fm = format_claude_paths(&builtin_rule.apply_to);
        let claude_content = format!(
            "{claude_fm}\n{}",
            format_with_bookends(&header_md, &preamble, &builtin_rule.body, &postamble)
        );
        let claude_path = claude_rules_dir.join(format!("{}.md", builtin_rule.name));
        render_design::write_generated(&claude_path, &claude_content);
        eprintln!("  {} (builtin)", claude_path.display());
        count += 1;

        // Copilot: .github/instructions/<name>.instructions.md
        let copilot_fm = format_copilot_apply_to(&builtin_rule.apply_to);
        let copilot_content = format!(
            "{copilot_fm}\n{}",
            format_with_bookends(&header_md, &preamble, &builtin_rule.body, &postamble)
        );
        let copilot_path =
            copilot_instructions_dir.join(format!("{}.instructions.md", builtin_rule.name));
        render_design::write_generated(&copilot_path, &copilot_content);
        eprintln!("  {} (builtin)", copilot_path.display());
        count += 1;
    }

    // --- Lint-derived rules and hooks (from forbidden-imports config) ---
    let claude_hooks_dir = repo_root.join(".claude").join("hooks");
    let copilot_hooks_dir = repo_root.join(".github").join("hooks");
    count += generate_lint_derived_content(
        cfg,
        &claude_rules_dir,
        &copilot_instructions_dir,
        &claude_hooks_dir,
        &copilot_hooks_dir,
        &header_md,
        &preamble,
        &postamble,
    );

    // --- Per-crate README-derived rules (scoped to that crate's directory) ---
    count += generate_crate_readme_rules(
        cfg,
        &claude_rules_dir,
        &copilot_instructions_dir,
        &vars,
        &header_md,
        &preamble,
        &postamble,
        registry,
    );

    // --- Consumer rules/*.md.tmpl -> .claude/rules/*.md + .github/instructions/*.instructions.md ---
    let rules_dir = agent_dir.join("rules");
    if rules_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&rules_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|x| x == "tmpl").unwrap_or(false))
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let raw = fs::read_to_string(&path).expect("failed to read rule template");
            let raw = render_agent_body(&raw, &vars, cfg, registry);
            let (frontmatter, body) = split_frontmatter(&raw);
            let apply_to = parse_apply_to(&frontmatter);

            let stem = path
                .file_stem()
                .unwrap()
                .to_string_lossy()
                .trim_end_matches(".md")
                .to_string();

            // Claude: .claude/rules/<name>.md with paths: frontmatter
            let claude_fm = format_claude_paths(&apply_to);
            let claude_content = format!(
                "{claude_fm}\n{}",
                format_with_bookends(&header_md, &preamble, &body, &postamble)
            );
            let claude_path = claude_rules_dir.join(format!("{stem}.md"));
            render_design::write_generated(&claude_path, &claude_content);
            eprintln!("  {}", claude_path.display());
            count += 1;

            // Copilot: .github/instructions/<name>.instructions.md with applyTo: frontmatter
            let copilot_fm = format_copilot_apply_to(&apply_to);
            let copilot_content = format!(
                "{copilot_fm}\n{}",
                format_with_bookends(&header_md, &preamble, &body, &postamble)
            );
            let copilot_path = copilot_instructions_dir.join(format!("{stem}.instructions.md"));
            render_design::write_generated(&copilot_path, &copilot_content);
            eprintln!("  {}", copilot_path.display());
            count += 1;
        }
    }

    // --- Phase 9: Builtin skills (written first, consumer overrides) ---
    let claude_skills_dir = repo_root.join(".claude").join("skills");
    let copilot_skills_dir = repo_root.join(".github").join("skills");

    for builtin_skill in &builtins.skills {
        // Skip if consumer has a skill with the same directory name
        if consumer_skill_names.contains(&builtin_skill.dir_name) {
            eprintln!(
                "  {} (builtin skill, overridden by consumer)",
                builtin_skill.dir_name
            );
            continue;
        }

        let skill_fm = format!(
            "---\nname: {}\ndescription: {}\n---",
            builtin_skill.skill_name, builtin_skill.skill_description
        );

        // Claude: .claude/skills/<name>/SKILL.md
        let claude_skill_dir = claude_skills_dir.join(&builtin_skill.dir_name);
        let _ = fs::create_dir_all(&claude_skill_dir);
        let claude_content = format!(
            "{skill_fm}\n{}",
            format_with_bookends(&header_md, &preamble, &builtin_skill.body, &postamble)
        );
        let claude_path = claude_skill_dir.join("SKILL.md");
        render_design::write_generated(&claude_path, &claude_content);
        eprintln!("  {} (builtin)", claude_path.display());
        count += 1;

        // Copilot: .github/skills/<name>/SKILL.md
        let copilot_skill_dir = copilot_skills_dir.join(&builtin_skill.dir_name);
        let _ = fs::create_dir_all(&copilot_skill_dir);
        let copilot_content = format!(
            "{skill_fm}\n{}",
            format_with_bookends(&header_md, &preamble, &builtin_skill.body, &postamble)
        );
        let copilot_path = copilot_skill_dir.join("SKILL.md");
        render_design::write_generated(&copilot_path, &copilot_content);
        eprintln!("  {} (builtin)", copilot_path.display());
        count += 1;

        // Anything runnable the skill ships, into both surfaces so either agent
        // can invoke it. Written verbatim rather than through write_generated:
        // that prepends an auto-generated banner, which would sit above the
        // shebang and stop a script being a script.
        for file in &builtin_skill.files {
            for skill_dir in [&claude_skill_dir, &copilot_skill_dir] {
                let dest = skill_dir.join(&file.rel_path);
                if let Some(parent) = dest.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                if fs::write(&dest, &file.contents).is_err() {
                    eprintln!("  warning: could not write {}", dest.display());
                    continue;
                }
                #[cfg(unix)]
                if file.executable {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o755));
                }
                eprintln!("  {} (builtin)", dest.display());
                count += 1;
            }
        }
    }

    // A knob turned off takes back what it once wrote. A builtin skill
    // directory that is no longer generated, and is not the consumer's own,
    // is swept here; otherwise the declared config and the generated surface
    // disagree forever, since nothing else ever deletes it. Only names from
    // the builtin catalogue are candidates, so a directory the consumer put
    // there by hand is never touched.
    let enabled: Vec<&str> = builtins.skills.iter().map(|s| s.dir_name.as_str()).collect();
    for dir_name in BUILTIN_SKILL_DIRS {
        if enabled.contains(dir_name) {
            continue;
        }
        if consumer_skill_names.iter().any(|n| n == dir_name) {
            continue;
        }
        for base in [&claude_skills_dir, &copilot_skills_dir] {
            let stale = base.join(dir_name);
            if stale.is_dir() && fs::remove_dir_all(&stale).is_ok() {
                eprintln!("  {} (builtin skill, removed: disabled)", stale.display());
            }
        }
    }

    // --- Consumer skills/*/SKILL.md.tmpl -> .claude/skills/*/SKILL.md + .github/skills/*/SKILL.md ---
    let skills_dir = agent_dir.join("skills");
    if skills_dir.is_dir() {
        let mut skill_dirs: Vec<_> = fs::read_dir(&skills_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        skill_dirs.sort_by_key(|e| e.file_name());

        for skill_entry in skill_dirs {
            let skill_path = skill_entry.path().join("SKILL.md.tmpl");
            if !skill_path.is_file() {
                continue;
            }

            let skill_name = skill_entry.file_name().to_string_lossy().to_string();

            // Substitute across the WHOLE template before splitting, matching the
            // personas phase. A skill's `description` is the line an agent reads
            // to decide whether the skill applies, so it is the one place in a
            // skill most likely to want the project's own name, and it was the
            // one place not getting it: the body was rendered and the
            // frontmatter was parsed straight off the raw text.
            let raw = fs::read_to_string(&skill_path).expect("failed to read skill template");
            let raw = render_agent_body(&raw, &vars, cfg, registry);
            let (frontmatter, body) = split_frontmatter(&raw);
            let name = parse_field(&frontmatter, "skill_name").unwrap_or(skill_name.clone());
            let desc = parse_field(&frontmatter, "skill_description").unwrap_or_default();

            // Both Claude and Copilot use the same skill frontmatter format
            let skill_fm = format!("---\nname: {name}\ndescription: {desc}\n---");

            // Claude: .claude/skills/<name>/SKILL.md
            let claude_skill_dir = claude_skills_dir.join(&skill_name);
            let _ = fs::create_dir_all(&claude_skill_dir);
            let claude_content = format!(
                "{skill_fm}\n{}",
                format_with_bookends(&header_md, &preamble, &body, &postamble)
            );
            let claude_path = claude_skill_dir.join("SKILL.md");
            render_design::write_generated(&claude_path, &claude_content);
            eprintln!("  {}", claude_path.display());
            count += 1;

            // Copilot: .github/skills/<name>/SKILL.md
            let copilot_skill_dir = copilot_skills_dir.join(&skill_name);
            let _ = fs::create_dir_all(&copilot_skill_dir);
            let copilot_content = format!(
                "{skill_fm}\n{}",
                format_with_bookends(&header_md, &preamble, &body, &postamble)
            );
            let copilot_path = copilot_skill_dir.join("SKILL.md");
            render_design::write_generated(&copilot_path, &copilot_content);
            eprintln!("  {}", copilot_path.display());
            count += 1;
        }
    }

    // --- Consumer agents/*.md.tmpl -> .claude/agents/*.md ---
    //
    // Claude-only by design. There is no Copilot equivalent of a sub-agent
    // persona, so unlike rules and skills this phase writes a single target.
    //
    // The frontmatter passes through VERBATIM, which is deliberate and differs
    // from the skills phase above. Skills re-encode (`skill_name` ->  `name`)
    // because one skill is emitted to two targets and needs identical
    // frontmatter in both, so normalising through mockspace's own field names
    // is what keeps them in step. An agent has one target and its frontmatter
    // schema (`name` / `description` / `tools` / `model`, and whatever Claude
    // Code adds next) is owned by Claude Code, not by mockspace. Re-encoding a
    // schema we do not own would silently drop every field we did not think to
    // enumerate. Passing it through means a new upstream field works the day it
    // ships, with no mockspace release in the path.
    //
    // Templates still get variable substitution throughout, frontmatter
    // included, so a persona can reference {{project_name}} and friends like any
    // other template.
    let agents_dir = agent_dir.join("agents");
    if agents_dir.is_dir() {
        let claude_agents_dir = repo_root.join(".claude").join("agents");
        let _ = fs::create_dir_all(&claude_agents_dir);

        let mut entries: Vec<_> = fs::read_dir(&agents_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().is_file()
                    && e.path()
                        .to_str()
                        .map(|p| p.ends_with(".md.tmpl"))
                        .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let stem = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(".md.tmpl"))
                .unwrap_or_default()
                .to_string();

            // Substitute across the WHOLE template before splitting, matching
            // the rules phase. Frontmatter is as entitled to {{project_name}} as
            // the body is, and a persona's `description` is a natural place to
            // want it. This does not weaken the pass-through: passing through
            // means mockspace does not rename or drop fields, not that it
            // refuses to expand variables inside them.
            let raw = fs::read_to_string(&path).expect("failed to read agent template");
            let raw = render_agent_body(&raw, &vars, cfg, registry);
            let (frontmatter, body) = split_frontmatter(&raw);

            // An agent with no frontmatter has no `name`/`description`, so
            // Claude Code cannot register it. Skipping silently would present
            // as "the persona just isn't there" with nothing to grep for, so
            // say it plainly and carry on with the rest.
            if frontmatter.trim().is_empty() {
                eprintln!(
                    "  {} SKIPPED: no frontmatter; an agent needs at least `name:` and `description:`",
                    path.display()
                );
                continue;
            }

            let content = render_agent_content(&frontmatter, &body);
            let out_path = claude_agents_dir.join(format!("{stem}.md"));
            fs::write(&out_path, &content).expect("failed to write claude agent");
            eprintln!("  {}", out_path.display());
            count += 1;
        }
    }

    // --- Phase 6: Builtin hooks (generated BEFORE consumer hooks) ---
    let claude_hooks_dir = repo_root.join(".claude").join("hooks");
    let copilot_hooks_dir = repo_root.join(".github").join("hooks");
    let _ = fs::create_dir_all(&claude_hooks_dir);
    let _ = fs::create_dir_all(&copilot_hooks_dir);

    let mut all_hooks: Vec<HookMeta> =
        generate_builtin_hooks(cfg, &claude_hooks_dir, &copilot_hooks_dir, &mut count);

    // --- Consumer hooks: hooks/*.sh.tmpl -> .claude/hooks/*.sh + .github/hooks/*.sh ---
    let hooks_dir = agent_dir.join("hooks");
    if hooks_dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(&hooks_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.ends_with(".sh.tmpl")
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let tmpl_name = entry.file_name().to_string_lossy().to_string();
            // Strip .tmpl suffix: "check-message.sh.tmpl" -> "check-message.sh"
            let out_name = tmpl_name.trim_end_matches(".tmpl");

            let template = fs::read_to_string(&path).expect("failed to read hook template");

            // Parse matchers from frontmatter or fall back to naming convention
            let matchers =
                parse_hook_matchers(&template).unwrap_or_else(|| matchers_from_name(out_name));

            // substitute general template variables first
            let template = render_agent_body(&template, &vars, cfg, registry);

            let repo_root_str = cfg.repo_root.to_string_lossy();

            // Claude: tool_input.field, hookSpecificOutput wrapper
            let claude_content = template
                .replace("{{HOOK_HELPERS}}", CLAUDE_HOOK_HELPERS)
                .replace("{{REPO_ROOT}}", &repo_root_str);
            let claude_path = claude_hooks_dir.join(out_name);
            fs::write(&claude_path, &claude_content).expect("failed to write claude hook");
            #[cfg(unix)]
            set_executable(&claude_path);
            eprintln!("  {}", claude_path.display());
            count += 1;

            // Copilot: toolArgs (stringified JSON), flat output
            let copilot_content = template
                .replace("{{HOOK_HELPERS}}", COPILOT_HOOK_HELPERS)
                .replace("{{REPO_ROOT}}", &repo_root_str);
            let copilot_path = copilot_hooks_dir.join(out_name);
            fs::write(&copilot_path, &copilot_content).expect("failed to write copilot hook");
            #[cfg(unix)]
            set_executable(&copilot_path);
            eprintln!("  {}", copilot_path.display());
            count += 1;

            all_hooks.push(HookMeta {
                name: out_name.to_string(),
                matchers,
            });
        }
    }

    // --- Phase 7: Auto-generate settings from discovered hooks ---
    count += generate_settings(repo_root, &all_hooks);

    count
}

/// Assemble content with preamble before body and postamble after.
/// Maximum word count for each of the built-in and consumer-authored
/// preamble/postamble bookends. Bookends are re-stamped on every rule
/// file and every skill body, so keeping them under this budget prevents
/// context bloat when multiple rules load simultaneously.
pub const BOOKEND_MAX_WORDS: usize = 25;

/// Built-in preamble stamped before every rule body and MAIN instructions.
/// Must stay within `BOOKEND_MAX_WORDS`: enforced by unit test below.
pub const BUILTIN_PREAMBLE: &str = concat!(
    "> **MOCKSPACE:** docs=design. source=untrusted. ",
    "Lints never exempt: stop if blocked. ",
    "Flow: topic → doc CL → lock → src CL → lock → close. ",
    "No shortcuts.",
);

/// Built-in postamble stamped after every rule body and MAIN instructions.
/// Must stay within `BOOKEND_MAX_WORDS`: enforced by unit test below.
pub const BUILTIN_POSTAMBLE: &str = concat!(
    "---\n",
    "> **VERIFY:** lint-only --commit green? src CL covers it? ",
    "Never edit `.claude/`/`.github/` directly. ",
    "Deprecate, never addendum.",
);
