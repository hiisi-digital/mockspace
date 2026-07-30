//! `mock help`, and what to say when a subcommand is not recognised.
//!
//! # Why help must not need a project
//!
//! `mock --help` used to print `no mockspace.toml found. Run from a mockspace
//! directory or use --dir <path>`. That is the correct answer to "run the
//! workflow here" and the wrong answer to "what does this tool do", which is the
//! one question a reader outside a project is most likely to ask, and often the
//! first thing anyone types. So help resolves before any project discovery and
//! works anywhere.
//!
//! # Why an unknown subcommand is an error
//!
//! An unrecognised positional argument used to fall through to the default run,
//! meaning `mock lck` silently did a full generate instead of locking, and a typo
//! looked like success. It is now an error that names the nearest real subcommand,
//! since the reader already knows what they meant and only needs the spelling.

use std::process::ExitCode;

/// One subcommand, with the one-line summary help prints.
struct Cmd {
    name:    &'static str,
    summary: &'static str,
}

/// Every subcommand, in the order help lists them: the round's own lifecycle
/// first, then everything else, because that order is the workflow.
const COMMANDS: &[Cmd] = &[
    Cmd {
        name:    "lock",
        summary: "seal the active changelist, advancing the round's phase",
    },
    Cmd {
        name:    "unlock",
        summary: "deprecate the src changelist and reopen the doc one",
    },
    Cmd {
        name:    "deprecate",
        summary: "supersede the active changelist, keeping it as an audit trail",
    },
    Cmd {
        name:    "close",
        summary: "archive a finished round into a timestamped directory",
    },
    Cmd {
        name:    "archive",
        summary: "archive an abandoned round without closing it",
    },
    Cmd {
        name:    "status",
        summary: "show the current round, its phase, and what may be edited",
    },
    Cmd {
        name:    "check",
        summary: "readiness report: git, phase, build, tests, lints",
    },
    Cmd {
        name:    "check-message",
        summary: "lint one commit message or forge body against the configured policy",
    },
    Cmd {
        name:    "query",
        summary: "query the registry",
    },
    Cmd {
        name:    "bench",
        summary: "run the bench harness",
    },
    Cmd {
        name:    "pdf",
        summary: "render the design documents to PDF",
    },
    Cmd {
        name:    "clean",
        summary: "remove generated output",
    },
    Cmd {
        name:    "migrate",
        summary: "migrate this repo to the current mockspace conventions",
    },
    Cmd {
        name:    "activate",
        summary: "point core.hooksPath at the mockspace gate",
    },
    Cmd {
        name:    "deactivate",
        summary: "restore git's default hooks path",
    },
    Cmd {
        name:    "help",
        summary: "show this message",
    },
];

/// Whether `arg` asks for help in any of its spellings.
#[must_use]
pub(crate) fn is_help_request(arg: &str) -> bool {
    matches!(arg, "help" | "--help" | "-h" | "-?")
}

/// Every known subcommand name, for suggestion and validation.
#[must_use]
pub(crate) fn known_commands() -> Vec<&'static str> {
    COMMANDS.iter().map(|c| c.name).collect()
}

/// Print the help text.
pub(crate) fn print_help() -> ExitCode {
    println!("mock: the mockspace design-round workflow engine.");
    println!();
    println!("USAGE");
    println!("    mock [subcommand] [options]");
    println!();
    println!("    With no subcommand: check, parse, lint, and regenerate the");
    println!("    documents and agent files for the current project.");
    println!();
    println!("SUBCOMMANDS");
    let width = COMMANDS.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in COMMANDS {
        println!("    {:<width$}  {}", c.name, c.summary, width = width);
    }
    println!();
    println!("OPTIONS");
    println!("    --dir <path>       the mock workspace to act on, instead of");
    println!("                       searching upward from the working directory");
    println!("    --scope <crates>   limit linting to a comma-separated crate list");
    println!("    --doc-only         limit linting to documentation");
    println!("    --lint-only        lint without regenerating anything");
    println!("    --commit           lint at the commit gate's severities");
    println!("    --strict           lint at the push gate's severities");
    println!("    --auto-commit      commit the state transition a subcommand makes");
    println!("    --nuke             wipe crate source, leaving stub lib.rs files");
    println!("    -h, --help         show this message");
    println!();
    println!("    --scope and --doc-only are verified against what is staged: a");
    println!("    narrower claim than the staged set is refused rather than obeyed.");
    println!();
    println!("Configuration lives in mockspace.toml at the repo root.");
    ExitCode::SUCCESS
}

