//! Load a registry belonging to another project, and hold it to the same gates.
//!
//! ## Why this exists
//!
//! Every other registry test runs against fixtures written alongside the code
//! that reads them, so both sides share whatever the author assumed. The only
//! large registry in the workspace was written by a different project, against
//! a different reading of the same design, before most of this code existed:
//! fifteen namespaces and a few thousand rows in `ikiuni_renderer`.
//!
//! A parser that agrees with its own fixtures and nothing else is a parser
//! nobody has tested. This is the arm that can disagree.
//!
//! ## Why it is opt-in rather than pinned to a path
//!
//! It reads a tree that is not part of this repository, so it is skipped unless
//! `MOCKSPACE_FOREIGN_REGISTRY` names one. A test that hard-codes a sibling
//! clone's path passes on one machine and fails everywhere else, which is worse
//! than a test that says plainly it did not run.
//!
//! It carries `#[ignore]` rather than a skip-and-return, and it panics rather
//! than returning when the variable is absent: `cargo test` captures stdout and
//! stderr for a test that passes, so a skip notice printed from inside a `#[test]`
//! that then returns `ok` is invisible under a plain `cargo test` and reads
//! exactly like a real pass. A failing test's captured output is always shown,
//! `#[ignore]` keeps it out of a default run, and `--ignored` still reports
//! *why* nothing ran the moment the variable is missing, because the panic
//! message is exactly what a failing test prints.
//!
//! Set it to the project's `mock/` directory:
//!
//! ```text
//! MOCKSPACE_FOREIGN_REGISTRY=~/Dev/clause-dev/ikiuni_renderer/mock \
//!     cargo test --test foreign_registry_loads -- --ignored
//! ```
//!
//! Anything this finds that is genuinely a defect gets a minimal fixture
//! committed here, so the finding survives without the sibling clone.

use std::path::{Path, PathBuf};

use mockspace::registry::{Registry, RegistryNamespace};

fn foreign_mock_dir() -> PathBuf {
    let raw = std::env::var("MOCKSPACE_FOREIGN_REGISTRY").unwrap_or_else(|_| {
        panic!(
            "set MOCKSPACE_FOREIGN_REGISTRY to another project's mock/ directory \
             to run this. It panics rather than returning, because a silent \
             return reads as a pass and verifies nothing."
        )
    });
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        PathBuf::from(std::env::var("HOME").expect("HOME is set")).join(rest)
    } else {
        PathBuf::from(raw)
    };
    if !expanded.is_dir() {
        panic!(
            "MOCKSPACE_FOREIGN_REGISTRY={} is not a directory",
            expanded.display()
        );
    }
    expanded
}

/// Every declared namespace the loader read a file for and produced no rows
/// for, by key.
///
/// Shared between the arm that reads a real corpus and the arm that builds
/// one, so a defect in this detection breaks both rather than only the arm
/// that happens to be skipped by default.
///
/// **Presence comes from `reg.files_read`, which is the loader's own record.**
/// An earlier version walked `registry/` itself and modelled a namespace's
/// files as `registry/<key>.toml` or a direct child of `registry/<key>/`. That
/// is a second implementation of a decision the loader had already made, and
/// the two disagreed in both directions: the loader recurses to unbounded
/// depth, so a namespace whose files sit at `registry/law/2026/deep.toml` read
/// as having none and a genuine drop there was invisible; and the loader takes
/// a row's namespace from the array-of-tables key rather than the path, so a
/// populated `registry/everything.toml` holding `[[law]]` read as no files at
/// all and failed the coverage assertion on a registry that had loaded fine.
/// Both are pinned as tests below. The file already condemned this exact
/// mistake in its own earlier revision and then committed it here.
fn namespaces_with_files_but_no_rows(
    namespaces: &[RegistryNamespace],
    reg: &Registry,
) -> Vec<String> {
    let mut out = Vec::new();
    for ns in namespaces {
        // A namespace has files if any file the loader read declares it, which
        // is the same question the loader answered when it built `by_namespace`.
        let has_files = reg
            .files_read
            .iter()
            .any(|p| file_declares_namespace(p, &ns.key));
        if has_files && reg.by_namespace.get(&ns.key).is_none_or(Vec::is_empty) {
            out.push(ns.key.clone());
        }
    }
    out
}

