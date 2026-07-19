#![allow(unused_imports)]
    use super::*;

    const LOCK_GIT: &str = r#"
version = 4

[[package]]
name = "arvo"
version = "0.1.0"
source = "git+ssh://git@github.com/orgrinrt/arvo.git?branch=dev#f5cf3063aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[package]]
name = "mockspace"
version = "0.1.0"
source = "git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#d50b59cd461f12958ebfcc3a6a19a7c62d1a472b"
"#;

    #[test]
    fn extracts_git_rev_for_mockspace() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(&lock, LOCK_GIT).unwrap();
        assert_eq!(
            mockspace_rev_from_lock(&lock).as_deref(),
            Some("d50b59cd461f12958ebfcc3a6a19a7c62d1a472b")
        );
    }

    #[test]
    fn path_source_yields_no_rev() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        // A path/workspace dependency has no `source` line at all.
        fs::write(
            &lock,
            "version = 4\n\n[[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert_eq!(mockspace_rev_from_lock(&lock), None);
    }

    #[test]
    fn missing_lock_yields_no_rev() {
        assert_eq!(mockspace_rev_from_lock(Path::new("/no/such/Cargo.lock")), None);
    }

    #[test]
    fn find_git_checkout_matches_by_rev_prefix() {
        // Fake a git/checkouts/mockspace-<hash>/<short-rev>/ tree.
        let checkouts = tempfile::tempdir().unwrap();
        let checkout = checkouts.path().join("mockspace-abc123def4560789/d50b59c");
        fs::create_dir_all(&checkout).unwrap();
        // A sibling source that must not match.
        fs::create_dir_all(checkouts.path().join("arvo-999/f5cf306")).unwrap();

        let found = find_git_checkout_in(
            checkouts.path(),
            "mockspace",
            "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b",
        );
        assert_eq!(found.as_deref(), Some(checkout.as_path()));
    }

    #[test]
    fn find_git_checkout_none_when_absent() {
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(
            find_git_checkout_in(empty.path(), "mockspace", "d50b59cd461f"),
            None
        );
    }

    #[test]
    fn find_git_checkout_does_not_match_a_differently_named_repo() {
        // `mockspace-hilavitkutin-stack-lints-<hash>` shares the `mockspace-`
        // prefix but is a different repo; it must not match.
        let checkouts = tempfile::tempdir().unwrap();
        fs::create_dir_all(
            checkouts
                .path()
                .join("mockspace-hilavitkutin-stack-lints-e5dc0929ff6a2451/d50b59c"),
        )
        .unwrap();
        assert_eq!(
            find_git_checkout_in(
                checkouts.path(),
                "mockspace",
                "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b"
            ),
            None,
            "a sibling repo sharing the name prefix must not match"
        );
    }

    #[test]
    fn resolve_falls_back_on_path_source() {
        // A path/workspace mockspace has no git rev, so the resolver returns
        // the fallback (the baked path) unchanged, with no environment access.
        let mock = tempfile::tempdir().unwrap();
        fs::write(
            mock.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        assert_eq!(
            resolve_mockspace_pin(mock.path(), fallback.path(), &mut actions),
            fallback.path()
        );
        // No git rev, so no "checkout absent" action: this is a clean fallback.
        assert!(actions.is_empty());
    }

    #[test]
    fn resolve_reports_absent_checkout_for_a_git_rev() {
        // The lock names a git rev whose checkout does not exist, so the
        // resolver falls back AND records the degraded case rather than hiding
        // it. The rev is all-f so it cannot collide with a real checkout under
        // the machine's CARGO_HOME (find_git_checkout reads the real cache).
        let mock = tempfile::tempdir().unwrap();
        fs::write(
            mock.path().join("Cargo.lock"),
            "version = 4\n\n[[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\nsource = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#ffffffffffffffffffffffffffffffffffffffff\"\n",
        )
        .unwrap();
        let fallback = tempfile::tempdir().unwrap();
        let mut actions = Vec::new();
        let resolved = resolve_mockspace_pin(mock.path(), fallback.path(), &mut actions);
        assert_eq!(resolved, fallback.path());
        assert!(
            actions.iter().any(|a| a.contains("checkout absent")),
            "the absent-checkout case must be reported, got {actions:?}"
        );
    }

    #[test]
    fn pinned_path_is_extracted_from_proxy_cargo() {
        let cargo = "[package]\nname = \"mockspace-proxy\"\n\n[dependencies]\nmockspace = { path = \"/some/where/mockspace\" }\n";
        assert_eq!(
            pinned_mockspace_path(cargo).as_deref(),
            Some("/some/where/mockspace")
        );
        assert_eq!(pinned_mockspace_path("no pin here"), None);
    }