/// Report an unrecognised subcommand, suggesting the nearest known one.
pub(crate) fn unknown_subcommand(given: &str) -> ExitCode {
    eprintln!("mock: `{given}` is not a subcommand.");
    if let Some(s) = crate::entry::suggest_subcommand(given) {
        eprintln!();
        eprintln!("  did you mean `mock {s}`?");
    }
    eprintln!();
    eprintln!("available subcommands:");
    for name in known_commands() {
        eprintln!("  {name}");
    }
    eprintln!("\n(run `cargo mock` with no subcommand to regenerate docs and agent rules)");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_help_spelling_is_recognised() {
        for arg in ["help", "--help", "-h", "-?"] {
            assert!(is_help_request(arg), "{arg} should ask for help");
        }
        for arg in ["lock", "--strict", "helper", "-x"] {
            assert!(!is_help_request(arg), "{arg} should not ask for help");
        }
    }

    #[test]
    fn a_close_typo_gets_the_command_it_meant() {
        assert_eq!(crate::entry::suggest_subcommand("lck"), Some("lock"));
        assert_eq!(crate::entry::suggest_subcommand("helpp"), Some("help"));
        assert_eq!(crate::entry::suggest_subcommand("statsu"), Some("status"));
        assert_eq!(crate::entry::suggest_subcommand("depreciate"), Some("deprecate"));
        assert_eq!(crate::entry::suggest_subcommand("actiavte"), Some("activate"));
    }

    #[test]
    fn an_abbreviation_gets_the_command_it_starts() {
        // Someone typing `dep` has not misspelled anything, they stopped early,
        // and a distance threshold alone would reject it.
        assert_eq!(crate::entry::suggest_subcommand("dep"), Some("deprecate"));
        assert_eq!(crate::entry::suggest_subcommand("chec"), Some("check"));
    }

    #[test]
    fn nonsense_gets_no_suggestion() {
        // A wrong guess is worse than none: it sends the reader to try something
        // they did not mean, and they may not notice.
        assert_eq!(crate::entry::suggest_subcommand("xyzzy"), None);
        assert_eq!(crate::entry::suggest_subcommand("frobnicate-everything"), None);
    }

    #[test]
    fn a_single_character_does_not_get_a_wild_guess() {
        // Distance one from many names at once, so any pick would be arbitrary.
        assert_eq!(crate::entry::suggest_subcommand("q"), None);
    }

    #[test]
    fn the_suggestion_is_case_insensitive() {
        assert_eq!(crate::entry::suggest_subcommand("LOCK"), Some("lock"));
        assert_eq!(crate::entry::suggest_subcommand("Lck"), Some("lock"));
    }

    #[test]
    fn every_command_suggests_itself() {
        // The sanity property: nothing in the list is unreachable, which would
        // mean a real command whose exact spelling gets "did you mean" for
        // something else.
        for name in known_commands() {
            assert_eq!(crate::entry::suggest_subcommand(name), Some(name), "{name} should suggest itself");
        }
    }

    #[test]
    fn every_listed_command_has_a_summary() {
        // Help is the surface a stranger reads first; a blank column there is
        // worse than the command being undocumented elsewhere.
        for c in COMMANDS {
            assert!(!c.summary.is_empty(), "{} has no summary", c.name);
            assert!(
                !c.summary.ends_with('.'),
                "{}'s summary should not end with a period",
                c.name
            );
        }
    }

    #[test]
    fn help_lists_itself_so_the_listing_is_complete() {
        assert!(known_commands().contains(&"help"));
    }
}