/// Whether `path` mentions `key` at the top level at all, in any table shape.
///
/// **Deliberately wider than the loader's own predicate, and that is the whole
/// value of it.** `load_registry` accepts only an array of tables
/// (`as_array_of_tables()`), so a file writing `[law]` where it meant `[[law]]`
/// hits that arm, `continue`s, and produces no rows **with nothing printed**.
/// That is the single-vs-double-bracket slip, it is the one drop path the
/// loader is silent about, and a check that mirrors the loader's predicate
/// cannot see it: a mirror never disagrees with what it mirrors.
///
/// So this asks the weaker question, "does this file talk about this namespace
/// at all", and lets the row count answer the rest. A file mentioning `law` in
/// any table shape, where no `law` row loaded, is a drop worth reporting
/// whichever arm swallowed it.
fn file_declares_namespace(path: &Path, key: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return false;
    };
    doc.get(key).is_some()
}

#[test]
#[ignore = "set MOCKSPACE_FOREIGN_REGISTRY to a mock/ directory to run this"]
fn a_foreign_registry_parses_and_validates() {
    let mock_dir = foreign_mock_dir();

    // No assertion on where mockspace.toml sits. `Config::from_dir` looks in
    // the mock dir first and then at the repo root, deliberately, because the
    // workspace is mid-migration from one to the other: four repos have moved
    // and four have not. An earlier version of this test asserted the config
    // was in the mock dir and therefore refused half of them, which is a test
    // re-implementing, worse, a decision the library had already made.
    //
    // The mock dir still has to be the mock dir. `load_registry` reads
    // `<mock_dir>/registry`, so pointing this at a repo root finds the config
    // and none of the rows.
    let cfg = mockspace::config::Config::from_dir(&mock_dir);
    let namespaces = &cfg.registry_namespaces;

    assert!(
        !namespaces.is_empty(),
        "the foreign project declares no namespaces, so this arm proves nothing. \
         Point the variable at a project that declares some."
    );
    eprintln!("namespaces declared: {}", namespaces.len());

    let reg = mockspace::registry::load_registry(&mock_dir, namespaces);
    eprintln!("rows loaded:         {}", reg.rows.len());
    for (ns, slugs) in &reg.by_namespace {
        eprintln!("  {ns:<24} {} rows", slugs.len());
    }

    // Coverage, tied to what is on disk rather than to a row count.
    //
    // A flat floor ("more than a hundred rows") only catches a silent drop on a
    // corpus already known to be large, and reports a small-but-correct registry
    // as a defect. It did: pointed at a project part-way through adopting the
    // registry it failed on seven rows that were all genuinely there.
    let empty = namespaces_with_files_but_no_rows(namespaces, &reg);
    assert!(
        empty.is_empty(),
        "{empty:?} have files the loader read and loaded no rows from: it \
         dropped them. Some drop paths print a line on stderr and some do not; \
         neither fails anything, which is what this turns into a failure. \
         Under {}/registry.",
        mock_dir.display()
    );

    // A namespace can only fail the check above if the loader read a file for
    // at least one. Without this, a project whose registry is entirely absent
    // would pass having exercised nothing, which is the silent vacuity this
    // file exists to avoid on its own coverage.
    assert!(
        !reg.files_read.is_empty(),
        "the loader read no .toml under {}/registry, so nothing above could \
         fail. Point the variable at a project whose registry is populated.",
        mock_dir.display()
    );
    eprintln!("files read:          {}", reg.files_read.len());

    // Three checks, not one, and the third is the one this test is named for.
    //
    // `validate` covers duplicate identifiers: a slug declared twice cannot be
    // referenced, and no per-file schema can see it, since each file is valid
    // on its own. An earlier version also asserted `reg.duplicates.is_empty()`
    // immediately before `validate(...).is_empty()`, where the second cannot
    // fail once the first has passed, since `validate` builds its only finding
    // kind from that same field. That pair is gone.
    let findings = mockspace::registry::validate(namespaces, &reg);
    let provenance_findings = mockspace::registry::validate_provenance(
        &cfg.repo_root,
        &cfg.registry_roots,
        &cfg.frozen_roots,
        &reg,
        namespaces,
    );
    for f in findings.iter().chain(&provenance_findings) {
        eprintln!("finding: {f:?}");
    }
    assert!(
        findings.is_empty(),
        "{} finding(s) from `validate` against a registry this code did not \
         write. Each is either a real defect in the loader or a shape the \
         design permits and the loader rejects. Copy a minimal reproduction \
         into this repository's fixtures before fixing either side.",
        findings.len()
    );

    // How many reference values `validate_provenance` actually looked at.
    //
    // **Without this the assertion below cannot fail on a corpus that declares
    // no reference-typed fields, and it reads as though every reference
    // resolved.** `validate_provenance` iterates the fields whose declared
    // type is `ref` or `ref[]`; a project declaring none gives it nothing to
    // inspect, it returns no findings, and the assertion passes having proved
    // nothing about references at all. The one large foreign corpus available
    // is exactly that project: fifteen namespaces, 2686 rows, thirty-six
    // `string[]` fields and zero of type `ref`.
    //
    // So the count is reported and asserted separately from the resolution. A
    // corpus with no references makes the resolution claim *silent* rather
    // than satisfied, and silence is not a pass.
    let ref_fields = mockspace::registry::reference_fields(namespaces);
    let inspected: usize = reg
        .rows
        .values()
        .map(|row| {
            ref_fields
                .get(&row.namespace)
                .map(|fields| {
                    fields
                        .iter()
                        .filter(|f| row.fields.contains_key(f.as_str()))
                        .count()
                })
                .unwrap_or(0)
        })
        .sum();
    eprintln!("reference values:    {inspected}");

    assert!(
        inspected == 0 || provenance_findings.is_empty(),
        "{} finding(s) from `validate_provenance`, i.e. a reference this \
         registry declares that does not resolve. Each is either a real defect \
         or a shape the design permits and the resolver rejects. Copy a \
         minimal reproduction into this repository's fixtures before fixing \
         either side.",
        provenance_findings.len()
    );
    // **Not asserted, and the distinction is the point.** A corpus with no
    // reference-typed fields makes the resolution claim *silent* rather than
    // satisfied, and an unmeasured dimension claims nothing. So the count is
    // reported, the assertion above stands only for what it inspected, and the
    // property itself is covered unconditionally by
    // `a_citation_that_resolves_to_nothing_is_a_finding` below, on a fixture
    // that does declare one.
    //
    // The arm was previously named for references resolving. It could not keep
    // that promise on an arbitrary corpus, and did not keep it on the only one
    // available.
    if inspected == 0 {
        eprintln!(
            "NOTE: this corpus declares no `ref` or `ref[]` fields, so \
             validate_provenance inspected nothing and this run says nothing \
             about reference resolution."
        );
    }

    // The fourth validator, which nothing here asserts on and which is reported
    // because its findings are real. `config_unknown_keys` reads keys the
    // config declares and mockspace does not implement, discarded in silence.
    // Its own documentation names this corpus: twelve of fifteen namespaces
    // declare `prefix`, mockspace has no such field, and all twelve are
    // dropped without a word.
    //
    // Asserting zero here would fail on the one corpus available, over a
    // finding that is about that project's config rather than about this
    // loader. Saying nothing at all was the earlier state, and it let the PR
    // body claim "validation returns no findings" while a validator nobody
    // called returned twelve.
    // `cfg.config_path`, not a path rebuilt here. `Config::from_dir` reads the
    // mock dir or the repo root, and this file already condemns re-deriving
    // that choice thirty lines up. Rebuilding it made the report silently
    // print nothing on a repo whose config sits at the root, which is the same
    // silence this whole change is about.
    if let Ok(text) = std::fs::read_to_string(&cfg.config_path) {
        let unknown = mockspace::registry::config_unknown_keys(&text);
        eprintln!("config unknown keys: {}", unknown.len());
        for f in &unknown {
            eprintln!("  {}", f.message);
        }
    }
}

