//! A registry of terms and concepts, resolved at doc-generation time.
//!
//! A project accumulates identifiers for the things its documents refer to:
//! probes, measurements, constants, invariants, vocabulary. Without somewhere
//! to hold them, each document family invents its own convention and the
//! conventions drift. Worse, nothing can answer "what are all the X", and a
//! gap is only visible against an enumeration.
//!
//! Data lives under `<mock>/registry/`, arbitrarily nested, every `*.toml`
//! beneath it loaded. **A file's namespace is the array-of-tables key it
//! declares, never its path.** That is what makes the nesting free: a project
//! may file by subject (`registry/water.toml` holding `[[spike]]`, `[[bench]]`,
//! and `[[constant]]` rows) and still query by kind.
//!
//! # Identity
//!
//! A row is identified by its namespace and a slug: `vocab::xpbd`,
//! `spike::actuator_fit_converges`. Slugs are snake_case and unique within
//! their namespace.
//!
//! Slugs rather than numbers because a number carries no meaning, and an
//! identifier carrying no meaning has to be *managed*: never reused, never
//! renumbered, never reordered, since any of those silently repoints every
//! reference to it. A slug needs none of that discipline. It says what it
//! refers to, it survives reordering, and it stays readable in prose.
//!
//! # References
//!
//! One syntax, `root::selector...`, covers both kinds:
//!
//! - `reg::vocab::xpbd` selects a row; `reg::vocab::xpbd::what` selects a field.
//! - `seed::DESIGN::844` selects a line in a file, under a declared root.
//!
//! Both resolve at generation and both are checked, so a reference pointing
//! nowhere is reported rather than rendered as something that looks fine.
//!
//! Designed for v2, prototyped here in v1. See
//! `docs/REFERENCE-SYNTAX.md`.

//! # Layout
//!
//! Split along the path a row takes rather than by size: it is loaded, its
//! references are found, those are resolved, the result is rendered, and what
//! a schema structurally cannot check is validated.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

mod load;
mod model;
mod refs;
mod render;
mod resolve;
mod validate;

pub use load::*;
pub use model::*;
pub use refs::*;
pub use render::*;
pub use resolve::*;
pub use validate::*;



#[cfg(test)]
mod tests {
    use super::*;

    fn ns(key: &str, value_field: Option<&str>) -> RegistryNamespace {
        RegistryNamespace {
            key: key.into(),
            title: None,
            description: None,
            value_field: value_field.map(|s| s.to_string()),
            render: RenderMode::Page,
            group_by: None,
            fields: vec![],
        }
    }

    fn r_all(text: &str, nss: &[RegistryNamespace], reg: &Registry) -> String {
        let cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        resolve_all(text, nss, reg, &BTreeMap::new(), Path::new("/r"), Path::new("/r/docs"), &cfg)
    }

