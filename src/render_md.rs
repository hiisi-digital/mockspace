use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::graph;
use crate::model::*;

/// Generate STRUCTURE.md from parsed crate data + README.md contents.
pub fn generate_structure_md(crates: &CrateMap, cfg: &Config) -> String {
    let mut md = String::new();
    let project_name = &cfg.project_name;

    // Compute depths and transitive reduction
    let mut depth_cache = BTreeMap::new();
    let mut depths = BTreeMap::new();
    for name in crates.keys() {
        depths.insert(name.clone(), graph::compute_depth(name, crates, &mut depth_cache));
    }
    let reduced = graph::transitive_reduction(crates);

    // Header
    writeln!(md, "# {project_name} — Structure Reference").unwrap();
    writeln!(md).unwrap();
    writeln!(md, "> Auto-generated from the mock workspace. This document is the canonical").unwrap();
    writeln!(md, "> description of every crate, type, and macro in the framework.").unwrap();
    writeln!(md).unwrap();
    writeln!(md, "See also: [STRUCTURE.GRAPH.svg](STRUCTURE.GRAPH.svg) for the visual dependency graph.").unwrap();
    writeln!(md).unwrap();

    // Table of contents
    writeln!(md, "## Table of Contents").unwrap();
    writeln!(md).unwrap();

    // Group by depth
    let max_depth = depths.values().copied().max().unwrap_or(0);
    let mut by_depth: Vec<Vec<&str>> = vec![Vec::new(); max_depth + 1];
    for (name, &d) in &depths {
        if crates[name.as_str()].short_name == *project_name {
            continue;
        }
        by_depth[d].push(name.as_str());
    }

    for (d, names) in by_depth.iter().enumerate() {
        if names.is_empty() { continue; }
        let label = cfg.layer_label(d);
        writeln!(md, "**Layer {d} — {label}**").unwrap();
        writeln!(md).unwrap();
        for name in names {
            let short = &crates[*name].short_name;
            writeln!(md, "- [{short}](#{short})").unwrap();
        }
        writeln!(md).unwrap();
    }

    writeln!(md, "---").unwrap();
    writeln!(md).unwrap();

    // Each crate section
    for (d, names) in by_depth.iter().enumerate() {
        if names.is_empty() { continue; }
        let label = cfg.layer_label(d);
        writeln!(md, "## Layer {d} — {label}").unwrap();
        writeln!(md).unwrap();

        for dir_name in names {
            let info = &crates[*dir_name];
            write_crate_section(&mut md, dir_name, info, &depths, &reduced, crates, &cfg.crates_dir, project_name, cfg);
        }
    }

    md
}

