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
        case("kaski-sim", "quick", 0.2, Outcome::Pass),
        case("kaski-sim", "slow", 90.0, Outcome::Fail),
        case("kaski-sim::it", "skipped_one", 0.0, Outcome::Skipped),
    ];
    let r = rows(&facts(&e), &cases, &[], &no_fp);
    assert_eq!(r.len(), 3);
    assert_eq!((r[0].test.as_str(), r[0].secs, r[0].outcome.as_str()), ("quick", 0.2, "pass"));
    assert_eq!(r[1].outcome, "fail");
    assert_eq!((r[2].member.as_str(), r[2].outcome.as_str()), ("kaski-sim", "skipped"));
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
fn threads_come_from_the_flag_in_any_spelling_else_the_cpus() {
    let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(test_threads(&a(&["-j", "3"]), 8), 3);
    assert_eq!(test_threads(&a(&["-j5"]), 8), 5);
    assert_eq!(test_threads(&a(&["--test-threads=2"]), 8), 2);
    assert_eq!(test_threads(&a(&["--test-threads", "6"]), 8), 6);
    assert_eq!(test_threads(&a(&["-j", "0"]), 8), 8);
    assert_eq!(test_threads(&a(&[]), 8), 8);
}
