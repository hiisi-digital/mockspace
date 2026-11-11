//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;

const DESIGN: &str = "# Design\n\n## The four lanes\n\ntext\n\n## Warm's objective\n\n## 2026 plan\n";

fn world() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let put = |p: &str, text: &str| {
        let at = dir.path().join(p);
        fs::create_dir_all(at.parent().unwrap()).unwrap();
        fs::write(at, text).unwrap();
    };
    put("corpus/DESIGN.md", DESIGN);
    put("corpus/Crates/My-Crate/NOTES.md", "# Notes\n\n## Tiling\n");
    put("corpus/2024-notes/A.md", "# A\n");
    put("corpus/dup/Thing.md", "# One\n");
    put("corpus/dup/Thing.md.tmpl", "# Two\n");
    put("corpus/same/a-b.md", "# One\n");
    put("corpus/same/a_b.md", "# Two\n");
    put("corpus/Intro.md", "# Hello\n");
    dir
}

fn roots() -> BTreeMap<String, String> {
    [("seed".to_string(), "corpus".to_string())].into_iter().collect()
}

fn run(old: &str) -> Outcome {
    let dir = world();
    let (roots, ns) = (roots(), BTreeSet::from(["vocab"]));
    expr(old, &Ctx {
        repo_root:  dir.path(),
        roots:      &roots,
        namespaces: &ns,
    })
}

/// The new spelling reaches the file the old one did, and the heading or the line.
fn resolves_to(new: &str, file: &str, line: Option<usize>) {
    let dir = world();
    let r = FileRef::read(new).unwrap();
    assert_eq!(r.locate(&dir.path().join("corpus")), Location::Found(dir.path().join("corpus").join(file)), "{new}");
    if let (Some(want), Anchor::Heading(h)) = (line, r.anchor()) {
        let got = refpath::heading_in(&dir.path().join("corpus").join(file), &h).map(|(l, _)| l);
        assert_eq!(got, Some(want), "{new}");
    }
}

// Mutation: leaving the case of a segment as written (a rewrite that only swaps `::#` for `::`)
// gives `seed::DESIGN::844`, which the standard refuses, so this fails on the first row.
#[test]
fn each_old_form_is_written_the_new_way_and_reaches_what_it_did() {
    let rows: &[(&str, &str, &str, Option<usize>)] = &[
        ("seed::DESIGN::844", "seed::design::844", "DESIGN.md", None),
        ("seed::DESIGN.md::844", "seed::design::844", "DESIGN.md", None),
        ("seed::DESIGN::#the-four-lanes", "seed::design::the_four_lanes", "DESIGN.md", Some(3)),
        ("seed :: DESIGN :: #the-four-lanes", "seed::design::the_four_lanes", "DESIGN.md", Some(3)),
        // the old reader accepted both the project slug and the forge's
        ("seed::DESIGN::#warm-s-objective", "seed::design::warms_objective", "DESIGN.md", Some(7)),
        ("seed::DESIGN::#warms-objective", "seed::design::warms_objective", "DESIGN.md", Some(7)),
        ("seed::Crates::My-Crate::NOTES::#tiling", "seed::crates::my_crate::notes::tiling", "Crates/My-Crate/NOTES.md", Some(3)),
        ("seed::Intro::1", "seed::intro::1", "Intro.md", None),
    ];
    for (old, new, file, line) in rows {
        assert_eq!(run(old), Outcome::Rewritten((*new).to_string()), "{old}");
        resolves_to(new, file, *line);
    }
}

// Mutation: a rewrite that did not look inside the call leaves `pathof( seed::DESIGN::12 )` as it
// was and the chain unread.
#[test]
fn an_operation_keeps_its_name_and_its_chain_and_has_its_argument_rewritten() {
    assert_eq!(run("pathof( seed::DESIGN::12 ).dir()"), Outcome::Rewritten("pathof(seed::design::12).dir()".into()));
    assert_eq!(run("refsto( vocab::xpbd )"), Outcome::Rewritten("refsto(vocab::xpbd)".into()));
    assert_eq!(run("pathof(seed::design::12)"), Outcome::Same);
    assert!(matches!(run("pathof(seed::Missing::12)"), Outcome::Left(_)));
    assert!(matches!(run("pathof(seed::design::12"), Outcome::Left(_)));
}

