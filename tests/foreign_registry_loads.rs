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
//! Set it to the project's `mock/` directory:
//!
//! ```text
//! MOCKSPACE_FOREIGN_REGISTRY=~/Dev/clause-dev/ikiuni_renderer/mock \
//!     cargo test --test foreign_registry_loads -- --nocapture
//! ```
//!
//! Anything this finds that is genuinely a defect gets a minimal fixture
//! committed here, so the finding survives without the sibling clone.

use std::path::PathBuf;

fn foreign_mock_dir() -> Option<PathBuf> {
    let raw = std::env::var("MOCKSPACE_FOREIGN_REGISTRY").ok()?;
    let expanded = if let Some(rest) = raw.strip_prefix("~/") {
        PathBuf::from(std::env::var("HOME").ok()?).join(rest)
    } else {
        PathBuf::from(raw)
    };
    expanded.is_dir().then_some(expanded)
}

#[test]
fn a_foreign_registry_parses_and_its_references_resolve() {
    let Some(mock_dir) = foreign_mock_dir() else {
        eprintln!(
            "SKIPPED: set MOCKSPACE_FOREIGN_REGISTRY to another project's mock/ \
             directory to run this. Skipping is reported rather than passing \
             silently, because a green test that did nothing is the failure this \
             file is about."
        );
        return;
    };

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
    //
    // The property that holds at any size is that a namespace with files under
    // `registry/` loads rows from them. A loader that dropped a whole namespace
    // silently is exactly what this arm is for, and that is invisible to a total.
    let registry_root = mock_dir.join("registry");
    let mut with_files = Vec::new();
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
        if has_files {
            with_files.push(ns.key.clone());
        }
    }

    assert!(
        !with_files.is_empty(),
        "no namespace has any .toml under {}, so nothing here could fail. \
         Point the variable at a project whose registry is populated.",
        registry_root.display()
    );

    let empty: Vec<&String> = with_files
        .iter()
        .filter(|k| reg.by_namespace.get(*k).is_none_or(|v| v.is_empty()))
        .collect();
    assert!(
        empty.is_empty(),
        "{empty:?} have files under {} and loaded no rows: the loader dropped \
         them without saying so.",
        registry_root.display()
    );

    // A slug declared twice cannot be referenced, and no per-file schema can
    // see it, so it is checked here rather than left to the schema pass.
    assert!(
        reg.duplicates.is_empty(),
        "duplicate slugs: {:?}",
        reg.duplicates
    );

    let findings = mockspace::registry::validate(namespaces, &reg);
    for f in &findings {
        eprintln!("finding: {f:?}");
    }
    assert!(
        findings.is_empty(),
        "{} finding(s) against a registry this code did not write. Each is either \
         a real defect in the loader or a shape the design permits and the loader \
         rejects. Copy a minimal reproduction into this repository's fixtures \
         before fixing either side.",
        findings.len()
    );
}

/// The drop-detection arm, on a fixture this test builds, so it runs always.
///
/// The test above is opt-in and therefore usually skipped, which would leave
/// its most important assertion unexercised on every ordinary run. This builds
/// the case that must fail: a namespace declared, with a `.toml` on disk under
/// it, that yields no rows. Run by hand while writing the arm; kept here so it
/// stays run.
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
    assert!(
        reg.by_namespace
            .get("ghostns")
            .is_none_or(|v| v.is_empty()),
        "the fixture was meant to yield no rows and yielded some, so the arm it \
         exists to exercise cannot fire"
    );

    // And the control on the control: the same shape WITH a row must load, or
    // the emptiness above would be telling us about the fixture rather than
    // about the loader.
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
}