fn write_crate_section(
    md: &mut String,
    dir_name: &str,
    info: &CrateInfo,
    depths: &BTreeMap<String, usize>,
    reduced: &BTreeMap<String, Vec<String>>,
    crates: &CrateMap,
    crates_dir: &Path,
    project_name: &str,
    cfg: &Config,
) {
    let short = &info.short_name;
    let depth = depths.get(dir_name).copied().unwrap_or(0);

    writeln!(md, "### {short}").unwrap();
    writeln!(md).unwrap();
    writeln!(md, "`{dir_name}` · depth {depth}").unwrap();
    writeln!(md).unwrap();

    // Include README.md content
    let readme_path = crates_dir.join(dir_name).join("README.md");
    if let Ok(readme) = fs::read_to_string(&readme_path) {
        let content: String = readme
            .lines()
            .skip_while(|l| l.starts_with('#') || l.is_empty())
            .map(|l| {
                if l.starts_with('#') {
                    format!("###{l}")
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        if !content.trim().is_empty() {
            writeln!(md, "{}", content.trim()).unwrap();
            writeln!(md).unwrap();
        }
    }

    // Dependencies table
    let direct_deps: Vec<&str> = info.deps.iter()
        .filter(|d| crates.get(d.as_str()).map(|c| c.short_name != project_name).unwrap_or(false))
        .map(|d| d.as_str())
        .collect();
    let reduced_deps: Vec<&str> = reduced.get(dir_name)
        .map(|v| v.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    if !direct_deps.is_empty() {
        writeln!(md, "#### Dependencies").unwrap();
        writeln!(md).unwrap();
        writeln!(md, "| Dependency | Direct | Transitive-reduced |").unwrap();
        writeln!(md, "|------------|--------|--------------------|").unwrap();
        for dep in &direct_deps {
            let dep_short = crates.get(*dep).map(|c| c.short_name.as_str()).unwrap_or(dep);
            let is_reduced = reduced_deps.contains(dep);
            let direct = "✓";
            let reduced_mark = if is_reduced { "✓ (edge drawn)" } else { "— (transitively implied)" };
            writeln!(md, "| {dep_short} | {direct} | {reduced_mark} |").unwrap();
        }
        writeln!(md).unwrap();
    }

    // Dependees
    let dependees: Vec<&str> = crates.iter()
        .filter(|(n, c)| c.short_name != project_name && c.deps.iter().any(|d| d == dir_name) && *n != dir_name)
        .map(|(_, c)| c.short_name.as_str())
        .collect();
    if !dependees.is_empty() {
        writeln!(md, "**Depended on by:** {}", dependees.join(", ")).unwrap();
        writeln!(md).unwrap();
    }

    // Build lookup: item_name -> MacroGenerated info
    let gen_lookup: BTreeMap<&str, &MacroGenerated> = info.macro_generated.iter()
        .map(|mg| (mg.generated_name.as_str(), mg))
        .collect();

    // Domain items (macro-generated)
    let domain_items: Vec<(&Item, &MacroGenerated)> = info.items.iter()
        .filter_map(|item| gen_lookup.get(item.name()).map(|mg| (item, *mg)))
        .collect();
    let extra_domain: Vec<&MacroGenerated> = info.macro_generated.iter()
        .filter(|mg| !info.items.iter().any(|i| i.name() == mg.generated_name))
        .collect();

    if !domain_items.is_empty() || !extra_domain.is_empty() {
        writeln!(md, "#### Domain Items").unwrap();
        writeln!(md).unwrap();
        writeln!(md, "| Kind | Name | Via Macro |").unwrap();
        writeln!(md, "|------|------|-----------|").unwrap();
        for (item, mg) in &domain_items {
            let kind = cfg.domain_kind(&mg.macro_name);
            writeln!(md, "| {kind} | `{}` | `{}!` |", item.name(), mg.macro_name).unwrap();
        }
        for mg in &extra_domain {
            let kind = cfg.domain_kind(&mg.macro_name);
            writeln!(md, "| {kind} | `{}` | `{}!` |", mg.generated_name, mg.macro_name).unwrap();
        }
        writeln!(md).unwrap();
    }

    // Raw Rust items (not macro-generated), split by visibility
    let raw_items: Vec<&Item> = info.items.iter()
        .filter(|item| !gen_lookup.contains_key(item.name()))
        .collect();

    let public_items: Vec<&&Item> = raw_items.iter()
        .filter(|i| i.visibility() == ApiVisibility::Public || i.visibility() == ApiVisibility::Unspecified)
        .collect();
    let internal_items: Vec<&&Item> = raw_items.iter()
        .filter(|i| i.visibility() == ApiVisibility::Internal)
        .collect();

    if !public_items.is_empty() {
        writeln!(md, "#### Public API").unwrap();
        writeln!(md).unwrap();
        render_item_list(md, &public_items);
    }

    if !internal_items.is_empty() {
        writeln!(md, "#### Internal API").unwrap();
        writeln!(md).unwrap();
        render_item_list(md, &internal_items);
    }

    writeln!(md, "---").unwrap();
    writeln!(md).unwrap();
}

fn render_item_list(md: &mut String, items: &[&&Item]) {
    let macros: Vec<_> = items.iter().filter(|i| matches!(i, Item::Macro(_))).collect();
    let traits: Vec<_> = items.iter().filter(|i| matches!(i, Item::Trait(_))).collect();
    let structs: Vec<_> = items.iter().filter(|i| matches!(i, Item::Struct(_))).collect();
    let enums: Vec<_> = items.iter().filter(|i| matches!(i, Item::Enum(_))).collect();
    let fns: Vec<_> = items.iter().filter(|i| matches!(i, Item::Fn(_))).collect();

    if !macros.is_empty() {
        for item in &macros {
            if let Item::Macro(m) = **item {
                let kind = if m.is_proc { "proc" } else { "declarative" };
                writeln!(md, "- `{}!` ({kind})", m.name).unwrap();
            }
        }
    }

    if !traits.is_empty() {
        for item in &traits {
            if let Item::Trait(t) = **item {
                let gen = if t.generics.is_empty() { String::new() } else { t.generics.clone() };
                let bounds = if t.bounds.is_empty() { String::new() } else { format!(": {}", t.bounds) };
                writeln!(md, "- `{}{gen}`{bounds}", t.name).unwrap();
                for m in &t.methods {
                    let params = if m.params.is_empty() { String::new() } else { m.params.clone() };
                    let ret = if m.ret.is_empty() { String::new() } else { format!(" → {}", m.ret) };
                    let gen2 = if m.generics.is_empty() { String::new() } else { m.generics.clone() };
                    writeln!(md, "  - `fn {}{gen2}({params}){ret}`", m.name).unwrap();
                }
            }
        }
    }

    if !structs.is_empty() {
        for item in &structs {
            if let Item::Struct(s) = **item {
                let gen = if s.generics.is_empty() { String::new() } else { s.generics.clone() };
                writeln!(md, "- `{}{gen}`", s.name).unwrap();
            }
        }
    }

    if !enums.is_empty() {
        for item in &enums {
            if let Item::Enum(e) = **item {
                writeln!(md, "- `{}`", e.name).unwrap();
            }
        }
    }

    if !fns.is_empty() {
        for item in &fns {
            if let Item::Fn(f) = **item {
                let params = if f.sig.params.is_empty() { String::new() } else { f.sig.params.clone() };
                let ret = if f.sig.ret.is_empty() { String::new() } else { format!(" → {}", f.sig.ret) };
                let gen = if f.sig.generics.is_empty() { String::new() } else { f.sig.generics.clone() };
                writeln!(md, "- `fn {}{gen}({params}){ret}`", f.sig.name).unwrap();
            }
        }
    }

    writeln!(md).unwrap();
}
