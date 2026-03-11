use std::collections::BTreeMap;
use std::fmt::Write;

use crate::config::Config;
use crate::graph;
use crate::model::*;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn generate_dot(crates: &CrateMap, cfg: &Config) -> String {
    let mut dot = String::new();
    let reduced = graph::transitive_reduction(crates);
    let project_name = &cfg.project_name;
    let crate_prefix = &cfg.crate_prefix;

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

    // Title
    writeln!(dot, "    labelloc=t;").unwrap();
    writeln!(dot, "    labeljust=l;").unwrap();
    writeln!(dot, "    label=<<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"0\"><TR><TD ALIGN=\"LEFT\"><FONT POINT-SIZE=\"18\" COLOR=\"#37474F\"><B>{project_name}</B></FONT></TD></TR><TR><TD ALIGN=\"LEFT\"><FONT POINT-SIZE=\"9\" COLOR=\"#999999\">crate architecture</FONT></TD></TR></TABLE>>;").unwrap();
    writeln!(dot).unwrap();

    // Crate nodes
    for (dir_name, info) in crates {
        if info.short_name == *project_name { continue; }
        let cid = dir_name.replace('-', "_");
        let (bg, border) = cfg.crate_color(&info.short_name);
        let label = render_crate_node(&cid, info, &bg, &border, cfg);
        writeln!(dot, "    {cid} [shape=none, margin=0, label=<{label}>];").unwrap();
    }
    writeln!(dot).unwrap();

    // Rank grouping by depth, with companion placement from config.
    let mut companion_depths: BTreeMap<String, usize> = BTreeMap::new();
    for (source, target) in &cfg.crate_grouping {
        let target_crate = format!("{crate_prefix}-{target}");
        if let Some(&d) = depths.get(&target_crate) {
            companion_depths.insert(source.clone(), d);
        }
    }

    let mut by_depth: Vec<Vec<&str>> = vec![Vec::new(); max_depth + 1];
    for (name, &d) in &depths {
        if crates[name.as_str()].short_name == *project_name { continue; }
        let short = &crates[name.as_str()].short_name;
        if let Some(&companion_d) = companion_depths.get(short.as_str()) {
            by_depth[companion_d].push(name.as_str());
        } else {
            by_depth[d].push(name.as_str());
        }
    }
    for (d, names) in by_depth.iter().enumerate() {
        if names.is_empty() { continue; }
        write!(dot, "    {{ rank=same; ").unwrap();
        for name in names {
            write!(dot, "{}; ", name.replace('-', "_")).unwrap();
        }
        writeln!(dot, "}} // depth {d}").unwrap();
    }
    writeln!(dot).unwrap();

    // Invisible edges for companion placement
    for (source, target) in &cfg.crate_grouping {
        let prefix_under = crate_prefix.replace('-', "_");
        writeln!(dot, "    // Force {source} next to {target}").unwrap();
        writeln!(dot, "    {prefix_under}_{target} -> {prefix_under}_{source} [style=invis, weight=10];").unwrap();
    }
    writeln!(dot).unwrap();

    // Dependency edges (transitively reduced)
    writeln!(dot, "    // Dependency edges (transitively reduced)").unwrap();
    for (dir_name, deps) in &reduced {
        if crates[dir_name.as_str()].short_name == *project_name { continue; }
        let from_cid = dir_name.replace('-', "_");
        for dep in deps {
            if crates[dep.as_str()].short_name == *project_name { continue; }
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

fn render_crate_node(_cid: &str, info: &CrateInfo, bg: &str, border: &str, cfg: &Config) -> String {
    let gen_lookup: BTreeMap<&str, &MacroGenerated> = info.macro_generated.iter()
        .map(|mg| (mg.generated_name.as_str(), mg))
        .collect();

    let mut domain_items: Vec<(&Item, &MacroGenerated)> = Vec::new();
    let mut raw_items: Vec<&Item> = Vec::new();
    for item in &info.items {
        if let Some(mg) = gen_lookup.get(item.name()) {
            domain_items.push((item, mg));
        } else {
            raw_items.push(item);
        }
    }
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

    // Domain items
    for (item, mg) in &domain_items {
        write_domain_row(&mut h, item, mg, cfg);
    }
    for mg in &extra_domain {
        let style = cfg.macro_style(&mg.macro_name);
        write!(h, "<HR/>").unwrap();
        write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\" PORT=\"{}\"><FONT COLOR=\"{}\" POINT-SIZE=\"7\">{}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\"><FONT COLOR=\"{}\"><B>{} {}</B></FONT></TD></TR>",
            style.bg, port_id(&mg.generated_name), style.fg, style.label, style.bg, style.fg, style.icon, mg.generated_name).unwrap();
    }

    if has_domain && has_raw {
        write!(h, "<HR/>").unwrap();
        write!(h, "<TR><TD COLSPAN=\"2\" ALIGN=\"LEFT\"><FONT COLOR=\"#AAAAAA\" POINT-SIZE=\"5\">rust</FONT></TD></TR>").unwrap();
    }

    for item in &raw_items {
        write_raw_row(&mut h, item);
    }

    write!(h, "</TABLE>").unwrap();
    h
}

fn write_domain_row(h: &mut String, item: &Item, mg: &MacroGenerated, cfg: &Config) {
    let style = cfg.macro_style(&mg.macro_name);
    write!(h, "<HR/>").unwrap();

    match item {
        Item::Trait(t) => {
            let gen = if t.generics.is_empty() { String::new() } else { esc(&t.generics) };
            let bounds_s = if t.bounds.is_empty() {
                String::new()
            } else {
                format!(" <FONT COLOR=\"#999999\" POINT-SIZE=\"5\">{}</FONT>", esc(&t.bounds))
            };
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\"><FONT COLOR=\"{}\" POINT-SIZE=\"7\">{}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\" PORT=\"{}\"><FONT COLOR=\"{}\"><B>{} {}{gen}</B></FONT>{bounds_s}</TD></TR>",
                style.bg, style.fg, style.label, style.bg, port_id(&t.name), style.fg, style.icon, t.name).unwrap();
            for m in &t.methods {
                let p = esc(&m.params);
                let r = if m.ret.is_empty() { String::new() } else { format!(" → {}", esc(&m.ret)) };
                let g = esc(&m.generics);
                write!(h, "<TR><TD></TD><TD ALIGN=\"LEFT\"><FONT COLOR=\"{}\" POINT-SIZE=\"5\">ƒ {}{g}({p}){r}</FONT></TD></TR>", style.fg, m.name).unwrap();
            }
        }
        Item::Struct(s) => {
            let gen = if s.generics.is_empty() { String::new() } else { esc(&s.generics) };
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\"><FONT COLOR=\"{}\" POINT-SIZE=\"7\">{}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\" PORT=\"{}\"><FONT COLOR=\"{}\"><B>{} {}{gen}</B></FONT></TD></TR>",
                style.bg, style.fg, style.label, style.bg, port_id(&s.name), style.fg, style.icon, s.name).unwrap();
        }
        Item::Enum(e) => {
            write!(h, "<TR><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\"><FONT COLOR=\"{}\" POINT-SIZE=\"7\">{}</FONT></TD><TD ALIGN=\"LEFT\" BGCOLOR=\"{}\" PORT=\"{}\"><FONT COLOR=\"{}\"><B>{} {}</B></FONT></TD></TR>",
                style.bg, style.fg, style.label, style.bg, port_id(&e.name), style.fg, style.icon, e.name).unwrap();
        }
        _ => write_raw_row(h, item),
    }
}

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
