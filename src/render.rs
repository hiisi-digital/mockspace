use std::collections::BTreeMap;
use std::fmt::Write;

use crate::graph;
use crate::model::*;

fn header_colors(short: &str) -> (&'static str, &'static str) {
    match short {
        "id" => ("#E1F5FE", "#0277BD"),
        "error" => ("#FCE4EC", "#C62828"),
        "storage" | "registry" | "reflect" => ("#E8EEF7", "#3A6EA5"),
        "macros" => ("#F5E6EC", "#9B2335"),
        "tree" | "resource" | "signal" => ("#E6F2E6", "#2D6A2D"),
        "behavior" => ("#FDF0E0", "#C05C00"),
        "scheduler" | "render" => ("#EDE6F3", "#5C2D82"),
        "input" | "focus" | "plugin" => ("#FDF8E0", "#B8860B"),
        "scope" => ("#F1F8E9", "#558B2F"),
        "action" => ("#FFF8E1", "#F57F17"),
        "runtime" => ("#F5E0E0", "#8B1A1A"),
        "gui" | "tui" => ("#E0F0F2", "#006060"),
        "testing" => ("#EAF2E0", "#3D6B1E"),
        _ => ("#F0F0F0", "#666666"),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn generate_dot(crates: &CrateMap, project_name: &str, crate_prefix: &str) -> String {
    let mut dot = String::new();
    let reduced = graph::transitive_reduction(crates);

    let mut depth_cache = BTreeMap::new();
    let mut depths = BTreeMap::new();
    for name in crates.keys() {
        depths.insert(name.clone(), graph::compute_depth(name, crates, &mut depth_cache));
    }
    let max_depth = depths.values().copied().max().unwrap_or(0);

    writeln!(dot, "digraph {project_name} {{").unwrap();
    writeln!(dot, "    rankdir=BT;").unwrap();
    writeln!(dot, "    newrank=true;").unwrap();
    writeln!(dot, "    bgcolor=\"#FAFAFA\";").unwrap();
    writeln!(dot, "    graph [fontname=\"Helvetica\", fontsize=10, pad=0.8, nodesep=0.4, ranksep=1.0];").unwrap();
    writeln!(dot, "    node [fontname=\"Helvetica\", fontsize=8];").unwrap();
    writeln!(dot, "    edge [fontname=\"Helvetica\", fontsize=7, color=\"#BBBBBB\", arrowsize=0.5, penwidth=0.8];").unwrap();
    writeln!(dot).unwrap();

    // Title in top-left
    writeln!(dot, "    labelloc=t;").unwrap();
    writeln!(dot, "    labeljust=l;").unwrap();
    writeln!(dot, "    label=<<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"0\"><TR><TD ALIGN=\"LEFT\"><FONT POINT-SIZE=\"18\" COLOR=\"#37474F\"><B>{project_name}</B></FONT></TD></TR><TR><TD ALIGN=\"LEFT\"><FONT POINT-SIZE=\"9\" COLOR=\"#999999\">crate architecture</FONT></TD></TR></TABLE>>;").unwrap();
    writeln!(dot).unwrap();

    // Crate nodes
    for (dir_name, info) in crates {
        if info.short_name == project_name {
            continue;
        }
        let cid = dir_name.replace('-', "_");
        let (bg, border) = header_colors(&info.short_name);
        let label = render_crate_node(&cid, info, bg, border);
        writeln!(dot, "    {cid} [shape=none, margin=0, label=<{label}>];").unwrap();
    }
    writeln!(dot).unwrap();

    // Rank grouping by depth.
    // Testing is forced to same rank as runtime (companion placement).
    let runtime_crate = format!("{crate_prefix}-runtime");
    let runtime_depth = depths.get(&runtime_crate).copied().unwrap_or(0);
    let mut by_depth: Vec<Vec<&str>> = vec![Vec::new(); max_depth + 1];
    for (name, &d) in &depths {
        if crates[name.as_str()].short_name == project_name {
            continue;
        }
        if crates[name.as_str()].short_name == "testing" {
            by_depth[runtime_depth].push(name.as_str());
        } else {
            by_depth[d].push(name.as_str());
        }
    }
    for (d, names) in by_depth.iter().enumerate() {
        if names.is_empty() {
            continue;
        }
        write!(dot, "    {{ rank=same; ").unwrap();
        for name in names {
            write!(dot, "{}; ", name.replace('-', "_")).unwrap();
        }
        writeln!(dot, "}} // depth {d}").unwrap();
    }
    writeln!(dot).unwrap();

    // Ordering constraint: put testing to the right of runtime
    writeln!(dot, "    // Force testing next to runtime").unwrap();
    let prefix_under = crate_prefix.replace('-', "_");
    writeln!(dot, "    {prefix_under}_runtime -> {prefix_under}_testing [style=invis, weight=10];").unwrap();
    writeln!(dot).unwrap();

    // Dependency edges (transitively reduced)
    writeln!(dot, "    // Dependency edges (transitively reduced)").unwrap();
    for (dir_name, deps) in &reduced {
        if crates[dir_name.as_str()].short_name == project_name {
            continue;
        }
        let from_cid = dir_name.replace('-', "_");
        for dep in deps {
            if crates[dep.as_str()].short_name == project_name {
                continue;
            }
            let to_cid = dep.replace('-', "_");
            writeln!(dot, "    {to_cid} -> {from_cid};").unwrap();
        }
    }
    writeln!(dot).unwrap();

    writeln!(dot, "}}").unwrap();
    dot
}

fn port_id(name: &str) -> String {
    format!("p_{name}")
}

// ---------------------------------------------------------------------------
// Crate node rendering
// ---------------------------------------------------------------------------

fn render_crate_node(_cid: &str, info: &CrateInfo, bg: &str, border: &str) -> String {
    // Build lookup: item_name -> MacroGenerated info
    let gen_lookup: BTreeMap<&str, &MacroGenerated> = info.macro_generated.iter()
        .map(|mg| (mg.generated_name.as_str(), mg))
        .collect();

    // Partition items: domain (macro-generated) vs raw Rust
    let mut domain_items: Vec<(&Item, &MacroGenerated)> = Vec::new();
    let mut raw_items: Vec<&Item> = Vec::new();
    for item in &info.items {
        if let Some(mg) = gen_lookup.get(item.name()) {
            domain_items.push((item, mg));
        } else {
            raw_items.push(item);
        }
    }
    // Also collect macro-generated items not in parsed items
    let mut extra_domain: Vec<&MacroGenerated> = Vec::new();
    for mg in &info.macro_generated {
        if !info.items.iter().any(|i| i.name() == mg.generated_name) {
            extra_domain.push(mg);
        }
    }

    let has_domain = !domain_items.is_empty() || !extra_domain.is_empty();
    let has_raw = !raw_items.is_empty();

    let mut h = String::new();
    write!(h, "<TABLE BORDER=\"2\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"3\" BGCOLOR=\"{bg}\" COLOR=\"{border}\" STYLE=\"ROUNDED\">").unwrap();

    // Crate header
    write!(h, "<TR><TD COLSPAN=\"2\" BGCOLOR=\"{border}\" ALIGN=\"CENTER\"><FONT COLOR=\"white\" POINT-SIZE=\"10\"><B>  {}  </B></FONT></TD></TR>", info.short_name).unwrap();

    if !has_domain && !has_raw {
        write!(h, "<TR><TD COLSPAN=\"2\"><FONT COLOR=\"#999999\" POINT-SIZE=\"7\"><I>(empty)</I></FONT></TD></TR>").unwrap();
    }

    // --- Domain items first (prominent) ---
    for (item, mg) in &domain_items {
        write_domain_row(&mut h, item, mg);
    }
    for mg in &extra_domain {
        let (kind_label, kind_icon, label_bg, label_fg) = domain_style(&mg.macro_name);
        write!(h, "<HR/>").unwrap();
        write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\" PORT=\"{}\"><FONT COLOR=\"{label_fg}\" POINT-SIZE=\"7\">{kind_label}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\"><FONT COLOR=\"{label_fg}\"><B>{kind_icon} {}</B></FONT></TD></TR>",
            port_id(&mg.generated_name), mg.generated_name).unwrap();
    }

    // --- Separator between domain and raw ---
    if has_domain && has_raw {
        write!(h, "<HR/>").unwrap();
        write!(h, "<TR><TD COLSPAN=\"2\" ALIGN=\"LEFT\"><FONT COLOR=\"#AAAAAA\" POINT-SIZE=\"5\">rust</FONT></TD></TR>").unwrap();
    }

    // --- Raw Rust items (compact, subdued) ---
    for item in &raw_items {
        write_raw_row(&mut h, item);
    }

    write!(h, "</TABLE>").unwrap();
    h
}

/// Domain item row: prominent, uses domain kind label + icon, shows generics/bounds.
fn write_domain_row(h: &mut String, item: &Item, mg: &MacroGenerated) {
    let (kind_label, kind_icon, label_bg, label_fg) = domain_style(&mg.macro_name);
    write!(h, "<HR/>").unwrap();

    match item {
        Item::Trait(t) => {
            let gen = if t.generics.is_empty() { String::new() } else { esc(&t.generics) };
            let bounds_s = if t.bounds.is_empty() {
                String::new()
            } else {
                format!(" <FONT COLOR=\"#999999\" POINT-SIZE=\"5\">{}</FONT>", esc(&t.bounds))
            };
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\"><FONT COLOR=\"{label_fg}\" POINT-SIZE=\"7\">{kind_label}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\" PORT=\"{}\"><FONT COLOR=\"{label_fg}\"><B>{kind_icon} {}{gen}</B></FONT>{bounds_s}</TD></TR>", port_id(&t.name), t.name).unwrap();
            // Show associated types from methods (signatures only, no fields)
            for m in &t.methods {
                let p = esc(&m.params);
                let r = if m.ret.is_empty() { String::new() } else { format!(" → {}", esc(&m.ret)) };
                let g = esc(&m.generics);
                write!(h, "<TR><TD></TD><TD ALIGN=\"LEFT\"><FONT COLOR=\"{label_fg}\" POINT-SIZE=\"5\">ƒ {}{g}({p}){r}</FONT></TD></TR>", m.name).unwrap();
            }
        }
        Item::Struct(s) => {
            let gen = if s.generics.is_empty() { String::new() } else { esc(&s.generics) };
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\"><FONT COLOR=\"{label_fg}\" POINT-SIZE=\"7\">{kind_label}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\" PORT=\"{}\"><FONT COLOR=\"{label_fg}\"><B>{kind_icon} {}{gen}</B></FONT></TD></TR>", port_id(&s.name), s.name).unwrap();
        }
        Item::Enum(e) => {
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\"><FONT COLOR=\"{label_fg}\" POINT-SIZE=\"7\">{kind_label}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{label_bg}\" PORT=\"{}\"><FONT COLOR=\"{label_fg}\"><B>{kind_icon} {}</B></FONT></TD></TR>", port_id(&e.name), e.name).unwrap();
        }
        _ => write_raw_row(h, item),
    }
}

