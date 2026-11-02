//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

use super::*;
use crate::suite::select::SkipWhy;

fn none() -> std::collections::BTreeMap<String, Vec<String>> {
    Default::default()
}

fn heavy() -> Vec<(String, String)> {
    vec![
        ("kaski-render".to_string(), "tests::drawn::a".to_string()),
        ("kaski-web::it".to_string(), "b".to_string()),
    ]
}

#[test]
fn the_config_carries_the_group_the_timeout_and_the_report() {
    let s = TestSettings {
        heavy_threads: 2,
        timeout_secs: 900,
        ..TestSettings::default()
    };
    let t = tool_config(&s, &heavy(), Path::new("/w/mock/target/state/junit.xml"));
    assert!(
        t.contains("[test-groups.\"@tool:mockspace:heavy\"]\nmax-threads = 2\n"),
        "{t}"
    );
    assert!(
        t.contains("slow-timeout = { period = \"60s\", terminate-after = 15 }"),
        "{t}"
    );
    assert!(t.contains("fail-fast = false"));
    assert!(t.contains("[profile.default.junit]\npath = \"/w/mock/target/state/junit.xml\"\n"), "{t}");
    assert!(t.contains("filter = 'binary_id(=kaski-render) & (test(=tests::drawn::a))'"), "{t}");
    assert!(t.contains("filter = 'binary_id(=kaski-web::it) & (test(=b))'"), "{t}");
    // The written config must be TOML nextest can read.
    assert!(t.parse::<toml_edit::DocumentMut>().is_ok(), "{t}");
}

#[test]
fn a_timeout_under_a_minute_shrinks_the_period_and_rounds_up() {
    let s = TestSettings {
        timeout_secs: 45,
        ..TestSettings::default()
    };
    assert!(tool_config(&s, &[], Path::new("junit.xml")).contains("period = \"45s\", terminate-after = 1"));
    let s = TestSettings {
        timeout_secs: 61,
        ..TestSettings::default()
    };
    assert!(tool_config(&s, &[], Path::new("junit.xml")).contains("period = \"60s\", terminate-after = 2"));
}

#[test]
fn the_profile_built_follows_the_flags_that_choose_it() {
    let words = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
    let p = |s: &str| build_profile(&words(s), "--cargo-profile");
    assert_eq!(p(""), "dev");
    assert_eq!(p("--features x"), "dev");
    assert_eq!(p("--release"), "release");
    assert_eq!(p("-r"), "release");
    assert_eq!(p("--cargo-profile ci"), "ci");
    assert_eq!(p("--cargo-profile=ci"), "ci");
    assert_eq!(build_profile(&words("--profile ci"), "--profile"), "ci");
    assert_eq!(build_profile(&words("--profile=ci"), "--profile"), "ci");
    assert_eq!(
        build_profile(&words("--profile ci"), "--cargo-profile"),
        "dev"
    );
}

