//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What a tool is for, and the audit that holds it to what it said.
//!
//! A tool is one of two things. A **check** that cannot be a lint, which says
//! why in one of the two closed [`NotALint`] reasons, or a **maker**, which
//! writes files and says which ones. The two are told apart in one declaration,
//! [`super::Tool::purpose`], because they are held to different things: a check
//! to the reason it gave for not gating, a maker to the paths it said it would
//! touch.
//!
//! A maker is not a third reason for a check to escape the gate. It answers a
//! different question, "what does running this produce", and a check that
//! writes nothing cannot claim to be one: a maker declaring no writes is
//! refused, so the declaration cannot be borrowed as a way out of
//! [`NotALint`].

use super::{Outcome, Tool, ToolReport};
use crate::{LintMode, glob_match_anchored};

/// The reason a check is a tool rather than a lint.
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

/// What a tool is for.
///
/// No default, for the reason on [`Tool::purpose`]. Both arms carry the thing
/// they are audited against, so neither can be declared without it: a check
/// cannot be written without a [`NotALint`] reason, and a maker cannot be
/// written without the list of what it writes, though that list can be empty
/// and is refused at registration when it is.
///
/// The control, which compiles: a check with its reason.
///
/// ```
/// use mockspace_lint_rules::tool::{
///     NotALint,
///     Purpose,
///     Tool,
///     ToolContext,
///     ToolReport,
/// };
/// struct Search;
/// impl Tool for Search {
///     fn name(&self) -> &'static str {
///         "search"
///     }
///
///     fn description(&self) -> &'static str {
///         "find a phrase"
///     }
///
///     fn purpose(&self) -> Purpose {
///         Purpose::Check(NotALint::NoFailingCase)
///     }
///
///     fn run(&self, _: &ToolContext<'_>) -> ToolReport {
///         ToolReport::reported("", 1)
///     }
/// }
/// ```
///
/// A tool that says nothing about what it is for does not compile:
///
/// ```compile_fail,E0046
/// use mockspace_lint_rules::tool::{Tool, ToolContext, ToolReport};
/// struct Search;
/// impl Tool for Search {
///     fn name(&self) -> &'static str { "search" }
///     fn description(&self) -> &'static str { "find a phrase" }
///     fn run(&self, _: &ToolContext<'_>) -> ToolReport { ToolReport::reported("", 1) }
/// }
/// ```
///
/// and neither does a check that gives no reason:
///
/// ```compile_fail,E0061
/// use mockspace_lint_rules::tool::{Purpose, Tool, ToolContext, ToolReport};
/// struct Search;
/// impl Tool for Search {
///     fn name(&self) -> &'static str { "search" }
///     fn description(&self) -> &'static str { "find a phrase" }
///     fn purpose(&self) -> Purpose { Purpose::Check() }
///     fn run(&self, _: &ToolContext<'_>) -> ToolReport { ToolReport::reported("", 1) }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// A check that cannot be a lint, and why.
    ///
    /// Its findings are corpus findings, and the reason is held to what the
    /// tool declares and does exactly as before makers existed.
    Check(NotALint),

    /// A maker: running it produces files.
    ///
    /// A generator, a scaffolder, a renderer, an exporter. Its product is what
    /// it writes rather than what it prints, so the question a gate would ask
    /// of it does not arise and it needs no [`NotALint`] reason.
    ///
    /// `writes` is every path it may write, create or delete, as patterns
    /// relative to the repository root in the [`crate::path_filter`] syntax,
    /// **anchored**: `CHANGELOG.md` means the one at the root and not every
    /// file of that name, which is where this differs from a lint's path
    /// filter. A failed make returns [`Outcome::Findings`], and a make whose
    /// own inputs or controls failed returns [`Outcome::Inconclusive`] and
    /// blocks as any tool's does.
    Make {
        /// What it may write, relative to the repository root. Never empty, and
        /// never a pattern that admits the whole tree: both are refused by
        /// [`contract_faults`] before the tool runs.
        writes: &'static [&'static str],
    },
}

impl Purpose {
    /// The one-word kind, `check` or `make`, for a column in a listing.
    #[must_use]
    pub fn kind(self) -> &'static str {
        match self {
            Self::Check(_) => "check",
            Self::Make {
                ..
            } => "make",
        }
    }

    /// The whole declaration, for help: the kind and what it is held to.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Self::Check(r) => format!("check, {}", r.as_token()),
            Self::Make {
                writes,
            } if writes.is_empty() => "make, declares nothing it writes".to_string(),
            Self::Make {
                writes,
            } => {
                let list: Vec<String> = writes.iter().map(|w| format!("`{w}`")).collect();
                format!("make, writes {}", list.join(", "))
            },
        }
    }
}