/// The drop-detection arm, on a fixture this test builds, so it runs always.
///
/// **Named for the detection, not for a finding.** No production validator
/// reports this condition: `validate`, `validate_provenance`,
/// `config_unknown_keys` and `namespace_root_collisions` all return nothing on
/// this fixture. It is reported by `namespaces_with_files_but_no_rows` in this
/// file and nowhere else, and an earlier name said "is a finding", which told a
/// reader the drop was caught in the library when it is caught only here.
///
/// The test above is opt-in and therefore usually skipped, which would leave
/// the loader's most important property unexercised on every ordinary run.
/// This builds the case that must fail: a namespace declared, with a `.toml`
/// on disk under it, that yields no rows, and runs it through the exact
/// detection function the opt-in arm uses (`namespaces_with_files_but_no_rows`
/// above) rather than re-deriving the same judgement independently. A defect
/// in that shared function now fails here too, on every ordinary run, instead
/// of only in the arm nobody runs by default.
#[test]
fn the_drop_detection_reports_a_namespace_with_files_and_no_rows() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    std::fs::create_dir_all(mock.join("registry/ghostns")).unwrap();

    // A drop: the file declares the namespace's array of tables, so the loader
    // reads it and attributes it to `ghostns`, and the table carries no `id`,
    // so no row comes out.
    //
    // **The loader does say something about this one**, on stderr, at
    // `load.rs:84`. An earlier version of this comment called it silent, which
    // was wrong and picked the one drop path that is reported to illustrate the
    // ones that are not. The check still earns its place: it turns a line on
    // stderr, which nothing reads and nothing fails on, into a failing test.
    // The genuinely silent path is the plain-table slip, covered below.
    //
    // An earlier fixture wrote loose keys (`x = 1`) instead. That is a file
    // which declares nothing, and under a detection that follows the table key
    // it is correctly not a drop, so it stopped being the case this test is
    // named for. The change is the point: the old path-shaped detection called
    // any `.toml` in the namespace's directory a drop, including files that
    // were never rows.
    std::fs::write(
        mock.join("registry/ghostns/a.toml"),
        "[[ghostns]]\nname = \"no id\"\n",
    )
    .unwrap();
    std::fs::write(
        mock.join("mockspace.toml"),
        "[[registry.namespace]]\nkey = \"ghostns\"\ntitle = \"Ghost\"\n",
    )
    .unwrap();

    // Presence, not a count. `from_dir` adds builtin namespaces alongside the
    // declared ones, so this fixture yields three. Asserting one is what a
    // reader assumes and it is wrong, which is also why a project declaring a
    // single namespace reports three.
    let cfg = mockspace::config::Config::from_dir(mock);
    assert!(
        cfg.registry_namespaces.iter().any(|n| n.key == "ghostns"),
        "the fixture's own declaration did not parse, so this proves nothing \
         about the loader. Parsed: {:?}",
        cfg.registry_namespaces
            .iter()
            .map(|n| &n.key)
            .collect::<Vec<_>>()
    );

    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);
    let found = namespaces_with_files_but_no_rows(&cfg.registry_namespaces, &reg);
    assert_eq!(
        found,
        vec!["ghostns".to_string()],
        "a namespace with a .toml file and no rows must be the one thing this \
         detection reports; got {found:?}"
    );

    // And the control on the control: the same shape WITH a row must no longer
    // be reported, or the report above was telling us about the fixture
    // rather than about the detection.
    // `id`, not `slug`. The TOML key that carries a row's identity is `id`,
    // while the struct field holding it is `slug`, and the two names are not
    // the same word anywhere in between. Writing `slug` here produced a row
    // the loader skipped with a line on stderr, which is exactly the shape
    // this control exists to distinguish from a loader defect.
    std::fs::write(
        mock.join("registry/ghostns/b.toml"),
        "[[ghostns]]\nid = \"real\"\n",
    )
    .unwrap();
    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);
    assert_eq!(
        reg.by_namespace.get("ghostns").map(Vec::len),
        Some(1),
        "a well-formed row did not load, so the emptiness above was the fixture"
    );
    let found = namespaces_with_files_but_no_rows(&cfg.registry_namespaces, &reg);
    assert!(
        found.is_empty(),
        "a namespace with a real row must not be reported as files-with-no-rows: \
         {found:?}"
    );
}