    fn reg_with(slug: &str, namespace: &str, fields: &[(&str, &str)]) -> Registry {
        let mut reg = Registry::default();
        let row = RegistryRow {
            slug: slug.into(),
            namespace: namespace.into(),
            source: PathBuf::from("t.toml"),
            fields: fields.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let q = row.qualified();
        reg.by_namespace.insert(namespace.into(), vec![q.clone()]);
        reg.rows.insert(q, row);
        reg
    }

    #[test]
    fn a_row_reference_becomes_a_link() {
        let reg = reg_with("xpbd", "vocab", &[]);
        let out = r_all("see {{ reg::vocab::xpbd }} now", &[ns("vocab", None)], &reg);
        assert_eq!(out, "see [xpbd](VOCAB.md#xpbd) now");
    }

    #[test]
    fn a_field_reference_renders_that_field() {
        // Lets a document state a constant once and have every mention of it
        // stay current.
        let reg = reg_with("froxel", "constant", &[("value", "32 km")]);
        let out = r_all("reaches {{ reg::constant::froxel::value }} out", &[ns("constant", None)], &reg);
        assert_eq!(out, "reaches 32 km out");
    }

    #[test]
    fn a_rust_path_is_not_a_citation() {
        // `::` is ordinary in prose about code. A citation needs a numeric
        // final segment, which is what keeps the two apart.
        assert!(FileRef::parse("std::mem::swap").is_none());
        assert!(FileRef::parse("seed::DESIGN::844").is_some());
    }

    #[test]
    fn a_citation_path_may_have_any_depth() {
        // One rule covers mock::DESIGN::12 and mock::crates::numeric::DESIGN::12,
        // so a deeper tree needs no root of its own.
        let r = FileRef::parse("mock::crates::numeric::DESIGN::12").unwrap();
        assert_eq!(r.root, "mock");
        assert_eq!(r.path, "crates/numeric/DESIGN");
        assert_eq!(r.anchor, Anchor::Line(12));
    }

    #[test]
    fn an_unknown_row_or_field_is_left_alone_and_reported() {
        let reg = reg_with("xpbd", "vocab", &[("what", "w")]);
        let text = "{{ reg::vocab::nope }} and {{ reg::vocab::xpbd::missing }}";
        assert_eq!(r_all(text, &[ns("vocab", None)], &reg), text);
        let d = dangling_references(text, &reg);
        assert!(d.contains("reg::vocab::nope"), "{d:?}");
        assert!(d.contains("reg::vocab::xpbd::missing"), "{d:?}");
    }

    #[test]
    fn code_fences_are_not_rewritten() {
        let reg = reg_with("xpbd", "vocab", &[]);
        let text = "a {{ reg::vocab::xpbd }}\n```\nb {{ reg::vocab::xpbd }}\n```";
        let out = r_all(text, &[ns("vocab", None)], &reg);
        assert!(out.contains("```\nb {{ reg::vocab::xpbd }}\n```"), "fence rewritten: {out}");
    }

    #[test]
    fn a_placeholder_is_required_for_a_reference() {
        // The braces are what make a reference stated rather than guessed. A
        // project with a root named `core` would otherwise silently link
        // `core::mem::12`, and prose about code would be rewritten by accident.
        let exprs = placeholder_exprs("a {{ reg::vocab::xpbd }} and bare reg::vocab::xpbd");
        assert_eq!(exprs.len(), 1);
        assert_eq!(exprs[0].1, "reg::vocab::xpbd");
    }

    #[test]
    fn one_syntax_covers_rows_fields_tables_and_files() {
        let reg = reg_with("xpbd", "vocab", &[("what", "constraint projection")]);
        let nss = vec![ns("vocab", None)];
        let roots = BTreeMap::new();
        let r = |s: &str| {
            let cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
            resolve_all(s, &nss, &reg, &roots, Path::new("/r"), Path::new("/r/docs"), &cfg)
        };
        assert_eq!(r("{{ reg::vocab::xpbd }}"), "[xpbd](VOCAB.md#xpbd)");
        assert_eq!(r("{{ reg::vocab::xpbd::what }}"), "constraint projection");
        // The id cell carries the row's anchor inline, so a reference lands on
        // the row itself rather than on a restatement of it below the table.
        assert!(r("{{ reg::vocab }}").contains("<a id=\"xpbd\"></a>xpbd"));
        // Unresolvable stays visibly unresolved rather than becoming a
        // plausible-looking wrong link.
        assert_eq!(r("{{ reg::vocab::nope }}"), "{{ reg::vocab::nope }}");
    }

    #[test]
    fn a_heading_anchor_parses_and_renders() {
        let r = FileRef::parse("seed::DESIGN::#the-four-lanes").unwrap();
        assert_eq!(r.anchor, Anchor::Heading("the-four-lanes".into()));
        assert_eq!(r.render(), "seed::DESIGN::#the-four-lanes");
    }

    #[test]
    fn heading_slugs_match_the_forge_form() {
        // So a reader can click the same anchor the link generates.
        assert_eq!(heading_slug("The four lanes and the joint LOD cut"),
                   "the-four-lanes-and-the-joint-lod-cut");
        assert_eq!(heading_slug("R(V), the drift class"), "r-v-the-drift-class");
    }

    #[test]
    fn a_line_anchor_still_parses() {
        // Kept for frozen roots, where the file genuinely does not move.
        assert_eq!(FileRef::parse("seed::DESIGN::844").unwrap().anchor, Anchor::Line(844));
        assert!(FileRef::parse("seed::DESIGN::0").is_none());
        assert!(FileRef::parse("seed::DESIGN::#").is_none());
    }

    #[test]
    fn a_builtin_namespace_is_addressed_directly() {
        // vocab::xpbd, not reg::vocab::xpbd. Builtins exist in every project so
        // the short form is safe everywhere; a project's own namespace stays
        // behind reg:: to read as what it is.
        let reg = reg_with("xpbd", "vocab", &[("what", "constraint projection")]);
        let nss = with_builtins(&[]);
        assert_eq!(
            r_all("{{ vocab::xpbd::what }}", &nss, &reg),
            "constraint projection"
        );
    }

    #[test]
    fn reference_is_builtin_and_renders_as_a_citation() {
        let mut reg = Registry::default();
        let row = RegistryRow {
            slug: "burns_hunt".into(),
            namespace: "reference".into(),
            source: PathBuf::from("t.toml"),
            fields: [
                ("title", "The Visibility Buffer"),
                ("authors", "Burns and Hunt"),
                ("venue", "JCGT"),
                ("year", "2013"),
            ]
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        };
        reg.by_namespace.insert("reference".into(), vec![row.qualified()]);
        reg.rows.insert(row.qualified(), row);
        // Short by default: it has to fit a table cell beside hundreds of
        // others, and the full form is one click away.
        assert_eq!(
            r_all("{{ reference::burns_hunt }}", &with_builtins(&[]), &reg),
            "[Burns and Hunt 2013](REFERENCE.md#burns-hunt)"
        );
        // The full form on request, for prose that wants the title.
        assert_eq!(
            r_all("{{ reference::burns_hunt::citation }}", &with_builtins(&[]), &reg),
            "[Burns and Hunt, The Visibility Buffer (JCGT 2013)](REFERENCE.md#burns-hunt)"
        );
        // A real field is still a real field.
        assert_eq!(
            r_all("{{ reference::burns_hunt::year }}", &with_builtins(&[]), &reg),
            "2013"
        );
    }

    fn field(name: &str, vis: FieldVisibility) -> RegistryField {
        RegistryField {
            name: name.into(),
            r#type: "string".into(),
            required: false,
            description: None,
            visibility: vis,
        }
    }

    #[test]
    fn an_internal_field_never_becomes_a_column() {
        // The field stays validated and greppable in the source; what it does
        // not do is reach a reader who cannot act on it.
        let mut ns = ns("law", None);
        ns.fields = vec![
            field("what", FieldVisibility::Public),
            field("seed_id", FieldVisibility::Internal),
        ];
        let reg = reg_with("keys", "law", &[("what", "a key is closed"), ("seed_id", "[24B]")]);
        let cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        let table = render_table(&ns, &reg, &cfg);
        assert!(table.contains("what"), "{table}");
        assert!(!table.contains("seed_id"), "internal column rendered: {table}");
        assert!(!table.contains("[24B]"), "internal value rendered: {table}");
    }

    #[test]
    fn an_internal_field_does_not_resolve_by_reference() {
        // Otherwise the guarantee has a documented way around it, which is
        // the same as not having it.
        let mut ns = ns("law", None);
        ns.fields = vec![field("seed_id", FieldVisibility::Internal)];
        let reg = reg_with("keys", "law", &[("seed_id", "[24B]")]);
        let text = "{{ reg::law::keys::seed_id }}";
        assert_eq!(r_all(text, &[ns], &reg), text);
    }

    #[test]
    fn an_internal_root_is_filtered_per_item_not_per_cell() {
        // A row sourced from both an internal corpus and a public document
        // keeps the citation a reader can follow. Dropping the whole cell
        // would lose the useful one in order to hide the useless one.
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.internal_roots = ["seed".to_string()].into_iter().collect();
        let mut ns = ns("law", None);
        ns.fields = vec![field("provenance", FieldVisibility::Public)];
        let reg = reg_with(
            "keys",
            "law",
            &[("provenance", "seed::DESIGN::844, mock::DESIGN::12")],
        );
        let table = render_table(&ns, &reg, &cfg);
        assert!(!table.contains("seed::DESIGN"), "internal root survived: {table}");
        assert!(table.contains("mock::DESIGN::12"), "public citation lost: {table}");
    }

    #[test]
    fn a_column_emptied_by_filtering_drops_like_any_empty_column() {
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.internal_roots = ["seed".to_string()].into_iter().collect();
        let mut ns = ns("law", None);
        ns.fields = vec![field("provenance", FieldVisibility::Public)];
        let reg = reg_with("keys", "law", &[("provenance", "seed::DESIGN::844")]);
        let table = render_table(&ns, &reg, &cfg);
        assert!(!table.contains("provenance"), "empty column kept: {table}");
    }

    #[test]
    fn prose_is_not_mistaken_for_a_citation_and_eaten() {
        // A field holding ordinary text that happens to contain `::` must
        // survive filtering intact.
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.internal_roots = ["seed".to_string()].into_iter().collect();
        let mut ns = ns("law", None);
        ns.fields = vec![field("what", FieldVisibility::Public)];
        let reg = reg_with("keys", "law", &[("what", "std::mem::swap is not a citation")]);
        let table = render_table(&ns, &reg, &cfg);
        assert!(table.contains("std::mem::swap is not a citation"), "{table}");
    }

    #[test]
    fn slugs_are_snake_case() {
        assert!(is_valid_slug("xpbd") && is_valid_slug("waist_a") && is_valid_slug("lane_2"));
        assert!(!is_valid_slug("Waist-A"));
        assert!(!is_valid_slug("2fast"));
        assert!(!is_valid_slug(""));
    }

    #[test]
    fn vocab_and_the_builtin_roots_need_no_declaration() {
        let merged = with_builtins(&[]);
        assert!(merged.iter().any(|n| n.key == "vocab"));
        let roots = builtin_roots("mock");
        assert_eq!(roots.get("mock").map(String::as_str), Some("mock"));
        assert_eq!(roots.get("live").map(String::as_str), Some("."));
        // Project-specific roots stay the project's: not every project indexes
        // a corpus, so nothing named `seed` is baked in.
        assert!(!roots.contains_key("seed"));
    }
}
