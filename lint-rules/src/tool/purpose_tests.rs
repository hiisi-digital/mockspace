//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Unit tests for what a tool declares it is for: the maker arm, and the
//! write audit that holds a maker to its declaration.

use super::*;

/// A maker that writes nothing it would admit to.
struct MakesNothing;
impl Tool for MakesNothing {
    fn name(&self) -> &'static str {
        "makes-nothing"
    }

    fn description(&self) -> &'static str {
        "declares itself a maker and names no path"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Make {
            writes: &[],
            roots:  &[],
        }
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

/// A maker whose declaration names the whole tree.
struct MakesEverything(&'static [&'static str]);
impl Tool for MakesEverything {
    fn name(&self) -> &'static str {
        "makes-everything"
    }

    fn description(&self) -> &'static str {
        "declares a pattern that constrains nothing"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Make {
            writes: self.0,
            roots:  &[],
        }
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

/// An honest maker, which reports what it produced with an advisory finding.
struct Changelog;
impl Tool for Changelog {
    fn name(&self) -> &'static str {
        "changelog"
    }

    fn description(&self) -> &'static str {
        "render the changelog and the generated docs"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Make {
            writes: &["CHANGELOG.md", "docs/gen/**"],
            roots:  &[],
        }
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport {
            outcome: Outcome::Findings(vec![LintError::warning(
                "CHANGELOG.md".to_string(),
                1,
                "changelog",
                "a commit carries no type, so it has no section".to_string(),
            )]),
            output:  String::new(),
        }
    }
}

/// A check, for the arms where a check must be left alone.
struct Inventory;
impl Tool for Inventory {
    fn name(&self) -> &'static str {
        "inventory"
    }

    fn description(&self) -> &'static str {
        "list what the registry holds"
    }

    fn purpose(&self) -> Purpose {
        Purpose::Check(NotALint::NoFailingCase)
    }

    fn run(&self, _ctx: &ToolContext<'_>) -> ToolReport {
        ToolReport::reported("", 1)
    }
}

fn written(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

#[test]
fn a_maker_declaring_nothing_it_writes_is_refused() {
    // The case that must fail. Without it `Purpose::Make` is a way for any
    // check to leave the gate without giving a `NotALint` reason.
    let found = contract_faults(&MakesNothing, None);
    assert_eq!(found.len(), 1, "expected one fault, got {found:?}");
    assert!(found[0].contains("declares nothing it writes"), "{found:?}");
}

#[test]
fn a_declared_write_that_constrains_nothing_is_refused() {
    // The whole wildcard alphabet, `*`, `?` and `**`, in every arrangement
    // that names no literal part of the tree. `**/?*` and `?*/**` match every
    // path as surely as `**` does, so a refusal that only knew `*` let them
    // through.
    for (pattern, why) in [
        ("**", "is only wildcards"),
        ("**/*", "is only wildcards"),
        ("./**", "is only wildcards"),
        ("**/?*", "is only wildcards"),
        ("?*/**", "is only wildcards"),
        ("**/*?", "is only wildcards"),
        ("*?*/**/?", "is only wildcards"),
        ("*", "is only wildcards"),
        ("?", "is only wildcards"),
        ("*/*", "is only wildcards"),
        ("", "is empty"),
        ("/etc/passwd", "leaves the repository root"),
        ("../sibling/out.md", "leaves the repository root"),
        ("docs/../../out.md", "leaves the repository root"),
        (".git/hooks/pre-commit", "has a literal `.git` segment"),
        ("**/.git/config", "has a literal `.git` segment"),
        ("vendor/.git/**", "has a literal `.git` segment"),
    ] {
        let decl: &'static [&'static str] = Box::leak(Box::new([pattern]));
        let found = contract_faults(&MakesEverything(decl), None);
        assert_eq!(found.len(), 1, "`{pattern}` must be refused: {found:?}");
        assert!(found[0].contains(why), "`{pattern}`: {found:?}");
    }
}

#[test]
fn a_declared_write_naming_part_of_the_tree_is_accepted() {
    // The negative arm of the one above, so that refusal is about the shape
    // and not about every pattern: these each name a real part of the tree.
    for pattern in [
        "docs/gen/**",
        "CHANGELOG.md",
        "**/*.generated.rs",
        "out/*",
        "*.md",
        "v?.txt",
        ".github/x.yml",
        ".gitignore",
    ] {
        let decl: &'static [&'static str] = Box::leak(Box::new([pattern]));
        assert_eq!(
            contract_faults(&MakesEverything(decl), None),
            Vec::<String>::new(),
            "`{pattern}` names part of the tree"
        );
    }
}

/// A run whose finding is at error, which a maker may never return.
fn blocking_run() -> ToolReport {
    ToolReport {
        outcome: Outcome::Findings(vec![LintError::error(
            "src/lib.rs".to_string(),
            1,
            "changelog",
            "coverage under 80".to_string(),
        )]),
        output:  String::new(),
    }
}

