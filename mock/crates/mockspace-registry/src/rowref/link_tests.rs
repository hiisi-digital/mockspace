//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! A `ref` or `ref[]` field holding links to rows: what is checked, which edges
//! it makes, and the controls proving each check reports nothing it should not.

use std::collections::BTreeSet;

use super::tests::{kinds, ns, reg};
use super::*;

fn keys<'a>(names: &[&'a str]) -> BTreeSet<&'a str> {
    names.iter().copied().collect()
}

#[test]
fn an_item_of_two_segments_naming_a_namespace_is_a_link() {
    assert_eq!(
        link_target("slot::display", &keys(&["slot"])),
        Some(("slot", "display"))
    );
}

/// The control: three segments are a file citation, whatever the first one is
/// called. Without it the test above is equally consistent with a function that
/// returns every item split on the first separator.
#[test]
fn a_file_citation_is_not_a_link() {
    assert_eq!(link_target("slot::design::12", &keys(&["slot"])), None);
    assert_eq!(
        link_target("research::dir::file::heading", &keys(&["research"])),
        None
    );
}

/// Two segments whose first names no namespace are not a link either, so
/// `validate_provenance` keeps reporting them as the malformed citations they
/// were before links existed.
#[test]
fn two_segments_naming_no_namespace_are_not_a_link() {
    assert_eq!(link_target("nothing::display", &keys(&["slot"])), None);
}

#[test]
fn a_link_naming_a_row_that_exists_is_accepted() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::display")])]);
    assert!(validate_links(&r, &nss).is_empty());
}

/// The case that must fail: a validator that walked no field would pass the
/// test above just as well.
#[test]
fn a_link_naming_no_row_is_reported() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::audio")])]);
    let found = validate_links(&r, &nss);
    assert_eq!(kinds(&found), ["unknown-row-reference"], "{found:?}");
    assert!(
        found[0].message.contains("slot::audio"),
        "{}",
        found[0].message
    );
}

// Mutation: reading an item by its first separator (the reader before the standard) makes
// `slot::Not-A-Slug` a link of a bad slug, so the link check reports it and the citation check
// never sees it; this fails on the second assertion.
#[test]
fn a_link_with_a_bad_slug_is_no_link_and_the_citation_check_names_the_rule() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r =
        reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::Not-A-Slug")])]);
    assert!(validate_links(&r, &nss).is_empty());
    let found = validate_provenance(Path::new("/nonexistent"), &BTreeMap::new(), &BTreeSet::new(), &r, &nss);
    assert_eq!(kinds(&found), ["malformed-provenance"], "{found:?}");
    assert!(found[0].message.contains("uppercase"), "{}", found[0].message);
    assert!(found[0].message.contains(crate::MIGRATION), "{}", found[0].message);
}

#[test]
fn one_dangling_link_among_good_ones_is_the_only_finding() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("slot", "audio", &[]),
        ("answer", "niri", &[(
            "related",
            "slot::display, slot::gone, slot::audio",
        )]),
    ]);
    let found = validate_links(&r, &nss);
    assert_eq!(kinds(&found), ["unknown-row-reference"], "{found:?}");
}

/// A `string[]` field is prose, and prose that happens to look like a link is
/// not one.
#[test]
fn an_untyped_field_is_not_checked_for_links() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "string[]")])];
    let r = reg(&[("answer", "niri", &[("related", "slot::gone")])]);
    assert!(validate_links(&r, &nss).is_empty());
}

#[test]
fn an_explicitly_empty_link_list_is_no_links() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("answer", "niri", &[("related", "")])]);
    assert!(validate_links(&r, &nss).is_empty());
}

/// `validate_row_references` is the entry point both callers use, so the links
/// are checked through it and no caller has a second function to remember.
#[test]
fn the_row_reference_check_covers_links() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("answer", "niri", &[("related", "slot::gone")])]);
    assert_eq!(kinds(&validate_row_references(&r, &nss)), [
        "unknown-row-reference"
    ]);
}

/// A link is not a file citation, so the provenance check must not report it
/// as one: that report would make the field unusable for the thing it is for.
#[test]
fn the_provenance_check_leaves_a_link_alone() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::display")])]);
    let found = validate_provenance(
        Path::new("/nonexistent"),
        &BTreeMap::new(),
        &BTreeSet::new(),
        &r,
        &nss,
    );
    assert!(found.is_empty(), "{found:?}");
}

/// The control: a two-segment item naming no namespace is still a malformed
/// citation, so the skip above is for links and not for every short item.
#[test]
fn the_provenance_check_still_reports_a_short_item_naming_no_namespace() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("answer", "niri", &[("related", "nothing::display")])]);
    let found = validate_provenance(
        Path::new("/nonexistent"),
        &BTreeMap::new(),
        &BTreeSet::new(),
        &r,
        &nss,
    );
    assert_eq!(kinds(&found), ["malformed-provenance"], "{found:?}");
}