// Mutation: rewriting every `::`-text, which turns `std::mem::swap` and a row link into citations.
#[test]
fn what_is_in_the_new_spelling_already_or_is_no_reference_is_left_alone_unnamed() {
    for same in [
        "seed::design::844",
        "seed::design::the_four_lanes",
        "seed::design::10..20",
        "reg::vocab::xpbd::what",
        "vocab::xpbd",
        "vocab",
        "project_name",
        "PROJECT_NAME",
        "crates::numeric",
        "refsto(vocab::xpbd)",
    ] {
        assert_eq!(run(same), Outcome::Same, "{same}");
    }
    assert_eq!(run("crates::mockspace-core"), Outcome::Rewritten("crates::mockspace_core".into()));
}

// Mutation: writing the new spelling where it resolves differently (dropping the check that it
// reaches the same file) rewrites `seed::dup::thing::1`, `seed::same::a_b::1` and the 2024 notes.
#[test]
fn a_reference_the_new_spelling_cannot_reach_the_same_target_with_is_left_and_named() {
    let left = |old: &str, part: &str| match run(old) {
        Outcome::Left(why) => assert!(why.contains(part), "{old}: {why}"),
        other => panic!("{old} should be left, got {other:?}"),
    };
    left("seed::Dup::Thing::1", "no file");
    left("seed::dup::Thing::1", "several files");
    left("seed::same::a-b::1", "several files");
    left("seed::2024-notes::A::1", "has no name");
    left("seed::DESIGN::#2026-plan", "has no name");
    left("seed::DESIGN::#no-such-heading", "no heading");
    left("seed::DESIGN::0", "neither a heading nor a line");
    left("seed::DESIGN::#", "empty heading");
    left("nosuchroot::DESIGN::1", "no declared root");
    left("vocab::Not-A-Slug", "uppercase");
}

// Mutation: a rewrite that is not a fixed point (it appends instead of replacing) fails on the
// second pass over its own output.
#[test]
fn a_second_pass_over_the_output_changes_nothing() {
    for old in [
        "seed::DESIGN::#the-four-lanes",
        "seed :: DESIGN :: 844",
        "pathof( seed::DESIGN::12 ).dir()",
        "seed::Crates::My-Crate::NOTES::#tiling",
        "crates::mockspace-core",
    ] {
        let Outcome::Rewritten(new) = run(old) else { panic!("{old}") };
        assert_eq!(run(&new), Outcome::Same, "{old} -> {new}");
    }
}

#[test]
fn a_text_has_each_placeholder_rewritten_outside_a_fence_with_its_spacing_kept() {
    let dir = world();
    let (roots, ns) = (roots(), BTreeSet::from(["vocab"]));
    let ctx = Ctx {
        repo_root:  dir.path(),
        roots:      &roots,
        namespaces: &ns,
    };
    let text = "a {{ seed::DESIGN::#the-four-lanes }} b {{seed::DESIGN::844}} c {{ project_name }}\n```\n{{ seed::DESIGN::12 }}\n```\nd {{ nosuchroot::X::1 }}\n";
    let found = in_text(text, &ctx);
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!(
        apply(text, &found),
        "a {{ seed::design::the_four_lanes }} b {{seed::design::844}} c {{ project_name }}\n```\n{{ seed::DESIGN::12 }}\n```\nd {{ nosuchroot::X::1 }}\n"
    );
    assert!(in_text(&apply(text, &found), &ctx).iter().all(|f| matches!(f.outcome, Outcome::Left(_))));
}
