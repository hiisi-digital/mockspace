use super::*;

fn env() -> EnvMeta {
    EnvMeta {
        timestamp: 1_800_000_000,
        machine_class: "linux-x86_64-c8-bare-amd".into(),
        cpus_online: 8,
        ..EnvMeta::default()
    }
}

fn facts(env: &EnvMeta) -> RunFacts<'_> {
    RunFacts {
        env,
        run: "r1",
        commit: "abc1234",
        flavour: "default",
        profile: "dev",
        test_threads: 4,
        load_1m_before: Some(0.5),
        load_1m_after: None,
    }
}

fn case(bin: &str, name: &str, secs: f64, outcome: Outcome) -> Case {
    Case { binary: bin.into(), name: name.into(), secs, outcome }
}

fn no_fp(_: &str) -> Option<String> {
    None
}

/// Before this, a 0.2 s test (under the heavy threshold) wrote nothing.
#[test]
fn a_fast_test_is_recorded_like_a_slow_one() {
    let e = env();
    let cases = [
        case("sim-core", "quick", 0.2, Outcome::Pass),
        case("sim-core", "slow", 90.0, Outcome::Fail),
        case("sim-core::it", "skipped_one", 0.0, Outcome::Skipped),
    ];
    let r = rows(&facts(&e), &cases, &[], &no_fp);
    assert_eq!(r.len(), 3);
    assert_eq!((r[0].test.as_str(), r[0].secs, r[0].outcome.as_str()), ("quick", 0.2, "pass"));
    assert_eq!(r[1].outcome, "fail");
    assert_eq!((r[2].member.as_str(), r[2].outcome.as_str()), ("sim-core", "skipped"));
    assert!(r.iter().all(|x| x.machine_class == "linux-x86_64-c8-bare-amd"));
    assert!(r.iter().all(|x| x.profile == "dev" && x.test_threads == 4));
}

#[test]
fn only_the_test_that_printed_work_carries_it() {
    let e = env();
    let cases = [case("sim", "a", 1.0, Outcome::Pass), case("sim", "b", 1.0, Outcome::Pass)];
    let work = [Work { binary: "sim".into(), name: "a".into(), unit: "ticks".into(), n: 9 }];
    let r = rows(&facts(&e), &cases, &work, &no_fp);
    assert_eq!((r[0].work_unit.as_deref(), r[0].work_n), (Some("ticks"), Some(9)));
    assert_eq!((r[1].work_unit.clone(), r[1].work_n), (None, None));
}

#[test]
fn a_shard_round_trips_and_a_second_write_of_it_is_refused() {
    let dir = std::env::temp_dir().join(format!("mock-timings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let e = env();
    let r = rows(&facts(&e), &[case("sim", "a", 0.25, Outcome::Pass)], &[], &no_fp);
    let path = spool(&dir, &r).unwrap().unwrap();
    assert!(path.ends_with("samples/2027-01/linux-x86_64-c8-bare-amd/r1.ndjson"));
    let back: Vec<TestSample> = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(back, r);
    let again = spool(&dir, &r).unwrap_err();
    assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(spool(&dir, &[]).unwrap(), None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn threads_come_from_every_spelling_nextest_takes_else_the_env_else_the_cpus() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    for (args, want) in [
        (&["-j", "3"][..], 3),
        (&["-j5"], 5),
        (&["-j=4"], 4),
        (&["--jobs", "7"], 7),
        (&["--jobs=9"], 9),
        (&["--test-threads=2"], 2),
        (&["--test-threads", "6"], 6),
        (&["-j", "num-cpus"], 8),
        (&["-j", "-2"], 6),
        (&["-j", "-20"], 1),
        (&["-j", "0"], 8),
        (&[], 8),
    ] {
        assert_eq!(test_threads(&a(args), None, 8), want, "{args:?}");
    }
    assert_eq!(test_threads(&a(&[]), Some("3"), 8), 3);
    assert_eq!(test_threads(&a(&["-j", "5"]), Some("3"), 8), 5, "the flag wins over the env");
}

#[test]
fn a_spooled_run_leaves_no_temp_file_and_run_ids_differ() {
    let dir = std::env::temp_dir().join(format!("mock-timings-tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let e = env();
    let r = rows(&facts(&e), &[case("sim", "a", 0.25, Outcome::Pass)], &[], &no_fp);
    let path = spool(&dir, &r).unwrap().unwrap();
    let left: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["r1.ndjson".to_string()]);
    std::fs::remove_dir_all(&dir).unwrap();
    let base = run_id_from(5, 1, 100, "/w", 0);
    assert_ne!(base, run_id_from(5, 1, 101, "/w", 0), "the pid alone");
    assert_ne!(base, run_id_from(5, 1, 100, "/v", 0), "the working directory alone");
    assert_ne!(base, run_id_from(5, 2, 100, "/w", 0), "the nanosecond alone");
    assert_ne!(base, run_id_from(5, 1, 100, "/w", 1), "the counter alone");
}

/// The whole step: a report with a passing test's work line in, one shard of
/// rows out, the count on the right row. That the call in `mod.rs` is made is
/// the nextest end-to-end tests' to show; this pins what the call does.
#[test]
fn a_report_in_becomes_a_shard_with_the_work_on_the_right_row() {
    let state = std::env::temp_dir().join(format!("mock-timings-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    let report = r#"<testsuites><testsuite name="sim">
        <testcase name="walks" classname="sim" time="1.500"><system-err>MOCK-WORK ticks 7
</system-err></testcase>
        <testcase name="plain" classname="sim" time="0.010"/>
        </testsuite></testsuites>"#;
    let cases = super::super::junit::parse(report);
    let passed = vec!["-j".to_string(), "3".to_string()];
    record_run(
        &Run {
            state:       &state,
            flavour:     "default",
            profile:     "dev",
            passed:      &passed,
            report,
            load_before: Some(0.25),
            threads_env: None,
            commit:      "abc1234-dirty",
            fingerprint: None,
        },
        &cases,
        "MOCK-WORK",
    );
    let shards = walk(&state.join(SPOOL_DIR));
    assert_eq!(shards.len(), 1, "{shards:?}");
    let rows: Vec<TestSample> = std::fs::read_to_string(&shards[0])
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    let walks = rows.iter().find(|r| r.test == "walks").unwrap();
    assert_eq!((walks.work_unit.as_deref(), walks.work_n), (Some("ticks"), Some(7)));
    assert_eq!((walks.secs, walks.test_threads, walks.commit.as_str()), (1.5, 3, "abc1234-dirty"));
    assert_eq!(walks.load_1m_before, Some(0.25));
    assert_eq!(rows.iter().find(|r| r.test == "plain").unwrap().work_n, None);
    std::fs::remove_dir_all(&state).unwrap();
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}