/// Why a declared write pattern is refused, or `None` when it is usable.
///
/// Three shapes are refused, each because it constrains nothing: an empty
/// pattern, one that leaves the repository (absolute, or with a `..` segment),
/// and one made only of wildcards with a `**` among them, which admits every
/// path in the tree. Declaring `**` would satisfy "declares what it writes"
/// while saying nothing at all, which is the same hole as an open
/// [`NotALint`].
fn refused_pattern(pattern: &str) -> Option<&'static str> {
    let p = pattern.strip_prefix("./").unwrap_or(pattern);
    if p.is_empty() {
        return Some("is empty");
    }
    if p.starts_with('/') || p.split('/').any(|s| s == "..") {
        return Some("leaves the repository root");
    }
    let segs: Vec<&str> = p.split('/').collect();
    if segs.contains(&"**") && segs.iter().all(|s| s.chars().all(|c| c == '*')) {
        return Some("admits every path in the tree");
    }
    None
}

/// Every way a tool contradicts its own [`Tool::purpose`] declaration.
///
/// This is what stops that declaration being what a declaration becomes when
/// nothing reads it: a field that can hold any value while everything still
/// compiles. **Not every arm is checked against something the tool actually
/// does; some are checked against a second declaration**, and that is a real
/// gap rather than a stated equivalence.
///
/// [`NotALint::NoFailingCase`] is checked against a real run: a tool that
/// declared no failing case and then returned a blocking finding has been
/// caught doing the thing, not merely declaring around it.
///
/// [`NotALint::TakesAQuestion`] is checked against [`Tool::args`], which is
/// itself only a declaration. This catches a tool that claims to ask a
/// question while declaring no required argument to ask it with, but it does
/// **not** catch a tool that declares a required argument and then ignores
/// it, never reading it from `ctx.args`. Closing that gap would mean running
/// the tool under varied inputs and diffing its behaviour, which this
/// contract does not attempt.
///
/// [`Purpose::Make`] is checked here only for the shape of its declaration:
/// at least one pattern, and none that constrains nothing. What it actually
/// wrote is checked against that declaration by [`undeclared_writes`], over
/// paths the engine observed rather than paths the tool reported, because a
/// report would be the thing under audit and the evidence for it at once.
///
/// `report` is `None` when auditing at registration, where no run has happened.
#[must_use]
pub fn contract_faults(tool: &dyn Tool, report: Option<&ToolReport>) -> Vec<String> {
    let mut out = Vec::new();

    match tool.purpose() {
        Purpose::Check(NotALint::TakesAQuestion) => {
            if !tool.args().iter().any(|a| a.required) {
                out.push(format!(
                    "tool `{}` declares `takes-a-question` and has no required argument. \
                     Nothing is being asked, so nothing stopped this from being a lint, \
                     which would run at a gate instead of waiting to be invoked.",
                    tool.name()
                ));
            }
        },
        Purpose::Check(NotALint::NoFailingCase) => {
            if let Some(r) = report {
                // An inconclusive outcome blocks every gate by design, and it is
                // a statement about the instrument rather than about the corpus.
                // Counting it here would report every tool whose controls failed
                // as having lied, which would push authors toward swallowing
                // control failures: the exact opposite of what
                // `Outcome::Inconclusive` is for.
                let inconclusive = matches!(r.outcome, Outcome::Inconclusive { .. });
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
        },
        Purpose::Make {
            writes,
        } => {
            if writes.is_empty() {
                out.push(format!(
                    "tool `{}` declares itself a maker and declares nothing it writes. \
                     A maker is held to the paths it names; naming none leaves nothing \
                     to hold it to, and a tool that writes nothing is a check, which \
                     says why it is not a lint.",
                    tool.name()
                ));
            }
            for w in writes {
                if let Some(why) = refused_pattern(w) {
                    out.push(format!(
                        "tool `{}` declares it writes `{w}`, which {why}. A declared write \
                         has to name part of the repository, or it constrains nothing.",
                        tool.name()
                    ));
                }
            }
        },
    }

    out
}

/// Every observed write outside what a maker declared, as one fault per path.
///
/// `written` is repository-relative, `/`-separated, and is what the engine saw
/// change across the run, not what the tool says it did. A check that writes
/// is not audited here: its declaration says nothing about writes, so there is
/// nothing to compare them against, and an empty result for a check is the
/// absence of a declaration rather than a pass.
#[must_use]
pub fn undeclared_writes(tool: &dyn Tool, written: &[String]) -> Vec<String> {
    let Purpose::Make {
        writes,
    } = tool.purpose()
    else {
        return Vec::new();
    };
    written
        .iter()
        .filter(|p| !writes.iter().any(|w| glob_match_anchored(w, p)))
        .map(|p| {
            format!(
                "tool `{}` wrote `{p}`, which is outside what it declares it writes ({}).",
                tool.name(),
                Purpose::Make {
                    writes,
                }
                .describe()
                .trim_start_matches("make, writes ")
            )
        })
        .collect()
}
