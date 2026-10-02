//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Tools: what a project runs as `mock <name>`, either a check that cannot be
//! a lint or a maker that writes files.
//!
//! # Why this exists beside the lint family
//!
//! A lint runs at a gate. It is handed its input by the engine, it answers a
//! question nobody asked it, and its findings block a commit, a build or a
//! push. That covers most of what a project wants to check, and where it
//! covers something, it is the better answer: a lint that runs pre-commit
//! stops bad state being committed at all, which no report can do.
//!
//! Two things it structurally cannot cover, and they are the two [`NotALint`]
//! variants. Everything else that looks like a third reason turns out to be
//! either a cost concern or a gap in the lint contract, and the honest fix for
//! those is to grow the lint contract rather than to widen this one.
//!
//! # Makers
//!
//! Not everything a project runs by name is a check. A generator, a
//! scaffolder or an exporter produces files, and the question a gate would ask
//! of it does not arise. Such a tool declares [`Purpose::Make`] with the paths
//! it writes instead of a [`NotALint`] reason, and is held to those paths: the
//! engine observes what changed across the run and reports anything outside
//! the declaration as a contract fault. A declared path may start with one of
//! the tool's arguments, for a maker whose output location is chosen on the
//! command line, and that argument then has a fixed root its value must fall
//! under; [`writes`] resolves and checks it. A maker has no way to express a
//! gating judgement: its findings report what it produced, at warning or
//! info, and a finding at error at any gate is a contract fault. A make that
//! fails returns `Inconclusive`, saying what failed on which input. Gating
//! stays with checks and lints. [`purpose`] carries the declaration and its
//! audit.
//!
//! # The failure this is shaped against
//!
//! Before this existed, something in this repository needed findings from a
//! check that was not a lint, had no shape to put them in, and grew its own:
//! a bespoke finding struct with its own kind and message, printed with the
//! word ERROR, wired to nothing. A registry declaring one identifier twice
//! exited zero, exactly as a sound one did, for as long as that lasted.
//!
//! So the rule this module enforces is not stylistic. **A tool's findings are
//! [`LintError`], the same type a lint produces.** Severity configuration then
//! works unchanged, rendering is shared, and a tool that turns out to be
//! gateable becomes a lint without rewriting a line of its findings. A third
//! finding type is how a gate stops gating. A maker reports what it produced
//! with the same type, at warning or info, rather than in a shape of its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{Level, LintError, LintMode};

pub mod purpose;
pub mod writes;
pub use purpose::{NotALint, Purpose, contract_faults, maker_faults};
pub use writes::{ArgRoot, resolve_writes};

// ---------------------------------------------------------------------------
// The outcome
// ---------------------------------------------------------------------------

/// What a tool has to say once it has run. For a maker, `Clean` is a make
/// that succeeded, `Findings` is advisory, and a make that failed is
/// `Inconclusive`.
///
/// `Vec<LintError>` is two-valued: empty is clean, non-empty is findings. It
/// has no way to say **"do not trust this run"**, so a check whose own controls
/// failed must either return empty, reporting a pass it never established, or
/// invent a finding, lying about the thing it was checking. Both are worse than
/// the truth, and the truth has nowhere to go.
///
/// That third value is not a new idea here. It had been reached for
/// independently three times, in two languages, with no cross-citation:
///
/// - a corpus of check scripts reserving exit code 2 for `INSTRUMENT FAILURE`,
///   keeping 1 for the corpus being wrong;
/// - [`crate::Severity`]'s neighbour in the registry, `SchemaCheck::Unavailable`,
///   whose own comment says a check that silently does not run "produces the
///   same green output as a check that ran and found nothing";
/// - a shipped test named `a_schema_check_that_examined_nothing_is_not_a_pass`.
///
/// A concept three parties reach separately and no shared type expresses is the
/// definition of something belonging in the contract.
#[derive(Debug, Clone)]
pub enum Outcome {
    /// Ran over a population and found nothing wrong.
    ///
    /// `examined` is required rather than optional, and it is not decoration. A
    /// clean verdict over an empty population is vacuous, and this count is the
    /// only thing distinguishing it from a real pass. The engine reports the
    /// two differently. It is not *refused* here, because an empty population
    /// is sometimes legitimate: [`ToolContext::all_crates`] is empty in a
    /// documentation repository, and that is a valid state rather than a fault.
    Clean {
        examined: usize,
    },

