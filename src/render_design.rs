//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::graph;
use crate::model::*;

/// Write a generated file, skipping the write when the only difference is the
/// generation timestamp.
///
/// Every generated file carries a `Generated at:` line, so a regeneration that
/// produces otherwise identical content still rewrites the file and leaves it
/// modified in the consumer's working tree. That noise is worse than untidy: a
/// timestamp-only diff is indistinguishable at a glance from a real one, so it
/// trains readers to skip generated-file diffs, and it dirties a tree that the
/// run should have left alone. Running `cargo mock` must be safe on a clean
/// tree.
///
/// Skipping leaves the previous timestamp in place, which makes the field mean
/// "when this content last changed" rather than "when a generator last ran".
/// That is the more useful of the two readings and the only one a reader can
/// act on.
///
/// Returns true when the file was written.
pub fn write_generated(path: &Path, content: &str) -> bool {
    let previous = fs::read_to_string(path).ok();
    write_generated_vs(path, content, previous.as_deref())
}

/// [`write_generated`] against a caller-supplied previous version.
///
/// Needed when something else writes the target before the header goes on:
/// graphviz renders the SVG straight to its final path, so by the time the
/// header is prepended the previous version is already gone and there is
/// nothing left on disk to compare against. The caller snapshots it first and
/// passes it here.
///
/// Returns true when the file was written.
pub fn write_generated_vs(path: &Path, content: &str, previous: Option<&str>) -> bool {
    if let Some(prev) = previous {
        if same_modulo_timestamp(prev, content) {
            // Put the previous bytes back. For a file this module owns end to
            // end that is a no-op and is skipped; for one an external tool has
            // already clobbered, it is what actually undoes the churn.
            if fs::read_to_string(path).ok().as_deref() != Some(prev) {
                fs::write(path, prev)
                    .unwrap_or_else(|e| panic!("failed to restore {}: {e}", path.display()));
            }
            return false;
        }
    }
    fs::write(path, content).unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
    true
}

/// Best-effort [`write_generated`]: same timestamp-skip, but a write
/// failure is ignored rather than a panic.
///
/// For generated files a caller deliberately writes with `.ok()` (a
/// failure there is tolerated, not fatal). Returns true when written.
pub fn write_generated_ok(path: &Path, content: &str) -> bool {
    if let Ok(existing) = fs::read_to_string(path) {
        if same_modulo_timestamp(&existing, content) {
            return false;
        }
    }
    fs::write(path, content).is_ok()
}

/// Compare two generated files ignoring the header `Generated at:` line.
///
/// Only the FIRST timestamp line in each file is dropped, which is the
/// header the generator emits. A body line that happens to start with
/// `Generated at:` is a real content line and stays in the comparison,
/// so a change touching only such a line is not false-skipped.
fn same_modulo_timestamp(a: &str, b: &str) -> bool {
    without_header_timestamp(a).eq(without_header_timestamp(b))
}

/// The file's lines with the first `Generated at:` line removed.
fn without_header_timestamp(s: &str) -> impl Iterator<Item = &str> {
    let mut dropped = false;
    s.lines().filter(move |l| {
        if !dropped && is_timestamp_line(l) {
            dropped = true;
            return false;
        }
        true
    })
}

/// True for the `Generated at:` line in any header form this module emits
/// (markdown and SVG indent it inside a comment; DOT prefixes it with `//`).
fn is_timestamp_line(line: &str) -> bool {
    line.trim_start()
        .trim_start_matches("//")
        .trim_start()
        .starts_with("Generated at:")
}

/// Generate a markdown generation header with timestamp.
pub fn generation_header_md(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| cfg.mock_dir.display().to_string());
    let mut hdr = String::new();
    let timestamp = now_rfc3339();

    writeln!(hdr, "<!--").unwrap();
    writeln!(hdr, "  AUTO-GENERATED: DO NOT EDIT DIRECTLY").unwrap();
    writeln!(hdr, "").unwrap();
    writeln!(hdr, "  Generated by: mockspace ({mock_rel})").unwrap();
    writeln!(hdr, "  Generated at: {timestamp}").unwrap();
    writeln!(hdr, "  Source: {mock_rel}/").unwrap();
    writeln!(hdr, "  To regenerate: cargo mock").unwrap();
    writeln!(hdr, "-->").unwrap();

    hdr
}

