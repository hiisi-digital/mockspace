//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

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

use mockspace_lint_rules::tool::ArgSpec;

/// One builtin subcommand, described the same way a project [`Tool`] is.
///
/// [`Tool`]: mockspace_lint_rules::tool::Tool
///
/// Before this carried `args` and `help`, a builtin was a name and a summary
/// and nothing else: a project [`Tool`] could declare its positional
/// arguments and a longer help body, and a builtin could not, so the same
/// engine described its own commands in a thinner shape than it required of
/// everyone else's. `mock tools` (see [`crate::tool_catalogue`]) is what
/// exposed the asymmetry: it has one enumeration to write, and that
/// enumeration needs one shape both populations already fit.
///
/// **Declared, not enforced.** A [`Tool`]'s `args` are checked against a real
/// invocation by [`mockspace_lint_rules::tool::missing_required`] and
/// [`mockspace_lint_rules::tool::contract_faults`]; a builtin's `args` are
/// not checked against anything, because a builtin is dispatched by hand in
/// `dispatch.rs` rather than through the tool contract. See
/// `every_builtin_declaring_a_required_argument_is_named_here_deliberately`
/// below for what that means and why it is pinned rather than merely noted.
pub(crate) struct Cmd {
    pub(crate) name:    &'static str,
    pub(crate) summary: &'static str,
    /// Declared positional arguments, in the same [`ArgSpec`] shape a project
    /// tool uses. Most builtins take their input as flags rather than
    /// positionals and declare none; `panel` is the exception and takes a
    /// verb. The test named above pins the exact set that declares anything,
    /// so a second one cannot appear unnoticed, and the field exists so a
    /// builtin that does take one has somewhere honest to say so,
    /// and so [`usage_from`](mockspace_lint_rules::tool::usage_from) renders
    /// a builtin's usage line exactly the way it renders a tool's.
    pub(crate) args:    &'static [ArgSpec],
    /// Longer help, shown by `mock tools --long`. Empty is allowed: not
    /// every builtin needs more said about it than the summary already says.
    pub(crate) help:    &'static str,
}

