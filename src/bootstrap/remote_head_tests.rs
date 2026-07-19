#![allow(unused_imports)]
    use super::*;

    #[test]
    fn parses_branch_source() {
        let s = parse_git_source(
            "git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#d50b59cd461f12958ebfcc3a6a19a7c62d1a472b",
        )
        .unwrap();
        assert_eq!(s.url, "ssh://git@github.com/hiisi-digital/mockspace.git");
        assert_eq!(s.branch.as_deref(), Some("dev"));
        assert_eq!(s.rev, "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b");
    }

    #[test]
    fn parses_source_without_branch() {
        // A bare git dep (default branch) has no `branch=` query, so there is
        // no moving target to track.
        let s = parse_git_source("git+https://example.com/mockspace.git#abcdef0").unwrap();
        assert_eq!(s.url, "https://example.com/mockspace.git");
        assert_eq!(s.branch, None);
        assert_eq!(s.rev, "abcdef0");
    }

    #[test]
    fn rejects_non_git_source() {
        assert_eq!(parse_git_source("registry+https://crates.io/#1.0.0"), None);
        assert_eq!(parse_git_source("git+https://example.com/no-rev.git"), None);
    }

    #[test]
    fn tag_and_rev_pins_have_no_branch() {
        // A tag or exact-rev pin is not a moving target, so branch is None and
        // ensure_mockspace_current early-returns on it.
        let tag = parse_git_source("git+https://e.com/m.git?tag=v1.2.3#abcdef0").unwrap();
        assert_eq!(tag.branch, None);
        assert_eq!(tag.rev, "abcdef0");
        let rev = parse_git_source("git+https://e.com/m.git?rev=abcdef0#abcdef0").unwrap();
        assert_eq!(rev.branch, None);
    }

    #[test]
    fn multi_param_query_extracts_branch() {
        // A query may carry several &-separated params in any order; the branch
        // is found among them.
        let s = parse_git_source("git+https://e.com/m.git?rev=x&branch=main#deadbee").unwrap();
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.rev, "deadbee");
    }

    #[test]
    fn every_branch_tracked_dep_is_found_not_only_mockspace() {
        // The freshness problem is not mockspace's. Any dependency tracking a
        // branch is a moving target whose lock nothing advances.
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(
            &lock,
            "version = 4\n\n             [[package]]\nname = \"mockspace\"\nversion = \"0.1.0\"\n             source = \"git+ssh://git@github.com/hiisi-digital/mockspace.git?branch=dev#d50b59cd461f12958ebfcc3a6a19a7c62d1a472b\"\n\n             [[package]]\nname = \"arvo\"\nversion = \"0.1.0\"\n             source = \"git+ssh://git@github.com/orgrinrt/arvo.git?branch=dev#aaaa59cd461f12958ebfcc3a6a19a7c62d1a472b\"\n\n             [[package]]\nname = \"pinned\"\nversion = \"0.1.0\"\n             source = \"git+ssh://git@example.com/pinned.git?tag=v1#bbbb59cd461f12958ebfcc3a6a19a7c62d1a472b\"\n\n             [[package]]\nname = \"local\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let deps = branch_tracked_git_deps(&lock);
        let names: Vec<&str> = deps.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["mockspace", "arvo"], "{names:?}");

        // A tag and a path are pins someone chose. Advancing either would be
        // overriding a decision rather than honouring one.
        assert!(!names.contains(&"pinned"));
        assert!(!names.contains(&"local"));

        assert_eq!(deps[0].1.branch.as_deref(), Some("dev"));
        assert_eq!(deps[0].1.rev, "d50b59cd461f12958ebfcc3a6a19a7c62d1a472b");
    }

    #[test]
    fn a_lock_with_no_git_deps_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(&lock, "version = 4\n\n[[package]]\nname = \"local\"\nversion = \"0.1.0\"\n").unwrap();
        assert!(branch_tracked_git_deps(&lock).is_empty());
        // A missing lock is a skip, not a panic.
        assert!(branch_tracked_git_deps(&dir.path().join("nope.lock")).is_empty());
    }

    #[test]
    fn auto_update_defaults_to_true() {
        let dir = tempfile::tempdir().unwrap();
        let toml = dir.path().join("mockspace.toml");
        // No [proxy] section: the default is auto.
        fs::write(&toml, "project_name = \"x\"\n").unwrap();
        assert!(proxy_auto_update(&toml));
        // Missing file: also the default.
        assert!(proxy_auto_update(&dir.path().join("nope.toml")));
    }

    #[test]
    fn auto_update_reads_explicit_false() {
        let dir = tempfile::tempdir().unwrap();
        let toml = dir.path().join("mockspace.toml");
        fs::write(&toml, "[proxy]\nauto_update = false\n").unwrap();
        assert!(!proxy_auto_update(&toml));
    }

    #[test]
    fn ls_remote_rejects_flag_smuggling_url() {
        // A leading-dash url or branch could be parsed by git as an option.
        // The guard refuses it before spawning, so no subprocess runs.
        assert_eq!(git_ls_remote_head("--upload-pack=touch pwned", "dev"), None);
        assert_eq!(git_ls_remote_head("ssh://ok.example/x.git", "-x"), None);
    }

    #[test]
    fn remote_check_due_respects_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("target/mockspace-proxy/.remote-check");
        // No marker yet: a check is due.
        assert!(remote_check_due(&marker, REMOTE_CHECK_TTL));
        touch(&marker);
        // Just touched: not due under a real TTL.
        assert!(!remote_check_due(&marker, REMOTE_CHECK_TTL));
        // Due under a zero TTL (any elapsed time exceeds it).
        assert!(remote_check_due(&marker, std::time::Duration::ZERO));
    }
