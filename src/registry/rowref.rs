use super::*;

/// The field types that are not a namespace name.
///
/// `ref` and `ref[]` hold a citation into a file: `root::path::anchor`. They
/// are named for what the string means rather than for where it points, which
/// is why a field holding a row reference cannot reuse them. A citation and a
/// row reference resolve against different things and fail in different ways,
/// so one word for both would make every finding ambiguous about which was
/// intended.
pub const SCALAR_TYPES: &[&str] = &["string", "string[]", "integer", "boolean", "ref", "ref[]"];

/// The namespace a field type names, and whether the field holds many.
///
/// A field type is either one of the scalars above or the key of a declared
/// namespace, so `type = "slot"` holds one slot and `type = "slot[]"` holds
/// several. There is no third syntax and no prefix, for the same reason a
/// reference addresses a namespace by its own name: slot zero is either a
/// scalar type or a declared namespace, and `namespace_type_collisions`
/// refuses a project that makes the two collide.
///
/// Returns `None` for a scalar. Returns the base name for anything else,
/// including a name no namespace declares, because deciding whether the name
/// is known is the caller's job and the two failures want different messages.
pub fn row_reference_target(ty: &str) -> Option<(&str, bool)> {
    if SCALAR_TYPES.contains(&ty) {
        return None;
    }
    match ty.strip_suffix("[]") {
        Some(base) => Some((base, true)),
        None => Some((ty, false)),
    }
}