#[test]
fn the_run_builds_with_the_generated_cargo_config_before_the_callers_flags() {
    let cargo = Path::new("/w/mock/target/state/cargo.toml");
    let passed = vec!["--config".to_string(), "x=1".to_string()];
    let spec = |cargo_config| {
        RunSpec {
            mock_dir: Path::new("."),
            tool_config: Path::new("nextest.toml"),
            cargo_config,
            packages: &[],
            passed: &passed,
            filterset: None,
            test_args: &[],
        }
    };
    let args = |c: &Command| {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    let with = args(&command(&spec(Some(cargo))));
    let at = with
        .iter()
        .position(|a| a == "/w/mock/target/state/cargo.toml")
        .unwrap();
    assert_eq!(with[at - 1], "--config");
    let callers = with.iter().position(|a| a == "x=1").unwrap();
    assert!(
        at < callers,
        "the caller's own --config must come later and win: {with:?}"
    );
    assert!(
        !args(&command(&spec(None)))
            .iter()
            .any(|a| a.ends_with("cargo.toml"))
    );
}

/// 653 tests of one binary made one 76 KB expression that nextest refused. The
/// split keeps every test and none twice, each override under the bound.
#[test]
fn a_long_heavy_list_is_split_into_overrides_that_name_every_test_once() {
    let mut heavy: Vec<(String, String)> = (0..653)
        .map(|n| ("kaski-sim::big".to_string(), format!("tests::module::heavy_case_{n:04}")))
        .collect();
    heavy.push(("kaski-web::it".to_string(), "b".to_string()));
    let t = tool_config(&TestSettings::default(), &heavy, Path::new("junit.xml"));
    let doc = t.parse::<toml_edit::DocumentMut>().unwrap();
    let overrides = doc["profile"]["default"]["overrides"].as_array_of_tables().unwrap();
    assert!(overrides.len() > 2, "{} overrides", overrides.len());
    let mut named = 0;
    for o in overrides {
        let f = o["filter"].as_str().unwrap();
        assert!(f.len() < OVERRIDE_FILTER_BYTES + 100, "{} bytes", f.len());
        assert_eq!(o["test-group"].as_str(), Some(HEAVY_GROUP));
        named += f.matches("test(=").count();
    }
    assert_eq!(named, heavy.len());
    for (_, name) in &heavy {
        assert_eq!(t.matches(&format!("test(={name})")).count(), 1, "{name}");
    }
}

#[test]
fn a_binary_narrowed_to_its_failures_leaves_its_other_tests_out() {
    let rerun = std::collections::BTreeMap::from([(
        "kaski-web::it".to_string(),
        vec!["a".to_string(), "b".to_string()],
    )]);
    assert_eq!(
        filterset(None, &[], &[], &rerun).as_deref(),
        Some("not ((binary_id(=kaski-web::it) & not (test(=a) | test(=b))))")
    );
    assert_eq!(
        filterset(Some("test(x)"), &[], &["lib".to_string()], &rerun).as_deref(),
        Some(
            "(test(x)) & not (binary_id(=lib) | (binary_id(=kaski-web::it) & not (test(=a) | \
             test(=b))))"
        )
    );
}

/// Nextest alone builds every example of every member, which is the cost this
/// avoids; a caller who names a target keeps exactly that target.
#[test]
fn a_run_builds_the_tests_unless_the_caller_chose_targets() {
    let args = |passed: &[&str]| {
        let passed: Vec<String> = passed.iter().map(|s| s.to_string()).collect();
        let spec = RunSpec {
            mock_dir: Path::new("."),
            tool_config: Path::new("nextest.toml"),
            cargo_config: None,
            packages: &[],
            passed: &passed,
            filterset: None,
            test_args: &[],
        };
        command(&spec)
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert!(args(&[]).contains(&"--tests".to_string()));
    assert!(args(&["--features", "x"]).contains(&"--tests".to_string()));
    for chosen in ["--all-targets", "--examples", "--lib", "--bin=x", "--test", "--benches"] {
        assert!(!args(&[chosen]).contains(&"--tests".to_string()), "{chosen}");
        assert!(selects_targets(&[chosen.to_string()]), "{chosen}");
    }
    assert!(!selects_targets(&["--features".to_string(), "examples".to_string()]));
}

#[test]
fn no_heavy_test_means_no_override() {
    assert!(!tool_config(&TestSettings::default(), &[], Path::new("junit.xml")).contains("overrides"));
}

#[test]
fn the_filterset_subtracts_the_skipped_from_the_callers() {
    let skip = vec![Skip {
        binary: "kaski-render".to_string(),
        name:   "tests::drawn::a".to_string(),
        why:    SkipWhy::Cached,
    }];
    let not = "not ((binary_id(=kaski-render) & test(=tests::drawn::a)))";
    assert_eq!(filterset(None, &[], &[], &none()), None);
    assert_eq!(
        filterset(Some("test(x)"), &[], &[], &none()).as_deref(),
        Some("test(x)")
    );
    assert_eq!(filterset(None, &skip, &[], &none()).as_deref(), Some(not));
    assert_eq!(
        filterset(Some("test(x)"), &skip, &[], &none()),
        Some(format!("(test(x)) & {not}"))
    );
}

#[test]
fn the_filterset_leaves_out_whole_binaries_beside_single_tests() {
    let reuse = vec!["kaski-web::sound_gate".to_string(), "kaski-web".to_string()];
    assert_eq!(
        filterset(None, &[], &reuse, &none()).as_deref(),
        Some("not (binary_id(=kaski-web::sound_gate) | binary_id(=kaski-web))")
    );
    let skip = vec![Skip {
        binary: "kaski-render".to_string(),
        name:   "tests::drawn::a".to_string(),
        why:    SkipWhy::Cached,
    }];
    assert_eq!(
        filterset(Some("test(x)"), &skip, &reuse, &none()).as_deref(),
        Some(
            "(test(x)) & not ((binary_id(=kaski-render) & test(=tests::drawn::a)) | \
             binary_id(=kaski-web::sound_gate) | binary_id(=kaski-web))"
        )
    );
}
/// The report is where the config says, whatever target directory or profile
/// the build was told: nextest keeps its own store beside the workspace, not
/// under them, which is why the path is named rather than derived.
#[test]
fn the_report_is_a_file_of_ours_that_the_config_names() {
    let state = Path::new("/w/mock/target/mockspace-test-state");
    let junit = junit_path(state);
    assert_eq!(junit, PathBuf::from("/w/mock/target/mockspace-test-state/junit.xml"));
    let t = tool_config(&TestSettings::default(), &[], &junit);
    assert!(
        t.contains("[profile.default.junit]\npath = \"/w/mock/target/mockspace-test-state/junit.xml\"\n"),
        "{t}"
    );
}

/// Without this nextest writes `<system-err>` for failures only, so a passing
/// test's work line never reaches the report.
#[test]
fn a_passing_tests_output_is_kept_so_its_work_line_reaches_the_report() {
    let t = tool_config(&TestSettings::default(), &[], Path::new("junit.xml"));
    let junit = t.split("[profile.default.junit]").nth(1).expect("a junit table");
    assert!(junit.lines().any(|l| l == "store-success-output = true"), "{t}");
}

/// Whatever the path holds, the config stays TOML that says that path.
#[test]
fn a_path_with_quotes_and_backslashes_is_escaped_into_the_config() {
    let odd = Path::new("/w/it's \"mine\"/back\\slash/junit.xml");
    let t = tool_config(&TestSettings::default(), &[], odd);
    let doc = t.parse::<toml_edit::DocumentMut>().unwrap_or_else(|e| panic!("{e}\n{t}"));
    assert_eq!(
        doc["profile"]["default"]["junit"]["path"].as_str(),
        Some(odd.to_str().unwrap())
    );
}

#[test]
fn only_success_and_test_failures_ran_to_the_end() {
    assert!(ran_to_the_end(Some(0)));
    assert!(ran_to_the_end(Some(100)));
    assert!(!ran_to_the_end(Some(101)));
    assert!(!ran_to_the_end(Some(1)));
    assert!(!ran_to_the_end(None));
}
