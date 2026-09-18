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
fn a_failed_ask_is_not_kept_so_every_run_past_the_hour_asks_again() {
    let dir = tempfile::tempdir().unwrap();
    pin(DEV, dir.path(), at(1000), &|_: &str, _: &str| Ok(A.to_string()));
    let later = at(1000 + TIP_TTL.as_secs() + 1);
    let asked = Cell::new(0);
    let fail = |_: &str, _: &str| {
        asked.set(asked.get() + 1);
        Err::<String, String>("offline".into())
    };
    for run in 1..=3 {
        pin(DEV, dir.path(), later, &fail);
        assert_eq!(asked.get(), run);
    }
    // With nothing ever kept, the same holds inside the hour too.
    let empty = tempfile::tempdir().unwrap();
    asked.set(0);
    for run in 1..=3 {
        pin(DEV, empty.path(), at(1000), &fail);
        assert_eq!(asked.get(), run);
    }
}

#[test]
fn a_spec_following_the_default_branch_is_not_resolved_and_goes_to_cargo_as_written() {
    let spec = r#"{ git = "u" }"#;
    assert_eq!(branch_pin(spec), None);
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(pin(spec, dir.path(), at(1000), &never), Pinned::Untouched);
    assert_eq!(spec_for_cargo("p", spec, dir.path(), &never), spec);
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
    // A tag named exactly like the branch, on an older commit. It does not make
    // this arm fail when the refspec is bare, because `ls-remote <url> dev` lists
    // `refs/heads/dev` ahead of `refs/tags/dev` and the first line wins either
    // way. What catches a bare refspec is the `release` arm of the next test,
    // where only a tag carries the name.
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
    assert_eq!(fallback_note("lint pack `p`", &Pinned::Untouched), None);
    assert_eq!(fallback_note("lint pack `p`", &Pinned::Current { rev: A.into() }), None);
    let stale = fallback_note(
        "lint pack `p`",
        &Pinned::Stale {
            rev: A.into(),
            age: Duration::from_secs(90 * 60 + 59),
            why: "offline".into(),
        },
    )
    .unwrap();
    for part in ["lint pack `p`", "(offline)", A, "resolved 90 minutes ago"] {
        assert!(stale.contains(part), "{part} missing from {stale}");
    }
    let unresolved =
        fallback_note("lint pack `p`", &Pinned::Unresolved { why: "offline".into() }).unwrap();
    for part in ["lint pack `p`", "(offline)", "never resolved here", "whatever the lockfile holds"] {
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
fn a_command_reading_stdin_reads_end_of_file_rather_than_waiting() {
    // The caller hands over a stdin that never closes, which is what a hook
    // with its own open stdin would pass down. Without the null the child
    // reads that pipe and never finishes, whatever the test runner's own
    // stdin happens to be.
    let (reader, _writer) = std::io::pipe().unwrap();
    let mut cmd = sh("cat; echo done");
    cmd.stdin(reader);
    let out = run_within(cmd, Duration::from_secs(5), "x").unwrap();
    assert_eq!(out.stdout, b"done\n");
}

#[test]
fn a_child_that_leaves_its_output_held_open_is_still_bounded() {
    let started = Instant::now();
    let got = run_within(sh("sleep 4 & echo hi"), Duration::from_millis(500), "leaver");
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    let why = got.unwrap_err();
    assert!(why.contains("leaver did not answer within"), "{why}");
}

#[test]
fn a_child_whose_output_closes_in_time_is_read_whole_after_it_exits() {
    // The control for the case above: a background writer that finishes inside
    // the deadline is waited for rather than cut off.
    let out = run_within(sh("(sleep 0.2; echo late) & echo early"), Duration::from_secs(5), "x").unwrap();
    assert_eq!(out.stdout, b"early\nlate\n");
}

/// Git's three ssh settings as a test states them, counting how often the
/// configuration was asked and taking `core_takes` to answer it.
struct Sources {
    ssh_command: Option<&'static str>,
    ssh:         Option<&'static str>,
    core:        Option<&'static str>,
    core_takes:  Duration,
    core_asked:  Cell<u32>,
}

impl Sources {
    fn of(ssh_command: Option<&'static str>, ssh: Option<&'static str>, core: Option<&'static str>) -> Self {
        Self { ssh_command, ssh, core, core_takes: Duration::ZERO, core_asked: Cell::new(0) }
    }

    fn silent() -> Self {
        Self::of(None, None, None)
    }
}

impl SshSources for Sources {
    fn ssh_command(&self) -> Option<std::ffi::OsString> {
        self.ssh_command.map(Into::into)
    }

    fn ssh(&self) -> Option<std::ffi::OsString> {
        self.ssh.map(Into::into)
    }

    fn core_ssh_command(&self, _: Duration) -> Option<String> {
        self.core_asked.set(self.core_asked.get() + 1);
        std::thread::sleep(self.core_takes);
        self.core.map(Into::into)
    }
}

fn ssh_env(git: &Command) -> Option<std::ffi::OsString> {
    git.get_envs()
        .find(|(k, _)| *k == "GIT_SSH_COMMAND")
        .and_then(|(_, v)| v.map(|v| v.to_os_string()))
}

#[test]
fn asking_a_remote_that_hangs_comes_back_within_the_deadline() {
    let url = "ssh://git@example.invalid/nothing";
    let mut git = ls_remote_command(url, "dev", &Sources::silent(), ASK_DEADLINE);
    git.env("GIT_SSH_COMMAND", "sh -c 'sleep 30' --");
    let started = Instant::now();
    let got = remote_head(git, url, "dev", Duration::from_millis(500));
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    let why = got.unwrap_err();
    assert!(why.contains("did not answer within"), "{why}");
}

#[test]
fn the_listing_asks_for_the_full_ref_with_prompting_turned_off() {
    for sources in [Sources::silent(), Sources::of(Some("ssh -i key"), None, None)] {
        let git = ls_remote_command("u", "dev", &sources, ASK_DEADLINE);
        let args: Vec<_> = git.get_args().collect();
        assert_eq!(
            args,
            ["-c", "credential.interactive=never", "ls-remote", "u", "refs/heads/dev"]
        );
        let envs: Vec<_> = git.get_envs().collect();
        assert!(envs.contains(&("GIT_TERMINAL_PROMPT".as_ref(), Some("0".as_ref()))), "{envs:?}");
    }
}

#[test]
fn ssh_is_put_in_batch_mode_only_when_none_of_the_three_says_how_it_runs() {
    let set = Some("ssh -i key");
    for ssh_command in [None, set] {
        for ssh in [None, set] {
            for core in [None, set] {
                let sources = Sources::of(ssh_command, ssh, core);
                let got = ssh_env(&ls_remote_command("u", "dev", &sources, ASK_DEADLINE));
                let silent = ssh_command.is_none() && ssh.is_none() && core.is_none();
                let want = silent.then(|| "ssh -o BatchMode=yes".into());
                assert_eq!(got, want, "GIT_SSH_COMMAND={ssh_command:?} GIT_SSH={ssh:?} core={core:?}");
            }
        }
    }
}

#[test]
fn the_configuration_is_asked_only_when_the_environment_says_nothing() {
    let asked = |sources: Sources| {
        ls_remote_command("u", "dev", &sources, ASK_DEADLINE);
        sources.core_asked.get()
    };
    assert_eq!(asked(Sources::silent()), 1);
    assert_eq!(asked(Sources::of(None, None, Some("ssh -i key"))), 1);
    assert_eq!(asked(Sources::of(Some("ssh -i key"), None, None)), 0);
    assert_eq!(asked(Sources::of(None, Some("ssh"), None)), 0);
}

#[test]
fn one_deadline_covers_the_configuration_and_the_listing_both() {
    // The configuration takes the whole deadline, so the listing is left
    // none of it and is given up on at once. Given the full deadline again
    // instead, git would run to its own answer: a failure about the path.
    let deadline = Duration::from_millis(600);
    let sources = Sources { core_takes: deadline, ..Sources::silent() };
    let started = Instant::now();
    let why = ls_remote_head_from(&sources, "file:///nonexistent/pack.git", "dev", deadline).unwrap_err();
    assert!(why.contains("did not answer within 0 seconds"), "{why}");
    assert!(started.elapsed() < deadline + Duration::from_millis(400), "{:?}", started.elapsed());
}

#[test]
fn the_configuration_is_read_as_git_would_read_it_here() {
    let git = core_ssh_command_query();
    assert_eq!(git.get_program(), "git");
    let args: Vec<_> = git.get_args().collect();
    assert_eq!(args, ["config", "--get", "core.sshCommand"]);
}

#[test]
fn a_configured_value_is_what_the_repository_sets_and_nothing_otherwise() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let mut c = Command::new("git");
        c.args(args)
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    };
    assert!(git(&["init", "-q"]).status().unwrap().success());
    let query = || {
        let mut q = core_ssh_command_query();
        q.current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        q
    };
    assert_eq!(configured_value(query(), ASK_DEADLINE), None);
    assert!(git(&["config", "core.sshCommand", "ssh -i key"]).status().unwrap().success());
    assert_eq!(configured_value(query(), ASK_DEADLINE), Some("ssh -i key".into()));
    assert!(git(&["config", "core.sshCommand", ""]).status().unwrap().success());
    assert_eq!(configured_value(query(), ASK_DEADLINE), None);
}