    /// Ran, and found these. Each finding's own severity decides which gates it
    /// blocks, exactly as a lint's does.
    Findings(Vec<LintError>),

    /// Could not answer: a control failed, a dependency is absent, an input was
    /// unreadable, or the population was empty in a way that makes a clean
    /// verdict meaningless.
    ///
    /// **Never a pass.** It blocks, and it blocks at every gate. The cheap
    /// alternative is to warn and carry on, and warning-and-carrying-on is
    /// precisely how a broken instrument survives for months looking green.
    Inconclusive {
        reason: String,
    },
}

impl Outcome {
    /// Whether this outcome should stop the run at `mode`.
    #[must_use]
    pub fn blocks(&self, mode: LintMode) -> bool {
        match self {
            Self::Clean {
                ..
            } => false,
            Self::Inconclusive {
                ..
            } => true,
            Self::Findings(f) => f.iter().any(|e| e.severity.effective(mode) == Level::Error),
        }
    }

    /// The findings. Empty for the other two variants, which is deliberate:
    /// an inconclusive run has no corpus findings to render, and rendering it
    /// as though it did would misreport a broken instrument as a broken corpus.
    #[must_use]
    pub fn findings(&self) -> &[LintError] {
        match self {
            Self::Findings(f) => f,
            _ => &[],
        }
    }
}

/// One declared argument.
///
/// Declared rather than parsed inside `run`, so that `mock help` can render
/// usage without running anything and the engine can refuse a missing required
/// argument before the tool is entered. Hand-rolled argument handling is where
/// a small CLI rots first, and every tool doing it slightly differently is the
/// state this replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArgSpec {
    /// Shown in usage. Conventionally lowercase, no angle brackets: the
    /// renderer adds those.
    pub name:        &'static str,
    /// A missing required argument is refused before `run` is called.
    pub required:    bool,
    /// One line, for `mock help <tool>`.
    pub description: &'static str,
}

// ---------------------------------------------------------------------------
// What a tool is handed
// ---------------------------------------------------------------------------

/// Context for a [`Tool`].
///
/// **Deliberately not tiered the way the lint family is**, and the reason is
/// worth stating because the symmetry is tempting. [`crate::CrateLint`],
/// [`crate::WorkspaceLint`], [`crate::RepoLint`] and [`crate::MessageLint`] are
/// keyed on their input because a lint's input is chosen *for it* by the
/// engine, so which one it gets is the distinction that carries information.
///
/// A tool's input is chosen by the person running it. Tiering tools the same
/// way would produce several traits that all take the same argument vector and
/// differ in nothing, which is ceremony wearing the lint family's clothes.
///
/// The path fields mirror [`crate::RepoContext`] rather than inventing a second
/// spelling of the same thing. Where the two ever disagree, `RepoContext` is
/// the one to follow.
pub struct ToolContext<'a> {
    /// Root of the mock workspace.
    pub mock_dir:   &'a Path,
    /// Root of the repository containing it.
    pub repo_root:  &'a Path,
    /// Every crate directory name in the workspace. **Empty is legitimate**, and
    /// a tool must behave when it is: a documentation repository has no crates
    /// and is exactly the kind of project that wants tools.
    pub all_crates: &'a BTreeSet<String>,
    /// Every directory holding source packages, absolute, in config order.
    pub src_dirs:   &'a [PathBuf],
    /// Everything after the tool's own name on the command line, flags
    /// included, in order, verbatim.
    pub args:       &'a [&'a str],
    /// Piped input, when there was any.
    pub stdin:      Option<&'a str>,
    /// The project's registry, flattened, with the reverse edges computed.
    ///
    /// A tool that inventories what the registry holds, or answers whether a
    /// subject is already taken, reads the same rows `mock query` resolves
    /// against. Empty where the project declares no registry.
    pub registry:   &'a crate::RegistryView,
}