/// One edge definition: a link in a `ref[]` field is a reverse edge `refsto`
/// finds, and the same edge reaches a lint through the view.
#[test]
fn a_link_is_a_reverse_edge() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::display")])]);
    assert_eq!(referrers(&r, &nss, "slot::display"), ["answer::niri"]);
    assert_eq!(reverse_edges(&r, &nss).get("slot::display").cloned().unwrap_or_default(), ["answer::niri"]);
}

/// The control: a link to a row nothing declares is no edge, as a typed
/// reference to one is none.
#[test]
fn a_link_to_no_row_is_no_edge() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("answer", "niri", &[("related", "slot::gone")])]);
    assert!(referrers(&r, &nss, "slot::gone").is_empty());
}

/// A file citation in the same field makes no edge.
#[test]
fn a_citation_in_a_link_field_is_no_edge() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("related", "slot::design::display")]),
    ]);
    assert!(referrers(&r, &nss, "slot::display").is_empty());
}

/// The scalar `ref` type holds one link as well.
#[test]
fn a_scalar_ref_field_holds_a_link() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::display")])]);
    assert_eq!(referrers(&r, &nss, "slot::display"), ["answer::niri"]);
}

/// The typed field and the relation field make edges the same way, and a row
/// reached by both is listed once.
#[test]
fn a_row_linked_by_a_typed_field_and_a_link_is_listed_once() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot"), ("related", "ref[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[
            ("slot", "display"),
            ("related", "slot::display"),
        ]),
    ]);
    assert_eq!(referrers(&r, &nss, "slot::display"), ["answer::niri"]);
}

#[test]
fn the_report_lists_both_ends_with_their_fields() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot"), ("related", "ref[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[
            ("slot", "display"),
            ("related", "slot::display"),
        ]),
    ]);
    let from = links_report(&r, &nss, "answer::niri").unwrap();
    assert!(from.contains("links to:"), "{from}");
    assert!(from.contains("slot     slot::display"), "{from}");
    assert!(from.contains("related  slot::display"), "{from}");
    assert!(from.contains("linked from: nothing"), "{from}");
    let to = links_report(&r, &nss, "slot::display").unwrap();
    assert!(to.contains("links to: nothing"), "{to}");
    assert!(to.contains("related  answer::niri"), "{to}");
}

/// The controls: an argument that names no row, and one that is no row id at
/// all, are errors that say which.
#[test]
fn the_report_refuses_what_is_not_a_row() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[])]);
    let missing = links_report(&r, &nss, "slot::displya").unwrap_err();
    assert!(missing.contains("no row `slot::displya`"), "{missing}");
    assert!(missing.contains("Did you mean `display`?"), "{missing}");
    let bare = links_report(&r, &nss, "display").unwrap_err();
    assert!(bare.contains("not `namespace::slug`"), "{bare}");
    let unknown = links_report(&r, &nss, "nothing::display").unwrap_err();
    assert!(unknown.contains("not `namespace::slug`"), "{unknown}");
}

/// A link in a table cell is rendered as a reference, which the pass after it
/// turns into the slug of the row, as a typed cell is. Rendered through the
/// whole pipeline for the reason the typed cell's test is.
#[test]
fn a_link_cell_renders_as_the_slug_of_the_row_it_names() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::display")])]);
    let mut cfg = crate::Env::from_dir(Path::new("/nonexistent")).unwrap();
    cfg.namespaces = nss.clone();
    let table = render_table(&nss[1], &r, &cfg);
    let rendered = resolve_all(
        &table,
        &nss,
        &r,
        &BTreeMap::new(),
        Path::new("/repo"),
        Path::new("/repo/docs"),
        &cfg,
    );
    assert!(rendered.contains("| display |"), "{rendered}");
    assert!(!rendered.contains("{{"), "{rendered}");
}

/// The control: a file citation in the same cell stays the text it was.
#[test]
fn a_citation_cell_stays_text() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("answer", "niri", &[("related", "mock::design::12")])]);
    let mut cfg = crate::Env::from_dir(Path::new("/nonexistent")).unwrap();
    cfg.namespaces = nss.clone();
    let table = render_table(&nss[1], &r, &cfg);
    assert!(table.contains("| mock::design::12 |"), "{table}");
    assert!(!table.contains("{{"), "{table}");
}

