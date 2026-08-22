//! Tools: checks invoked as `mock <name>` because they cannot be lints.
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
//! finding type is how a gate stops gating.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{Level, LintError, LintMode};

// ---------------------------------------------------------------------------
// The outcome
// ---------------------------------------------------------------------------

/// What a check has to say once it has run.
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

// ---------------------------------------------------------------------------
// Why this is not a lint
// ---------------------------------------------------------------------------

/// The reason a tool is a tool rather than a lint.
///
/// **Closed, with no `Other`, and that is the load-bearing part.** An open
/// reason is not a reason: with a free-text escape hatch, every check that is
/// merely inconvenient to gate acquires a plausible sentence, and within a year
/// the gate is empty while every check still looks justified. The inflexibility
/// is what the type is for.
///
/// **Two reasons rather than three**, because a third drafted variant covering
/// checks a person runs before writing rather than before committing turned out
/// to have nothing that could falsify it. A declaration nothing constrains is a
/// comment with a type, so it was cut, and its example
/// (a "has this already been answered" search) is [`Self::TakesAQuestion`]
/// anyway.
///
/// **Cost and history are deliberately absent.** "Too slow for pre-commit" and
/// "needs to read git history" both look like reasons and are not: a repo lint
/// is handed the repository root and may run git itself, and which gates a lint
/// is worth running at is a declaration the lint contract should grow. Admitting
/// either here would drain the gate one reasonable-sounding exemption at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotALint {
    /// It takes a question from the person running it, and a gate has nobody to
    /// ask.
    ///
    /// A configured default does not rescue it, because a configured default is
    /// a different check: a corpus search pinned to one fixed phrase forever
    /// answers nothing anyone wanted to know.
    ///
    /// Constrained by [`contract_faults`]: [`Tool::args`] must declare at least
    /// one required argument, or the claim is false.
    TakesAQuestion,

    /// The answer is the output, and no threshold separates pass from fail.
    ///
    /// An inventory, a ranking, a list of candidates for a judgement somebody
    /// still has to make. Gating on one would mean inventing a threshold nobody
    /// has justified, and an invented threshold is worse than no gate, because
    /// people defend numbers.
    ///
    /// Constrained by [`contract_faults`]: a run may not return findings that
    /// block a gate.
    NoFailingCase,
}

impl NotALint {
    /// The token this reason is written as in reports.
    #[must_use]
    pub fn as_token(self) -> &'static str {
        match self {
            Self::TakesAQuestion => "takes-a-question",
            Self::NoFailingCase => "no-failing-case",
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
}

/// What a tool returns.
pub struct ToolReport {
    /// Clean, findings, or "do not trust this run".
    pub outcome: Outcome,
    /// Rendered for a terminal, printed verbatim. A tool whose whole product is
    /// its output puts it here and returns [`Outcome::Clean`].
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

/// A check invoked as `mock <name>`, because it cannot be a lint.
///
/// One trait rather than a family, for the reason on [`ToolContext`]. What tools
/// vary in is not their input but **why they are not gating**, and that is a
/// method because it does not change the signature.
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

    /// Why this is not a lint.
    ///
    /// **No default implementation, deliberately.** A default is a decision
    /// every author skips, and this declaration is the only thing standing
    /// between a tools directory and a project where nothing gates any more.
    /// Requiring it costs one line and asks the question at the moment the
    /// author is best placed to answer it.
    fn not_a_lint(&self) -> NotALint;

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

    /// Do the work.
    ///
    /// Called only after the engine has checked that every required argument in
    /// [`Self::args`] is present, so a tool need not re-check arity.
    fn run(&self, ctx: &ToolContext<'_>) -> ToolReport;
}

/// Render the usage line for a tool from its declared arguments.
#[must_use]
pub fn usage_line(tool: &dyn Tool) -> String {
    let mut s = format!("mock {}", tool.name());
    for a in tool.args() {
        if a.required {
            s.push_str(&format!(" <{}>", a.name));
        } else {
            s.push_str(&format!(" [{}]", a.name));
        }
    }
    s
}

/// Which required arguments are missing from `args`.
///
/// Positional and in order: the nth declared argument is satisfied by the nth
/// non-flag word. Flags are skipped rather than counted, because a tool that
/// takes `-q` should not have it swallow the phrase it was asked to search for.
#[must_use]
pub fn missing_required<'t>(tool: &'t dyn Tool, args: &[&str]) -> Vec<&'t ArgSpec> {
    let supplied = args.iter().filter(|a| !a.starts_with('-')).count();
    tool.args()
        .iter()
        .enumerate()
        .filter(|(i, a)| a.required && *i >= supplied)
        .map(|(_, a)| a)
        .collect()
}

