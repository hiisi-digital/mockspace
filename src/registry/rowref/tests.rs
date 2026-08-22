//! The four checks: what each reports, and the control proving it does not
//! report the same thing everywhere.
//!
//! The rendering and query paths are in `render_tests`, which shares these
//! helpers.

use super::*;

use super::*;

pub(super) fn ns(key: &str, fields: &[(&str, &str)]) -> RegistryNamespace {
    RegistryNamespace {
        key:         key.into(),
        title:       None,
        description: None,
        value_field: None,
        render:      RenderMode::Page,
        group_by:    None,
        fields:      fields
            .iter()
            .map(|(name, ty)| RegistryField {
                name:        (*name).into(),
                r#type:      (*ty).into(),
                required:    false,
                description: None,
                visibility:  FieldVisibility::Public,
            })
            .collect(),
    }
}

/// A registry of several rows across several namespaces, which the shared
/// single-row helper cannot express and every check here needs.
pub(super) fn reg(rows: &[(&str, &str, &[(&str, &str)])]) -> Registry {
    let mut reg = Registry::default();
    for (namespace, slug, fields) in rows {
        let row = RegistryRow {
            slug:      (*slug).into(),
            namespace: (*namespace).into(),
            source:    PathBuf::from("t.toml"),
            fields:    fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        };
        let q = row.qualified();
        reg.by_namespace
            .entry((*namespace).into())
            .or_default()
            .push(q.clone());
        reg.rows.insert(q, row);
    }
    reg
}

pub(super) fn kinds(found: &[RegistryFinding]) -> Vec<&str> {
    found.iter().map(|f| f.kind).collect()
}

#[test]
fn a_slug_naming_a_row_that_exists_is_accepted() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    assert!(
        validate_row_references(&r, &nss).is_empty(),
        "a reference to a row that exists must be accepted"
    );
}

/// The case that must fail. Without it the test above is equally consistent
/// with a validator that reports nothing at all, which is what an earlier
/// draft of this module was: `row_reference_fields` returned an empty map
/// because it filtered on `is_reference_type` rather than on the namespace
/// set, and every reference in the corpus passed.
#[test]
fn a_slug_naming_no_row_is_reported() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "audio")]),
    ]);
    let found = validate_row_references(&r, &nss);
    assert_eq!(kinds(&found), ["unknown-row-reference"], "{found:?}");
    assert!(
        found[0].message.contains("slot::audio"),
        "the message must name what was not found: {}",
        found[0].message
    );
}

/// The type is what makes a field hold references. The same value in a
/// `string[]` field is prose, and prose that happens to name a row is not a
/// reference to it.
#[test]
fn the_same_bad_value_in_an_untyped_field_is_not_reported() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "string[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "audio")]),
    ]);
    assert!(
        validate_row_references(&r, &nss).is_empty(),
        "a field declared `string[]` must not be validated as references"
    );
}

#[test]
fn a_qualified_value_is_refused_and_the_message_says_what_to_write() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "slot::display")]),
    ]);
    let found = validate_row_references(&r, &nss);
    assert_eq!(kinds(&found), ["malformed-row-reference"], "{found:?}");
    assert!(
        found[0].message.contains("Write `display`"),
        "the message must give the accepted form: {}",
        found[0].message
    );
}

#[test]
fn a_near_miss_names_the_slug_it_is_near() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "compositor", &[]),
        ("answer", "niri", &[("slot", "compositer")]),
    ]);
    let found = validate_row_references(&r, &nss);
    assert!(
        found[0].message.contains("Did you mean `compositor`?"),
        "{}",
        found[0].message
    );
}

/// The control on the suggestion: two short unrelated slugs must not be
/// offered for each other. The first draft compared against a fixed
/// distance of two and suggested `at` for `on`.
#[test]
fn an_unrelated_slug_is_not_offered_as_a_correction() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[("slot", "at", &[]), ("answer", "x", &[("slot", "on")])]);
    let found = validate_row_references(&r, &nss);
    assert!(
        !found[0].message.contains("Did you mean"),
        "an unrelated slug was offered as a correction: {}",
        found[0].message
    );
}

#[test]
fn a_type_naming_nothing_is_reported() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slott")])];
    let found = unknown_field_types(&nss);
    assert_eq!(kinds(&found), ["unknown-field-type"], "{found:?}");
}

#[test]
fn a_type_naming_a_declared_namespace_is_not_reported() {
    let nss = [
        ns("slot", &[]),
        ns("answer", &[("slot", "slot"), ("slots", "slot[]")]),
    ];
    assert!(unknown_field_types(&nss).is_empty());
}