/// A citation that resolves to nothing is a finding, on a fixture this test
/// builds, so reference resolution is covered on every ordinary run.
///
/// **This is the arm the opt-in test cannot be.** `validate_provenance` only
/// inspects fields whose declared type is `ref` or `ref[]`, so a corpus that
/// declares none gives it nothing to look at and it returns no findings. The
/// one large foreign registry available declares thirty-six `string[]` fields
/// and zero reference-typed ones, which made the opt-in assertion pass while
/// examining nothing, under a test named for references resolving.
///
/// The fixture declares a root, puts one file under it, and cites it twice:
/// once at a path that exists and once at a path that does not. One finding is
/// therefore the expected count, and it is what distinguishes a resolver that
/// reports everything from one that reports nothing.
#[test]
fn a_citation_that_resolves_to_nothing_is_a_finding() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    std::fs::create_dir_all(mock.join("registry")).unwrap();
    std::fs::create_dir_all(mock.join("notes")).unwrap();

    std::fs::write(
        mock.join("notes/present.md"),
        "# A heading\n\nSomething to cite.\n",
    )
    .unwrap();

    std::fs::write(
        mock.join("mockspace.toml"),
        r#"
[ref.roots.notes]
path = "notes"

[[registry.namespace]]
key = "ruling"
title = "Ruling"

[[registry.namespace.field]]
name = "provenance"
type = "ref[]"
"#,
    )
    .unwrap();
    std::fs::write(
        mock.join("registry/ruling.toml"),
        "[[ruling]]\nid = \"k1\"\nprovenance = [\"notes::present::#a-heading\"]\n\n\
         [[ruling]]\nid = \"k2\"\nprovenance = [\"notes::absent::#a-heading\"]\n",
    )
    .unwrap();

    let cfg = mockspace::config::Config::from_dir(mock);

    // The control that the fixture declares what it means to. Without it a typo
    // in the type name leaves `reference_fields` empty, the resolver inspects
    // nothing, and the count below is zero for a reason that has nothing to do
    // with resolution. It has already earned its place: an earlier revision
    // wrote `[[registry.namespace.fields]]`, plural, and this is what said so.
    let ref_fields = mockspace::registry::reference_fields(&cfg.registry_namespaces);
    assert_eq!(
        ref_fields.get("ruling").map(Vec::as_slice),
        Some(["provenance".to_string()].as_slice()),
        "the fixture's reference-typed field did not parse, so nothing below \
         inspects a citation. Parsed: {ref_fields:?}"
    );

    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);
    assert_eq!(reg.rows.len(), 2, "the fixture's own rows did not load");

    let findings = mockspace::registry::validate_provenance(
        &cfg.repo_root,
        &cfg.registry_roots,
        &cfg.frozen_roots,
        &reg,
        &cfg.registry_namespaces,
    );
    for f in &findings {
        eprintln!("finding: {f:?}");
    }
    assert_eq!(
        findings.len(),
        1,
        "one of the two citations names a file that is not there, so exactly \
         one finding is expected. Zero means the resolver inspected nothing; \
         two means it rejected the citation that does resolve. Got: {findings:?}"
    );
    assert!(
        findings[0].message.contains("k2"),
        "the finding must be against the citation that points nowhere, not the \
         one that resolves: {:?}",
        findings[0]
    );
}