/// Every subcommand, in the order help lists them: the round's own lifecycle
/// first, then everything else, because that order is the workflow.
const COMMANDS: &[Cmd] = &[
    Cmd {
        name:    "lock",
        summary: "seal the active changelist, advancing the round's phase",
        args:    &[],
        help:    "Seals the active changelist (doc or src), advancing the round \
                  from DOC to DRAFT or from IMPL to CLOSED. Accepts --auto-commit \
                  to commit the rename with git plumbing (unsigned, no hooks run).",
    },
    Cmd {
        name:    "unlock",
        summary: "deprecate the src changelist and reopen the doc one",
        args:    &[],
        help:    "Deprecates the src changelist and reopens the doc one. \
                  Destructive to the changelist bookkeeping only: source is left \
                  exactly as it is. Accepts --auto-commit.",
    },
    Cmd {
        name:    "deprecate",
        summary: "supersede the active changelist, keeping it as an audit trail",
        args:    &[],
        help:    "Supersedes the active changelist rather than deleting it, so it \
                  stays as an audit trail. In IMPL this also unlocks the doc \
                  changelist. Accepts --auto-commit.",
    },
    Cmd {
        name:    "close",
        summary: "archive a finished round into a timestamped directory",
        args:    &[],
        help:    "Archives a finished round (both changelists locked) into \
                  design_rounds/<timestamp>/ with .meta and .history. Valid only \
                  from CLOSED. Accepts --auto-commit.",
    },
    Cmd {
        name:    "archive",
        summary: "archive an abandoned round without closing it",
        args:    &[],
        help:    "Archives an abandoned round into a <timestamp>-abandoned/ \
                  subdirectory at any phase, without requiring it to reach \
                  CLOSED first. Accepts --auto-commit.",
    },
    Cmd {
        name:    "status",
        summary: "show the current round, its phase, and what may be edited",
        args:    &[],
        help:    "Reports whether the mockspace git hooks are active, and how to \
                  flip that if not.",
    },
    Cmd {
        name:    "check",
        summary: "readiness report: git, phase, build, tests, lints",
        args:    &[],
        help:    "A full readiness report: working-tree cleanliness, branch/remote \
                  sync, round phase, build, tests, and lints, without regenerating \
                  anything.",
    },
    Cmd {
        name:    "check-message",
        summary: "lint one commit message or forge body against the configured policy",
        args:    &[],
        help:    "Lints one authored message against the configured message \
                  policy. --domain selects which policy (see the domain tokens in \
                  the error this prints when it is missing or wrong). --file \
                  <path> reads the message from a file; with no --file it reads \
                  the message from stdin, which is how a hook passes text it \
                  extracted from a command.",
    },
    Cmd {
        name:    "query",
        summary: "query the registry",
        args:    &[],
        help:    "Runs a query expression (a root or namespace path, the same \
                  syntax a `{{ root::selector }}` reference uses) against the \
                  registry. The expression is the token right after `query`; an \
                  absent one queries nothing.",
    },
    Cmd {
        name:    "bench",
        summary: "run the bench harness",
        args:    &[],
        help:    "Runs the bench harness. Everything after `bench` (a variant \
                  name, flags such as --release) is forwarded to it verbatim, so \
                  its own usage governs from that point on.",
    },
    Cmd {
        name:    "panel",
        summary: "mint or consolidate a panel seat, or report a panel's state",
        args:    &[ArgSpec {
            name:        "verb",
            required:    true,
            description: "seat, consolidate, or status",
        }],
        help:    "Mints and consolidates panel seats against a formalised, \
                  indexed inventory file at <mock>/panel/<slug>.toml.\n\n\
                  `mock panel seat <slug> <persona> <topic...>` mints the next \
                  seat number (never counted or guessed; always one past the \
                  highest number already in the file). Refused once the seat \
                  cap (99, the last seat this ever allows) is reached, or once \
                  the consolidation cadence is due and has not been met.\n\n\
                  `mock panel consolidate <slug> <note...>` records a \
                  consolidation covering every seat minted so far. Refused \
                  when there is nothing new to consolidate.\n\n\
                  `mock panel status [slug]` reports one panel's seat count, \
                  whether it is open (has minted seats no consolidation \
                  covers yet), and how many seats remain before the next \
                  consolidation is due. With no slug, reports on every panel \
                  declared under <mock>/panel/.",
    },
    Cmd {
        name:    "test",
        summary: "run the tests of every tree mockspace owns, not only the members",
        args:    &[],
        help:    "Runs `cargo test` over each tree separately: the mock \
                  workspace's own members, every tool crate under \
                  <mock>/tools/, every bench crate under <mock>/benches/, and \
                  the generated lint crate carrying <mock>/lints/*.rs.\n\n\
                  Only the first is reached by a plain `cargo test` in <mock>/. \
                  The other three are compiled by mockspace outside the \
                  workspace, so a repository can hold tools, benches and lints \
                  with tests and run none of them while appearing to. A \
                  repository whose `members` list is empty runs nothing at \
                  all.\n\n\
                  Everything after `test` is forwarded to each cargo \
                  invocation, so `mock test --release` and `mock test -- \
                  --nocapture` both work.\n\n\
                  The lint tree needs the generated crate to exist; `mock \
                  check` generates it. It is reported as absent rather than \
                  generated here, so one path knows how.",
    },
    Cmd {
        name:    "pdf",
        summary: "render the design documents to PDF",
        args:    &[],
        help:    "Renders the design documents to PDF. Everything after `pdf` is \
                  forwarded to the renderer, aside from a --dir <path> already \
                  consumed to locate the mock workspace.",
    },
    Cmd {
        name:    "clean",
        summary: "remove generated output",
        args:    &[],
        help:    "Removes generated output (docs/ and the agent integration \
                  files), so the next run starts from nothing rather than \
                  merging into what is already there.",
    },
    Cmd {
        name:    "migrate",
        summary: "migrate this repo to the current mockspace conventions",
        args:    &[],
        help:    "Renames legacy YYYY-MM-DD_*.md design-round filenames to the \
                  compact timestamp format this repo's convention now expects. \
                  Accepts --auto-commit.",
    },
    Cmd {
        name:    "activate",
        summary: "point core.hooksPath at the mockspace gate",
        args:    &[],
        help:    "Sets core.hooksPath at the generated mockspace hooks. Existing \
                  personal hooks under .git/hooks/ still run first; the generated \
                  hooks source them before doing anything else.",
    },
    Cmd {
        name:    "deactivate",
        summary: "restore git's default hooks path",
        args:    &[],
        help:    "Unsets core.hooksPath, so git falls back to .git/hooks/ \
                  directly.",
    },
    Cmd {
        name:    "tools",
        summary: "list every subcommand and project tool, with usage",
        args:    &[],
        help:    "Enumerates every builtin subcommand and every project tool \
                  declared under <mock>/tools/, each with its usage line and \
                  one-line summary. --long also prints each one's declared \
                  arguments and its longer help text where it has one. This is \
                  the live answer: it is computed at the moment it is asked, \
                  from the same declared shape every tool and every builtin \
                  already carries, rather than from a list somebody \
                  hand-maintained and that a new tool would not appear in.",
    },
    Cmd {
        name:    "help",
        summary: "show this message",
        args:    &[],
        help:    "",
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

/// Every builtin subcommand, in full, for anything that needs more than the
/// bare name: [`crate::tool_catalogue`] is the one caller today.
#[must_use]
pub(crate) fn commands() -> &'static [Cmd] {
    COMMANDS
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
    println!("                       (writes the commit with git plumbing, so no hook");
    println!("                       runs and the commit is not signed)");
    println!("    -h, --help         show this message");
    println!();
    println!("    --scope and --doc-only are verified against what is staged: a");
    println!("    narrower claim than the staged set is refused rather than obeyed.");
    println!();
    println!("Configuration lives in mockspace.toml at the repo root.");
    println!();
    println!("    `--nuke` takes a tier down so it can be written again from the");
    println!("    one above it. `--nuke` alone takes the source, leaving the designs;");
    println!("    `--nuke=docs` takes the designs and the source under them, which is");
    println!("    the order the design chain requires. It names every file first and");
    println!("    then asks; `--y` answers for you. It refuses a tree with anything");
    println!("    uncommitted, because git is the only thing that gives any of it back.");
    ExitCode::SUCCESS
}

/// Report an unrecognised subcommand, suggesting the nearest known one.
///
/// `tools` is the project's own `<mock>/tools/` directory names. They are
/// subcommands this binary cannot know at compile time, and leaving them out
/// would tell a reader who mistyped one that their tool does not exist.
pub(crate) fn unknown_subcommand(given: &str, tools: &[String]) -> ExitCode {
    eprintln!("mock: `{given}` is not a subcommand.");
    if let Some(s) = suggest_among(given, tools) {
        eprintln!();
        eprintln!("  did you mean `mock {s}`?");
    }
    eprintln!();
    eprintln!("available subcommands:");
    for name in known_commands() {
        eprintln!("  {name}");
    }
    if !tools.is_empty() {
        eprintln!();
        eprintln!("tools in this project:");
        for name in tools {
            eprintln!("  {name}");
        }
    }
    eprintln!("\n(run `cargo mock` with no subcommand to regenerate docs and agent rules)");
    ExitCode::from(2)
}

/// The nearest name among the builtins and this project's tools.
///
/// A tool is as likely to be mistyped as a builtin, and it is the one the
/// reader is less sure of the spelling of, since they may have written it
/// themselves that morning.
fn suggest_among(given: &str, tools: &[String]) -> Option<String> {
    let builtin = crate::entry::suggest_subcommand(given).map(str::to_string);
    let lowered = given.to_ascii_lowercase();
    let nearest_tool = tools
        .iter()
        .map(|t| {
            (
                t,
                crate::entry::levenshtein(&lowered, &t.to_ascii_lowercase()),
            )
        })
        .min_by_key(|(_, d)| *d)
        .filter(|(t, d)| *d <= (lowered.len() / 2).max(2) || t.starts_with(&lowered));
    match (builtin, nearest_tool) {
        (Some(b), Some((t, dt))) => {
            let db = crate::entry::levenshtein(&lowered, &b.to_ascii_lowercase());
            Some(if dt < db { t.clone() } else { b })
        },
        (Some(b), None) => Some(b),
        (None, Some((t, _))) => Some(t.clone()),
        (None, None) => None,
    }
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
        assert_eq!(
            crate::entry::suggest_subcommand("depreciate"),
            Some("deprecate")
        );
        assert_eq!(
            crate::entry::suggest_subcommand("actiavte"),
            Some("activate")
        );
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
        assert_eq!(
            crate::entry::suggest_subcommand("frobnicate-everything"),
            None
        );
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
            assert_eq!(
                crate::entry::suggest_subcommand(name),
                Some(name),
                "{name} should suggest itself"
            );
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
    fn a_mistyped_tool_name_suggests_the_tool() {
        // A tool is a subcommand this binary cannot know at compile time, so
        // without this the reader who mistyped their own tool is told it does
        // not exist.
        let tools = vec!["phrase-search".to_string(), "corpus-talk".to_string()];
        assert_eq!(
            super::suggest_among("phrase-serach", &tools),
            Some("phrase-search".to_string())
        );
        assert_eq!(
            super::suggest_among("corpus", &tools),
            Some("corpus-talk".to_string())
        );
    }

    #[test]
    fn a_builtin_still_wins_when_it_is_the_nearer_name() {
        // The case that must fail if tools were simply preferred: `stat` is
        // one edit from a builtin and nowhere near either tool.
        let tools = vec!["phrase-search".to_string(), "corpus-talk".to_string()];
        assert_eq!(
            super::suggest_among("stauts", &tools),
            Some("status".to_string())
        );
        assert_eq!(
            super::suggest_among("lck", &tools),
            Some("lock".to_string())
        );
    }

    #[test]
    fn nonsense_gets_no_suggestion_even_with_tools_present() {
        // The negative arm. Adding a candidate set must not make every input
        // resolve to whichever tool happened to be nearest.
        let tools = vec!["phrase-search".to_string()];
        assert_eq!(super::suggest_among("xyzzy", &tools), None);
        assert_eq!(super::suggest_among("frobnicate-everything", &tools), None);
    }

    #[test]
    fn with_no_tools_the_suggestion_is_exactly_what_it_was_before() {
        // Existing consumers have no tools directory, so this pins that the
        // whole mechanism is inert for them.
        for probe in ["lck", "helpp", "statsu", "dep", "xyzzy", "q"] {
            assert_eq!(
                super::suggest_among(probe, &[]).as_deref(),
                crate::entry::suggest_subcommand(probe),
                "`{probe}` must be unchanged when a project declares no tools"
            );
        }
    }

    #[test]
    fn help_lists_itself_so_the_listing_is_complete() {
        assert!(known_commands().contains(&"help"));
    }
}
