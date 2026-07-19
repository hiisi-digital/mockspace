//! Dependency-graph rendering for the workspace's crate set.
//!
//! Walks a [`CrateGraph`] and emits Graphviz `.dot` source. Edges
//! point from each crate to its declared dependencies, restricted to
//! workspace members so external deps do not clutter the graph.
//!
//! SVG output is best-effort: the renderer shells out to the system
//! `dot` binary when it is on PATH. Absent Graphviz, only the `.dot`
//! source is produced.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::project::{CrateGraph, CrateInfo};

/// Emit `.dot` source describing the workspace's dependency graph.
///
/// Nodes are workspace-member crates only; external deps surface as
/// edges (or are dropped when the target is not also a workspace
/// member, since the node would not exist). Order is deterministic:
/// nodes are sorted alphabetically by name, edges follow per-crate
/// alphabetical dep order. proc-macro crates render with a filled
/// node style so they are visually distinct.
pub fn render_dot(graph: &CrateGraph) -> String {
    let mut members: Vec<&CrateInfo> = graph
        .crates
        .iter()
        .filter(|c| c.is_workspace_member)
        .collect();
    members.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = String::new();
    out.push_str("digraph workspace {\n");
    out.push_str("  rankdir=LR;\n");
    out.push_str("  node [shape=box, style=rounded];\n");
    out.push('\n');

    for info in &members {
        let style_suffix = if info.is_proc_macro {
            ", style=\"rounded,filled\", fillcolor=\"#fff4d6\""
        } else {
            ""
        };
        out.push_str(&format!(
            "  \"{}\" [label=\"{}\"{}];\n",
            info.name, info.name, style_suffix,
        ));
    }

    out.push('\n');

    for info in &members {
        let mut deps: Vec<&String> = info.deps.iter().collect();
        deps.sort();
        for dep in deps {
            // Emit the edge only when the target is also a workspace
            // member. Checking against by_name alone would let a
            // future graph-builder change that captures non-member
            // crates produce a dangling edge whose node was never
            // emitted; the is_workspace_member gate makes the
            // invariant explicit at this site.
            let is_member = graph
                .by_name
                .get(dep)
                .map(|&i| graph.crates[i].is_workspace_member)
                .unwrap_or(false);
            if is_member {
                out.push_str(&format!("  \"{}\" -> \"{}\";\n", info.name, dep));
            }
        }
    }

    out.push_str("}\n");
    out
}

/// Try to render `.dot` source to SVG by piping through the system
/// `dot` binary. Returns `None` when Graphviz is absent from PATH,
/// when the spawn fails, or when `dot` exits non-zero.
///
/// The intent is best-effort augmentation: callers always have the
/// `.dot` source to fall back on, and the `.svg` lands alongside as
/// a convenience for direct viewing.
pub fn try_render_svg(dot_source: &str) -> Option<String> {
    let mut child = Command::new("dot")
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Scope the borrow so `child.wait_with_output` can take the
    // value back out of `child`.
    {
        let stdin = child.stdin.as_mut()?;
        stdin.write_all(dot_source.as_bytes()).ok()?;
    }

    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn graph(members: &[(&str, bool, &[&str])]) -> CrateGraph {
        let crates: Vec<CrateInfo> = members
            .iter()
            .map(|(name, is_proc_macro, deps)| {
                CrateInfo {
                    name:                (*name).to_string(),
                    root_path:           std::path::PathBuf::from(*name),
                    is_proc_macro:       *is_proc_macro,
                    is_workspace_member: true,
                    deps:                deps.iter().map(|s| (*s).to_string()).collect(),
                }
            })
            .collect();
        let mut by_name = HashMap::new();
        for (i, c) in crates.iter().enumerate() {
            by_name.insert(c.name.clone(), i);
        }
        CrateGraph {
            crates,
            by_name,
        }
    }

    #[test]
    fn render_dot_emits_header_and_footer() {
        let g = graph(&[]);
        let dot = render_dot(&g);
        assert!(dot.starts_with("digraph workspace {\n"));
        assert!(dot.ends_with("}\n"));
    }

    #[test]
    fn render_dot_sorts_nodes_alphabetically() {
        // Insert in non-alpha order; output must still be sorted.
        let g = graph(&[("zebra", false, &[]), ("alpha", false, &[]), ("mango", false, &[])]);
        let dot = render_dot(&g);
        let alpha = dot.find("\"alpha\"").unwrap();
        let mango = dot.find("\"mango\"").unwrap();
        let zebra = dot.find("\"zebra\"").unwrap();
        assert!(alpha < mango);
        assert!(mango < zebra);
    }

    #[test]
    fn render_dot_styles_proc_macros_distinctly() {
        let g = graph(&[("macro_crate", true, &[]), ("lib_crate", false, &[])]);
        let dot = render_dot(&g);
        assert!(
            dot.contains("\"macro_crate\" [label=\"macro_crate\", style=\"rounded,filled\""),
            "proc-macro crate should render filled, got: {dot}"
        );
        assert!(
            dot.contains("\"lib_crate\" [label=\"lib_crate\"]"),
            "non-proc-macro crate should render unfilled, got: {dot}"
        );
    }

    #[test]
    fn render_dot_emits_edges_to_workspace_members_only() {
        // alpha -> beta (in-workspace); alpha -> external (out-of-workspace, dropped).
        let g = graph(&[("alpha", false, &["beta", "external_crate"]), ("beta", false, &[])]);
        let dot = render_dot(&g);
        assert!(dot.contains("\"alpha\" -> \"beta\";"));
        assert!(
            !dot.contains("\"alpha\" -> \"external_crate\";"),
            "external (non-workspace) deps must not render as edges"
        );
    }

    #[test]
    fn render_dot_is_deterministic_across_runs() {
        let g = graph(&[("alpha", false, &["beta"]), ("beta", false, &[])]);
        let first = render_dot(&g);
        let second = render_dot(&g);
        assert_eq!(first, second);
    }

    #[test]
    fn render_dot_sorts_per_crate_deps_alphabetically() {
        let g = graph(&[
            ("alpha", false, &["zeta", "beta", "mango"]),
            ("beta", false, &[]),
            ("mango", false, &[]),
            ("zeta", false, &[]),
        ]);
        let dot = render_dot(&g);
        let edges_section_start = dot.rfind("\n\n  \"alpha\"").unwrap();
        let edges = &dot[edges_section_start ..];
        let beta = edges.find("alpha\" -> \"beta\"").unwrap();
        let mango = edges.find("alpha\" -> \"mango\"").unwrap();
        let zeta = edges.find("alpha\" -> \"zeta\"").unwrap();
        assert!(beta < mango);
        assert!(mango < zeta);
    }

    // try_render_svg: integration with `dot` is best-effort.
    // We cannot reliably test the success path without depending on
    // Graphviz being installed on the test host. The error path
    // (binary absent) returns None, which is just confirmed by the
    // empty-graph case below: even if dot IS present, an empty
    // digraph still renders, so this test only confirms the function
    // does not panic.
    #[test]
    fn try_render_svg_does_not_panic_on_empty_graph() {
        let _ = try_render_svg("digraph empty {}");
    }
}
