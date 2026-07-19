#![allow(unused_imports)]
    use super::*;

    fn mkdir(root: &Path, rel: &str) {
        fs::create_dir_all(root.join(rel)).unwrap();
    }

    /// Create a standalone crate at `rel` with a `Cargo.toml` and a `target/`.
    fn mkcrate_with_target(root: &Path, rel: &str) {
        let crate_dir = root.join(rel);
        fs::create_dir_all(crate_dir.join("target")).unwrap();
        fs::write(crate_dir.join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
    }

    #[test]
    fn collects_nested_bench_and_sketch_targets_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // cleanable: real cargo crates (Cargo.toml beside target) nested under
        // benches / tests / sketches
        mkcrate_with_target(root, "mock/benches/variants/foo");
        mkcrate_with_target(root, "mock/benches/engine_vs_std");
        mkcrate_with_target(root, "mock/research/sketches/202601010000_x");
        mkcrate_with_target(root, "crates/bar/tests/fixture_crate");
        // NOT cleanable: the active build dirs (have a manifest beside them but
        // no benches/tests/sketches segment).
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        mkdir(root, "target");
        fs::write(root.join("mock/Cargo.toml"), "[workspace]\n").unwrap();
        mkdir(root, "mock/target");
        // NOT a target dir at all
        mkdir(root, "crates/bar/src");

        let mut found = nested_artifact_targets(root);
        found.sort();
        let rels: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();

        assert!(rels.contains(&"mock/benches/variants/foo/target".to_string()), "got: {rels:?}");
        assert!(rels.contains(&"mock/benches/engine_vs_std/target".to_string()), "got: {rels:?}");
        assert!(rels.contains(&"mock/research/sketches/202601010000_x/target".to_string()), "got: {rels:?}");
        assert!(rels.contains(&"crates/bar/tests/fixture_crate/target".to_string()), "got: {rels:?}");
        assert!(!rels.contains(&"target".to_string()), "must not collect repo-root target: {rels:?}");
        assert!(!rels.contains(&"mock/target".to_string()), "must not collect mockspace install: {rels:?}");
        assert_eq!(found.len(), 4, "got: {rels:?}");
    }

    #[test]
    fn spares_research_targets_outside_sketches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // A research memo crate NOT under sketches/ is audit-trail, not a
        // disposable sketch. It must be spared even with a manifest present.
        mkcrate_with_target(root, "mock/research/202601010000_memo");
        // A sketch under research/sketches/ is disposable.
        mkcrate_with_target(root, "mock/research/sketches/202601010000_x");
        let found = nested_artifact_targets(root);
        let rels: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(rels, vec!["mock/research/sketches/202601010000_x/target".to_string()], "got: {rels:?}");
    }

    #[test]
    fn requires_sibling_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // A `target` dir under benches/ with NO Cargo.toml beside it is not a
        // cargo build dir; do not delete it.
        mkdir(root, "mock/benches/not_a_crate/target");
        assert!(nested_artifact_targets(root).is_empty());
    }

    #[test]
    fn does_not_descend_into_target_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // a real crate whose target nests an inner target (cargo nesting)
        mkcrate_with_target(root, "mock/benches/foo");
        mkdir(root, "mock/benches/foo/target/debug/target");
        let found = nested_artifact_targets(root);
        // exactly one: the outer target; the walk must not recurse into it.
        assert_eq!(found.len(), 1, "should stop at first target dir, got: {found:?}");
        assert!(found[0].ends_with("mock/benches/foo/target"));
    }

    #[test]
    fn empty_when_no_nested_targets() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        mkdir(root, "target");
        mkdir(root, "mock/target");
        mkdir(root, "src");
        assert!(nested_artifact_targets(root).is_empty());
    }

    #[test]
    fn levenshtein_basic_distances() {
        assert_eq!(super::levenshtein("lock", "lock"), 0);
        assert_eq!(super::levenshtein("locks", "lock"), 1);
        assert_eq!(super::levenshtein("clsoe", "close"), 2);
        assert_eq!(super::levenshtein("", "close"), 5);
    }

    #[test]
    fn suggest_subcommand_catches_typos() {
        assert_eq!(super::suggest_subcommand("locks"), Some("lock"));
        assert_eq!(super::suggest_subcommand("closs"), Some("close"));
        assert_eq!(super::suggest_subcommand("actlvate"), Some("activate"));
        assert_eq!(super::suggest_subcommand("depricate"), Some("deprecate"));
    }

    #[test]
    fn suggest_subcommand_declines_far_words() {
        // a wholly unrelated word should not be forced onto a subcommand
        assert_eq!(super::suggest_subcommand("frobnicate"), None);
        assert_eq!(super::suggest_subcommand("xyzzy"), None);
    }

    #[test]
    fn every_dispatched_subcommand_is_in_the_known_list() {
        // guards against the list and the match drifting apart
        for name in ["activate", "deactivate", "status", "query", "check",
                     "clean", "pdf", "lock", "deprecate", "unlock", "close",
                     "archive", "migrate", "bench"] {
            assert!(
                super::KNOWN_SUBCOMMANDS.contains(&name),
                "{name} dispatched but missing from KNOWN_SUBCOMMANDS"
            );
        }
    }
