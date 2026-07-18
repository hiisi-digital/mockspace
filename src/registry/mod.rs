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
        let d = dangling_references(text, &reg, &["vocab".to_string()].into_iter().collect());
        assert!(d.contains("vocab::nope"), "{d:?}");
        assert!(d.contains("vocab::xpbd::missing"), "{d:?}");
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
    fn a_citation_into_an_internal_root_leaves_no_trace_in_prose() {
        // The reference stays in the source, where it is checked and is the
        // provenance record. It does not reach a reader who cannot open what it
        // names, and it does not leave the punctuation that framed it behind.
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.internal_roots = ["seed".to_string()].into_iter().collect();
        cfg.registry_roots = [("seed".to_string(), "corpus".to_string())]
            .into_iter()
            .collect();
        let reg = Registry::default();
        let r = |s: &str| {
            resolve_all(s, &[], &reg, &cfg.registry_roots.clone(), Path::new("/r"), Path::new("/r/docs"), &cfg)
        };
        assert_eq!(
            r("The split becomes structural ({{ seed::DESIGN::189 }})."),
            "The split becomes structural."
        );
        assert_eq!(
            r("Bounded ({{ seed::DESIGN::618 }} {{ seed::IDENTITY::61 }})."),
            "Bounded."
        );
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
    fn a_crate_document_resolves_references_like_any_other() {
        // A crate document is where most references naturally live: it is where
        // a crate states which laws bind it and which constants it depends on.
        // Per-crate generation applied placeholders but never resolved
        // references, so every one rendered literally and nothing reported it,
        // which reads as a templating bug rather than as a missing check.
        let tmp = tempfile::tempdir().unwrap();
        let mock = tmp.path().join("mock");
        let crate_dir = mock.join("crates").join("proj-store");
        std::fs::create_dir_all(&crate_dir).unwrap();
        std::fs::write(
            crate_dir.join("DESIGN.md.tmpl"),
            "# store\n\nBound by {{ reg::law::keys }}.\n",
        )
        .unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            "[package]\nname = \"proj-store\"\n",
        )
        .unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();

        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.mock_dir = mock.clone();
        cfg.crates_dir = mock.join("crates");
        cfg.docs_dir = docs.clone();
        cfg.repo_root = tmp.path().to_path_buf();
        cfg.crate_prefix = "proj".into();
        cfg.registry_namespaces = vec![ns("law", None)];

        let reg = reg_with("keys", "law", &[("statement", "a key is closed")]);
        let crates = crate::parse::discover_crates(&cfg.crates_dir, &cfg.crate_prefix);
        let ph = crate::render_design::Placeholders::compute(&crates, &cfg);
        let plan = crate::document::plan(&cfg, &crates);
        crate::document::render_all(&plan, &ph, &reg, &cfg);

        let out: String = std::fs::read_dir(&docs)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
            .collect();
        assert!(!out.is_empty(), "no per-crate document was generated");
        assert!(
            !out.contains("{{ reg::law::keys }}"),
            "the reference rendered literally: {out}"
        );
        assert!(out.contains("LAW.md#keys"), "no link to the row: {out}");
    }

    #[test]
    fn an_unresolved_token_in_a_generated_document_is_reported() {
        // The net that catches a generation path forgetting to resolve. Two
        // did, and the symptom was a literal reference in a finished document
        // with nothing saying so.
        let doc = "# T\n\nBound by {{ reg::law::keys }}.\n\n```\nwrite {{ reg::law::x }}\n```\n\nDone.\n";
        let found = unresolved_in_generated(doc);
        assert_eq!(found, vec!["{{ reg::law::keys }}"], "{found:?}");
    }

    #[test]
    fn a_resolved_document_reports_nothing() {
        assert!(unresolved_in_generated("# T\n\nAll [good](LAW.md#keys).\n").is_empty());
    }

    #[test]
    fn a_crate_reference_resolves_before_its_document_is_written() {
        // The docs directory is cleaned at the start of a run and refilled
        // during it. Globbing for the file therefore answered "has this been
        // written yet" rather than "what is this crate's document", so a
        // reference from a crate rendered early to one rendered later resolved
        // to nothing, silently, in the finished document.
        let tmp = tempfile::tempdir().unwrap();
        let crates_dir = tmp.path().join("mock").join("crates");
        std::fs::create_dir_all(crates_dir.join("proj-plan-validate")).unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        // Deliberately empty: the referenced document does not exist yet.

        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.crates_dir = crates_dir;
        cfg.docs_dir = docs.clone();
        cfg.repo_root = tmp.path().to_path_buf();
        cfg.crate_prefix = "proj".into();
        cfg.ordered_docs = true;
        // Built from what will exist, not from what has been written.
        let plan = vec![crate::document::Planned::computed(
            crate::document::DocId::Crate {
                upper: "PLAN_VALIDATE".into(),
                subject: "OVERVIEW".into(),
                depth: 4,
            },
            String::new(),
        )];
        cfg.doc_index = crate::document::DocIndex::build(&plan, &cfg);

        let reg = Registry::default();
        let out = resolve_all(
            "see {{ crates::plan-validate }} now",
            &[],
            &reg,
            &BTreeMap::new(),
            tmp.path(),
            &docs,
            &cfg,
        );
        assert_eq!(out, "see [plan-validate](140_PLAN_VALIDATE.md) now", "{out}");
    }

    #[test]
    fn a_hyphenated_crate_short_name_resolves() {
        // `crates::plan-validate` is the common shape: most crate names in a
        // prefixed family have a hyphen in the part after the prefix.
        let tmp = tempfile::tempdir().unwrap();
        let crates_dir = tmp.path().join("mock").join("crates");
        std::fs::create_dir_all(crates_dir.join("proj-plan-validate")).unwrap();
        let docs = tmp.path().join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(docs.join("140_PLAN_VALIDATE.md"), "x").unwrap();

        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.crates_dir = crates_dir;
        cfg.docs_dir = docs.clone();
        cfg.repo_root = tmp.path().to_path_buf();
        cfg.crate_prefix = "proj".into();

        let reg = Registry::default();
        let out = resolve_all(
            "see {{ crates::plan-validate }} now",
            &[],
            &reg,
            &BTreeMap::new(),
            tmp.path(),
            &docs,
            &cfg,
        );
        assert_eq!(out, "see [plan-validate](140_PLAN_VALIDATE.md) now", "{out}");
    }

    #[test]
    fn row_data_resolves_against_the_same_index_documents_do() {
        // The wiring, not the unit. Every other test builds the index by hand,
        // so none of them notices when the index is built after the data has
        // already resolved against an empty one. That ordering shipped once and
        // is exactly the bug the index exists to prevent: the fallbacks name
        // `LAW.md` where the file is written as `902_LAW.md`.
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.ordered_docs = true;
        let nss = vec![ns("law", None)];
        cfg.registry_namespaces = nss.clone();

        let plan = vec![crate::document::Planned::computed(
            crate::document::DocId::Registry {
                page: "LAW.md".into(),
                index: 2,
            },
            String::new(),
        )];
        cfg.doc_index = crate::document::DocIndex::build(&plan, &cfg);

        // A row whose field references another row, which is what resolve_data
        // settles before any document reads it.
        let mut reg = reg_with("keys", "law", &[("statement", "closed")]);
        let row = RegistryRow {
            slug: "derived".into(),
            namespace: "law".into(),
            source: PathBuf::from("t.toml"),
            fields: [("statement".to_string(), "see {{ law::keys }}".to_string())]
                .into_iter()
                .collect(),
        };
        let q = row.qualified();
        reg.by_namespace.get_mut("law").unwrap().push(q.clone());
        reg.rows.insert(q, row);

        let (resolved, findings) = resolve_data(
            &nss,
            &reg,
            &BTreeMap::new(),
            Path::new("/r"),
            Path::new("/r/docs"),
            &cfg,
        );
        assert!(findings.is_empty(), "{findings:?}");
        let got = resolved
            .get("law::derived")
            .and_then(|r| r.fields.get("statement"))
            .unwrap();
        assert!(
            got.contains("902_LAW.md#keys"),
            "row data resolved against an empty index: {got}"
        );
    }

    #[test]
    fn a_namespace_resolves_without_the_reg_prefix() {
        // `reg::` carried no information: slot zero is either a declared root
        // or a declared namespace. It cost four characters on every reference
        // and read as ceremony.
        let reg = reg_with("keys", "law", &[("statement", "a key is closed")]);
        let nss = vec![ns("law", None)];
        assert_eq!(
            r_all("{{ law::keys::statement }}", &nss, &reg),
            "a key is closed"
        );
    }

    #[test]
    fn pathof_a_row_renders_its_provenance() {
        // "Where does this come from" is a different question from "what does
        // this say", and the answer for a row is the sources it rests on. The
        // TOML file it happens to sit in is a fact about filing.
        let reg = reg_with(
            "keys",
            "law",
            &[("provenance", "mock::DESIGN::12, mock::DESIGN::30")],
        );
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.registry_namespaces = vec![ns("law", None)];
        let out = resolve_all(
            "{{ pathof(law::keys) }}",
            &cfg.registry_namespaces.clone(),
            &reg,
            &BTreeMap::new(),
            Path::new("/r"),
            Path::new("/r/docs"),
            &cfg,
        );
        // Unresolvable roots yield nothing rather than a broken path, so the
        // shape is what matters here: it asked provenance, not the row.
        assert!(!out.contains("keys"), "rendered the row, not its sources: {out}");
    }

    #[test]
    fn pathof_an_internal_citation_renders_nothing() {
        let mut cfg = crate::config::Config::from_dir(Path::new("/nonexistent"));
        cfg.internal_roots = ["seed".to_string()].into_iter().collect();
        cfg.registry_roots = [("seed".to_string(), "corpus".to_string())]
            .into_iter()
            .collect();
        let reg = Registry::default();
        let out = resolve_all(
            "from {{ pathof(seed::DESIGN::844) }}.",
            &[],
            &reg,
            &cfg.registry_roots.clone(),
            Path::new("/r"),
            Path::new("/r/docs"),
            &cfg,
        );
        assert_eq!(out, "from.", "internal path leaked: {out}");
    }

    #[test]
    fn a_namespace_alone_renders_its_table() {
        let reg = reg_with("xpbd", "vocab", &[]);
        let out = r_all("{{ vocab }}", &[ns("vocab", None)], &reg);
        assert!(out.contains("xpbd"), "{out}");
        assert!(out.contains('|'), "not a table: {out}");
    }

    #[test]
    fn an_unknown_single_word_is_left_alone() {
        // Otherwise a stray word in prose would be eaten as a namespace.
        let reg = reg_with("xpbd", "vocab", &[]);
        assert_eq!(r_all("{{ nope }}", &[ns("vocab", None)], &reg), "{{ nope }}");
    }

    #[test]
    fn the_reg_prefix_still_resolves() {
        // Thousands of references were written with it.
        let reg = reg_with("keys", "law", &[("statement", "a key is closed")]);
        let nss = vec![ns("law", None)];
        assert_eq!(
            r_all("{{ reg::law::keys::statement }}", &nss, &reg),
            "a key is closed"
        );
    }

    #[test]
    fn a_namespace_that_is_also_a_root_is_reported() {
        // Otherwise `law::x` is ambiguous and the answer is a precedence rule
        // nobody can remember.
        let roots = [("law".to_string(), "docs/law".to_string())]
            .into_iter()
            .collect();
        let found = namespace_root_collisions(&[ns("law", None)], &roots);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].message.contains("ambiguous"), "{found:?}");
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