/// The drop detection sees a silent drop in a file nested deeper than one
/// directory.
///
/// `collect_toml_files` recurses to unbounded depth on purpose. An earlier
/// version of the detection modelled a namespace's files as
/// `registry/<key>.toml` or a *direct* child of `registry/<key>/`, so a file at
/// `registry/law/2026/deep.toml` matched neither and the namespace read as
/// having no files at all. A genuine drop there was invisible to the very check
/// written to catch it.
///
/// **The assertion is that the drop is reported, not that nothing is.** An
/// earlier revision of this test asserted `found.is_empty()` on a fixture whose
/// rows all loaded, which passes when the detection works and equally when it
/// is blind, and it did: restoring the path-shaped model left it green. That is
/// the tautological-assertion class this file exists to keep out.
#[test]
fn the_drop_detection_sees_a_drop_in_a_file_nested_two_levels_deep() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    std::fs::create_dir_all(mock.join("registry/law/2026")).unwrap();
    std::fs::write(
        mock.join("mockspace.toml"),
        "[[registry.namespace]]\nkey = \"law\"\ntitle = \"Law\"\n",
    )
    .unwrap();

    // Declares `law` and yields no row, because the table carries no `id`.
    std::fs::write(
        mock.join("registry/law/2026/deep.toml"),
        "[[law]]\nname = \"no id\"\n",
    )
    .unwrap();

    let cfg = mockspace::config::Config::from_dir(mock);
    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);

    // Premise, not subject: the loader reached the nested file.
    assert!(
        reg.files_read.iter().any(|p| p.ends_with("deep.toml")),
        "the loader's own record does not name the nested file: {:?}",
        reg.files_read
    );
    assert!(
        reg.by_namespace.get("law").is_none_or(Vec::is_empty),
        "the fixture's row loaded, so there is no drop here to detect"
    );

    let found = namespaces_with_files_but_no_rows(&cfg.registry_namespaces, &reg);
    assert_eq!(
        found,
        vec!["law".to_string()],
        "a namespace whose only file is two directories down, declaring rows \
         that did not load, must be reported. Empty means the detection cannot \
         see past the first level; got {found:?}"
    );
}