#[test]
fn a_configuration_too_slow_to_answer_reads_as_nothing_set() {
    let started = Instant::now();
    assert_eq!(configured_value(sh("sleep 30; echo late"), Duration::from_millis(300)), None);
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    assert_eq!(configured_value(sh("echo 'ssh -i key'"), ASK_DEADLINE), Some("ssh -i key".into()));
    assert_eq!(configured_value(sh("echo 'ssh -i key'; exit 1"), ASK_DEADLINE), None);
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

#[test]
fn an_ssh_the_listing_started_outlives_the_deadline_it_was_given_up_on_in() {
    let dir = tempfile::tempdir().unwrap();
    let mark = dir.path().join("ssh-finished");
    let url = "ssh://git@example.invalid/nothing";
    let mut git = ls_remote_command(url, "dev", &Sources::of(Some("chosen"), None, None), ASK_DEADLINE);
    git.env("GIT_SSH_COMMAND", format!("sh -c 'sleep 2; touch {}' --", mark.display()));
    let why = remote_head(git, url, "dev", Duration::from_millis(500)).unwrap_err();
    assert!(why.contains("did not answer within"), "{why}");
    assert!(!mark.exists(), "ssh finished before the deadline, so this shows nothing");
    std::thread::sleep(Duration::from_secs(3));
    assert!(mark.exists(), "the ssh git started was stopped with it");
}

// --- this process, read from a child of the test binary --------------------------

/// Set in the child, which then prints what `ThisProcess` reads and nothing else.
const CHILD: &str = "MOCKSPACE_PACK_PIN_THIS_PROCESS";

/// One line `name` of what `ThisProcess` reads when the test binary runs
/// again in `dir`, with `env` set and every other ssh setting and global
/// configuration cleared.
fn this_process_in(dir: &Path, env: &[(&str, &str)], name: &str) -> String {
    let mut child = Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "pack_pin::tests::this_process_reports_itself",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(dir)
        .env(CHILD, "1")
        .env_remove("GIT_SSH_COMMAND")
        .env_remove("GIT_SSH")
        .env_remove("GIT_DIR")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for (k, v) in env {
        child.env(k, v);
    }
    let out = child.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{name}=")))
        .unwrap_or_else(|| panic!("the child printed no {name}: {stdout}"))
        .to_string()
}

fn sources_in(dir: &Path, env: &[(&str, &str)]) -> [String; 3] {
    ["ssh_command", "ssh", "core"].map(|name| this_process_in(dir, env, name))
}

#[test]
#[ignore = "run as a child by this_process_in, which reads what it prints"]
fn this_process_reports_itself() {
    assert!(std::env::var_os(CHILD).is_some(), "run only as a child, by `this_process_in`");
    let p = ThisProcess;
    // The harness prints the test's name with no newline before its output.
    println!();
    println!("ssh_command={:?}", p.ssh_command());
    println!("ssh={:?}", p.ssh());
    println!("core={:?}", p.core_ssh_command(ASK_DEADLINE));
    println!(
        "batch={:?}",
        ssh_env(&ls_remote_command("u", "dev", &p, ASK_DEADLINE))
    );
}

#[test]
fn this_process_reads_both_variables_and_the_clone_it_runs_in() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    assert_eq!(sources_in(dir.path(), &[]), ["None".to_string(), "None".into(), "None".into()]);
    assert_eq!(
        sources_in(dir.path(), &[("GIT_SSH_COMMAND", "ssh -i one"), ("GIT_SSH", "/bin/two")]),
        [r#"Some("ssh -i one")"#.to_string(), r#"Some("/bin/two")"#.into(), "None".into()]
    );
    git(dir.path(), &["config", "core.sshCommand", "ssh -i three"]);
    assert_eq!(
        sources_in(dir.path(), &[]),
        ["None".to_string(), "None".into(), r#"Some("ssh -i three")"#.into()]
    );
}

#[test]
fn this_process_puts_ssh_in_batch_mode_only_in_a_clone_that_says_nothing() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    let batch = |env: &[(&str, &str)]| this_process_in(dir.path(), env, "batch");
    assert_eq!(batch(&[]), r#"Some("ssh -o BatchMode=yes")"#);
    assert_eq!(batch(&[("GIT_SSH", "/bin/two")]), "None");
    git(dir.path(), &["config", "core.sshCommand", "ssh -i three"]);
    assert_eq!(batch(&[]), "None");
}
