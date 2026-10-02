//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a project tool is for, as `mock tools`, `mock tools --long` and
//! `mock help <tool>` render it.

use mockspace_lint_rules::tool::{NotALint, Purpose, Tool, ToolContext, ToolReport};

use super::*;

struct Gen;
impl Tool for Gen {
    fn name(&self) -> &'static str {
        "gen"
    }

    fn description(&self) -> &'static str {
        "write the generated pages"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Make {
            writes: &["docs/gen/**", "CHANGELOG.md"],
        }
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

struct Ask;
impl Tool for Ask {
    fn name(&self) -> &'static str {
        "ask"
    }

    fn description(&self) -> &'static str {
        "answer a question"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::TakesAQuestion)
    }

    fn args(&self) -> &[ArgSpec] {
        &[ArgSpec {
            name:        "question",
            required:    true,
            description: "what to ask",
        }]
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

fn listings() -> Vec<Listing> {
    enumerate(&LintPack {
        tools: vec![Box::new(Gen), Box::new(Ask)],
        ..LintPack::default()
    })
}

fn one(name: &str) -> String {
    let l = listings();
    render_one(l.iter().find(|l| l.name == name).unwrap())
}

#[test]
fn a_makers_purpose_renders_in_its_help() {
    // `mock help gen` prints exactly this block.
    let help = one("gen");
    assert!(
        help.contains("purpose: make, writes `docs/gen/**`, `CHANGELOG.md`"),
        "a maker's help must say it makes and what it writes:\n{help}"
    );
}

#[test]
fn a_checks_purpose_renders_with_its_reason() {
    let help = one("ask");
    assert!(help.contains("purpose: check, takes-a-question"), "{help}");
    assert!(!help.contains("make"), "{help}");
}

#[test]
fn a_builtin_carries_no_purpose_line() {
    // The negative arm: a builtin is the engine, not a tool of either kind,
    // and inventing a purpose for it would be a claim nothing declared.
    let help = one("lock");
    assert!(!help.contains("purpose:"), "{help}");
}

#[test]
fn the_table_names_each_project_tools_kind() {
    let table = render_table(&listings());
    let gen_line = table.lines().find(|l| l.contains("mock gen")).unwrap();
    let ask_line = table.lines().find(|l| l.contains("mock ask")).unwrap();
    assert!(gen_line.contains(" make "), "{gen_line:?}");
    assert!(ask_line.contains(" check "), "{ask_line:?}");
}

#[test]
fn the_long_listing_carries_the_same_block_as_help() {
    let long = render_long(&listings());
    assert!(
        long.contains(&one("gen")),
        "`--long` and `help` must agree:\n{long}"
    );
    assert!(long.contains(&one("ask")));
}