/// The scalar types, written out rather than read from `SCALAR_TYPES`.
///
/// Building the fixture from the same constant the check reads makes a
/// closed loop: removing `boolean` from the list would keep this green
/// while turning every `boolean` field in every consumer into a hard error.
/// A literal list is the only version of this test that can disagree with
/// the code.
#[test]
fn every_scalar_type_is_accepted() {
    let literal = ["string", "string[]", "integer", "boolean", "ref", "ref[]"];
    let fields: Vec<(&str, &str)> = literal.iter().map(|t| (*t, *t)).collect();
    let nss = [ns("answer", &fields)];
    let found = unknown_field_types(&nss);
    assert!(found.is_empty(), "a scalar type was reported: {found:?}");
    assert_eq!(
        literal.len(),
        SCALAR_TYPES.len(),
        "a scalar type was added or removed and this list did not follow"
    );
}

#[test]
fn a_namespace_named_after_a_scalar_type_is_refused() {
    let nss = [ns("string", &[]), ns("answer", &[])];
    assert_eq!(kinds(&namespace_type_collisions(&nss)), [
        "namespace-shadows-type"
    ]);
    assert!(namespace_type_collisions(&[ns("slot", &[])]).is_empty());
}

#[test]
fn an_array_field_validates_every_element() {
    let nss = [ns("slot", &[]), ns("answer", &[("slots", "slot[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slots", "display, audio, input")]),
    ]);
    let found = validate_row_references(&r, &nss);
    assert_eq!(found.len(), 2, "both absent elements must report: {found:?}");
}

#[test]
fn referrers_finds_the_rows_that_point_at_one() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
        ("answer", "wlr", &[("slot", "display")]),
        ("answer", "pipewire", &[("slot", "audio")]),
    ]);
    assert_eq!(referrers(&r, &nss, "slot::display"), [
        "answer::niri",
        "answer::wlr"
    ]);
}

/// The control: a row holding the same text in an ordinary field is not a
/// referrer. Without this, `referrers` scanning every field would pass the
/// test above and be wrong about every project that writes prose.
#[test]
fn an_untyped_field_holding_the_same_slug_is_not_a_referrer() {
    let nss = [ns("slot", &[]), ns("answer", &[("note", "string")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("note", "display")]),
    ]);
    assert!(referrers(&r, &nss, "slot::display").is_empty());
}

#[test]
fn a_row_nothing_points_at_has_no_referrers() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("slot", "audio", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    assert!(
        referrers(&r, &nss, "slot::audio").is_empty(),
        "an unanswered slot is the finding, and it is an empty list"
    );
}

#[test]
fn a_partial_slug_match_is_not_a_referrer() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display_scaling")]),
    ]);
    assert!(
        referrers(&r, &nss, "slot::display").is_empty(),
        "a slug that merely starts with the target must not count"
    );
}

/// The view a lint and a tool are handed must carry the same reverse edges
/// `refsto` computes, or a check and a document disagree about the same data.
#[test]
fn the_view_carries_the_rows_and_the_reverse_edges() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "slot[]")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("slot", "audio", &[]),
        ("answer", "niri", &[("slot", "display")]),
        ("answer", "wlr", &[("slot", "display, audio")]),
    ]);
    let v = build_view(&r, &nss);
    assert_eq!(v.len(), 4);
    assert_eq!(v.rows_in("slot"), ["slot::audio", "slot::display"]);
    assert_eq!(v.referrers("slot::display"), ["answer::niri", "answer::wlr"]);
    assert_eq!(v.referrers("slot::audio"), ["answer::wlr"]);
    assert_eq!(v.field("answer::niri", "slot"), Some("display"));
    // The same question `refsto` answers, from the same data.
    assert_eq!(v.referrers("slot::display"), referrers(&r, &nss, "slot::display"));
}

/// The control: a field that is not typed as a namespace contributes no edge,
/// so a view built by scanning every field would fail this.
#[test]
fn an_untyped_field_contributes_no_edge_to_the_view() {
    let nss = [ns("slot", &[]), ns("answer", &[("slot", "string")])];
    let r = reg(&[
        ("slot", "display", &[]),
        ("answer", "niri", &[("slot", "display")]),
    ]);
    let v = build_view(&r, &nss);
    assert_eq!(v.len(), 2, "the rows are still there");
    assert!(
        v.referrers("slot::display").is_empty(),
        "an ordinary string field must not become a relation"
    );
}