/// The row-reference fields of each namespace: field name, target namespace.
pub fn row_reference_fields(
    namespaces: &[RegistryNamespace],
) -> BTreeMap<String, Vec<(String, String)>> {
    let known: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
    namespaces
        .iter()
        .map(|ns| {
            let fields = ns
                .fields
                .iter()
                .filter_map(|f| {
                    let (target, _) = row_reference_target(&f.r#type)?;
                    known
                        .contains(target)
                        .then(|| (f.name.clone(), target.to_string()))
                })
                .collect();
            (ns.key.clone(), fields)
        })
        .collect()
}

/// A field type that names neither a scalar nor a declared namespace.
///
/// Reported rather than defaulted. The schema generator's type match ends in a
/// catch-all that produces a string, so before this check a misspelled
/// `strng`, or a `slot` written before the namespace it names was declared,
/// became an unvalidated string field and every value in it went unchecked
/// while the declaration read as though it constrained something.
pub fn unknown_field_types(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    let known: BTreeSet<&str> = namespaces.iter().map(|n| n.key.as_str()).collect();
    let mut out = Vec::new();
    for ns in namespaces {
        for f in &ns.fields {
            let Some((target, _)) = row_reference_target(&f.r#type) else {
                continue;
            };
            if known.contains(target) {
                continue;
            }
            out.push(RegistryFinding {
                kind:    "unknown-field-type",
                message: format!(
                    "namespace `{}` declares field `{}` with type `{}`, which is neither a builtin type ({}) nor a declared namespace. A type nothing recognises used to become a plain string, so the field read as constrained and was not.",
                    ns.key,
                    f.name,
                    f.r#type,
                    SCALAR_TYPES.join(", ")
                ),
                source:  None,
            });
        }
    }
    out
}

/// A namespace whose key is also a builtin type name.
///
/// `key = "string"` would make `type = "string"` ambiguous between the scalar
/// and a reference into that namespace, and the ambiguity would resolve
/// silently in the scalar's favour because the scalar is checked first. Refused
/// at the declaration rather than left to surprise whoever writes the field.
pub fn namespace_type_collisions(namespaces: &[RegistryNamespace]) -> Vec<RegistryFinding> {
    namespaces
        .iter()
        .filter(|ns| SCALAR_TYPES.contains(&ns.key.as_str()))
        .map(|ns| RegistryFinding {
            kind:    "namespace-shadows-type",
            message: format!(
                "namespace `{}` has the same name as a builtin field type. A field declaring that type would mean the scalar, silently, and no field could ever reference this namespace.",
                ns.key
            ),
            source:  None,
        })
        .collect()
}

/// Check every row reference against the rows that exist.
///
/// A row reference is a bare slug. The field's type already names the
/// namespace, so repeating it in the value would be a second place for the two
/// to disagree, and a qualified value is refused rather than accepted as a
/// second spelling: one thing written two ways needs normalising everywhere it
/// is read, and the normalisations drift.
pub fn validate_row_references(
    reg: &Registry,
    namespaces: &[RegistryNamespace],
) -> Vec<RegistryFinding> {
    let bearing = row_reference_fields(namespaces);
    let mut out = Vec::new();

    for row in reg.rows.values() {
        let empty = Vec::new();
        let fields = bearing.get(&row.namespace).unwrap_or(&empty);
        for (name, target) in fields {
            let Some(raw) = row.fields.get(name) else {
                continue;
            };
            for item in raw.split(", ").map(str::trim).filter(|s| !s.is_empty()) {
                if !is_valid_slug(item) {
                    let hint = match item.split_once("::") {
                        Some((ns, slug)) if ns == target => {
                            format!(" Write `{slug}`: the field's type already says `{target}`.")
                        },
                        Some((ns, _)) => format!(
                            " It names namespace `{ns}`, but this field is typed `{target}`; a field references one namespace and the type is where that is stated."
                        ),
                        None => String::new(),
                    };
                    out.push(RegistryFinding {
                        kind:    "malformed-row-reference",
                        message: format!(
                            "{}: field `{name}` holds `{item}`, which is not a slug (snake_case, starting with a letter).{hint}",
                            row.qualified()
                        ),
                        source:  Some(row.source.clone()),
                    });
                    continue;
                }
                let qualified = format!("{target}::{item}");
                if reg.rows.contains_key(&qualified) {
                    continue;
                }
                let near = nearest(reg, target, item);
                out.push(RegistryFinding {
                    kind:    "unknown-row-reference",
                    message: format!(
                        "{}: field `{name}` references `{qualified}`, which no row declares.{near}",
                        row.qualified()
                    ),
                    source:  Some(row.source.clone()),
                });
            }
        }
    }
    out
}

/// The closest existing slug in the target namespace, when one is close enough
/// to be worth naming. A typo and a genuinely absent row are different
/// problems, and the message should not make them read alike.
fn nearest(reg: &Registry, namespace: &str, slug: &str) -> String {
    let empty = Vec::new();
    let ids = reg.by_namespace.get(namespace).unwrap_or(&empty);
    if ids.is_empty() {
        return format!(" Namespace `{namespace}` has no rows at all.");
    }
    let best = ids
        .iter()
        .filter_map(|q| q.split_once("::").map(|(_, s)| s))
        .map(|s| (edit_distance(slug, s), s))
        .min_by_key(|(d, _)| *d);
    match best {
        // A third of the length, so a short slug is not matched to an
        // unrelated short slug: `at` and `on` differ by two and share nothing.
        Some((d, s)) if d * 3 <= slug.len().max(1) => format!(" Did you mean `{s}`?"),
        _ => String::new(),
    }
}

/// Levenshtein distance, two rows at a time.
///
/// Written out rather than pulled in: the crate has no dependency for this,
/// and a suggestion is the only consumer, so being approximate would be fine
/// and being exact costs a dozen lines.
fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Every row that references `qualified` through a typed row-reference field.
///
/// The direction the data is not stored in. A row states what it references;
/// what references it is derived, so the two cannot disagree and neither has
/// to be maintained. This is what makes a demand-side question answerable:
/// given a slot, which rows answer it.
///
/// Returned as `namespace::slug`, sorted, each appearing once however many of
/// its fields point at the target.
pub fn referrers(reg: &Registry, namespaces: &[RegistryNamespace], qualified: &str) -> Vec<String> {
    let Some((target_ns, target_slug)) = qualified.split_once("::") else {
        return Vec::new();
    };
    let bearing = row_reference_fields(namespaces);
    let mut out = BTreeSet::new();
    for row in reg.rows.values() {
        let empty = Vec::new();
        for (name, target) in bearing.get(&row.namespace).unwrap_or(&empty) {
            if target != target_ns {
                continue;
            }
            let Some(raw) = row.fields.get(name) else {
                continue;
            };
            if raw
                .split(", ")
                .map(str::trim)
                .any(|item| item == target_slug)
            {
                out.insert(row.qualified());
            }
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ns(key: &str, fields: &[(&str, &str)]) -> RegistryNamespace {
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
    fn reg(rows: &[(&str, &str, &[(&str, &str)])]) -> Registry {
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

    fn kinds(found: &[RegistryFinding]) -> Vec<&str> {
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

    #[test]
    fn every_scalar_type_is_accepted() {
        let fields: Vec<(&str, &str)> = SCALAR_TYPES.iter().map(|t| (*t, *t)).collect();
        let nss = [ns("answer", &fields)];
        assert!(
            unknown_field_types(&nss).is_empty(),
            "a scalar type was reported as unknown"
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
        let out = resolved("[{{ refsto(slot::nosuch) }}]", &nss, &r);
        assert_ne!(out, "[]", "a nonexistent row must not read as an empty answer");
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
}
