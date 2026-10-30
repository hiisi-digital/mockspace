//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

/// The lint the generated forbidden-imports hook is named after and asks.
const FORBIDDEN_IMPORTS_LINT: &str = "forbidden-imports";

/// Generate agent rules and hooks derived from lint configuration.
///
/// For each `forbidden-imports` rule the repository declares (the hook does not look at
/// the lint's severity; the engine answers for that on every edit), generates:
/// - An agent rule file per scope (tells the agent what's forbidden)
/// - One agent hook, named after the lint, that asks the lint about each edit
///
/// Returns how many files were written and the metadata of each hook, which the
/// settings step needs to register it under the tools it guards. A hook written
/// but not returned sits in `hooks/` under no event and nothing ever runs it.
pub(crate) fn generate_lint_derived_content(
    cfg: &Config,
    claude_rules_dir: &Path,
    copilot_instructions_dir: &Path,
    claude_hooks_dir: &Path,
    copilot_hooks_dir: &Path,
    header_md: &str,
    preamble: &str,
    postamble: &str,
) -> (usize, Vec<HookMeta>) {
    use std::collections::{BTreeMap as Map, BTreeSet as Set};

    let mut count = 0;
    let mut hook_metas: Vec<HookMeta> = Vec::new();

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
    // name -> (scope, forbidden, reason, disabled). A rule naming no scope applies to every
    // crate, as the lint reads it (`*`), so it gets the hook and a rule file like any other.
    let mut rules_by_name: Map<String, (String, String, String, bool)> = Map::new();

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
            .or_insert_with(|| ("*".to_string(), String::new(), String::new(), false));
        match field {
            "scope" => entry.0 = value.clone(),
            "forbidden" => entry.1 = value.clone(),
            "reason" => entry.2 = value.clone(),
            "enabled" if value == "false" => entry.3 = true,
            _ => {},
        }
    }

    // Group by scope
    let mut by_scope: Map<String, Vec<(String, String)>> = Map::new(); // scope -> [(forbidden, reason)]
    for (_name, (scope, forbidden, reason, disabled)) in &rules_by_name {
        if *disabled || scope.is_empty() || forbidden.is_empty() {
            continue;
        }
        by_scope
            .entry(scope.clone())
            .or_default()
            .push((forbidden.clone(), reason.clone()));
    }

    // The per-scope hooks an earlier render wrote are gone either way, and a
    // render with no rules left would otherwise leave them registered under no
    // lint that still stands behind them.
    for dir in [claude_hooks_dir, copilot_hooks_dir] {
        sweep_orphan_generated_files(dir, "lint-forbidden-", ".sh", &Set::new());
    }

    if by_scope.is_empty() {
        // With no rule left the hook has nothing to ask about. Only one this
        // renderer wrote is taken: a hook of the same name some other hand wrote
        // is not ours to remove.
        for dir in [claude_hooks_dir, copilot_hooks_dir] {
            let path = dir.join(format!("{FORBIDDEN_IMPORTS_LINT}.sh"));
            let ours = fs::read_to_string(&path)
                .is_ok_and(|t| t.contains(&format!("Auto-generated: the `{FORBIDDEN_IMPORTS_LINT}` lint")));
            if ours {
                let _ = fs::remove_file(&path);
            }
        }
        return (0, Vec::new());
    }

    // The directories are made here so the writes below do not depend on the order of
    // the render phases, and a failure to make one stops the render instead of
    // being dropped.
    for dir in [claude_rules_dir, copilot_instructions_dir, claude_hooks_dir, copilot_hooks_dir] {
        fs::create_dir_all(dir).expect("failed to create a hooks or rules directory");
    }

    let src_dirs_repo = src_dirs_from_repo(cfg);
    for (scope, rules) in &by_scope {
        let apply_to_globs: Vec<String> = src_dirs_repo
            .iter()
            .map(|d| format!("{d}/{scope}/**"))
            .collect();
        let apply_to_list = apply_to_globs
            .iter()
            .map(|g| format!("\"{g}\""))
            .collect::<Vec<_>>()
            .join(", ");

        // the scope is a glob and this becomes a filename, so the characters
        // a filename cannot carry are spelled out rather than dropped.
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

        let claude_fm = format!("---\napply_to: [{apply_to_list}]\n---\n\n");
        let copilot_fm = format!("---\napplyTo: [{apply_to_list}]\n---\n\n");

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

    // One hook, named after the lint, replaces the per-scope hooks the slugs
    // named: the lint holds the scopes itself, so a hook per scope ran the whole
    // lint once for each of them.
    let hook_name = format!("{FORBIDDEN_IMPORTS_LINT}.sh");
    let launcher_lookup = super::helpers::find_launcher(
        "the forbidden-imports gate cannot run: no mockspace launcher on PATH. The verdict is the lint's and the engine gives it, so guessing a weaker rule here would contradict it. Install with: cargo install cargo-mock",
    );
    let hook_body = format!(
        "#!/usr/bin/env bash\n\
         # Auto-generated: the `{FORBIDDEN_IMPORTS_LINT}` lint, asked about this edit.\n\
         # The verdict is the lint's, from `mock check-edit`; nothing here judges the text.\n\
         set -uo pipefail\n\
         __INPUT=$(cat)\n\
         {{{{HOOK_HELPERS}}}}\n\
         command -v jq >/dev/null 2>&1 \\\n\
             || deny \"the forbidden-imports gate cannot run: jq is not installed, so the edit cannot be read and a verdict would be a guess.\"\n\
         FILE_PATH=$(_extract \"file_path\")\n\
         COMMAND=\"\"\n\
         _scope_or_allow\n\
         case \"$FILE_PATH\" in *.rs) ;; *) allow ;; esac\n\
         # A field goes to a file byte for byte. `$(...)` strips trailing newlines, and an\n\
         # old_string that is only newlines, or a new_string ending in one, would then name a\n\
         # different file than the one the edit makes. `_extract` ends with the one newline\n\
         # jq adds, which is the only one removed.\n\
         _extract_to() {{\n\
             local v\n\
             v=$(_extract \"$1\"; printf x)\n\
             v=${{v%x}}\n\
             printf '%s' \"${{v%$'\\n'}}\" > \"$2\"\n\
         }}\n\
         {launcher_lookup}\
         [ -d \"$__HOOK_REPO_ROOT\" ] \\\n\
             || deny \"the forbidden-imports gate cannot run: its repository $__HOOK_REPO_ROOT is not there. Regenerate the hooks with cargo mock from where the repository now is.\"\n\
         WORK=$(mktemp -d) || deny \"the forbidden-imports gate cannot run: no temporary directory.\"\n\
         trap 'rm -rf \"$WORK\"' EXIT\n\
         _extract_to old_string \"$WORK/old\"\n\
         _extract_to new_string \"$WORK/new\"\n\
         if [[ ! -s \"$WORK/old\" && ! -s \"$WORK/new\" ]]; then _extract_to content \"$WORK/new\"; fi\n\
         ARGS=(check-edit --lint {FORBIDDEN_IMPORTS_LINT} --file \"$FILE_PATH\")\n\
         if [[ -s \"$WORK/old\" ]]; then\n\
             ARGS+=(--replace \"$WORK/old\")\n\
             if [[ \"$(_extract replace_all)\" == \"true\" ]]; then ARGS+=(--all); fi\n\
         fi\n\
         _OUT=$(cd \"$__HOOK_REPO_ROOT\" 2>/dev/null && \"$launcher\" \"${{ARGS[@]}}\" < \"$WORK/new\" 2>&1)\n\
         _RC=$?\n\
         if [ \"$_RC\" -eq 0 ]; then allow; fi\n\
         deny \"$_OUT\"\n"
    );
    let repo_root_str = cfg.repo_root.to_string_lossy();
    for (hooks_dir, helpers, who) in [
        (claude_hooks_dir, CLAUDE_HOOK_HELPERS, "claude"),
        (copilot_hooks_dir, COPILOT_HOOK_HELPERS, "copilot"),
    ] {
        let marked = super::wire::with_generated_marker(
            &hook_body
                .replace("{{HOOK_HELPERS}}", helpers)
                .replace("{{REPO_ROOT}}", &repo_root_str),
        );
        let hook_path = hooks_dir.join(&hook_name);
        write_executable(&hook_path, &marked)
            .unwrap_or_else(|e| panic!("failed to write forbidden-imports {who} hook: {e}"));
        count += 1;
    }
    // It reads the text being written, so it guards the tools that write, and
    // Bash is not one of them.
    hook_metas.push(HookMeta {
        name:     hook_name,
        matchers: vec!["Write".into(), "Edit".into()],
    });

    (count, hook_metas)
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
/// working inside that crate's directory picks up that crate's own summary
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

    // An unreadable or absent source directory yields no crates, and therefore an empty
    // active set. It must NOT return early: the sweep below still has to run,
    // or removing the last crate would strand every rule it had written.
    // Every source directory: a grouped project would otherwise write per-crate
    // agent rules for one group, and the sweep below would then strand the rules
    // of every crate outside it.
    let entries = crate::parse::package_dirs_in(&cfg.src_dirs);

    let mut count = 0;
    let mut active: Set<String> = Set::new();

    for entry in entries {
        let crate_name = entry
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let readme = entry.join("README.md.tmpl");
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
        let apply_to = vec![format!("{}/**", repo_relative(cfg, &entry))];

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