/// Raw Rust item row: compact, subdued colors, no struct fields or enum variants.
/// Shows name + generics/bounds + associated types for traits, signatures for fns.
fn write_raw_row(h: &mut String, item: &Item) {
    write!(h, "<HR/>").unwrap();
    match item {
        Item::Trait(t) => {
            let gen = if t.generics.is_empty() { String::new() } else { esc(&t.generics) };
            let bounds_s = if t.bounds.is_empty() {
                String::new()
            } else {
                format!(" <FONT COLOR=\"#AAAAAA\" POINT-SIZE=\"5\">{}</FONT>", esc(&t.bounds))
            };
            write!(h, "<TR><TD ALIGN=\"LEFT\"><FONT COLOR=\"#78909C\" POINT-SIZE=\"6\">trait</FONT></TD><TD ALIGN=\"LEFT\" PORT=\"{}\"><FONT COLOR=\"#546E7A\" POINT-SIZE=\"7\"><B>◆ {}{gen}</B></FONT>{bounds_s}</TD></TR>", port_id(&t.name), t.name).unwrap();
            for m in &t.methods {
                let p = esc(&m.params);
                let r = if m.ret.is_empty() { String::new() } else { format!(" → {}", esc(&m.ret)) };
                let g = esc(&m.generics);
                write!(h, "<TR><TD></TD><TD ALIGN=\"LEFT\"><FONT COLOR=\"#90A4AE\" POINT-SIZE=\"5\">ƒ {}{g}({p}){r}</FONT></TD></TR>", m.name).unwrap();
            }
        }
        Item::Struct(s) => {
            let gen = if s.generics.is_empty() { String::new() } else { esc(&s.generics) };
            write!(h, "<TR><TD ALIGN=\"LEFT\"><FONT COLOR=\"#78909C\" POINT-SIZE=\"6\">struct</FONT></TD><TD ALIGN=\"LEFT\" PORT=\"{}\"><FONT COLOR=\"#546E7A\" POINT-SIZE=\"7\"><B>■ {}{gen}</B></FONT></TD></TR>", port_id(&s.name), s.name).unwrap();
        }
        Item::Enum(e) => {
            write!(h, "<TR><TD ALIGN=\"LEFT\"><FONT COLOR=\"#78909C\" POINT-SIZE=\"6\">enum</FONT></TD><TD ALIGN=\"LEFT\" PORT=\"{}\"><FONT COLOR=\"#546E7A\" POINT-SIZE=\"7\"><B>▲ {}</B></FONT></TD></TR>", port_id(&e.name), e.name).unwrap();
        }
        Item::Fn(f) => {
            let p = esc(&f.sig.params);
            let r = if f.sig.ret.is_empty() { String::new() } else { format!(" → {}", esc(&f.sig.ret)) };
            let g = esc(&f.sig.generics);
            write!(h, "<TR><TD ALIGN=\"LEFT\"><FONT COLOR=\"#78909C\" POINT-SIZE=\"6\">fn</FONT></TD><TD ALIGN=\"LEFT\" PORT=\"{}\"><FONT COLOR=\"#546E7A\" POINT-SIZE=\"7\"><B>ƒ {}{g}</B></FONT>({p}){r}</TD></TR>", port_id(&f.sig.name), f.sig.name).unwrap();
        }
        Item::Macro(m) => {
            let kind = if m.is_proc { "proc" } else { "macro" };
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"#FFCDD2\"><FONT COLOR=\"#B71C1C\" POINT-SIZE=\"7\">{kind}!</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"#FFF8E1\" PORT=\"{}\"><FONT COLOR=\"#D32F2F\"><B>⚙ {}!</B></FONT></TD></TR>", port_id(&m.name), m.name).unwrap();
        }
    }
}

