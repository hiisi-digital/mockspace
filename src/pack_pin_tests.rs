use super::*;
use std::cell::Cell;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DEV: &str = r#"{ git = "https://example.invalid/pack.git", branch = "dev" }"#;

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn never(_: &str, _: &str) -> Result<String, String> {
    panic!("the remote was asked while a current resolution was held")
}

// --- reading a spec -----------------------------------------------------------

#[test]
fn a_git_spec_with_a_branch_is_a_branch_pin() {
    assert_eq!(
        branch_pin(DEV),
        Some(BranchPin {
            git:    "https://example.invalid/pack.git".into(),
            branch: "dev".into(),
        })
    );
}

#[test]
fn a_spec_fixed_by_rev_tag_version_or_path_is_not_a_branch_pin() {
    for spec in [
        r#"{ git = "u", rev = "abc" }"#,
        r#"{ git = "u", tag = "v1" }"#,
        r#"{ git = "u", branch = "dev", rev = "abc" }"#,
        r#"{ git = "u", branch = "dev", tag = "v1" }"#,
        r#"{ git = "u" }"#,
        r#"{ branch = "dev" }"#,
        r#"{ path = "../pack" }"#,
        r#""0.1""#,
        r#"{ version = "0.1" }"#,
        r#"{ git = 3, branch = "dev" }"#,
        "not toml at all {",
    ] {
        assert_eq!(branch_pin(spec), None, "{spec}");
    }
}

#[test]
fn the_rev_replaces_the_branch_and_every_other_key_stays() {
    let spec = r#"{ git = "u", branch = "dev", package = "p", features = ["x"] }"#;
    let out = with_rev(spec, A).unwrap();
    let t = inline_table(&out).unwrap();
    assert!(!t.contains_key("branch"), "{out}");
    assert_eq!(t.get("rev").and_then(|v| v.as_str()), Some(A));
    assert_eq!(t.get("git").and_then(|v| v.as_str()), Some("u"));
    assert_eq!(t.get("package").and_then(|v| v.as_str()), Some("p"));
    assert!(t.contains_key("features"), "{out}");
}