/// Generate a DOT comment header with timestamp.
pub fn generation_header_dot(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| cfg.mock_dir.display().to_string());
    let mut hdr = String::new();
    let timestamp = now_rfc3339();

    writeln!(hdr, "// AUTO-GENERATED: DO NOT EDIT DIRECTLY").unwrap();
    writeln!(hdr, "//").unwrap();
    writeln!(hdr, "// Generated by: mockspace ({mock_rel})").unwrap();
    writeln!(hdr, "// Generated at: {timestamp}").unwrap();
    writeln!(hdr, "// Source: {mock_rel}/").unwrap();
    writeln!(hdr, "// To regenerate: cargo mock").unwrap();
    writeln!(hdr).unwrap();

    hdr
}

/// Generate an SVG comment to prepend before the SVG content.
pub fn generation_header_svg(cfg: &Config) -> String {
    let mock_rel = cfg
        .mock_dir
        .strip_prefix(&cfg.repo_root)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| cfg.mock_dir.display().to_string());
    let mut hdr = String::new();
    let timestamp = now_rfc3339();

    writeln!(hdr, "<!--").unwrap();
    writeln!(hdr, "  AUTO-GENERATED: DO NOT EDIT DIRECTLY").unwrap();
    writeln!(hdr, "").unwrap();
    writeln!(hdr, "  Generated by: mockspace ({mock_rel})").unwrap();
    writeln!(hdr, "  Generated at: {timestamp}").unwrap();
    writeln!(hdr, "  Source: {mock_rel}/").unwrap();
    writeln!(hdr, "  To regenerate: cargo mock").unwrap();
    writeln!(hdr, "-->").unwrap();

    hdr
}

/// The document-name stem for a crate, with the project's own crate prefix
/// stripped.
///
/// A document already lives in one project's docs directory, so repeating the
/// project name in every filename says nothing: `NUMERIC.md` carries exactly
/// the information `IKIUNI_RENDERER_NUMERIC_OVERVIEW.md` did, and a
/// directory listing of thirty crates becomes readable.
///
/// A crate that does not carry the prefix keeps its whole name, which is the
/// only case where two crates could produce one stem. That collision is
/// reported by the caller rather than silently resolved.
pub fn crate_doc_stem(crate_name: &str, crate_prefix: &str) -> String {
    let stripped = if crate_prefix.is_empty() {
        crate_name
    } else {
        crate_name
            .strip_prefix(&format!("{crate_prefix}-"))
            .unwrap_or(crate_name)
    };
    stripped.to_uppercase().replace('-', "_")
}

/// Apply the standalone prefix to a document name, or leave it alone when the
/// project has not opted into ordering.
///
/// Four bands, in reading order: `00` the documents to start with, `10` upward
/// the crates by dependency depth, `90` the registry tables, and `99`
/// everything else. Registries get their own band because a lookup table is a
/// different kind of thing from a document read start to finish, and burying
/// one among the other supplementary documents loses that.
pub fn ordered_doc_name(base: &str, cfg: &Config) -> String {
    crate::document::DocId::root(base, cfg).file_name(cfg)
}

/// Create the docs output directory if it is not there yet.
///
/// Every generated file lands in it, and a repo generating for the first time
/// has none.
pub fn ensure_docs_dir(cfg: &Config) {
    if let Err(e) = fs::create_dir_all(&cfg.docs_dir) {
        panic!("failed to create {}: {e}", cfg.docs_dir.display());
    }
}

