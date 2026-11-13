//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The body of the generated `catalogues` rule.
//!
//! The rule loads on the first file read in a repository, so what it carries is
//! paid for by every session there. It carries only what the repository decided:
//! the tools it declares and the lints it set itself. What is the same in every
//! repository, the builtin subcommands and every lint running at the pack's
//! default, is one command away and answered live by `mock tools` and
//! `mock lints`, which is where the rule points.

use mockspace_lint_rules::LintPack;

use crate::config::Config;

/// The rule body for `cfg` against the loaded `pack`.
///
/// Names only: a project tool's usage and summary, and a lint's source column, are
/// what `mock tools` and `mock lints` print on demand, and the rule is read again for every
/// worktree path a session touches. A tool is named, a lint is named under the severity it
/// runs at, and the rest is one command away.
pub(crate) fn body(cfg: &Config, pack: &LintPack) -> String {
    use std::fmt::Write as _;
    let tools = crate::tool_catalogue::enumerate(pack);
    let tool_names: Vec<&str> = tools
        .iter()
        .filter(|t| t.source == crate::tool_catalogue::Source::Project)
        .map(|t| t.name.as_str())
        .collect();
    let lints = crate::lint_catalogue::enumerate(pack, &cfg.lint_overrides);
    let absent = crate::lint_catalogue::configured_but_absent(pack, &cfg.lint_overrides);
    let set: Vec<&crate::lint_catalogue::Listing> = lints
        .iter()
        .filter(|l| l.is_configured() && l.runs())
        .collect();
    let mut labels: Vec<&'static str> = set.iter().map(|l| l.effective().label()).collect();
    labels.sort_unstable();
    labels.dedup();
    let mut set_here = String::new();
    for label in labels {
        let names: Vec<&str> = set
            .iter()
            .filter(|l| l.effective().label() == label)
            .map(|l| l.name.as_str())
            .collect();
        let _ = writeln!(set_here, "{label}: {}", names.join(", "));
    }
    if !absent.is_empty() {
        let _ = writeln!(
            set_here,
            "configured and matching no lint that runs: {}",
            absent.join(", ")
        );
    }
    let tools_line = if tool_names.is_empty() {
        "(none declared)".to_string()
    } else {
        tool_names.join(", ")
    };
    let mut tools_set = String::new();
    for (name, params) in &cfg.tool_params.params {
        let keys: Vec<&str> = params.keys().collect();
        let _ = writeln!(tools_set, "{name}: {}", keys.join(", "));
    }
    let tools_absent = cfg.tool_params.absent(&pack.tools);
    if !tools_absent.is_empty() {
        let _ = writeln!(
            tools_set,
            "configured and matching no tool: {}",
            tools_absent.join(", ")
        );
    }
    let tools_set = if tools_set.is_empty() {
        String::new()
    } else {
        format!("\nTOOLS SET HERE, under [tools.<name>] in mockspace.toml\n{tools_set}")
    };
    format!(
        "## What is available here, and what is checking you\n\n\
         `mock tools` says what each tool does and `mock lints` lists every lint with its severity and where it comes from. Below are only the names this repository declares, as of the last `cargo mock`; the lints that run at the pack's default are not listed.\n\n\
         A tool reads its configuration from `[tools.<name>]` in `mockspace.toml`; `mock help <tool>` names the keys it reads, and a key it does not read stops it running.\n\n\
         ```\nPROJECT TOOLS: {tools_line}\n{tools_set}\nLINTS SET HERE\n{set_here}```\n"
    )
}

#[cfg(test)]
mod tests {
    use mockspace_lint_rules::tool::{ConfigKey, NotALint, Purpose, Tool, ToolContext, ToolReport};

    use super::*;

    struct Probe;

    impl Tool for Probe {
        fn name(&self) -> &'static str {
            "probe"
        }

        fn description(&self) -> &'static str {
            ""
        }

        fn purpose(&self) -> Purpose {
            Purpose::Check(NotALint::TakesAQuestion)
        }

        fn config_keys(&self) -> &[ConfigKey] {
            &[ConfigKey {
                name:        "subjects",
                description: "",
            }]
        }

        fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
            unreachable!()
        }
    }

    fn body_for(toml: &str) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("mock/crates")).unwrap();
        std::fs::write(
            repo.join("mockspace.toml"),
            format!("src_dirs = [\"crates\"]\n{toml}"),
        )
        .unwrap();
        let cfg = Config::from_dir(&repo.join("mock"));
        let mut pack = LintPack::default();
        pack.tools.push(Box::new(Probe));
        body(&cfg, &pack)
    }

    #[test]
    fn the_rule_names_each_tool_this_repository_configures_and_a_table_matching_none() {
        let b = body_for("\n[tools.probe]\nsubjects = [\"workflow\"]\n\n[tools.gone]\nx = 1\n");
        assert!(b.contains("TOOLS SET HERE"), "{b}");
        assert!(b.contains("probe: subjects"), "{b}");
        assert!(b.contains("configured and matching no tool: gone"), "{b}");
    }

    #[test]
    fn a_repository_that_configures_no_tool_gets_no_such_block() {
        // THE CONTROL: the block is the tables' doing.
        let b = body_for("");
        assert!(!b.contains("TOOLS SET HERE"), "{b}");
        assert!(b.contains("PROJECT TOOLS: probe"), "{b}");
    }
}
