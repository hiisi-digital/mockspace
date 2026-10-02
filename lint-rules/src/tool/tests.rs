//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Unit tests for the tool contract, kept apart from it so neither file carries the other.

use super::*;

fn ctx<'a>(
    args: &'a [&'a str],
    crates: &'a BTreeSet<String>,
    dirs: &'a [PathBuf],
    registry: &'a crate::RegistryView,
) -> ToolContext<'a> {
    ToolContext {
        mock_dir: Path::new("/mock"),
        repo_root: Path::new("/"),
        all_crates: crates,
        src_dirs: dirs,
        args,
        stdin: None,
        registry,
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

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::TakesAQuestion)
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

/// Declares one required positional and one flag that takes a value.
struct RootedSearch;
impl Tool for RootedSearch {
    fn name(&self) -> &'static str {
        "rooted-search"
    }

    fn description(&self) -> &'static str {
        "find a phrase under a declared root"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::TakesAQuestion)
    }

    fn args(&self) -> &[ArgSpec] {
        &[ArgSpec {
            name:        "phrase",
            required:    true,
            description: "the phrase to look for",
        }]
    }

    fn value_flags(&self) -> &[&'static str] {
        &["--root"]
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
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

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::TakesAQuestion)
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

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::NoFailingCase)
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

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::NoFailingCase)
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::inconclusive("positive control did not match")
    }
}

// -- the audit discriminates, which is what makes the declaration real ---

#[test]
fn an_honest_tool_passes_the_audit() {
    let (c, d) = empty();
    let r = PhraseSearch.run(&ctx(&["needle"], &c, &d, &Default::default()));
    assert_eq!(
        contract_faults(&PhraseSearch, Some(&r)),
        Vec::<String>::new()
    );
}

#[test]
fn a_tool_declaring_a_question_it_never_asks_is_caught() {
    // The case that must fail. Without it `purpose` is a field that
    // accepts any value while everything still compiles.
    let found = contract_faults(&LiesAboutAsking, None);
    assert_eq!(found.len(), 1, "expected one fault, got {found:?}");
    assert!(found[0].contains("no required argument"), "{found:?}");
}

#[test]
fn a_tool_declaring_no_failing_case_that_blocks_a_gate_is_caught() {
    let (c, d) = empty();
    let r = LiesAboutGating.run(&ctx(&[], &c, &d, &Default::default()));
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
    let r = BrokenInstrument.run(&ctx(&[], &c, &d, &Default::default()));
    assert_eq!(
        contract_faults(&BrokenInstrument, Some(&r)),
        Vec::<String>::new()
    );
    assert!(
        r.outcome.blocks(LintMode::Commit),
        "inconclusive must block"
    );
}

#[test]
fn registration_time_audit_cannot_see_a_gating_lie_and_says_so_by_finding_nothing() {
    // `None` is the registration-time call, where no run has happened. It
    // must not guess. This pins that the gating check is genuinely
    // deferred rather than silently answered as clean at registration.
    assert_eq!(
        contract_faults(&LiesAboutGating, None),
        Vec::<String>::new()
    );
    // and the same tool IS caught once a run exists, so the emptiness above
    // is deferral rather than blindness
    let (c, d) = empty();
    let r = LiesAboutGating.run(&ctx(&[], &c, &d, &Default::default()));
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

// `a_clean_verdict_carries_what_it_examined` deleted: it constructed
// `Outcome::Clean { examined: 0 }` and `{ examined: 241 }`, destructured
// them, and asserted the fields equalled the literals its own definition
// set two lines above. No value of anything in this crate could make it
// fail; it was not weak coverage, it was decoration. The real property
// (a clean verdict over zero is reported differently from a real pass) is
// exercised where it actually decides something: `entry::tool::run`'s
// dispatch-level tests in `src/entry/tool.rs`.

// -- argument declaration ------------------------------------------------

#[test]
fn a_missing_required_argument_is_reported_before_the_tool_runs() {
    assert_eq!(missing_required(&PhraseSearch, &[]).len(), 1);
    // the required one supplied, and the optional one absent, is not a
    // fault: `missing_required` only ever reports the required argument.
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
fn a_declared_value_flags_value_does_not_satisfy_a_required_argument() {
    // The case that must fail: without `value_flags` telling this
    // function that `--root` consumes the token after it, `src` is the
    // only non-flag word in `["--root", "src"]` and is miscounted as the
    // phrase, exactly as `mock search --root src` was.
    let missing = missing_required(&RootedSearch, &["--root", "src"]);
    assert_eq!(
        missing.len(),
        1,
        "the flag's value must not stand in for the phrase"
    );
    assert_eq!(missing[0].name, "phrase");
    // supplying the phrase alongside the flag and its value is fine,
    // whichever order they arrive in
    assert_eq!(
        missing_required(&RootedSearch, &["--root", "src", "needle"]).len(),
        0
    );
    assert_eq!(
        missing_required(&RootedSearch, &["needle", "--root", "src"]).len(),
        0
    );
}

#[test]
fn an_undeclared_value_flag_still_has_its_value_miscounted() {
    // Documenting the boundary rather than hiding it: `PhraseSearch`
    // never declares `--root`, so this function has no way to know it
    // takes a value, and `src` is read as the phrase. Declaring a tool's
    // own value-taking flags is the tool author's responsibility, exactly
    // as it is for the dispatcher's own value-taking globals. If this
    // ever tightens (a global default flag shape, say), the tightening
    // is the fix, not this assertion.
    let missing = missing_required(&PhraseSearch, &["--root", "src"]);
    assert_eq!(
        missing.len(),
        0,
        "an undeclared value flag's value reads as the positional"
    );
}

#[test]
fn usage_distinguishes_required_from_optional() {
    assert_eq!(
        usage_line(&PhraseSearch),
        "mock phrase-search <phrase> [dir]"
    );
    assert_eq!(usage_line(&LiesAboutAsking), "mock lies-about-asking");
}

#[test]
fn usage_from_agrees_with_usage_line_for_every_shape_it_covers() {
    // `usage_line` is defined in terms of `usage_from` now; this pins that
    // the delegation is real rather than two copies that happen to agree
    // today. A tool with no name and args tuple, and a callable that only
    // has `name()` and `args()` (never a `dyn Tool`), must render
    // identically to the `dyn Tool` path.
    assert_eq!(
        usage_from("phrase-search", PhraseSearch.args()),
        usage_line(&PhraseSearch)
    );
    assert_eq!(
        usage_from("rooted-search", RootedSearch.args()),
        usage_line(&RootedSearch)
    );
    assert_eq!(
        usage_from("lies-about-asking", LiesAboutAsking.args()),
        usage_line(&LiesAboutAsking)
    );
}

#[test]
fn usage_from_needs_no_tool_at_all() {
    // The case `usage_line` cannot express: a builtin subcommand that
    // never implements `Tool`, described only by a name and a slice of
    // `ArgSpec`. If this could not be written, the factoring above would
    // have bought nothing.
    let args = [
        ArgSpec {
            name:        "slug",
            required:    true,
            description: "which panel",
        },
        ArgSpec {
            name:        "note",
            required:    false,
            description: "what was decided",
        },
    ];
    assert_eq!(
        usage_from("panel-consolidate", &args),
        "mock panel-consolidate <slug> [note]"
    );
    assert_eq!(usage_from("status", &[]), "mock status");
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

        fn purpose(&self) -> Purpose {
            Purpose::Check(NotALint::NoFailingCase)
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

        fn purpose(&self) -> Purpose {
            Purpose::Check(NotALint::NoFailingCase)
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
