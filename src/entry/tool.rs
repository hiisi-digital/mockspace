//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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
//!
//! # A maker is watched while it runs
//!
//! A tool declaring [`Purpose::Make`] has the worktree observed before and
//! after its run, by [`super::tool_writes`], and every path it changed is
//! printed and held to what it declared. A declared write rooted at an
//! argument is resolved from the command line first, and a value outside its
//! declared root, or whose path in the tree passes through a symlink, is
//! refused with exit 2 before the tool is entered. A write
//! outside the resolved declaration, or
//! any finding at error from a maker, whose findings never block, is a
//! contract fault, and so is a false `no-failing-case`. Every contract fault
//! exits 2, whatever the tool's own outcome said, because a tool that broke its declaration has said something
//! different from a corpus finding and the exit code is what a script reads.
//!
//! The observation fails closed. Where git cannot report the tree before the
//! run, a maker is refused rather than run unheld; where it cannot report it
//! after, the run is inconclusive.

use std::io::IsTerminal;

use mockspace_lint_rules::tool::{
    Outcome,
    Purpose,
    ToolContext,
    contract_faults,
    duplicate_tool_names,
    maker_faults,
    missing_required,
    resolve_writes,
    symlinked_writes,
    usage_line,
};

use super::*;

/// Every subcommand name a tool may not take, because the engine already
/// answers to it.
///
/// Refusing the collision is the only honest option. Silently letting a tool
/// win would break `mock check` in whatever repo declared it; silently letting
/// the builtin win would leave a tool that is present, compiled, and
/// unreachable, which is worse because nothing reports it.
///
/// **Called from [`super::dispatch`], before the subcommand is matched, not
/// from here.** `run` is reached only through the dispatcher's catch-all arm,
/// which is reached only for a name no literal match arm (and no
/// [`super::help::is_help_request`] check ahead of it) already claimed. So by
/// the time `name` reaches this function it can never equal a builtin name,
/// and a check here would be provably dead code: exactly the bug this
/// function used to be. See `tool_is_shadowed_by_a_builtin` in `dispatch.rs`
/// for the real enforcement point.
pub(crate) fn builtin_collision(name: &str) -> bool {
    super::help::known_commands().contains(&name) || super::help::is_help_request(name)
}

/// Why a named tool is not in the pack.
///
/// The two are told apart because they send a reader to different places, and
/// the message said "was built" for both. That cost two debugging sessions
/// looking for a name mismatch in a repo where nothing had been compiled.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum NotFound {
    /// Nothing was compiled, because nothing asked for it. The cdylib carrying a
    /// project's tools is built only when the engine is handed
    /// `--mockspace-lint-rules-dep`, which the launcher supplies and a
    /// directly-invoked binary does not.
    NothingBuilt,
    /// The cdylib was asked for and registered no tool at all. A different
    /// situation with a different remedy: the crate is missing its
    /// `lint_pack!`, or its package name does not match its directory.
    RegisteredNothing,
    /// Other tools registered and this name did not, so the tool's own
    /// `name()` and its directory disagree.
    NameMismatch,
}

/// Why `name` is not in the pack, from the fact rather than from a proxy.
///
/// `dep_supplied` is whether `--mockspace-lint-rules-dep` was on the command
/// line, which is the thing that decides whether a cdylib is built at all.
///
/// The first version asked only whether the pack was empty, and inferred
/// "nothing was built" from it. Those come apart in a real case: a tool
/// directory whose crate compiles but registers nothing yields an empty pack
/// **under the launcher**, and the reader was then told to run it through the
/// launcher, which is what they had just done. The discriminator was on the
/// command line the whole time, and this function's own doc comment named it.
pub(crate) fn why_not_found(pack: &LintPack, dep_supplied: bool) -> NotFound {
    match (dep_supplied, pack.tools.is_empty()) {
        (false, _) => NotFound::NothingBuilt,
        (true, true) => NotFound::RegisteredNothing,
        (true, false) => NotFound::NameMismatch,
    }
}