/// The drop detection follows the array-of-tables key, not the file name.
///
/// A row's namespace comes from the TOML key that declares it, so a single
/// `registry/everything.toml` holding `[[law]]` is an ordinary way to keep a
/// registry. The earlier path-shaped detection saw no file named for `law` and
/// no directory called `law`, concluded the namespace had no files, and could
/// therefore miss a drop in it entirely.
///
/// Asserts the drop is reported, for the reason given on the test above.
#[test]
fn the_drop_detection_follows_the_table_key_not_the_file_name() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    std::fs::create_dir_all(mock.join("registry")).unwrap();
    std::fs::write(
        mock.join("mockspace.toml"),
        "[[registry.namespace]]\nkey = \"law\"\ntitle = \"Law\"\n",
    )
    .unwrap();
    std::fs::write(
        mock.join("registry/everything.toml"),
        "[[law]]\nname = \"no id\"\n",
    )
    .unwrap();

    let cfg = mockspace::config::Config::from_dir(mock);
    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);

    assert!(
        reg.by_namespace.get("law").is_none_or(Vec::is_empty),
        "the fixture's row loaded, so there is no drop here to detect"
    );

    let found = namespaces_with_files_but_no_rows(&cfg.registry_namespaces, &reg);
    assert_eq!(
        found,
        vec!["law".to_string()],
        "a namespace whose rows are declared in a differently-named file, and \
         did not load, must be reported. Empty means the detection is keyed on \
         the file name rather than the table key; got {found:?}"
    );
}

/// The drop detection sees the single-vs-double-bracket slip, which is the one
/// drop the loader really is silent about.
///
/// `load_registry` accepts only an array of tables. A file writing `[law]`
/// where it meant `[[law]]` reaches `as_array_of_tables()`, gets `None`,
/// `continue`s, and produces no rows **with nothing printed**. Every other drop
/// path is either loud (a missing `id`, a TOML parse failure) or deliberate (a
/// key no namespace declares), so this is the one that costs a reader real time.
///
/// **It is also the case a mirror cannot catch.** An earlier version of
/// `file_declares_namespace` asked exactly what the loader asks, and a check
/// that mirrors the thing it checks agrees with it everywhere, including where
/// it is wrong. The predicate is deliberately wider for this reason.
#[test]
fn the_drop_detection_sees_a_plain_table_where_an_array_was_meant() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    std::fs::create_dir_all(mock.join("registry")).unwrap();
    std::fs::write(
        mock.join("mockspace.toml"),
        "[[registry.namespace]]\nkey = \"law\"\ntitle = \"Law\"\n",
    )
    .unwrap();

    // One bracket, not two. Valid TOML, names the namespace, yields nothing.
    std::fs::write(mock.join("registry/law.toml"), "[law]\nid = \"a\"\n").unwrap();

    let cfg = mockspace::config::Config::from_dir(mock);
    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);

    // Premise, not subject: the loader read the file and produced nothing.
    assert!(
        reg.files_read.iter().any(|p| p.ends_with("law.toml")),
        "the loader did not read the fixture at all: {:?}",
        reg.files_read
    );
    assert!(
        reg.by_namespace.get("law").is_none_or(Vec::is_empty),
        "the fixture's plain table loaded as rows, so there is no drop here to \
         detect and this test is about something that no longer happens"
    );

    let found = namespaces_with_files_but_no_rows(&cfg.registry_namespaces, &reg);
    assert_eq!(
        found,
        vec!["law".to_string()],
        "a file naming a namespace in a table shape the loader does not accept, \
         yielding no rows and printing nothing, must be reported. Empty means \
         the detection asks the same question the loader asks and so cannot \
         disagree with it; got {found:?}"
    );
}