fn changelog_run() -> ToolReport {
    let (c, d) = (BTreeSet::new(), Vec::new());
    let ctx = ToolContext {
        mock_dir:   Path::new("/mock"),
        repo_root:  Path::new("/"),
        all_crates: &c,
        src_dirs:   &d,
        args:       &[],
        stdin:      None,
        registry:   &Default::default(),
    };
    Changelog.run(&ctx)
}

#[test]
fn a_maker_that_blocks_and_wrote_nothing_has_broken_its_contract() {
    // A check declaring `Make` over a path it never touches and returning
    // blocking findings is a check with a failing case and no `NotALint`
    // reason. Subsumed by the rule below, and kept because it still holds.
    let r = blocking_run();
    assert!(r.outcome.blocks(LintMode::Commit), "the run blocks");
    let found = maker_faults(&Changelog, &r, &[], &[]);
    assert_eq!(found.len(), 1, "expected one fault, got {found:?}");
    assert!(found[0].contains("never block"), "{found:?}");
}

#[test]
fn a_maker_that_wrote_and_blocks_has_broken_its_contract() {
    // The case that must fail: a threshold check hiding as a maker writes the
    // one file it declared and then blocks on something else entirely. A
    // maker cannot express a gating judgement, so any finding at error at
    // any gate is a fault, whatever it wrote.
    let r = blocking_run();
    let found = maker_faults(&Changelog, &r, &[], &written(&["CHANGELOG.md"]));
    assert_eq!(found.len(), 1, "expected one fault, got {found:?}");
    assert!(found[0].contains("never block"), "{found:?}");
}

#[test]
fn a_makers_advisory_findings_report_and_are_not_faults() {
    // The control: a maker reports what it produced at warning or info, and
    // that is neither a contract fault nor a block, wrote or not.
    let r = changelog_run();
    assert!(
        !r.outcome.blocks(LintMode::Push),
        "a warning blocks nothing"
    );
    assert_eq!(contract_faults(&Changelog, Some(&r)), Vec::<String>::new());
    assert_eq!(
        maker_faults(&Changelog, &r, &[], &written(&["CHANGELOG.md"])),
        Vec::<String>::new()
    );
    assert_eq!(maker_faults(&Changelog, &r, &[], &[]), Vec::<String>::new());
}

#[test]
fn a_make_that_failed_before_writing_is_inconclusive_and_not_a_fault() {
    // Inconclusive is how a make says it could not get as far as writing.
    let r = ToolReport::inconclusive("the commit log could not be read");
    assert_eq!(maker_faults(&Changelog, &r, &[], &[]), Vec::<String>::new());
}

#[test]
fn a_write_outside_the_declaration_is_a_fault() {
    let found = maker_faults(
        &Changelog,
        &changelog_run(),
        &[],
        &written(&["CHANGELOG.md", "docs/gen/a/b.md", "src/lib.rs"]),
    );
    assert_eq!(found.len(), 1, "only `src/lib.rs` is undeclared: {found:?}");
    assert!(found[0].contains("`src/lib.rs`"), "{found:?}");
    assert!(
        found[0].contains("`docs/gen/**`"),
        "names the declaration: {found:?}"
    );
}

#[test]
fn a_bare_declared_name_does_not_reach_a_nested_file_of_that_name() {
    // The case that fails if the audit matched with the lint filter's lifting
    // `glob_match`: there `CHANGELOG.md` is `**/CHANGELOG.md`, and a maker
    // that declared the root changelog could rewrite every crate's.
    let found = maker_faults(
        &Changelog,
        &changelog_run(),
        &[],
        &written(&["crates/a/CHANGELOG.md"]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn writes_inside_the_declaration_are_not_faults() {
    assert_eq!(
        maker_faults(
            &Changelog,
            &changelog_run(),
            &[],
            &written(&["CHANGELOG.md", "docs/gen/x.md"])
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        maker_faults(&Changelog, &ToolReport::reported("", 1), &[], &[]),
        Vec::<String>::new()
    );
}

#[test]
fn a_check_is_not_audited_for_writes() {
    // Its declaration says nothing about writes, so there is nothing to hold
    // them to. Pinned so that turning this on is a deliberate change.
    assert_eq!(
        maker_faults(&Inventory, &changelog_run(), &[], &written(&["src/lib.rs"])),
        Vec::<String>::new()
    );
}

#[test]
fn the_purpose_renders_its_kind_and_what_it_is_held_to() {
    assert_eq!(Inventory.purpose().kind(), "check");
    assert_eq!(Inventory.purpose().describe(), "check, no-failing-case");
    assert_eq!(Changelog.purpose().kind(), "make");
    assert_eq!(
        Changelog.purpose().describe(),
        "make, writes `CHANGELOG.md`, `docs/gen/**`"
    );
    assert_eq!(
        MakesNothing.purpose().describe(),
        "make, declares nothing it writes"
    );
}