#[test]
fn a_spec_without_a_branch_is_not_rewritten() {
    assert_eq!(with_rev(r#"{ git = "u", rev = "abc" }"#, A), None);
    assert_eq!(with_rev(r#""0.1""#, A), None);
}

// --- the cache and the fallbacks ------------------------------------------------

#[test]
fn an_untouched_spec_never_reaches_the_remote_or_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let spec = r#"{ git = "u", rev = "abc" }"#;
    assert_eq!(pin(spec, dir.path(), at(1000), &never), Pinned::Untouched);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    assert_eq!(spec_for_cargo("p", spec, dir.path(), &never), spec);
}

#[test]
fn a_first_resolution_asks_the_remote_and_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let asked = Cell::new(0);
    let resolve = |_: &str, _: &str| {
        asked.set(asked.get() + 1);
        Ok(A.to_string())
    };
    assert_eq!(pin(DEV, dir.path(), at(1000), &resolve), Pinned::Current { rev: A.into() });
    assert_eq!(asked.get(), 1);
    // Inside the window the kept resolution answers and the remote is not asked.
    let later = at(1000 + TIP_TTL.as_secs());
    assert_eq!(pin(DEV, dir.path(), later, &never), Pinned::Current { rev: A.into() });
}

#[test]
fn past_the_window_the_remote_is_asked_again_and_a_moved_tip_is_taken() {
    let dir = tempfile::tempdir().unwrap();
    pin(DEV, dir.path(), at(1000), &|_: &str, _: &str| Ok(A.to_string()));
    let later = at(1000 + TIP_TTL.as_secs() + 1);
    let got = pin(DEV, dir.path(), later, &|_: &str, _: &str| Ok(B.to_string()));
    assert_eq!(got, Pinned::Current { rev: B.into() });
    // And the new tip is what is kept.
    assert_eq!(pin(DEV, dir.path(), later, &never), Pinned::Current { rev: B.into() });
}

#[test]
fn an_unreachable_remote_falls_back_to_the_last_resolution_at_any_age() {
    let dir = tempfile::tempdir().unwrap();
    pin(DEV, dir.path(), at(1000), &|_: &str, _: &str| Ok(A.to_string()));
    let much_later = at(1000 + 30 * 24 * 3600);
    let got = pin(DEV, dir.path(), much_later, &|_: &str, _: &str| Err("offline".into()));
    assert_eq!(
        got,
        Pinned::Stale {
            rev: A.into(),
            age: Duration::from_secs(30 * 24 * 3600),
            why: "offline".into(),
        }
    );
    let spec = spec_for_cargo("p", DEV, dir.path(), &|_: &str, _: &str| Err("offline".into()));
    assert_eq!(branch_pin(&spec), None, "{spec}");
    assert!(spec.contains(A), "{spec}");
}

#[test]
fn an_unreachable_remote_with_nothing_kept_leaves_the_spec_to_cargo() {
    let dir = tempfile::tempdir().unwrap();
    let fail = |_: &str, _: &str| Err::<String, String>("offline".into());
    assert_eq!(
        pin(DEV, dir.path(), at(1000), &fail),
        Pinned::Unresolved { why: "offline".into() }
    );
    assert_eq!(spec_for_cargo("p", DEV, dir.path(), &fail), DEV);
}

#[test]
fn a_failed_resolution_does_not_overwrite_what_was_kept() {
    let dir = tempfile::tempdir().unwrap();
    pin(DEV, dir.path(), at(1000), &|_: &str, _: &str| Ok(A.to_string()));
    let later = at(1000 + TIP_TTL.as_secs() + 1);
    pin(DEV, dir.path(), later, &|_: &str, _: &str| Err("offline".into()));
    // Still stale at A, and the age still counts from the one real resolution.
    match pin(DEV, dir.path(), later, &|_: &str, _: &str| Err("offline".into())) {
        Pinned::Stale { rev, age, .. } => {
            assert_eq!(rev, A);
            assert_eq!(age, Duration::from_secs(TIP_TTL.as_secs() + 1));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_corrupt_cache_file_is_treated_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    let file = cache_file(dir.path(), &branch_pin(DEV).unwrap());
    let bad_time = format!("notanumber\n{A}");
    for junk in ["", bad_time.as_str(), "1000\nshort", "1000"] {
        std::fs::write(&file, junk.as_bytes()).unwrap();
        let fail = |_: &str, _: &str| Err::<String, String>("offline".into());
        assert!(
            matches!(pin(DEV, dir.path(), at(1000), &fail), Pinned::Unresolved { .. }),
            "{junk:?}"
        );
    }
}

#[test]
fn two_branches_of_one_repo_and_one_branch_of_two_repos_are_kept_apart() {
    let dir = tempfile::tempdir().unwrap();
    let main = r#"{ git = "https://example.invalid/pack.git", branch = "main" }"#;
    let other = r#"{ git = "https://example.invalid/other.git", branch = "dev" }"#;
    pin(DEV, dir.path(), at(1000), &|_: &str, _: &str| Ok(A.to_string()));
    pin(main, dir.path(), at(1000), &|_: &str, _: &str| Ok(B.to_string()));
    let fail = |_: &str, _: &str| Err::<String, String>("offline".into());
    assert_eq!(pin(DEV, dir.path(), at(1000), &never), Pinned::Current { rev: A.into() });
    assert_eq!(pin(main, dir.path(), at(1000), &never), Pinned::Current { rev: B.into() });
    assert!(matches!(pin(other, dir.path(), at(1000), &fail), Pinned::Unresolved { .. }));
}

#[test]
fn the_url_and_branch_are_what_the_remote_is_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let seen = std::cell::RefCell::new(Vec::new());
    pin(DEV, dir.path(), at(1000), &|u: &str, b: &str| {
        seen.borrow_mut().push((u.to_string(), b.to_string()));
        Ok(A.to_string())
    });
    assert_eq!(
        seen.into_inner(),
        vec![("https://example.invalid/pack.git".to_string(), "dev".to_string())]
    );
}

// --- the real remote, a local repository --------------------------------------

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(["-c", "tag.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn ls_remote_reads_the_tip_of_the_named_branch_and_follows_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "dev"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let first = git(repo, &["rev-parse", "HEAD"]);
    // A tag named exactly like the branch, on an older commit, must not answer.
    git(repo, &["tag", "-m", "t", "dev", "HEAD"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let url = format!("file://{}", repo.display());
    let second = git(repo, &["rev-parse", "HEAD"]);
    assert_ne!(first, second);
    assert_eq!(ls_remote_head(&url, "dev"), Ok(second.clone()));

    git(repo, &["commit", "-q", "--allow-empty", "-m", "three"]);
    let third = git(repo, &["rev-parse", "HEAD"]);
    assert_eq!(ls_remote_head(&url, "dev"), Ok(third));
}

#[test]
fn ls_remote_refuses_a_branch_that_is_not_there_and_a_tag_named_like_one() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "dev"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(repo, &["tag", "-m", "t", "release"]);
    let url = format!("file://{}", repo.display());
    assert!(ls_remote_head(&url, "nope").is_err());
    assert!(ls_remote_head(&url, "release").is_err(), "a tag answered for a branch");
}

#[test]
fn ls_remote_refuses_a_remote_that_does_not_exist() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("file://{}/missing", dir.path().display());
    assert!(ls_remote_head(&url, "dev").is_err());
}

#[test]
fn a_real_branch_pin_is_written_at_the_tip_and_moves_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("pack");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "dev"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let first = git(&repo, &["rev-parse", "HEAD"]);
    let spec = format!(r#"{{ git = "file://{}", branch = "dev" }}"#, repo.display());
    let pins = dir.path().join("pins");

    let out = spec_for_cargo("p", &spec, &pins, &ls_remote_head);
    assert!(out.contains(&first), "{out}");
    assert_eq!(branch_pin(&out), None, "{out}");

    // Moved on the remote, and the kept resolution aged out: the new tip is used.
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let second = git(&repo, &["rev-parse", "HEAD"]);
    let later = SystemTime::now() + TIP_TTL + Duration::from_secs(1);
    assert_eq!(pin(&spec, &pins, later, &ls_remote_head), Pinned::Current { rev: second });
}

// --- the generated manifest -----------------------------------------------------

fn manifest_for(packs: &[(String, String)], resolve: &dyn Fn(&str, &str) -> Result<String, String>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let cfg = crate::config::Config::from_dir(dir.path());
    let gen_dir = dir.path().join("gen");
    crate::custom_lints::write_cdylib_crate(
        &gen_dir,
        &dir.path().join("lints"),
        &[],
        packs,
        &[],
        "{ package = \"mockspace-lint-rules\", git = \"u\", rev = \"r\" }",
        &cfg,
        resolve,
    )
    .unwrap();
    std::fs::read_to_string(gen_dir.join("Cargo.toml")).unwrap()
}

#[test]
fn the_generated_manifest_names_a_branch_pack_by_the_rev_of_its_tip() {
    let fixed = r#"{ git = "https://example.invalid/fixed.git", rev = "abc" }"#;
    let manifest = manifest_for(
        &[("pack".into(), DEV.into()), ("fixed".into(), fixed.into())],
        &|_: &str, _: &str| Ok(A.to_string()),
    );
    let line = manifest.lines().find(|l| l.starts_with("pack = ")).expect(&manifest);
    let t = inline_table(line.trim_start_matches("pack = ")).expect(line);
    assert_eq!(t.get("rev").and_then(|v| v.as_str()), Some(A), "{line}");
    assert!(!t.contains_key("branch"), "the branch reached cargo: {line}");
    // A pack pinned some other way is written exactly as it was declared.
    assert!(manifest.contains(&format!("fixed = {fixed}\n")), "{manifest}");
}

#[test]
fn the_generated_manifest_keeps_the_branch_when_nothing_was_ever_resolved() {
    let manifest = manifest_for(&[("pack".into(), DEV.into())], &|_: &str, _: &str| {
        Err("offline".into())
    });
    assert!(manifest.contains(&format!("pack = {DEV}\n")), "{manifest}");
}

// --- stamps, names and notes -----------------------------------------------------

#[test]
fn a_stamp_from_the_future_is_not_taken_as_fresh() {
    let dir = tempfile::tempdir().unwrap();
    pin(DEV, dir.path(), at(10_000), &|_: &str, _: &str| Ok(A.to_string()));
    let asked = Cell::new(0);
    let resolve = |_: &str, _: &str| {
        asked.set(asked.get() + 1);
        Ok(B.to_string())
    };
    // The clock went back: the kept stamp is ahead of now, so the remote is asked.
    assert_eq!(pin(DEV, dir.path(), at(5_000), &resolve), Pinned::Current { rev: B.into() });
    assert_eq!(asked.get(), 1);
    // The same stamp an instant behind now is fresh, which is what the check above
    // has to be distinguished from.
    pin(DEV, dir.path(), at(10_000), &|_: &str, _: &str| Ok(A.to_string()));
    assert_eq!(pin(DEV, dir.path(), at(10_001), &never), Pinned::Current { rev: A.into() });
}

#[test]
fn a_listing_names_its_tip_by_a_sha1_or_a_sha256_object_name() {
    let sha256 = "c".repeat(64);
    assert_eq!(tip_from_listing(&format!("{A}\trefs/heads/dev\n")), Some(A.into()));
    assert_eq!(tip_from_listing(&format!("{sha256}\trefs/heads/dev\n")), Some(sha256.clone()));
    for bad in [
        String::new(),
        "\n".into(),
        format!("{}\trefs/heads/dev", &A[..39]),
        format!("{A}a\trefs/heads/dev"),
        format!("{}\trefs/heads/dev", "g".repeat(40)),
        format!("{}\trefs/heads/dev", "c".repeat(63)),
    ] {
        assert_eq!(tip_from_listing(&bad), None, "{bad:?}");
    }
}

#[test]
fn a_sha256_resolution_is_kept_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let sha256 = "c".repeat(64);
    pin(DEV, dir.path(), at(1000), &|_: &str, _: &str| Ok(sha256.clone()));
    assert_eq!(pin(DEV, dir.path(), at(1001), &never), Pinned::Current { rev: sha256 });
}

#[test]
fn the_hash_naming_a_cache_file_is_fnv1a_and_does_not_move() {
    // The published FNV-1a 64 vectors.
    assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
    // A name kept from one toolchain has to be found by the next, so the name of
    // a known pin is fixed here rather than recomputed.
    let file = cache_file(Path::new("c"), &branch_pin(DEV).unwrap());
    assert_eq!(file, Path::new("c").join("0c94766fb95f83ec"));
}

#[test]
fn a_note_is_said_only_where_the_run_fell_back_and_says_what_it_fell_back_to() {
    assert_eq!(fallback_note("p", &Pinned::Untouched), None);
    assert_eq!(fallback_note("p", &Pinned::Current { rev: A.into() }), None);
    let stale = fallback_note(
        "p",
        &Pinned::Stale {
            rev: A.into(),
            age: Duration::from_secs(90 * 60 + 59),
            why: "offline".into(),
        },
    )
    .unwrap();
    for part in ["`p`", "(offline)", A, "resolved 90 minutes ago"] {
        assert!(stale.contains(part), "{part} missing from {stale}");
    }
    let unresolved = fallback_note("p", &Pinned::Unresolved { why: "offline".into() }).unwrap();
    for part in ["`p`", "(offline)", "never resolved here", "whatever the lockfile holds"] {
        assert!(unresolved.contains(part), "{part} missing from {unresolved}");
    }
}

// --- asking with a deadline -----------------------------------------------------

fn sh(script: &str) -> std::process::Command {
    let mut c = std::process::Command::new("sh");
    c.args(["-c", script]);
    c
}

#[test]
fn a_command_past_its_deadline_is_killed_and_reported() {
    let started = Instant::now();
    let got = run_within(sh("sleep 30"), Duration::from_millis(200), "sleeper");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    let why = got.unwrap_err();
    assert!(why.contains("sleeper did not answer within"), "{why}");
}

#[test]
fn a_command_inside_its_deadline_hands_back_both_streams_and_its_status() {
    let out = run_within(sh("echo out; echo err >&2; exit 3"), Duration::from_secs(5), "x").unwrap();
    assert_eq!(out.stdout, b"out\n");
    assert_eq!(out.stderr, b"err\n");
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn a_command_writing_more_than_a_pipe_holds_is_not_stalled() {
    let out = run_within(sh("head -c 1000000 /dev/zero"), Duration::from_secs(5), "x").unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 1_000_000);
}

#[test]
fn a_command_that_cannot_be_started_is_an_error_not_a_hang() {
    let why = run_within(
        std::process::Command::new("/nonexistent/binary"),
        Duration::from_secs(5),
        "ghost",
    )
    .unwrap_err();
    assert!(why.contains("could not run ghost"), "{why}");
}
