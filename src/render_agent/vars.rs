//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

#![allow(unused_imports)]

use super::*;

/// Collect consumer rule template names (stems without .md.tmpl extension).
pub(crate) fn collect_consumer_rule_names(agent_dir: &Path) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let rules_dir = agent_dir.join("rules");
    if rules_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&rules_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().map(|x| x == "tmpl").unwrap_or(false) {
                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy().trim_end_matches(".md").to_string();
                        names.insert(name);
                    }
                }
            }
        }
    }
    names
}

/// Collect consumer skill directory names.
pub(crate) fn collect_consumer_skill_names(agent_dir: &Path) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    let skills_dir = agent_dir.join("skills");
    if skills_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&skills_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() && path.join("SKILL.md.tmpl").is_file() {
                    if let Some(name) = path.file_name() {
                        names.insert(name.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    names
}

/// Compute template variables from parsed crate data.
pub(crate) fn compute_template_vars(crates: &CrateMap, cfg: &Config) -> Vec<(String, String)> {
    let mut vars = Vec::new();

    // {{project_name}}
    vars.push(("project_name".to_string(), cfg.project_name.clone()));

    // {{mock_dir}}: relative path from repo root to mock workspace
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .unwrap_or(&cfg.mock_dir)
        .to_string_lossy()
        .to_string();
    vars.push(("mock_dir".to_string(), mock_rel));

    // {{crate_count}}
    let crate_count = crates
        .values()
        .filter(|c| c.short_name != cfg.project_name)
        .count();
    vars.push(("crate_count".to_string(), crate_count.to_string()));

    // {{crate_table}}
    vars.push(("crate_table".to_string(), compute_crate_table(crates, cfg)));

    // {{macro_table}}
    vars.push(("macro_table".to_string(), compute_macro_table(crates, cfg)));

    vars
}

/// Build a markdown table of crates.
pub(crate) fn compute_crate_table(crates: &CrateMap, cfg: &Config) -> String {
    let mut table = String::new();
    writeln!(table, "| Crate | Purpose |").unwrap();
    writeln!(table, "|-------|---------|").unwrap();

    let mut sorted: Vec<_> = crates.iter().collect();
    sorted.sort_by_key(|(_, c)| &c.short_name);

    let prefix = &cfg.crate_prefix;
    let prefix_dash = format!("{prefix}-");
    for (dir_name, info) in sorted {
        if info.short_name == cfg.project_name {
            continue;
        }
        let display_name = if dir_name.starts_with(&prefix_dash) {
            format!("{prefix}-{}", info.short_name)
        } else {
            // crate name doesn't have the workspace prefix: use as-is
            dir_name.clone()
        };
        let purpose = infer_crate_purpose(info);
        writeln!(table, "| {display_name} | {purpose} |").unwrap();
    }

    table.trim_end().to_string()
}

/// Infer a one-line purpose from a crate's items.
pub(crate) fn infer_crate_purpose(info: &CrateInfo) -> String {
    let mut types: Vec<String> = Vec::new();
    for item in &info.items {
        match item {
            Item::Trait(t) => types.push(t.name.clone()),
            Item::Struct(s) => types.push(s.name.clone()),
            _ => {},
        }
    }
    if types.is_empty() {
        return format!("{} subsystem", info.short_name);
    }
    let max = 3.min(types.len());
    let shown: Vec<_> = types[.. max].iter().map(|t| format!("`{t}`")).collect();
    let suffix = if types.len() > max {
        format!(" +{}", types.len() - max)
    } else {
        String::new()
    };
    format!("{}{suffix}", shown.join(", "))
}

/// Build the macros table for agent instructions from config.
pub(crate) fn compute_macro_table(crates: &CrateMap, cfg: &Config) -> String {
    let macros = cfg.effective_agent_macros();

    // Check which macros actually exist
    let mut found = std::collections::BTreeSet::new();
    for info in crates.values() {
        for item in &info.items {
            if let Item::Macro(m) = item {
                if m.name.starts_with("define_") {
                    found.insert(m.name.clone());
                }
            }
        }
    }

    let mut table = String::new();
    writeln!(table, "| Macro | Purpose | Usage |").unwrap();
    writeln!(table, "|-------|---------|-------|").unwrap();

    for (name, purpose, usage) in macros {
        if found.contains(name.as_str()) {
            writeln!(table, "| `{name}!` | {purpose} | {usage} |").unwrap();
        }
    }

    // Any extra macros not in config
    for name in &found {
        if !macros.iter().any(|(n, ..)| n == name) {
            writeln!(table, "| `{name}!` | Custom | `{name}!(...)` |").unwrap();
        }
    }

    table.trim_end().to_string()
}

/// Read a template file and substitute variables.
pub(crate) fn read_and_substitute(path: &Path, vars: &[(String, String)]) -> String {
    let raw = fs::read_to_string(path).expect("failed to read template");
    let (_frontmatter, body) = split_frontmatter(&raw);
    substitute_vars(&body, vars)
}

/// Replace {{var}} placeholders in text.
pub(crate) fn substitute_vars(text: &str, vars: &[(String, String)]) -> String {
    let mut result = text.to_string();
    for (key, value) in vars {
        result = result.replace(&format!("{{{{{key}}}}}"), value);
    }
    result
}

/// Assemble one `.claude/agents/<name>.md` from an already-substituted
/// template's frontmatter and body.
///
/// The frontmatter is emitted verbatim. Claude Code owns that schema, so
/// mockspace re-encoding it would drop any field it does not enumerate; passing
/// it through means an upstream addition works without a mockspace change.
///
/// Bookends are deliberately not applied. A persona is a character definition
/// read as a whole, and wrapping it in the workspace preamble and postamble
/// would dilute the thing that makes it worth having.
pub(crate) fn render_agent_content(frontmatter: &str, body: &str) -> String {
    // `split_frontmatter` returns the body starting AT the newline that ends the
    // closing `---` line, so the separator is already present. Adding another
    // would insert a blank line into every persona on every regeneration.
    let sep = if body.starts_with('\n') { "" } else { "\n" };
    format!("---\n{}\n---{sep}{body}", frontmatter.trim())
}

/// Split YAML frontmatter from body content.
pub(crate) fn split_frontmatter(text: &str) -> (String, String) {
    let trimmed = text.trim_start();
    if !trimmed.starts_with("---") {
        return (String::new(), text.to_string());
    }

    let after_first = &trimmed[3 ..];
    if let Some(end) = after_first.find("\n---") {
        let fm = after_first[.. end].to_string();
        let body = after_first[end + 4 ..].to_string();
        (fm, body)
    } else {
        (String::new(), text.to_string())
    }
}

/// Parse apply_to field from frontmatter YAML.
pub(crate) fn parse_apply_to(frontmatter: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_list = false;

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("apply_to:") {
            let value = trimmed.trim_start_matches("apply_to:").trim();
            if value.starts_with('[') {
                // Inline array: ["pattern1", "pattern2"]
                let inner = value.trim_start_matches('[').trim_end_matches(']');
                for item in inner.split(',') {
                    let p = item.trim().trim_matches('"').trim_matches('\'');
                    if !p.is_empty() {
                        patterns.push(p.to_string());
                    }
                }
            } else if value.is_empty() {
                in_list = true;
            } else {
                patterns.push(value.trim_matches('"').trim_matches('\'').to_string());
            }
        } else if in_list && trimmed.starts_with("- ") {
            let p = trimmed[2 ..].trim().trim_matches('"').trim_matches('\'');
            if !p.is_empty() {
                patterns.push(p.to_string());
            }
        } else if in_list && !trimmed.is_empty() && !trimmed.starts_with('#') {
            in_list = false;
        }
    }

    patterns
}

/// Parse a simple key: value field from frontmatter.
pub(crate) fn parse_field(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&prefix) {
            let value = trimmed[prefix.len() ..].trim();
            return Some(value.trim_matches('"').trim_matches('\'').to_string());
        }
    }
    None
}

/// Format glob patterns as Claude's paths: YAML list.
pub(crate) fn format_claude_paths(patterns: &[String]) -> String {
    if patterns.is_empty() {
        return String::new();
    }
    let mut fm = String::from("---\npaths:\n");
    for p in patterns {
        writeln!(fm, "  - \"{p}\"").unwrap();
    }
    fm.push_str("---");
    fm
}

/// Format glob patterns as Copilot's applyTo: string.
pub(crate) fn format_copilot_apply_to(patterns: &[String]) -> String {
    if patterns.is_empty() {
        return String::new();
    }
    let joined = patterns.join(",");
    format!("---\napplyTo: \"{joined}\"\n---")
}

/// Read an optional template file, returning empty string if missing.
pub(crate) fn read_optional_template(path: &Path) -> String {
    if path.is_file() {
        let raw = fs::read_to_string(path).unwrap_or_default();
        let (_fm, body) = split_frontmatter(&raw);
        body.trim().to_string()
    } else {
        String::new()
    }
}