/// What a tool returns.
pub struct ToolReport {
    /// Clean, findings, or "do not trust this run".
    pub outcome: Outcome,
    /// Rendered for a terminal, printed verbatim. A check whose whole product is
    /// its output puts it here and returns [`Outcome::Clean`]. A maker's product
    /// is what it wrote, and it does not list that here: the engine observes
    /// the writes itself and reports them, so the list cannot disagree with the
    /// tree.
    pub output:  String,
}

impl ToolReport {
    /// A report that is only output, examining `examined` things.
    #[must_use]
    pub fn reported(output: impl Into<String>, examined: usize) -> Self {
        Self {
            outcome: Outcome::Clean {
                examined,
            },
            output:  output.into(),
        }
    }

    /// A report whose instrument failed.
    #[must_use]
    pub fn inconclusive(reason: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Inconclusive {
                reason: reason.into(),
            },
            output:  String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// Something a project runs as `mock <name>`: a check that cannot be a lint,
/// or a maker that writes files.
///
/// One trait rather than a family, for the reason on [`ToolContext`]. What tools
/// vary in is not their input but **what they are for**: a check and why it is
/// not gating, or a maker and what it writes. That is a method because it does
/// not change the signature.
pub trait Tool {
    /// The subcommand name.
    ///
    /// For a tool discovered under `mock/tools/<dir>/`, this must equal `<dir>`.
    /// The engine checks it and refuses a mismatch rather than picking one,
    /// because a tool invocable under a name its own source does not know is a
    /// tool nobody can grep for.
    fn name(&self) -> &'static str;

    /// One line for `mock help`.
    fn description(&self) -> &'static str;

    /// What this tool is for: a check, with the reason it is not a lint, or a
    /// maker, with what it writes.
    ///
    /// **No default implementation, deliberately.** A default is a decision
    /// every author skips, and for a check this declaration is the only thing
    /// standing between a tools directory and a project where nothing gates any
    /// more. Requiring it costs one line and asks the question at the moment
    /// the author is best placed to answer it. A check cannot be declared
    /// without its [`NotALint`] reason, since [`Purpose::Check`] carries one.
    /// A maker declares the paths it writes, with a root for any argument a
    /// path starts with, and its findings never block:
    /// one at error at any gate is a contract fault, and a make that fails
    /// returns [`Outcome::Inconclusive`]. See [`Purpose::Make`].
    fn purpose(&self) -> Purpose;

    /// Declared arguments, in the order they are expected.
    fn args(&self) -> &[ArgSpec] {
        &[]
    }

    /// Longer help, shown under the usage line. Optional.
    fn help(&self) -> &'static str {
        ""
    }

    /// Whether this tool wants piped standard input.
    ///
    /// **Opt-in, and the default is what stops `mock <tool>` hanging.** Reading
    /// stdin whenever it is not a terminal sounds like the careful version and
    /// is the opposite: every non-interactive caller (a git hook, CI, an agent,
    /// a shell pipeline whose writer has not finished) hands over a pipe that
    /// does not close, and the read blocks forever while looking exactly like
    /// a slow tool.
    ///
    /// Found by hanging. The first version of this contract had no such flag
    /// and read stdin whenever `!is_terminal()`, which deadlocked the very
    /// first end-to-end invocation from a non-interactive shell.
    ///
    /// Same shape and same reason as [`crate::Lint::invocation_wanted`], which
    /// is opt-in because most lints do not care and the engine does not always
    /// have one to give.
    fn wants_stdin(&self) -> bool {
        false
    }

    /// Flag names that consume the token after them, so [`missing_required`]
    /// does not read that token as satisfying a positional argument.
    ///
    /// Declared rather than inferred: [`missing_required`] cannot tell
    /// `--root src` from `-q needle` apart by shape alone, since both are one
    /// flag followed by one plain word, and guessing which flags take a value
    /// is exactly the kind of inference this contract avoids elsewhere.
    /// Mirrors the dispatcher's own value-taking globals (`--dir`, `--scope`,
    /// `--mockspace-lint-rules-dep`), which are consumed the same way one
    /// layer up, before a tool ever sees its argument vector.
    ///
    /// Empty by default: a tool with no value-taking flags of its own needs
    /// nothing here, and every tool that predates this method keeps its
    /// current (correct) behaviour.
    fn value_flags(&self) -> &[&'static str] {
        &[]
    }

    /// Do the work.
    ///
    /// Called only after the engine has checked that every required argument in
    /// [`Self::args`] is present, so a tool need not re-check arity.
    fn run(&self, ctx: &ToolContext<'_>) -> ToolReport;
}

/// Render a usage line from a bare name and a set of declared arguments,
/// with no [`Tool`] object required.
///
/// Factored out of [`usage_line`] so a caller describing something that is
/// not a `dyn Tool`, a builtin subcommand, say, can render the identical
/// usage shape from the same two facts every [`Tool`] already carries: its
/// name and its declared arguments. Both callers now go through one
/// definition, so a builtin and a project tool render usage identically by
/// construction rather than by two authors independently matching the
/// bracket convention.
#[must_use]
pub fn usage_from(name: &str, args: &[ArgSpec]) -> String {
    let mut s = format!("mock {name}");
    for a in args {
        if a.required {
            s.push_str(&format!(" <{}>", a.name));
        } else {
            s.push_str(&format!(" [{}]", a.name));
        }
    }
    s
}

/// Render the usage line for a tool from its declared arguments.
#[must_use]
pub fn usage_line(tool: &dyn Tool) -> String {
    usage_from(tool.name(), tool.args())
}

/// Which required arguments are missing from `args`.
///
/// Positional and in order: the nth declared argument is satisfied by the nth
/// non-flag word. Two kinds of token are skipped rather than counted: a bare
/// flag, because a tool that takes `-q` should not have it swallow the phrase
/// it was asked to search for, and a flag declared in [`Tool::value_flags`]
/// together with the one token after it, because that token is the flag's
/// value rather than a positional word.
///
/// Without the second half, `mock search --root src` counted `src` as the
/// tool's positional argument. It is `--root`'s value; the tool never
/// declared `--root` as anything the engine understands, so nothing told this
/// function to skip it, and a required `phrase` read as supplied when it was
/// not.
#[must_use]
pub fn missing_required<'t>(tool: &'t dyn Tool, args: &[&str]) -> Vec<&'t ArgSpec> {
    let supplied = positional_args(tool, args).len();
    tool.args()
        .iter()
        .enumerate()
        .filter(|(i, a)| a.required && *i >= supplied)
        .map(|(_, a)| a)
        .collect()
}

