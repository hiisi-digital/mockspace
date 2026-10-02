//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! `mock help <name>`: one command described in full.
//!
//! # Why it answers in two places
//!
//! `mock help` resolves before project discovery, so it works outside a
//! project, and a builtin's description is compiled in, so `mock help lock`
//! is answered there too. A project tool's description is not: it lives in
//! the loaded pack, and the pack exists only after discovery and the cdylib
//! build. So the early help check lets `mock help <name>` through when the
//! name is not a builtin, and the dispatcher answers it once the pack is
//! loaded, from the same [`crate::tool_catalogue::render_one`] that `mock
//! tools --long` prints, so the two cannot describe one tool differently.

use std::process::ExitCode;

use mockspace_lint_rules::LintPack;

use crate::tool_catalogue::{Source, enumerate, render_one};

/// The name `mock help <name>` asks about, when the invocation is that shape.
///
/// The first positional must be the word `help` and the second is the name.
/// The value-taking globals and their values are skipped the way the
/// dispatcher skips them, so `mock --dir x help greet` asks about `greet`
/// rather than about `x`.
pub(crate) fn target(args: &[String]) -> Option<&str> {
    let mut positional = Vec::new();
    let mut skip_next = false;
    for a in args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if super::dispatch::VALUE_GLOBALS.contains(&a.as_str()) {
            skip_next = true;
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        positional.push(a.as_str());
    }
    match positional.as_slice() {
        ["help", name, ..] => Some(name),
        _ => None,
    }
}

/// Whether `name` is a builtin, which `mock help` can describe with no project.
pub(crate) fn is_builtin(name: &str) -> bool {
    super::help::known_commands().contains(&name)
}

/// Print a builtin's entry. Needs no project and no pack.
pub(crate) fn print_builtin(name: &str) -> ExitCode {
    let listings = enumerate(&LintPack::default());
    match listings
        .iter()
        .find(|l| l.source == Source::Builtin && l.name == name)
    {
        Some(l) => {
            print!("{}", render_one(l));
            ExitCode::SUCCESS
        },
        None => super::help::print_help(),
    }
}

/// Print a project tool's entry from the loaded pack, or say why it cannot.
///
/// `dispatchable` is every name `mock <name>` could reach, which tells a tool
/// whose directory exists and whose crate was never loaded apart from a name
/// that is nothing at all.
pub(crate) fn print_tool(pack: &LintPack, name: &str, dispatchable: &[String]) -> ExitCode {
    let listings = enumerate(pack);
    if let Some(l) = listings
        .iter()
        .find(|l| l.source == Source::Project && l.name == name)
    {
        print!("{}", render_one(l));
        return ExitCode::SUCCESS;
    }
    if dispatchable.iter().any(|n| n == name) {
        eprintln!(
            "mock: `{name}` is a tool and nothing was loaded from it, so its declaration cannot be read."
        );
        eprintln!(
            "  a project's tools are compiled into a cdylib the launcher asks for, so run \
             this through `cargo mock help {name}`."
        );
        return ExitCode::from(2);
    }
    super::help::unknown_subcommand(name, dispatchable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn help_followed_by_a_name_asks_about_that_name() {
        assert_eq!(target(&args(&["mock", "help", "greet"])), Some("greet"));
        assert_eq!(
            target(&args(&["mock", "--dir", "x", "help", "greet"])),
            Some("greet"),
            "a global's value is not the name"
        );
    }

    #[test]
    fn help_alone_or_a_help_flag_asks_about_nothing_in_particular() {
        // The control: these keep printing the general help, as before.
        assert_eq!(target(&args(&["mock", "help"])), None);
        assert_eq!(target(&args(&["mock", "--help"])), None);
        assert_eq!(target(&args(&["mock", "greet", "help"])), None);
    }

    #[test]
    fn a_builtin_is_told_apart_from_a_tool() {
        assert!(is_builtin("lock"));
        assert!(!is_builtin("phrase-search"));
    }
}