/// The `{{...}}` placeholder vocabulary, computed once per run.
///
/// One vocabulary for every template: a placeholder means the same thing
/// wherever it appears, so `{{project_name}}` in `WORKFLOW.md.tmpl` or in a
/// crate's own `DESIGN.md.tmpl` expands exactly as it does in the top-level
/// `DESIGN.md.tmpl`. Templates that use no placeholder pass through unchanged.
///
/// Computed once because several members scan the mock tree (the deep-dive
/// list and the crate summaries read directories and files), and the values
/// are identical for every template in a run.
///
/// There is deliberately no escape for a literal `{{name}}`: no template needs
/// to write one today. Add an escape when one does, rather than carrying an
/// unused mechanism.
pub struct Placeholders {
    project_name:    String,
    mock_dir:        String,
    crate_count:     String,
    macros_table:    String,
    primary_items:   String,
    crate_layers:    String,
    deep_dives:      String,
    crate_summaries: String,
}

#[cfg(test)]
impl Placeholders {
    /// An all-empty set, for tests that exercise substitution rather than the
    /// vocabulary computation.
    fn empty_for_test() -> Self {
        Self {
            project_name:    String::new(),
            mock_dir:        String::new(),
            crate_count:     String::new(),
            macros_table:    String::new(),
            primary_items:   String::new(),
            crate_layers:    String::new(),
            deep_dives:      String::new(),
            crate_summaries: String::new(),
        }
    }
}

/// Substitute one placeholder, with or without spaces inside the braces.
///
/// `{{project_name}}` and `{{ project_name }}` are the same placeholder. They
/// were not: substitution matched the exact unspaced form while reference
/// resolution trimmed, so the two syntaxes that look identical behaved
/// differently, and a spaced placeholder rendered literally with nothing saying
/// why. Anyone writing references all day writes the spaced form by habit.
fn replace_placeholder(text: &str, name: &str, value: &str) -> String {
    text.replace(&format!("{{{{{name}}}}}"), value)
        .replace(&format!("{{{{ {name} }}}}"), value)
}

impl Placeholders {
    /// Compute the vocabulary for one generation run.
    pub fn compute(crates: &CrateMap, cfg: &Config) -> Self {
        let mock_dir = cfg
            .mock_dir
            .strip_prefix(&cfg.repo_root)
            .unwrap_or(&cfg.mock_dir)
            .to_string_lossy()
            .to_string();

        Self {
            project_name: cfg.project_name.clone(),
            mock_dir,
            crate_count: crates.len().to_string(),
            macros_table: compute_macros_table(crates, cfg),
            primary_items: compute_primary_items_per_crate(crates, cfg),
            crate_layers: compute_crate_layers(crates, cfg),
            deep_dives: collect_deep_dives(&cfg.src_dirs),
            crate_summaries: compute_crate_summaries(&cfg.mock_dir, &cfg.crate_prefix, cfg, crates),
        }
    }

    /// Expand every placeholder in a template body.
    pub fn apply(&self, template: &str) -> String {
        // Repeated to a fixed point rather than once, because composition
        // nests: `{{crate_summaries}}` expands into the per-crate summaries,
        // and a summary may itself name a placeholder. A single pass inserts
        // that text after substitution has already run over it, so it survives
        // into the finished document literally.
        //
        // Bounded by construction. Each pass either changes the text or stops,
        // and the values are fixed strings rather than a graph, so nesting is
        // one level deep in practice. The cap is a guard against a value that
        // contains its own placeholder, which would otherwise never settle.
        let mut result = template.to_string();
        for _ in 0 .. 4 {
            let next = self.apply_once(&result);
            if next == result {
                break;
            }
            result = next;
        }
        result
    }

    /// One substitution pass over every placeholder.
    fn apply_once(&self, template: &str) -> String {
        let mut result = template.to_string();
        for (name, value) in [
            ("project_name", &self.project_name),
            ("mock_dir", &self.mock_dir),
            ("crate_count", &self.crate_count),
            ("macros_table", &self.macros_table),
            // Both the generic and the legacy name for the same value.
            ("signals_per_crate", &self.primary_items),
            ("primary_items_per_crate", &self.primary_items),
            ("crate_layers", &self.crate_layers),
            ("deep_dives", &self.deep_dives),
            ("crate_summaries", &self.crate_summaries),
        ] {
            result = replace_placeholder(&result, name, value);
        }
        result
    }
}

