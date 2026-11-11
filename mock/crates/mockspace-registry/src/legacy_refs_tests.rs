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

#[test]
fn a_preset_shorthand_is_folded_into_the_standards_names() {
    let said = |s: &str| preset_shorthand(s);
    assert_eq!(said("stack-lints::no-heap"), Outcome::Rewritten("stack_lints::no_heap".into()));
    assert_eq!(said("mockspace::No-Bare-Numeric"), Outcome::Rewritten("mockspace::no_bare_numeric".into()));
    assert_eq!(said("mockspace::no_bare_numeric"), Outcome::Same);
}

#[test]
fn a_preset_shorthand_that_is_not_a_host_and_a_name_is_left_and_says_why() {
    for bad in ["no-heap", "a::b::c", "stack-lints::9lives", "::x", "x::"] {
        let Outcome::Left(why) = preset_shorthand(bad) else { panic!("{bad}") };
        assert!(!why.is_empty(), "{bad}");
    }
}

fn ctx_in<'a>(dir: &'a Path, roots: &'a BTreeMap<String, String>, ns: &'a BTreeSet<&'a str>) -> Ctx<'a> {
    Ctx {
        repo_root:  dir,
        roots,
        namespaces: ns,
    }
}

// The same placeholder outside a fence and inside one: the one outside is rewritten, and the
// example inside the fence stays as the author wrote it.
//
// Mutation: rewriting by a search for the placeholder's text across the whole file, as `apply`
// did, rewrites the fenced copy too.
#[test]
fn a_placeholder_spelled_alike_inside_a_fence_is_not_rewritten() {
    let dir = world();
    let (roots, ns) = (roots(), BTreeSet::from(["vocab"]));
    let ctx = ctx_in(dir.path(), &roots, &ns);
    let text = "see {{ seed::DESIGN::844 }}\n\n```md\nwrite {{ seed::DESIGN::844 }} like this\n```\n";
    let found = in_text(text, &ctx);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(&text[found[0].span.clone()], "{{ seed::DESIGN::844 }}");
    assert_eq!(apply(text, &found), "see {{ seed::design::844 }}\n\n```md\nwrite {{ seed::DESIGN::844 }} like this\n```\n");
}

// A tilde fence is a fence, and a fence of four backticks is not closed by three: a placeholder in
// either is an example and stays as written.
//
// Mutation: toggling on three backticks alone, as the reader did, rewrites both fenced copies.
#[test]
fn a_placeholder_inside_a_tilde_fence_or_a_longer_fence_is_not_rewritten() {
    let dir = world();
    let (roots, ns) = (roots(), BTreeSet::from(["vocab"]));
    let ctx = ctx_in(dir.path(), &roots, &ns);
    let text = "~~~md\n{{ seed::DESIGN::844 }}\n~~~\n\n````md\n```\n{{ seed::DESIGN::844 }}\n````\nsee {{ seed::DESIGN::844 }}\n";
    let found = in_text(text, &ctx);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].span.start, text.rfind("{{").unwrap());
}

// An item of a list is rewritten where it stands, so one that is the start of a later item does
// not rewrite that one's first bytes.
//
// Mutation: rewriting by a search for the item's text turns `crates::Foo-bar` into
// `crates::foo-bar`, which is no name, before its own rewrite can find it.
#[test]
fn an_item_of_a_list_that_starts_another_is_rewritten_alone() {
    let dir = world();
    let (roots, ns) = (roots(), BTreeSet::from(["vocab"]));
    let ctx = ctx_in(dir.path(), &roots, &ns);
    let value = "crates::Foo, crates::Foo-bar,crates::Foo";
    let found = in_list(value, &ctx);
    let spans: Vec<&str> = found.iter().map(|f| &value[f.span.clone()]).collect();
    assert_eq!(spans, ["crates::Foo", "crates::Foo-bar", "crates::Foo"]);
    assert_eq!(apply(value, &found), "crates::foo, crates::foo_bar,crates::foo");
}

// Mutation: reading an item that holds a placeholder as an item of the list as well names it a
// second time, beside what the placeholder reader finds in it.
#[test]
fn an_item_holding_a_placeholder_is_left_to_the_placeholder_reader() {
    let dir = world();
    let (roots, ns) = (roots(), BTreeSet::from(["vocab"]));
    let ctx = ctx_in(dir.path(), &roots, &ns);
    assert!(in_list("{{ crates::Foo }}, crates::Bar", &ctx).iter().all(|f| f.token == "crates::Bar"));
}
