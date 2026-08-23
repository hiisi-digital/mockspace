//! The two paths a row reference reaches a document by, and the query that
//! reads them backwards.
//!
//! Each of the three defects these pin was found by a reviewer and
//! reproduced before it was fixed: an embed target linking to a page nobody
//! writes, a missing slug vanishing from a field reference while the table
//! cell reported the same datum, and a value-namespace target splicing
//! unescaped text into a table row.

use super::tests::*;
use super::*;

/// A row-reference cell renders as a link to the row it names.
///
/// Rendered through the whole pipeline rather than by inspecting the cell,
/// because the cell emits a reference and something else turns it into a
/// link. Checking the intermediate form would pass while the document
/// shipped `{{ slot::display }}` as literal text, which is the failure this
/// test exists for.
#[test]
fn a_row_reference_cell_becomes_a_link() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
    cfg.registry_namespaces = nss.clone();
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
        rendered.contains("[display](SLOT.md#display)"),
        "a row-reference cell did not become a link: {rendered}"
    );
    assert!(
        !rendered.contains("{{"),
        "an unresolved reference reached the document: {rendered}"
    );
}

/// The control: the same value in a `string[]` field stays text. The type
/// is what makes a cell a link, so a renderer that linked every slug-shaped
/// value would pass the test above and rewrite prose.
#[test]
fn an_untyped_cell_holding_the_same_value_stays_text() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "string[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
    cfg.registry_namespaces = nss.clone();
    let table = render_table(&nss[1], &r, &cfg);
    assert!(
        !table.contains("{{"),
        "an ordinary field was rendered as a reference: {table}"
    );
    assert!(table.contains("| display |"), "{table}");
}

fn resolved(text: &str, nss: &[RegistryNamespace], r: &Registry) -> String {
    let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
    cfg.registry_namespaces = nss.to_vec();
    resolve_all(
        text,
        nss,
        r,
        &BTreeMap::new(),
        Path::new("/repo"),
        Path::new("/repo/docs"),
        &cfg,
    )
}

fn answered() -> (Vec<RegistryNamespace>, Registry) {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("slot", "audio", &[]),
        ("answer", "niri", &[("slot", "display")]),
        ("answer", "wlr", &[("slot", "display")]),
    ]);
    (nss, r)
}

#[test]
fn refsto_names_what_answers_a_row() {
    let (nss, r) = answered();
    assert_eq!(
        resolved("{{ refsto(slot::display) }}", &nss, &r),
        "[niri](ANSWER.md#niri), [wlr](ANSWER.md#wlr)"
    );
}

/// An unanswered row resolves to nothing, which is the finding. It must not
/// be confused with the reference failing to resolve at all.
#[test]
fn refsto_on_an_unanswered_row_is_empty_rather_than_dangling() {
    let (nss, r) = answered();
    assert_eq!(resolved("[{{ refsto(slot::audio) }}]", &nss, &r), "[]");
}

/// The control on that: a row that does not exist is a different answer
/// from a row nothing answers, and returning an empty list for both would
/// report every typo as a gap.
#[test]
fn refsto_on_a_row_that_does_not_exist_does_not_resolve() {
    let (nss, r) = answered();
    // Asserted exactly rather than as "not empty": `assert_ne!` admits a
    // panic message, a wrong row, and anything else that is not `[]`.
    assert_eq!(
        resolved("[{{ refsto(slot::nosuch) }}]", &nss, &r),
        "[{{ refsto(slot::nosuch) }}]",
        "a nonexistent row must keep its reference, which the unresolved-token scan then reports"
    );
}

/// A field reference and that field's table cell must agree. They are two
/// renderings of one value, and the first version of this had the cell
/// linking while the reference showed bare slugs.
#[test]
fn a_reference_to_a_row_reference_field_renders_links() {
    let (nss, r) = answered();
    assert_eq!(
        resolved("{{ answer::niri::slot }}", &nss, &r),
        "[display](SLOT.md#display)"
    );
}

/// The control: an ordinary field holding the same text renders as text.
#[test]
fn a_reference_to_an_ordinary_field_renders_its_value() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "string")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    assert_eq!(resolved("{{ answer::niri::slot }}", &nss, &r), "display");
}

fn embed(key: &str, fields: &[(&str, &str)]) -> RegistryNamespace {
    let mut n = ns(key, fields);
    n.render = RenderMode::Embed;
    n
}

/// A namespace with no page has nothing to link to, so a reference into it
/// is the slug. Before this, it produced a link to a file the generator
/// never writes, and the comment above the cell renderer claimed the case
/// was handled.
#[test]
fn a_reference_into_an_embed_namespace_is_plain_text() {
    let nss = vec![embed("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    assert_eq!(resolved("{{ answer::niri::slot }}", &nss, &r), "display");
    assert_eq!(resolved("{{ slot::display }}", &nss, &r), "display");
}

/// The control: the same shape with a page renders a link, so the test
/// above is not equally consistent with a renderer that never links.
#[test]
fn a_reference_into_a_page_namespace_is_a_link() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    assert_eq!(
        resolved("{{ answer::niri::slot }}", &nss, &r),
        "[display](SLOT.md#display)"
    );
}

/// A slug that resolves to nothing keeps its reference form, so the
/// unresolved-token scan reports it. Dropping it made a field of two slugs
/// render as one, with nothing saying so.
#[test]
fn a_field_reference_to_a_missing_row_keeps_its_reference() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "slot[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display, nosuch")]),
    ]);
    assert_eq!(
        resolved("{{ answer::niri::slot }}", &nss, &r),
        "[display](SLOT.md#display), {{ slot::nosuch }}"
    );
}

/// The same datum through the table cell, which is the path that was
/// already right. The two must agree, and asserting them apart is what
/// caught them disagreeing.
#[test]
fn the_cell_and_the_field_reference_agree_about_a_missing_row() {
    let nss = vec![ns("slot", &[]), ns("answer", &[("slot", "slot[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display, nosuch")]),
    ]);
    let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
    cfg.registry_namespaces = nss.clone();
    let table = render_table(&nss[1], &r, &cfg);
    assert!(table.contains("{{ slot::nosuch }}"), "{table}");
}

#[test]
fn a_reference_into_a_value_namespace_is_refused_at_the_declaration() {
    let mut slot = ns("slot", &[("use", "string")]);
    slot.value_field = Some("use".into());
    let nss = [slot, ns("answer", &[("slot", "slot")])];
    assert_eq!(kinds(&value_field_targets(&nss)), [
        "row-reference-to-a-value-namespace"
    ]);
}

/// The control: the same target without `value_field` is fine, so the check
/// is about the declaration rather than about row references generally.
#[test]
fn a_reference_into_an_ordinary_namespace_is_not_refused() {
    let nss = [ns("slot", &[("use", "string")]), ns("answer", &[("slot", "slot")])];
    assert!(value_field_targets(&nss).is_empty());
}

#[test]
fn a_stray_separator_is_reported_wherever_it_sits() {
    let nss = [ns("slot", &[]), ns("answer", &[("slots", "slot[]")])];
    for raw in [", display", "display, , display", "display, "] {
        let r = reg(&[
            ("slot", "display", &[]),
            ("answer", "niri", &[("slots", raw)]),
        ]);
        let found = validate_row_references(&r, &nss);
        assert_eq!(
            kinds(&found),
            ["malformed-row-reference"],
            "`{raw}` went unreported: {found:?}"
        );
    }
}
