#![allow(unused_imports)]
use super::*;

/// Generate agent rules and hooks derived from lint configuration.
///
/// For each `forbidden-imports` rule at error/build-gate severity,
/// generates:
/// - An agent rule file per scope (tells the agent what's forbidden)
/// - An agent hook per scope (blocks writes containing forbidden patterns)
pub(crate) fn generate_lint_derived_content(
    cfg: &Config,
    claude_rules_dir: &Path,
    copilot_instructions_dir: &Path,
    claude_hooks_dir: &Path,
    copilot_hooks_dir: &Path,
    header_md: &str,
    preamble: &str,
    postamble: &str,
) -> usize {
    use std::collections::{BTreeMap as Map, BTreeSet as Set};

    let mut count = 0;

    // Track slugs we wrote this render so we can sweep orphans afterwards.
    // Without this, files written for a previous mockspace.toml configuration
    // (e.g. before a scope was renamed or removed) persist on disk forever.
    let mut active_slugs: Set<String> = Set::new();

    // Collect forbidden-imports rules grouped by scope
    let mut scope_rules: Map<String, Vec<(String, String)>> = Map::new();

    // Look for forbidden-imports params in lint config
    for (key, value) in &cfg
        .lint_overrides
        .params
        .get("forbidden-imports")
        .cloned()
        .unwrap_or_default()
    {
        // Keys are like "rule.no-dyn.scope", "rule.no-dyn.forbidden", "rule.no-dyn.reason"
        if !key.starts_with("rule.") {
            continue;
        }

        let parts: Vec<&str> = key.splitn(3, '.').collect();
        if parts.len() < 3 {
            continue;
        }

        let field = parts[2];

        if field == "scope" {
            // Pre-populate scope entry
            scope_rules.entry(value.clone()).or_default();
        }
    }

    // Now collect forbidden + reason per scope
    let params = cfg
        .lint_overrides
        .params
        .get("forbidden-imports")
        .cloned()
        .unwrap_or_default();
    let mut rules_by_name: Map<String, (String, String, String)> = Map::new(); // name -> (scope, forbidden, reason)

    for (key, value) in &params {
        if !key.starts_with("rule.") {
            continue;
        }
        let parts: Vec<&str> = key.splitn(3, '.').collect();
        if parts.len() < 3 {
            continue;
        }

        let rule_name = parts[1].to_string();
        let field = parts[2];

        let entry = rules_by_name
            .entry(rule_name)
            .or_insert_with(|| (String::new(), String::new(), String::new()));
        match field {
            "scope" => entry.0 = value.clone(),
            "forbidden" => entry.1 = value.clone(),
            "reason" => entry.2 = value.clone(),
            "enabled" if value == "false" => {
                entry.0 = "__disabled__".to_string();
            },
            _ => {},
        }
    }

    // Group by scope
    let mut by_scope: Map<String, Vec<(String, String)>> = Map::new(); // scope -> [(forbidden, reason)]
    for (_name, (scope, forbidden, reason)) in &rules_by_name {
        if scope == "__disabled__" || scope.is_empty() || forbidden.is_empty() {
            continue;
        }
        by_scope
            .entry(scope.clone())
            .or_default()
            .push((forbidden.clone(), reason.clone()));
    }

    if by_scope.is_empty() {
        return 0;
    }

    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .replace('\\', "/");

    for (scope, rules) in &by_scope {
        // Convert scope to apply_to glob
        let apply_to_glob = format!("{mock_rel}/crates/{scope}/**");

        // Generate a slug for the filename
        let slug = scope
            .replace('*', "star")
            .replace(',', "-")
            .replace(' ', "");
        active_slugs.insert(slug.clone());

        // --- Agent Rule ---
        let mut body = String::new();
        let _ = writeln!(
            body,
            "## Forbidden imports for scope `{scope}` (auto-generated from lint config)\n"
        );
        let _ = writeln!(
            body,
            "The following are **forbidden** in crates matching `{scope}`."
        );
        let _ = writeln!(
            body,
            "Violations are caught by the `forbidden-imports` lint at error level.\n"
        );
        for (forbidden, reason) in rules {
            let _ = writeln!(body, "- `{forbidden}`: {reason}");
        }
        let _ = writeln!(
            body,
            "\nDo NOT write code that uses any of these. Check before writing."
        );

        let claude_fm = format!("---\napply_to: [\"{apply_to_glob}\"]\n---\n\n");
        let copilot_fm = format!("---\napplyTo: [\"{apply_to_glob}\"]\n---\n\n");

        let rule_name = format!("lint-forbidden-{slug}");
        let claude_content = format!("{claude_fm}{header_md}\n{preamble}\n\n{body}\n{postamble}");
        let copilot_content = format!("{copilot_fm}{header_md}\n{preamble}\n\n{body}\n{postamble}");

        let claude_path = claude_rules_dir.join(format!("{rule_name}.md"));
        render_design::write_generated_ok(&claude_path, &claude_content);
        eprintln!("  {} (lint-derived)", claude_path.display());
        count += 1;

        let copilot_path = copilot_instructions_dir.join(format!("{rule_name}.instructions.md"));
        render_design::write_generated_ok(&copilot_path, &copilot_content);
        count += 1;

        // --- Agent Hook ---
        // Generate a hook that blocks writes containing forbidden patterns
        let mut forbidden_patterns: Vec<String> = Vec::new();
        for (forbidden, _) in rules {
            for pat in forbidden.split(',') {
                let pat = pat.trim();
                if !pat.is_empty() {
                    // Convert pattern to grep-able regex
                    let grep_pat = pat.replace("::", "::").replace("*", "");
                    forbidden_patterns.push(grep_pat);
                }
            }
        }

        if forbidden_patterns.is_empty() {
            continue;
        }

        // Build the scope match pattern for the hook
        let scope_grep = scope.replace('*', ".*").replace(',', "|");
        let patterns_grep = forbidden_patterns
            .iter()
            .map(|p| format!("\"{}\"", p.replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(" ");

        let hook_body = format!(
            "#!/usr/bin/env bash\n\
             # Auto-generated from [lints.forbidden-imports]: scope: {scope}\n\
             set -uo pipefail\n\
             __INPUT=$(cat)\n\
             {{{{HOOK_HELPERS}}}}\n\
             FILE_PATH=$(_extract \"file_path\")\n\
             COMMAND=\"\"\n\
             _scope_or_allow\n\
             TARGET=\"$FILE_PATH\"\n\
             CONTENT=$(_extract \"new_string\")\n\
             if [[ -z \"$CONTENT\" ]]; then CONTENT=$(_extract \"content\"); fi\n\
             # Only check crates matching scope\n\
             if ! echo \"$TARGET\" | grep -qE \"{mock_rel}/crates/({scope_grep})/\"; then allow; fi\n\
             # Check for forbidden patterns\n\
             for PAT in {patterns_grep}; do\n\
                 if echo \"$CONTENT\" | grep -qF \"$PAT\"; then\n\
                     deny \"Forbidden import/type '$PAT' in scope '{scope}'. See lint config.\"\n\
                 fi\n\
             done\n"
        );

        let hook_name = format!("lint-forbidden-{slug}.sh");

        let repo_root_str = cfg.repo_root.to_string_lossy();
        let claude_hook = hook_body
            .replace("{{HOOK_HELPERS}}", CLAUDE_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", &repo_root_str);
        let copilot_hook = hook_body
            .replace("{{HOOK_HELPERS}}", COPILOT_HOOK_HELPERS)
            .replace("{{REPO_ROOT}}", &repo_root_str);

        let claude_hook_path = claude_hooks_dir.join(&hook_name);
        fs::write(&claude_hook_path, &claude_hook).ok();
        #[cfg(unix)]
        set_executable(&claude_hook_path);

        let copilot_hook_path = copilot_hooks_dir.join(&hook_name);
        fs::write(&copilot_hook_path, &copilot_hook).ok();
        #[cfg(unix)]
        set_executable(&copilot_hook_path);
        count += 2;
    }

    // Sweep orphan rendered files. When a scope is removed from
    // mockspace.toml (e.g. after a crate rename), the previously-rendered
    // lint-forbidden-<oldslug>.* files persist on disk because the render
    // pipeline is additive. Walk the four target directories, detect
    // files whose slug is no longer in active_slugs, delete them.
    const P: &str = "lint-forbidden-";
    sweep_orphan_generated_files(claude_rules_dir, P, ".md", &active_slugs);
    sweep_orphan_generated_files(
        copilot_instructions_dir,
        P,
        ".instructions.md",
        &active_slugs,
    );
    sweep_orphan_generated_files(claude_hooks_dir, P, ".sh", &active_slugs);
    sweep_orphan_generated_files(copilot_hooks_dir, P, ".sh", &active_slugs);

    count
}

/// Delete any file in `dir` whose name matches `<prefix><slug><suffix>` where
/// the slug is not in `active`. Conservative: only touches files carrying the
/// exact generated prefix and the given suffix, so a consumer-authored rule
/// that happens to sit in the same directory is never a candidate.
pub(crate) fn sweep_orphan_generated_files(
    dir: &Path,
    prefix: &str,
    suffix: &str,
    active: &std::collections::BTreeSet<String>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let Some(rest) = name.strip_prefix(prefix) else { continue };
        let Some(slug) = rest.strip_suffix(suffix) else { continue };
        if active.contains(slug) {
            continue;
        }
        let _ = fs::remove_file(entry.path());
        eprintln!("  removed orphan: {}", entry.path().display());
    }
}

/// Generate one path-scoped agent rule per crate, carrying that crate's
/// `README.md.tmpl`.
///
/// The per-crate doc set splits by depth on purpose: `README.md.tmpl` is the
/// short summary of what the crate is, `DESIGN.md.tmpl` is the long form, and
/// `DEEPDIVE_*.md.tmpl` are the internals. The README is therefore already
/// exactly the right size and shape for an always-loaded rule, so anyone
/// working inside `crates/<name>/` picks up that crate's own summary
/// implicitly, without a second hand-written copy that would drift from it.
///
/// Deriving it means the README stays the single source: edit the README and
/// the rule follows on the next render. A crate with no README simply gets no
/// rule.
/// How a rule under `.claude/rules/` reaches the docs directory.
///
/// Two levels up to the repository root, then down. Computed rather than
/// hardcoded so a project that puts its docs elsewhere still gets working
/// links.
pub(crate) fn docs_link_prefix(cfg: &Config) -> String {
    // A docs directory outside the repository has no relative path from a rule
    // inside it. Producing `../../Users/...` would be a link that resolves on
    // one machine, so the prefix is dropped and the link points at a bare
    // filename, which is at least honestly wrong rather than convincingly so.
    let Ok(rel) = cfg.docs_dir.strip_prefix(&cfg.repo_root) else {
        return String::new();
    };
    format!("../../{}/", rel.to_string_lossy().replace('\\', "/"))
}

/// Resolve a consumer template's references for output that lands outside the
/// docs directory.
///
/// Every agent-facing template is a consumer template: it carries the same
/// placeholders and the same registry references the documents do. Five of the
/// six paths that render one resolved placeholders and not references, so a rule
/// an agent reads carried a literal reference where the document it mirrors
/// carried the law. Routing them all through here is what stops the sixth from
/// being written the same way.
pub(crate) fn render_agent_body(
    raw: &str,
    vars: &[(String, String)],
    cfg: &Config,
    registry: &crate::registry::Registry,
) -> String {
    let mut link_cfg: Config = cfg.clone();
    link_cfg.doc_link_prefix = docs_link_prefix(cfg);
    crate::registry::resolve_all(
        &substitute_vars(raw, vars),
        &cfg.registry_namespaces,
        registry,
        &cfg.registry_roots,
        &cfg.repo_root,
        &cfg.docs_dir,
        &link_cfg,
    )
}

pub(crate) fn generate_crate_readme_rules(
    cfg: &Config,
    claude_rules_dir: &Path,
    copilot_instructions_dir: &Path,
    vars: &[(String, String)],
    header_md: &str,
    preamble: &str,
    postamble: &str,
    registry: &crate::registry::Registry,
) -> usize {
    use std::collections::BTreeSet as Set;

    let crates_dir = cfg.mock_dir.join("crates");

    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .replace('\\', "/");

    // An unreadable or absent crates/ yields no crates, and therefore an empty
    // active set. It must NOT return early: the sweep below still has to run,
    // or removing the last crate would strand every rule it had written.
    let mut entries: Vec<_> = fs::read_dir(&crates_dir)
        .map(|e| {
            e.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());

    let mut count = 0;
    let mut active: Set<String> = Set::new();

    for entry in entries {
        let crate_name = entry.file_name().to_string_lossy().to_string();
        let readme = entry.path().join("README.md.tmpl");
        let Ok(raw) = fs::read_to_string(&readme) else { continue };
        if raw.trim().is_empty() {
            continue;
        }

        // The README is a consumer template, so it carries the same `{{...}}`
        // placeholders every other consumer template does, and the same
        // registry references. Resolving them here is what stops a rule an
        // agent reads from carrying a literal `{{ law::keys }}` where the
        // document it mirrors carries the law.
        //
        // Links are prefixed, because a rule lives under `.claude/` while the
        // documents it links to live under `docs/`, so a bare filename would
        // point at a sibling that is not there.
        let body = render_agent_body(&raw, vars, cfg, registry);
        let apply_to = vec![format!("{mock_rel}/crates/{crate_name}/**")];

        active.insert(crate_name.clone());
        let rule_name = format!("crate-readme-{crate_name}");

        let claude_content = format!(
            "{}\n{}",
            format_claude_paths(&apply_to),
            format_with_bookends(header_md, preamble, &body, postamble)
        );
        let claude_path = claude_rules_dir.join(format!("{rule_name}.md"));
        render_design::write_generated(&claude_path, &claude_content);
        eprintln!("  {} (crate readme)", claude_path.display());
        count += 1;

        let copilot_content = format!(
            "{}\n{}",
            format_copilot_apply_to(&apply_to),
            format_with_bookends(header_md, preamble, &body, postamble)
        );
        let copilot_path = copilot_instructions_dir.join(format!("{rule_name}.instructions.md"));
        render_design::write_generated(&copilot_path, &copilot_content);
        eprintln!("  {} (crate readme)", copilot_path.display());
        count += 1;
    }

    // A renamed or deleted crate leaves its rule behind otherwise: the render
    // pipeline is additive.
    const P: &str = "crate-readme-";
    sweep_orphan_generated_files(claude_rules_dir, P, ".md", &active);
    sweep_orphan_generated_files(copilot_instructions_dir, P, ".instructions.md", &active);

    count
}