/// Domain-specific styling for macro-generated items.
/// Returns (kind_label, icon, bg_color, fg_color).
fn domain_style(macro_name: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    match macro_name {
        "define_signal" =>    ("signal",    "📡", "#E8EAF6", "#283593"),
        "define_resource" =>  ("resource",  "📦", "#E0F2F1", "#00695C"),
        "define_behavior" =>  ("behavior",  "🔄", "#FFF3E0", "#E65100"),
        "define_marker" =>    ("marker",    "🏷", "#F3E5F5", "#6A1B9A"),
        "define_blueprint" => ("blueprint", "📐", "#E3F2FD", "#1565C0"),
        "define_registry" =>  ("registry",  "📋", "#FCE4EC", "#880E4F"),
        "define_storage" =>   ("storage",   "🗄", "#ECEFF1", "#37474F"),
        "define_id" =>        ("id",        "🆔", "#E1F5FE", "#0277BD"),
        "define_action" =>    ("action",    "⚡", "#FFF8E1", "#F57F17"),
        "define_provider" =>   ("provider",  "🔐", "#FBE9E7", "#BF360C"),
        "define_span" =>      ("span",      "📊", "#E8F5E9", "#2E7D32"),
        "define_scope" =>     ("scope",     "🔍", "#F1F8E9", "#558B2F"),
        "define_error" =>     ("error",     "⚠", "#FCE4EC", "#C62828"),
        _ =>                  ("generated", "⚙",  "#F5F5F5", "#616161"),
    }
}
