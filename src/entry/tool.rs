//! `mock <name>`: running a tool discovered under `<mock>/tools/`.
//!
//! # Why an unknown subcommand does not build anything
//!
//! The name a tool is invoked by is its **directory name**, readable without
//! compiling. That is not a convenience, it is what makes dispatch affordable:
//! the alternative is that `mock lck` builds a cdylib to discover that a typo
//! was a typo. So [`crate::bootstrap::tool_names`] answers "is this a tool"
//! from the filesystem, and only a name that matches a directory reaches the
//! loader.
//!
//! # Why the name is checked against the directory
//!
//! A tool's directory decides the subcommand, and its source declares
//! `Tool::name()`. Those can disagree, and if they do the tool is invocable
//! under a name its own source does not contain, which is a thing nobody can
//! grep for. Rather than pick a winner, the mismatch is refused and named.

#![allow(unused_imports)]
use super::*;
use mockspace_lint_rules::tool::{
    NotALint, Outcome, Tool, ToolContext, ToolReport, contract_faults, duplicate_tool_names,
    missing_required, usage_line,
};

/// Every subcommand name a tool may not take, because the engine already
/// answers to it.
///
/// Refusing the collision is the only honest option. Silently letting a tool
/// win would break `mock check` in whatever repo declared it; silently letting
/// the builtin win would leave a tool that is present, compiled, and
/// unreachable, which is worse because nothing reports it.
fn builtin_collision(name: &str) -> bool {
    super::help::known_commands().contains(&name) || super::help::is_help_request(name)
}

/// Run the tool named `name`, having already established that a directory of
/// that name exists.
pub(crate) fn run(cfg: &Config, pack: &LintPack, name: &str, args: &[&str]) -> ExitCode {
    if builtin_collision(name) {
        eprintln!(
            "mock: `{name}` is a builtin subcommand, so the tool at \
             {}/tools/{name} can never be reached.",
            cfg.mock_dir.display()
        );
        eprintln!("  rename the directory; the directory name is the subcommand.");
        return ExitCode::from(2);
    }

    let dupes = duplicate_tool_names(&pack.tools);
    if !dupes.is_empty() {
        eprintln!(
            "mock: {} tool name(s) registered more than once: {}",
            dupes.len(),
            dupes.join(", ")
        );
        eprintln!("  `mock <name>` would run whichever loaded first, so this is refused.");
        return ExitCode::from(2);
    }

    let Some(tool) = pack.tools.iter().find(|t| t.name() == name) else {
        // The directory exists and nothing in the built pack answers to it.
        // Almost always a name mismatch, so say that rather than "not found".
        eprintln!(
            "mock: `{}/tools/{name}` was built, but no tool in it declares \
             `fn name() -> \"{name}\"`.",
            cfg.mock_dir.display()
        );
        eprintln!(
            "  the directory name is the subcommand, so the tool's own `name()` \
             must match it."
        );
        let others: Vec<&str> = pack.tools.iter().map(|t| t.name()).collect();
        if !others.is_empty() {
            eprintln!("  registered tools: {}", others.join(", "));
        }
        return ExitCode::from(2);
    };

    // Registration-time half of the contract audit. Checked before the run,
    // because a tool that has already lied about asking a question should not
    // then be handed an argument vector it never declared.
    let faults = contract_faults(tool.as_ref(), None);
    if !faults.is_empty() {
        for f in &faults {
            eprintln!("mock: {f}");
        }
        return ExitCode::from(2);
    }

    let missing = missing_required(tool.as_ref(), args);
    if !missing.is_empty() {
        eprintln!("mock: `{name}` needs {} more argument(s).", missing.len());
        eprintln!("  usage: {}", usage_line(tool.as_ref()));
        for a in &missing {
            eprintln!("    <{}>  {}", a.name, a.description);
        }
        return ExitCode::from(2);
    }

    // Discovered here rather than threaded in, because a tool is dispatched
    // before the generate path runs and there is no crate map yet. An empty set
    // is a legitimate answer: a documentation repository has no crates and is
    // exactly the kind of project that wants tools.
    let crates = crate::parse::discover_crates_in(&cfg.src_dirs, &cfg.crate_prefix);
    let all_crate_names: std::collections::BTreeSet<String> = crates.keys().cloned().collect();

    let stdin = read_stdin_if_piped();
    let ctx = ToolContext {
        mock_dir:   &cfg.mock_dir,
        repo_root:  &cfg.repo_root,
        all_crates: &all_crate_names,
        src_dirs:   &cfg.src_dirs,
        args,
        stdin:      stdin.as_deref(),
    };

    let report = tool.run(&ctx);

    if !report.output.is_empty() {
        print!("{}", report.output);
        if !report.output.ends_with('\n') {
            println!();
        }
    }

    // Post-run half of the audit. A `no-failing-case` tool that returned a
    // blocking finding has contradicted itself, and the finding it produced is
    // reported alongside rather than instead: both facts are true and the
    // reader needs both.
    for f in contract_faults(tool.as_ref(), Some(&report)) {
        eprintln!("mock: {f}");
    }

    match &report.outcome {
        Outcome::Clean {
            examined,
        } => {
            if *examined == 0 {
                // Not a pass. A clean verdict over nothing is the shape that
                // reads green and establishes nothing, and it is worth saying
                // out loud every time.
                eprintln!("{name}: examined nothing, so this is not a pass.");
            } else {
                eprintln!("{name}: clean, {examined} examined.");
            }
            ExitCode::SUCCESS
        },
        Outcome::Inconclusive {
            reason,
        } => {
            eprintln!("{name}: INCONCLUSIVE, so the run says nothing about the corpus.");
            eprintln!("  {reason}");
            ExitCode::FAILURE
        },
        Outcome::Findings(findings) => {
            for e in findings {
                eprintln!("{e}");
            }
            // A tool is invoked rather than gated, so the mode that decides
            // whether its findings block is the strictest one: there is no
            // hook here whose severity would select a laxer reading.
            if report.outcome.blocks(LintMode::Push) {
                eprintln!("{name}: {} finding(s).", findings.len());
                ExitCode::FAILURE
            } else {
                eprintln!("{name}: {} advisory finding(s).", findings.len());
                ExitCode::SUCCESS
            }
        },
    }
}

/// Read piped stdin, or `None` when stdin is a terminal.
///
/// A tool reading a terminal would hang waiting for input nobody is typing,
/// which looks exactly like the tool being slow.
fn read_stdin_if_piped() -> Option<String> {
    use std::io::{IsTerminal, Read};
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).ok()?;
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_name_that_merely_starts_with_a_builtin_is_allowed() {
        // `checkers` is not `check`. A prefix match here would refuse real
        // tool names for no reason, and the suggestion machinery already
        // treats prefixes specially, so this is worth pinning apart.
        assert!(!builtin_collision("checkers"));
        assert!(!builtin_collision("statuses"));
        assert!(builtin_collision("check"));
        assert!(builtin_collision("status"));
    }
}