// ---------------------------------------------------------------------------
// The audit that makes the declaration real
// ---------------------------------------------------------------------------

/// Every way a tool contradicts its own [`Tool::not_a_lint`] declaration.
///
/// This is what stops that declaration being what a declaration becomes when
/// nothing reads it: a field that can hold any value while everything still
/// compiles. Both variants are checked against something the tool actually
/// does, rather than against another declaration.
///
/// [`NotALint::TakesAQuestion`] is checkable statically, from
/// [`Tool::args`]. [`NotALint::NoFailingCase`] can only be checked against a
/// real run, so it is checked at every invocation, which is cheap and fires
/// exactly when the lie would have mattered.
///
/// `report` is `None` when auditing at registration, where no run has happened.
#[must_use]
pub fn contract_faults(tool: &dyn Tool, report: Option<&ToolReport>) -> Vec<String> {
    let mut out = Vec::new();

    if tool.not_a_lint() == NotALint::TakesAQuestion
        && !tool.args().iter().any(|a| a.required)
    {
        out.push(format!(
            "tool `{}` declares `takes-a-question` and has no required argument. \
             Nothing is being asked, so nothing stopped this from being a lint, \
             which would run at a gate instead of waiting to be invoked.",
            tool.name()
        ));
    }

    if let (NotALint::NoFailingCase, Some(r)) = (tool.not_a_lint(), report) {
        // An inconclusive outcome blocks every gate by design, and it is a
        // statement about the instrument rather than about the corpus. Counting
        // it here would report every tool whose controls failed as having lied,
        // which would push authors toward swallowing control failures: the
        // exact opposite of what `Outcome::Inconclusive` is for.
        let inconclusive = matches!(
            r.outcome,
            Outcome::Inconclusive {
                ..
            }
        );
        let blocking = [LintMode::Commit, LintMode::Build, LintMode::Push]
            .into_iter()
            .any(|m| r.outcome.blocks(m));
        if blocking && !inconclusive {
            out.push(format!(
                "tool `{}` declares `no-failing-case` and returned a finding that \
                 blocks a gate. It has a failing case, so it is a lint, and as a \
                 tool that finding blocks nothing until somebody runs it.",
                tool.name()
            ));
        }
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
    seen.into_iter().filter(|(_, n)| *n > 1).map(|(n, _)| n.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        args: &'a [&'a str],
        crates: &'a BTreeSet<String>,
        dirs: &'a [PathBuf],
    ) -> ToolContext<'a> {
        ToolContext {
            mock_dir: Path::new("/mock"),
            repo_root: Path::new("/"),
            all_crates: crates,
            src_dirs: dirs,
            args,
            stdin: None,
        }
    }

    fn empty() -> (BTreeSet<String>, Vec<PathBuf>) {
        (BTreeSet::new(), Vec::new())
    }

    /// Keeps its declaration: it asks for a phrase and gets one.
    struct PhraseSearch;
    impl Tool for PhraseSearch {
        fn name(&self) -> &'static str {
            "phrase-search"
        }
        fn description(&self) -> &'static str {
            "find a phrase across hard-wrapped lines, which grep cannot"
        }
        fn not_a_lint(&self) -> NotALint {
            NotALint::TakesAQuestion
        }
        fn args(&self) -> &[ArgSpec] {
            &[
                ArgSpec {
                    name:        "phrase",
                    required:    true,
                    description: "the phrase to look for",
                },
                ArgSpec {
                    name:        "dir",
                    required:    false,
                    description: "where to look, defaulting to the canon",
                },
            ]
        }
        fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
            ToolReport::reported("3 hits", 241)
        }
    }

    /// Lies: declares a question it never asks.
    struct LiesAboutAsking;
    impl Tool for LiesAboutAsking {
        fn name(&self) -> &'static str {
            "lies-about-asking"
        }
        fn description(&self) -> &'static str {
            "declares a question it never asks"
        }
        fn not_a_lint(&self) -> NotALint {
            NotALint::TakesAQuestion
        }
        fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
            ToolReport::reported("", 1)
        }
    }

    /// Lies the other way: declares no failing case, then blocks a gate.
    struct LiesAboutGating;
    impl Tool for LiesAboutGating {
        fn name(&self) -> &'static str {
            "lies-about-gating"
        }
        fn description(&self) -> &'static str {
            "declares no failing case and then fails one"
        }
        fn not_a_lint(&self) -> NotALint {
            NotALint::NoFailingCase
        }
        fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
            ToolReport {
                outcome: Outcome::Findings(vec![LintError::error(
                    "canon".to_string(),
                    1,
                    "lies-about-gating",
                    "this blocks every gate".to_string(),
                )]),
                output:  String::new(),
            }
        }
    }

    /// Its controls failed. Must not be punished as a liar.
    struct BrokenInstrument;
    impl Tool for BrokenInstrument {
        fn name(&self) -> &'static str {
            "broken-instrument"
        }
        fn description(&self) -> &'static str {
            "its positive control did not match"
        }
        fn not_a_lint(&self) -> NotALint {
            NotALint::NoFailingCase
        }
        fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
            ToolReport::inconclusive("positive control did not match")
        }
    }

    // -- the audit discriminates, which is what makes the declaration real ---

    #[test]
    fn an_honest_tool_passes_the_audit() {
        let (c, d) = empty();
        let r = PhraseSearch.run(&ctx(&["needle"], &c, &d));
        assert_eq!(contract_faults(&PhraseSearch, Some(&r)), Vec::<String>::new());
    }

    #[test]
    fn a_tool_declaring_a_question_it_never_asks_is_caught() {
        // The case that must fail. Without it `not_a_lint` is a field that
        // accepts any value while everything still compiles.
        let found = contract_faults(&LiesAboutAsking, None);
        assert_eq!(found.len(), 1, "expected one fault, got {found:?}");
        assert!(found[0].contains("no required argument"), "{found:?}");
    }

    #[test]
    fn a_tool_declaring_no_failing_case_that_blocks_a_gate_is_caught() {
        let (c, d) = empty();
        let r = LiesAboutGating.run(&ctx(&[], &c, &d));
        let found = contract_faults(&LiesAboutGating, Some(&r));
        assert_eq!(found.len(), 1, "expected one fault, got {found:?}");
        assert!(found[0].contains("has a failing case"), "{found:?}");
    }

    #[test]
    fn a_failed_control_is_not_punished_as_a_failing_case() {
        // The control on the control. `Inconclusive` blocks every gate by
        // design, so a naive audit reports every tool whose instrument broke as
        // a liar, which would teach authors to swallow control failures.
        let (c, d) = empty();
        let r = BrokenInstrument.run(&ctx(&[], &c, &d));
        assert_eq!(contract_faults(&BrokenInstrument, Some(&r)), Vec::<String>::new());
        assert!(r.outcome.blocks(LintMode::Commit), "inconclusive must block");
    }

    #[test]
    fn registration_time_audit_cannot_see_a_gating_lie_and_says_so_by_finding_nothing() {
        // `None` is the registration-time call, where no run has happened. It
        // must not guess. This pins that the gating check is genuinely
        // deferred rather than silently answered as clean at registration.
        assert_eq!(contract_faults(&LiesAboutGating, None), Vec::<String>::new());
        // and the same tool IS caught once a run exists, so the emptiness above
        // is deferral rather than blindness
        let (c, d) = empty();
        let r = LiesAboutGating.run(&ctx(&[], &c, &d));
        assert_eq!(contract_faults(&LiesAboutGating, Some(&r)).len(), 1);
    }

    // -- the three-valued outcome is genuinely three-valued ------------------

    #[test]
    fn the_three_outcomes_are_three_and_not_two() {
        let clean = Outcome::Clean {
            examined: 10,
        };
        let found = Outcome::Findings(vec![LintError::error("c".to_string(), 1, "l", "x".into())]);
        let inconc = Outcome::Inconclusive {
            reason: "controls failed".into(),
        };

        assert!(!clean.blocks(LintMode::Commit));
        assert!(found.blocks(LintMode::Commit));
        // If this behaved like `clean` the whole variant would be decoration.
        assert!(inconc.blocks(LintMode::Commit));

        // And inconclusive is not findings: nothing downstream can render a
        // broken instrument as a corpus defect.
        assert!(inconc.findings().is_empty());
        assert_eq!(found.findings().len(), 1);
    }

    #[test]
    fn a_warning_only_finding_does_not_block_but_is_still_a_finding() {
        // The distinction between "no findings" and "findings that do not
        // block" is what lets a tool report without gating, and it is the
        // property `NoFailingCase` leans on.
        let warn = Outcome::Findings(vec![LintError::warning(
            "c".to_string(),
            1,
            "l",
            "advisory".into(),
        )]);
        assert!(!warn.blocks(LintMode::Commit));
        assert_eq!(warn.findings().len(), 1);
    }

    #[test]
    fn a_clean_verdict_carries_what_it_examined() {
        // A clean verdict over an empty population is vacuous, and this count
        // is the only thing that distinguishes it from a real pass.
        let vacuous = Outcome::Clean {
            examined: 0,
        };
        let real = Outcome::Clean {
            examined: 241,
        };
        match (&vacuous, &real) {
            (
                Outcome::Clean {
                    examined: a,
                },
                Outcome::Clean {
                    examined: b,
                },
            ) => {
                assert_eq!(*a, 0);
                assert_eq!(*b, 241);
            },
            _ => panic!("wrong variant"),
        }
    }

    // -- argument declaration ------------------------------------------------

    #[test]
    fn a_missing_required_argument_is_reported_before_the_tool_runs() {
        assert_eq!(missing_required(&PhraseSearch, &[]).len(), 1);
        assert_eq!(missing_required(&PhraseSearch, &["needle"]).len(), 0);
        // the optional one being absent is not a fault
        assert_eq!(missing_required(&PhraseSearch, &["needle"]).len(), 0);
    }

    #[test]
    fn a_flag_does_not_satisfy_a_required_argument() {
        // The case that must fail, and the reason this is not `args.len()`:
        // `mock phrase-search -q` supplied a flag and no phrase, and counting
        // words would have called that satisfied and handed the tool nothing.
        let missing = missing_required(&PhraseSearch, &["-q"]);
        assert_eq!(missing.len(), 1, "a flag must not stand in for the phrase");
        assert_eq!(missing[0].name, "phrase");
        // and a flag alongside a real argument is still fine
        assert_eq!(missing_required(&PhraseSearch, &["-q", "needle"]).len(), 0);
    }

    #[test]
    fn usage_distinguishes_required_from_optional() {
        assert_eq!(usage_line(&PhraseSearch), "mock phrase-search <phrase> [dir]");
        assert_eq!(usage_line(&LiesAboutAsking), "mock lies-about-asking");
    }

    // -- name collisions -----------------------------------------------------

    #[test]
    fn two_tools_sharing_a_name_are_reported() {
        struct A;
        impl Tool for A {
            fn name(&self) -> &'static str {
                "audit"
            }
            fn description(&self) -> &'static str {
                "one"
            }
            fn not_a_lint(&self) -> NotALint {
                NotALint::NoFailingCase
            }
            fn run(&self, _c: &ToolContext<'_>) -> ToolReport {
                ToolReport::reported("", 1)
            }
        }
        struct B;
        impl Tool for B {
            fn name(&self) -> &'static str {
                "audit"
            }
            fn description(&self) -> &'static str {
                "two"
            }
            fn not_a_lint(&self) -> NotALint {
                NotALint::NoFailingCase
            }
            fn run(&self, _c: &ToolContext<'_>) -> ToolReport {
                ToolReport::reported("", 1)
            }
        }
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(A), Box::new(B)];
        assert_eq!(duplicate_tool_names(&tools), vec!["audit".to_string()]);

        // the negative: distinct names report nothing, so the check above is
        // detecting the collision rather than the count
        let ok: Vec<Box<dyn Tool>> = vec![Box::new(PhraseSearch), Box::new(LiesAboutAsking)];
        assert_eq!(duplicate_tool_names(&ok), Vec::<String>::new());
    }
}