/// Run the tool named `name`, having already established that a directory of
/// that name exists, and that its name does not collide with a builtin (the
/// dispatcher refuses that before this is ever called).
pub(crate) fn run(
    cfg: &Config,
    pack: &LintPack,
    name: &str,
    args: &[&str],
    dep_supplied: bool,
) -> ExitCode {
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
        // Two different situations, and saying "was built" for both sent
        // several debugging sessions looking for a name mismatch that was not
        // there. The cdylib carrying a project's tools is only built when the
        // engine is given `--mockspace-lint-rules-dep`, which the launcher
        // supplies and a directly-invoked binary does not, so a repo that
        // declares a tool and registers none built nothing at all.
        //
        // Where some tools registered and this one did not, a name mismatch is
        // the likely cause and the message below says so.
        match why_not_found(pack, dep_supplied) {
            NotFound::NothingBuilt => {
                eprintln!(
                    "mock: `{}/tools/{name}` exists and nothing was built from it.",
                    cfg.mock_dir.display()
                );
                eprintln!(
                    "  a project's tools are compiled into a cdylib the launcher asks for, so \
                     run this through `cargo mock {name}` rather than the engine binary \
                     directly."
                );
                return ExitCode::from(2);
            },
            NotFound::RegisteredNothing => {
                eprintln!(
                    "mock: `{}/tools/{name}` was built and registered no tool at all.",
                    cfg.mock_dir.display()
                );
                eprintln!(
                    "  the crate compiled, so this is not a build that did not happen: check \
                     that it invokes `lint_pack!` and that its package name matches its \
                     directory."
                );
                return ExitCode::from(2);
            },
            NotFound::NameMismatch => {},
        }
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

    let stdin = if should_read_stdin(tool.wants_stdin(), std::io::stdin().is_terminal()) {
        read_stdin()
    } else {
        None
    };
    let registry = crate::registry::load_registry(&cfg.mock_dir, &cfg.registry_namespaces);
    let view = crate::registry::build_view(&registry, &cfg.registry_namespaces);
    let ctx = ToolContext {
        mock_dir: &cfg.mock_dir,
        repo_root: &cfg.repo_root,
        all_crates: &all_crate_names,
        src_dirs: &cfg.src_dirs,
        args,
        registry: &view,
        stdin: stdin.as_deref(),
    };

    // Taken immediately around the run and nowhere else, so nothing the engine
    // itself does is attributed to the tool. Fail closed: a maker whose writes
    // cannot be observed is not run, since running it would leave a tree
    // nobody held to the declaration.
    let maker = matches!(tool.purpose(), Purpose::Make { .. });
    // What a maker may write on this command line: its writes rooted at an
    // argument resolved from `args`. A value leaving its declared root is
    // refused here, before the tool is entered, rather than found afterwards.
    let resolved = match resolve_writes(tool.as_ref(), args) {
        Ok(r) => r,
        Err(refused) => {
            for f in &refused {
                eprintln!("mock: {f}");
            }
            eprintln!("  `{name}` was not run.");
            return ExitCode::from(2);
        },
    };
    // The strings passed; the tree may still carry a value out through a
    // committed symlink, which only the filesystem can say.
    let linked = symlinked_writes(tool.as_ref(), args, &cfg.repo_root);
    if !linked.is_empty() {
        for f in &linked {
            eprintln!("mock: {f}");
        }
        eprintln!("  `{name}` was not run.");
        return ExitCode::from(2);
    }
    let declared: Vec<&str> = resolved.iter().map(String::as_str).collect();
    let before = if maker {
        match super::tool_writes::snapshot(&cfg.repo_root, &declared) {
            Some(b) => Some(b),
            None => {
                eprintln!(
                    "mock: `{name}` is a maker, and git cannot report this tree, so what it \
                     writes could not be observed or held to what it declares. Not run."
                );
                return ExitCode::from(2);
            },
        }
    } else {
        None
    };
    let report = tool.run(&ctx);
    let after = if maker {
        super::tool_writes::snapshot(&cfg.repo_root, &declared)
    } else {
        None
    };

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
    let mut broke_contract = false;
    let mut unobserved = false;
    for f in contract_faults(tool.as_ref(), Some(&report)) {
        eprintln!("mock: {f}");
        broke_contract = true;
    }

    // A maker is held to what it declared it writes, over what was observed,
    // and to having written something if its run blocks.
    if let Some(b) = &before {
        match super::tool_writes::after_run(b, after) {
            Ok(wrote) => {
                eprint!("{}", wrote_message(name, &wrote));
                for f in maker_faults(tool.as_ref(), &report, args, &wrote) {
                    eprintln!("mock: {f}");
                    broke_contract = true;
                }
            },
            Err(why) => {
                eprintln!("{name}: INCONCLUSIVE, so the run says nothing about the tree.");
                eprintln!("  {why}");
                unobserved = true;
            },
        }
    }

    let code = match &report.outcome {
        Outcome::Clean {
            examined,
        } if maker => {
            eprintln!("{name}: made, {examined} examined.");
            ExitCode::SUCCESS
        },
        Outcome::Clean {
            examined,
        } => {
            if *examined == 0 {
                // Exits SUCCESS, deliberately: an empty population is
                // sometimes the honest state of the world (a documentation
                // repository has no crates, and `ToolContext::all_crates` says
                // so truthfully), and the engine has no way to tell that case
                // apart from a tool whose glob or directory got renamed out
                // from under it. So this cannot refuse; it can only say, every
                // time, that zero examined establishes nothing, which is worth
                // reading even when the exit code says the run was fine.
                eprintln!("{}", examined_nothing_message(name));
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
    };

    // The same code a refused declaration exits with: the tool broke its
    // contract, which is a different statement from a corpus finding.
    if broke_contract {
        ExitCode::from(2)
    } else if unobserved {
        ExitCode::FAILURE
    } else {
        code
    }
}

/// What a maker is reported to have written, one path a line.
///
/// Said even when it is nothing, because a maker that wrote nothing is worth
/// reading: either there was nothing to do or its output went somewhere the
/// observation cannot see.
fn wrote_message(name: &str, wrote: &[String]) -> String {
    if wrote.is_empty() {
        return format!("{name}: wrote nothing git can see.\n");
    }
    let mut s = format!("{name}: wrote {} path(s):\n", wrote.len());
    for p in wrote {
        s.push_str("    ");
        s.push_str(p);
        s.push('\n');
    }
    s
}

/// The message printed for a clean verdict over zero, alongside the
/// [`ExitCode::SUCCESS`] this always returns with.
///
/// Kept as a pure function rather than inlined, so the wording can be pinned
/// without capturing stderr: this used to read `"examined nothing, so this is
/// not a pass"` printed one line above an exit code that said the opposite.
/// The words claimed a failure; the process claimed success. Fixed by
/// rewording rather than by refusing (see the call site for why refusing is
/// not available here), so the pin is on the words never asserting something
/// the exit code contradicts.
fn examined_nothing_message(name: &str) -> String {
    format!(
        "{name}: examined nothing. A clean verdict over an empty population \
         proves nothing about the corpus; check the tool's inputs if this is \
         unexpected."
    )
}

/// Whether to read standard input, given what the tool asked for and what
/// stdin is attached to.
///
/// A pure function because the bug it encodes is a logic error rather than an
/// I/O one, and because the version that blocked could not be tested at all.
///
/// **Both conditions are required, and `wants` is the one that matters.** The
/// first version of this read whenever stdin was not a terminal, on the
/// reasoning that a pipe means somebody piped something. That is false: a git
/// hook, a CI step and an agent all hand over a pipe whose writer may never
/// write and may never close, so the read blocks forever and the tool looks
/// slow rather than stuck. It deadlocked the first end-to-end invocation.
///
/// The terminal check stays as the second condition, for the interactive case:
/// a tool that wants stdin, run by hand with nothing piped, should not sit
/// waiting for input nobody is typing.
fn should_read_stdin(wants: bool, is_terminal: bool) -> bool {
    wants && !is_terminal
}

/// Read standard input to end.
fn read_stdin() -> Option<String> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).ok()?;
    Some(s)
}

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tool_tests;
