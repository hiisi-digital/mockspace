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

/// Every declared namespace with a `.toml` under `registry/` and no rows
/// loaded for it, by key.
///
/// Shared between the arm that reads a real corpus and the arm that builds
/// one, so a defect in this detection breaks both rather than only the arm
/// that happens to be skipped by default. Presence is checked on disk rather
/// than assumed from a row count, because a flat total treats a small,
/// correctly-empty registry the same as a namespace the loader silently
/// dropped, and the two are not the same finding.
fn namespaces_with_files_but_no_rows(
    mock_dir: &Path,
    namespaces: &[RegistryNamespace],
    reg: &Registry,
) -> Vec<String> {
    let registry_root = mock_dir.join("registry");
    let mut out = Vec::new();
    for ns in namespaces {
        let flat = registry_root.join(format!("{}.toml", ns.key));
        let nested = registry_root.join(&ns.key);
        let has_files = flat.is_file()
            || std::fs::read_dir(&nested)
                .map(|d| {
                    d.filter_map(Result::ok)
                        .any(|e| e.path().extension().is_some_and(|x| x == "toml"))
                })
                .unwrap_or(false);
        if has_files && reg.by_namespace.get(&ns.key).is_none_or(Vec::is_empty) {
            out.push(ns.key.clone());
        }
    }
    out
}

#[test]
#[ignore = "set MOCKSPACE_FOREIGN_REGISTRY to a mock/ directory to run this"]
fn a_foreign_registry_parses_and_its_references_resolve() {
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
    let empty = namespaces_with_files_but_no_rows(&mock_dir, namespaces, &reg);
    assert!(
        empty.is_empty(),
        "{empty:?} have files under {}/registry and loaded no rows: the loader \
         dropped them without saying so.",
        mock_dir.display()
    );

    // A namespace can only fail the check above if at least one has files at
    // all. Without this, a project whose registry is entirely absent (every
    // namespace vacuous) would pass the assertion above having exercised
    // nothing, which is the exact silent-vacuity failure this file exists to
    // avoid on its own coverage.
    let registry_root = mock_dir.join("registry");
    let any_files = namespaces.iter().any(|ns| {
        registry_root.join(format!("{}.toml", ns.key)).is_file()
            || std::fs::read_dir(registry_root.join(&ns.key))
                .map(|d| {
                    d.filter_map(Result::ok)
                        .any(|e| e.path().extension().is_some_and(|x| x == "toml"))
                })
                .unwrap_or(false)
    });
    assert!(
        any_files,
        "no namespace has any .toml under {}, so nothing here could fail. \
         Point the variable at a project whose registry is populated.",
        registry_root.display()
    );

    // Two checks, not one. `validate` covers duplicate identifiers (a slug
    // declared twice cannot be referenced, and no per-file schema can see it,
    // since each file is valid on its own); reference resolution is
    // `validate_provenance`, which this test's own name promises and an
    // earlier version never called. An earlier version of this test also
    // asserted `reg.duplicates.is_empty()` directly, immediately before
    // asserting `validate(...).is_empty()`: the second cannot fail once the
    // first has passed, since `validate` builds its only finding kind from
    // that same field. The one assertion below is the whole check.
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
    assert!(
        provenance_findings.is_empty(),
        "{} finding(s) from `validate_provenance`, i.e. a reference this \
         registry declares that does not resolve. Each is either a real defect \
         or a shape the design permits and the resolver rejects. Copy a \
         minimal reproduction into this repository's fixtures before fixing \
         either side.",
        provenance_findings.len()
    );
}

/// The drop-detection arm, on a fixture this test builds, so it runs always.
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
fn a_declared_namespace_with_files_and_no_rows_is_a_finding() {
    let dir = tempfile::tempdir().expect("a temporary tree");
    let mock = dir.path();
    std::fs::create_dir_all(mock.join("registry/ghostns")).unwrap();

    // Valid TOML, and not a row: the array-of-tables key is what makes a row,
    // so a file of loose keys parses and declares nothing.
    std::fs::write(mock.join("registry/ghostns/a.toml"), "x = 1\n").unwrap();
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
        cfg.registry_namespaces.iter().map(|n| &n.key).collect::<Vec<_>>()
    );

    let reg = mockspace::registry::load_registry(mock, &cfg.registry_namespaces);
    let found = namespaces_with_files_but_no_rows(mock, &cfg.registry_namespaces, &reg);
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
    let found = namespaces_with_files_but_no_rows(mock, &cfg.registry_namespaces, &reg);
    assert!(
        found.is_empty(),
        "a namespace with a real row must not be reported as files-with-no-rows: \
         {found:?}"
    );
}