/// Compute the macros table from config + crate scan.
fn compute_macros_table(crates: &CrateMap, cfg: &Config) -> String {
    // Scan for actually-present macros
    let mut found: BTreeMap<String, bool> = BTreeMap::new();
    for info in crates.values() {
        for item in &info.items {
            if let Item::Macro(m) = item {
                if m.name.starts_with("define_") {
                    found.insert(m.name.clone(), true);
                }
            }
        }
    }

    let mut table = String::new();
    writeln!(table, "| Need | Macro |").unwrap();
    writeln!(table, "|------|-------|").unwrap();

    // Configured macros first (preserves config order)
    for (name, desc, usage) in &cfg.known_macros {
        if found.contains_key(name.as_str()) {
            writeln!(table, "| {desc} | {usage} |").unwrap();
            found.remove(name.as_str());
        }
    }

    // Any extra macros not in config
    for name in found.keys() {
        writeln!(table, "| Custom | `{name}!(...)` |").unwrap();
    }

    table.trim_end().to_string()
}

/// Compute per-crate table for the primary domain macro.
fn compute_primary_items_per_crate(crates: &CrateMap, cfg: &Config) -> String {
    let primary = match &cfg.primary_domain_macro {
        Some(m) => m.as_str(),
        None => return format!("*No primary domain macro configured.*"),
    };
    let label = &cfg.primary_domain_label;

    let mut items_by_crate: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for info in crates.values() {
        let short = &info.short_name;
        if short == &cfg.project_name {
            continue;
        }
        for mg in &info.macro_generated {
            if mg.macro_name == primary {
                items_by_crate
                    .entry(short.clone())
                    .or_default()
                    .push(mg.generated_name.clone());
            }
        }
    }

    if items_by_crate.is_empty() {
        return format!("*No {} found in any crate.*", label.to_lowercase());
    }

    let mut table = String::new();
    writeln!(table, "| Crate | {label} |").unwrap();
    writeln!(table, "|-------|{}|", "-".repeat(label.len() + 2)).unwrap();

    for (crate_name, mut items) in items_by_crate {
        items.sort();
        let formatted = items
            .iter()
            .map(|s| format!("`{s}`"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(table, "| {crate_name} | {formatted} |").unwrap();
    }

    table.trim_end().to_string()
}

/// Compute the crate layers section.
fn compute_crate_layers(crates: &CrateMap, cfg: &Config) -> String {
    let mut depth_cache = BTreeMap::new();
    let mut depths = BTreeMap::new();
    for name in crates.keys() {
        depths.insert(
            name.clone(),
            graph::compute_depth(name, crates, &mut depth_cache),
        );
    }
    let max_depth = depths.values().copied().max().unwrap_or(0);

    let mut by_depth: Vec<Vec<String>> = vec![Vec::new(); max_depth + 1];
    for (name, &d) in &depths {
        let short = &crates[name.as_str()].short_name;
        if short == &cfg.project_name {
            continue;
        }
        by_depth[d].push(short.clone());
    }

    let mut layers = String::new();
    writeln!(layers, "```").unwrap();
    for (d, names) in by_depth.iter().enumerate() {
        if names.is_empty() {
            continue;
        }
        let label = cfg.layer_label(d);
        let mut sorted = names.clone();
        sorted.sort();
        let joined = sorted.join(", ");
        let pad = if d < 10 { " " } else { "" };
        writeln!(layers, "Layer {d}{pad}({label:14}): {joined}").unwrap();
    }
    writeln!(layers, "```").unwrap();

    layers.trim_end().to_string()
}

/// Collect deep-dive markdown files from per-crate DEEPDIVE_*.md.tmpl files.
fn collect_deep_dives(src_dirs: &[PathBuf]) -> String {
    // Every source directory. Hardcoding `mock_dir/crates` here never read the
    // configured roots at all, so a grouped project silently contributed the
    // deep dives of one group and none of the others.
    let mut all_dives = Vec::new();

    let crate_dirs = crate::parse::package_dirs_in(src_dirs);

    for crate_entry in crate_dirs {
        let crate_path = crate_entry.clone();
        let mut entries: Vec<_> = fs::read_dir(&crate_path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.starts_with("DEEPDIVE_") && name.ends_with(".md.tmpl")
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                all_dives.push(content.trim().to_string());
            }
        }
    }

    all_dives.join("\n\n")
}

/// Get current timestamp as RFC 3339.
fn now_rfc3339() -> String {
    use std::process::Command;
    let output = Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

/// Generate standalone DESIGN-DEEP-DIVES.md from per-crate DEEPDIVE_*.md.tmpl files.
pub fn generate_deep_dives_md(cfg: &Config) -> String {
    let header = generation_header_md(cfg);
    let dives = collect_deep_dives(&cfg.src_dirs);
    if dives.is_empty() {
        return String::new();
    }

    let name = &cfg.project_name;
    let mut md = String::new();
    writeln!(md, "{header}").unwrap();
    writeln!(md, "# {name}: Deep Dives").unwrap();
    writeln!(md).unwrap();
    writeln!(
        md,
        "> Detailed technical deep dives into {name} subsystems."
    )
    .unwrap();
    writeln!(
        md,
        "> See also: [DESIGN.md](DESIGN.md) for the high-level design."
    )
    .unwrap();
    writeln!(md).unwrap();
    writeln!(md, "---").unwrap();
    writeln!(md).unwrap();
    writeln!(md, "{dives}").unwrap();

    md
}

/// Compute crate summaries from per-crate README.md.tmpl files.
fn compute_crate_summaries(
    mock_dir: &Path,
    crate_prefix: &str,
    cfg: &Config,
    crates: &CrateMap,
) -> String {
    let mut depth_cache = BTreeMap::new();
    // Every source directory, for the same reason as `collect_deep_dives`: the
    // hardcoded single root ignored the configured ones entirely.
    let crate_dirs = crate::parse::package_dirs_in(&cfg.src_dirs);

    let mut summaries = String::new();

    for crate_entry in crate_dirs {
        let crate_path = crate_entry.clone();
        let crate_name = crate_entry
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let readme_path = crate_path.join("README.md.tmpl");

        if let Ok(content) = fs::read_to_string(&readme_path) {
            writeln!(summaries, "{}", content.trim()).unwrap();

            let has_design = crate_path.join("DESIGN.md.tmpl").exists();
            let deep_dives = find_deep_dives(&crate_path);

            if has_design || !deep_dives.is_empty() {
                writeln!(summaries).unwrap();
                let crate_upper = crate_doc_stem(&crate_name, crate_prefix);
                // Named by the one thing that names documents, so these links
                // cannot go dead when the scheme changes. They did once,
                // in every repo at once, because they were built independently.
                let depth = graph::compute_depth(&crate_name, crates, &mut depth_cache);
                let doc = |subject: &str| {
                    crate::document::DocId::Crate {
                        upper: crate_upper.clone(),
                        subject: subject.to_string(),
                        depth,
                    }
                    .file_name(cfg)
                };
                if has_design {
                    writeln!(summaries, "- [Overview]({})", doc("OVERVIEW")).unwrap();
                }
                for (subject, _) in &deep_dives {
                    writeln!(
                        summaries,
                        "- [{subject} deep dive]({})",
                        doc(&subject.to_uppercase())
                    )
                    .unwrap();
                }
            }

            writeln!(summaries).unwrap();
        }
    }

    summaries.trim_end().to_string()
}

/// Find DEEPDIVE_*.md.tmpl files in a crate directory.
pub fn find_deep_dives(crate_path: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut dives = Vec::new();
    if let Ok(entries) = fs::read_dir(crate_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("DEEPDIVE_") && name.ends_with(".md.tmpl") {
                let subject = name
                    .trim_start_matches("DEEPDIVE_")
                    .trim_end_matches(".md.tmpl")
                    .to_string();
                dives.push((subject, entry.path()));
            }
        }
    }
    dives.sort_by(|a, b| a.0.cmp(&b.0));
    dives
}

#[cfg(test)]
mod tests {
    use super::*;

    const HDR_A: &str = "<!--\n  Generated at: 2026-07-16T10:30:43Z\n  Source: mock/\n-->\nbody line one\nbody line two\n";
    const HDR_B: &str = "<!--\n  Generated at: 2026-07-17T13:34:56Z\n  Source: mock/\n-->\nbody line one\nbody line two\n";

    #[test]
    fn timestamp_only_change_is_equal() {
        assert!(same_modulo_timestamp(HDR_A, HDR_B));
    }

    #[test]
    fn body_change_is_not_equal() {
        let changed = HDR_B.replace("body line two", "body line TWO");
        assert!(!same_modulo_timestamp(HDR_A, &changed));
    }

    #[test]
    fn non_timestamp_header_change_is_not_equal() {
        let changed = HDR_B.replace("Source: mock/", "Source: elsewhere/");
        assert!(!same_modulo_timestamp(HDR_A, &changed));
    }

    #[test]
    fn a_body_line_starting_with_generated_at_is_not_false_skipped() {
        // The header timestamp is the FIRST such line; a body line that
        // happens to start with "Generated at:" is real content and must
        // stay in the comparison. Change only the body occurrence.
        let a = "<!--\n  Generated at: 2026-01-01T00:00:00Z\n-->\nprose\nGenerated at: build 7\n";
        let b = "<!--\n  Generated at: 2026-09-09T00:00:00Z\n-->\nprose\nGenerated at: build 8\n";
        assert!(
            !same_modulo_timestamp(a, b),
            "a real change on a second Generated-at line must not be skipped"
        );
    }

    #[test]
    fn dot_comment_timestamp_line_is_recognised() {
        assert!(is_timestamp_line("// Generated at: 2026-07-17T13:34:56Z"));
        assert!(is_timestamp_line("  Generated at: 2026-07-17T13:34:56Z"));
        assert!(!is_timestamp_line("  Source: mock/"));
        assert!(!is_timestamp_line("body Generated at: not a header line"));
    }

    #[test]
    fn write_generated_skips_timestamp_only_and_restores_previous() {
        let dir = std::env::temp_dir().join(format!("mockspace_wg_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gen.md");

        assert!(write_generated(&path, HDR_A), "first write must land");
        // Simulate an external clobber (graphviz) plus a timestamp-only regen:
        // the previous is HDR_A, the target on disk is headerless junk, the new
        // content is HDR_B. Must skip AND restore HDR_A, not leave the junk.
        std::fs::write(&path, "clobbered by an external tool").unwrap();
        assert!(
            !write_generated_vs(&path, HDR_B, Some(HDR_A)),
            "timestamp-only must skip"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            HDR_A,
            "previous must be restored"
        );

        // A real body change writes.
        let changed = HDR_B.replace("body line two", "body line THREE");
        assert!(write_generated(&path, &changed), "body change must write");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);

        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod placeholder_spacing_tests {
    use super::*;

    #[test]
    fn a_placeholder_substitutes_with_or_without_spaces() {
        // The two syntaxes look identical and one silently did nothing. A
        // document full of `{{ reg::law::x }}` teaches the spaced form, so the
        // spaced placeholder is what gets written next.
        assert_eq!(replace_placeholder("a {{name}} b", "name", "X"), "a X b");
        assert_eq!(replace_placeholder("a {{ name }} b", "name", "X"), "a X b");
    }

    #[test]
    fn a_placeholder_inside_an_expansion_still_substitutes() {
        // Composition nests: the summaries expand into a document and a summary
        // may name a placeholder of its own. One pass inserts that text after
        // substitution has already run over it.
        let mut ph = Placeholders::empty_for_test();
        ph.project_name = "proj".into();
        ph.crate_summaries = "# a\n\n{{ project_name }} does not invent its own.\n".into();
        let out = ph.apply("{{crate_summaries}}");
        assert!(!out.contains("{{"), "nested placeholder survived: {out}");
        assert!(out.contains("proj does not invent"), "{out}");
    }

    #[test]
    fn a_different_placeholder_is_left_alone() {
        assert_eq!(
            replace_placeholder("{{ other }}", "name", "X"),
            "{{ other }}"
        );
    }
}