/// The positional words of `args`, in order, read the way
/// [`missing_required`] reads them: the nth is the value of the nth declared
/// argument. A bare flag is skipped, and so is a [`Tool::value_flags`] flag
/// with the token after it.
///
/// One definition for both, so the argument a maker's writes are resolved
/// against is the one the arity check counted as supplied.
#[must_use]
pub fn positional_args<'a>(tool: &dyn Tool, args: &[&'a str]) -> Vec<&'a str> {
    let value_flags = tool.value_flags();
    let mut out = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if value_flags.contains(a) {
            skip_next = true;
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        out.push(*a);
    }
    out
}

/// Every tool name registered more than once, sorted.
///
/// The lint side already refuses this for lints, after an incident where a pack
/// and the builtin set both registered one name, every finding doubled, and the
/// config could not address either copy. A tool name collision is worse, since
/// a name is also a subcommand: two tools called `audit` means `mock audit` runs
/// whichever the loader happened to push first.
#[must_use]
pub fn duplicate_tool_names(tools: &[Box<dyn Tool>]) -> Vec<String> {
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for t in tools {
        *seen.entry(t.name()).or_insert(0) += 1;
    }
    seen.into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(n, _)| n.to_string())
        .collect()
}

#[cfg(test)]
mod purpose_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod writes_tests;
