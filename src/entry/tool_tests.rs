//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Tests for `mock <tool>` dispatch, kept beside `tool.rs` rather than in it.

use super::*;

#[test]
fn stdin_is_read_only_when_the_tool_asked_for_it() {
    // The whole truth table, because the bug was one cell of it: a tool
    // that never asked, handed a pipe, read it and blocked forever.
    assert!(!should_read_stdin(false, false), "the cell that hung");
    assert!(!should_read_stdin(false, true));
    assert!(should_read_stdin(true, false), "asked for, and piped");
    assert!(!should_read_stdin(true, true), "asked for, but interactive");
}

#[test]
fn every_builtin_subcommand_is_refused_as_a_tool_name() {
    // Not a sample. Every name the dispatcher answers to, so a subcommand
    // added later is covered without anyone remembering to add it here.
    for name in super::super::help::known_commands() {
        assert!(
            builtin_collision(name),
            "`{name}` is a builtin and must be refused as a tool name"
        );
    }
}

#[test]
fn help_spellings_are_refused_too() {
    // `--help` cannot be a directory name in practice, but `help` can, and
    // a tool called `help` would shadow the one command a stranger types
    // first.
    for name in ["help", "--help", "-h", "-?"] {
        assert!(builtin_collision(name), "`{name}` must be refused");
    }
}

#[test]
fn an_ordinary_tool_name_is_not_refused() {
    // The negative arm. Without it the collision check could return true
    // for everything and both tests above would still pass.
    for name in ["phrase-search", "corpus-talk", "claim-inventory", "already-said"] {
        assert!(!builtin_collision(name), "`{name}` should be allowed");
    }
}

#[test]
fn the_zero_examined_message_never_claims_a_failure() {
    // The case that must fail: the old wording was
    // "{name}: examined nothing, so this is not a pass.", printed one
    // line above `ExitCode::SUCCESS`. Two facts in direct contradiction,
    // and a reader has to trust one of them; a shell or CI reading the
    // exit code trusts the wrong one.
    let msg = examined_nothing_message("audit");
    assert!(
        !msg.to_lowercase().contains("not a pass"),
        "the message must not assert the opposite of the exit code it \
         is printed beside: {msg:?}"
    );
    assert!(msg.contains("examined nothing"), "{msg:?}");
    assert!(
        msg.contains("audit"),
        "the tool's own name must appear: {msg:?}"
    );
}

#[test]
fn a_name_that_merely_starts_with_a_builtin_is_allowed() {
    // `checkers` is not `check`. A prefix match here would refuse real
    // tool names for no reason, and the suggestion machinery already
    // treats prefixes specially, so this is worth pinning apart.
    assert!(!builtin_collision("checkers"));
    assert!(!builtin_collision("statuses"));
    assert!(builtin_collision("check"));
    assert!(builtin_collision("status"));
}

mod not_found_tests {
    use mockspace_lint_rules::LintPack;
    use mockspace_lint_rules::tool::{NotALint, Purpose, Tool, ToolContext, ToolReport};

    use super::super::{NotFound, why_not_found};

    struct Registered;
    impl Tool for Registered {
        fn name(&self) -> &'static str {
            "registered"
        }

        fn description(&self) -> &'static str {
            "a tool that did register"
        }

        fn purpose(&self) -> Purpose {
            Purpose::Check(NotALint::NoFailingCase)
        }

        fn run(&self, _: &ToolContext<'_>) -> ToolReport {
            ToolReport::reported("", 0)
        }
    }

    /// No tool registered at all is a build that did not happen, not a name
    /// that does not match.
    #[test]
    fn an_empty_tool_set_means_nothing_was_built() {
        assert_eq!(
            why_not_found(&LintPack::default(), false),
            NotFound::NothingBuilt
        );
    }

    /// The control, and the arm that makes the one above an assertion rather
    /// than a restatement: with a tool registered under a different name, the
    /// cdylib plainly did build and the mismatch message is the right one.
    #[test]
    fn a_registered_tool_under_another_name_is_a_mismatch() {
        let mut pack = LintPack::default();
        pack.tools.push(Box::new(Registered));
        assert_eq!(why_not_found(&pack, true), NotFound::NameMismatch);
    }
}