/// A link is held both ways without being written both ways: the row that
/// states it lists the target as outgoing, and the target lists it as incoming,
/// each naming the field.
#[test]
fn a_link_is_listed_from_both_ends_with_its_field() {
    let nss = [ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("slot", "display", &[]), ("answer", "niri", &[("related", "slot::display")])]);
    let from = links_of(&r, &nss, "answer::niri");
    assert_eq!(from.outgoing, [Link::new("related", "slot::display")]);
    assert!(from.incoming.is_empty(), "{from:?}");
    let to = links_of(&r, &nss, "slot::display");
    assert_eq!(to.incoming, [Link::new("related", "answer::niri")]);
    assert!(to.outgoing.is_empty(), "{to:?}");
}

/// A root that shares a namespace's name makes a two-segment item a link, as the
/// namespace claims it, and the setup is reported as `namespace-root-collision`.
/// Both are held at once so the collision is a loud finding and not a silent
/// precedence: the item reads as a link to a row that does not exist, and the
/// declaration that made it ambiguous is reported beside it.
#[test]
fn a_namespace_named_like_a_root_is_a_link_and_the_setup_is_reported() {
    let nss = [ns("research", &[]), ns("answer", &[("related", "ref[]")])];
    assert_eq!(
        link_target("research::dir", &keys(&["research"])),
        Some(("research", "dir"))
    );
    let r = reg(&[("answer", "niri", &[("related", "research::dir")])]);
    let found = validate_links(&r, &nss);
    assert_eq!(kinds(&found), ["unknown-row-reference"], "{found:?}");
    let roots = BTreeMap::from([("research".to_string(), "research".to_string())]);
    let setup = namespace_root_collisions(&nss, &roots);
    assert_eq!(kinds(&setup), ["namespace-root-collision"], "{setup:?}");
}

/// The control: the same root with no namespace of that name is no collision, and
/// the item is a citation prefix and not a link.
#[test]
fn a_root_no_namespace_shares_is_no_collision_and_its_short_item_is_no_link() {
    let nss = [ns("answer", &[("related", "ref[]")])];
    let roots = BTreeMap::from([("research".to_string(), "research".to_string())]);
    assert!(namespace_root_collisions(&nss, &roots).is_empty());
    assert_eq!(link_target("research::dir", &keys(&["answer"])), None);
}

/// A row linking itself is an edge from the row to itself, so it is listed once
/// outgoing and once incoming and `refsto` finds the row among its own referrers.
#[test]
fn a_row_linking_itself_is_listed_at_both_ends() {
    let nss = [ns("answer", &[("related", "ref[]")])];
    let r = reg(&[("answer", "niri", &[("related", "answer::niri")])]);
    assert!(validate_links(&r, &nss).is_empty());
    let both = links_of(&r, &nss, "answer::niri");
    assert_eq!(both.outgoing, [Link::new("related", "answer::niri")]);
    assert_eq!(both.incoming, [Link::new("related", "answer::niri")]);
    assert_eq!(referrers(&r, &nss, "answer::niri"), ["answer::niri"]);
}

/// A cell holding a link, a citation and a citation into an internal root renders
/// the link as the row's slug, keeps the citation as text, and drops the internal
/// one, in the order the cell was written.
#[test]
fn a_cell_with_a_link_and_two_citations_keeps_the_link_and_the_public_citation() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[(
            "related",
            "seed::notes::niri::tiling, slot::display, mock::design::12",
        )]),
    ]);
    let mut cfg = crate::Env::from_dir(Path::new("/nonexistent")).unwrap();
    cfg.namespaces = nss.clone();
    cfg.internal_roots = ["seed".to_string()].into_iter().collect();
    let table = render_table(&nss[1], &r, &cfg);
    let rendered = resolve_all(
        &table,
        &nss,
        &r,
        &BTreeMap::new(),
        Path::new("/repo"),
        Path::new("/repo/docs"),
        &cfg,
    );
    assert!(
        rendered.contains("| display, mock::design::12 |"),
        "{rendered}"
    );
    assert!(!rendered.contains("seed::"), "{rendered}");
    assert!(!rendered.contains("{{"), "{rendered}");
}

/// A link is never an internal-root citation, even when the namespace carries the
/// name of an internal root: the cell drops the citation and keeps the link.
#[test]
fn an_internal_root_filter_does_not_eat_a_link() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("related", "ref[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[(
            "related",
            "slot::display, slot::notes::x::h",
        )]),
    ]);
    let mut cfg = crate::Env::from_dir(Path::new("/nonexistent")).unwrap();
    cfg.namespaces = nss.clone();
    cfg.internal_roots = ["slot".to_string()].into_iter().collect();
    let table = render_table(&nss[1], &r, &cfg);
    assert!(table.contains("{{ slot::display }}"), "{table}");
    assert!(!table.contains("slot::notes"), "{table}");
}
