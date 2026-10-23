//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! How a plan becomes a `cargo nextest run`.
//!
//! The settings reach nextest as a tool config file, written fresh for every
//! run under the mock directory's `target/` and passed with
//! `--tool-config-file mockspace:<path>`. That is nextest's own mechanism for a
//! tool wrapping it: the tool's file sits below the repository's
//! `.config/nextest.toml`, so anything a repository configures for itself
//! still wins, and above nextest's defaults.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::select::Skip;
use super::settings::TestSettings;

/// The group the heavy tests run in. Nextest requires a group a tool defines
/// to be named `@tool:<tool>:<name>`.
pub const HEAVY_GROUP: &str = "@tool:mockspace:heavy";

/// Whether `cargo nextest` answers here.
pub fn installed(mock_dir: &Path) -> bool {
    crate::entry::cargo_gate::cargo(mock_dir, &["nextest", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The tool config for one run.
pub fn tool_config(s: &TestSettings, heavy: &[(String, String)], junit: &Path) -> String {
    // The slow period is how often nextest reports a test still running; the
    // timeout is that period times a whole count, so the period is shrunk to
    // fit a timeout shorter than a minute.
    let period = s.timeout_secs.min(60);
    let periods = s.timeout_secs.div_ceil(period);
    let mut t = String::new();
    t.push_str(
        "# Written by `cargo mock test` for each run, from `[test]` in mockspace.toml\n\
         # and the timings of earlier runs. Edits here are lost; the repository's own\n\
         # .config/nextest.toml wins over anything set here.\n\n",
    );
    let _ = writeln!(t, "[test-groups.\"{HEAVY_GROUP}\"]");
    let _ = writeln!(t, "max-threads = {}\n", s.heavy_threads);
    t.push_str("[profile.default]\n");
    let _ = writeln!(
        t,
        "slow-timeout = {{ period = \"{period}s\", terminate-after = {periods} }}"
    );
    t.push_str("fail-fast = false\n\n");
    // Quoted and escaped as TOML, whatever characters the path holds.
    let _ = writeln!(
        t,
        "[profile.default.junit]\npath = {}",
        toml_edit::Value::from(junit.to_string_lossy().as_ref())
    );
    if !heavy.is_empty() {
        t.push_str("\n[[profile.default.overrides]]\n");
        let _ = writeln!(t, "filter = '{}'", any_of(heavy));
        let _ = writeln!(t, "test-group = \"{HEAVY_GROUP}\"");
    }
    t
}

/// The cargo profile a run builds, which is where a member's opt level goes:
/// `release` for `--release`, the profile named by `profile_flag`, else `dev`.
/// The flag is `--cargo-profile` for nextest and `--profile` for `cargo test`.
pub fn build_profile(passed: &[String], profile_flag: &str) -> String {
    let mut it = passed.iter();
    while let Some(a) = it.next() {
        if a == "--release" || a == "-r" {
            return "release".to_string();
        }
        if a == profile_flag {
            if let Some(p) = it.next() {
                return p.clone();
            }
        } else if let Some(p) = a
            .strip_prefix(profile_flag)
            .and_then(|r| r.strip_prefix('='))
        {
            return p.to_string();
        }
    }
    "dev".to_string()
}

/// A filterset matching exactly these tests.
fn any_of<'a>(tests: impl IntoIterator<Item = &'a (String, String)>) -> String {
    tests
        .into_iter()
        .map(|(bin, name)| format!("(binary_id(={bin}) & test(={name}))"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The `-E` for a run: the caller's own filter, less the tests skipped and the
/// binaries left out whole.
pub fn filterset(caller: Option<&str>, skip: &[Skip], reuse: &[String]) -> Option<String> {
    let skipped: Vec<(String, String)> = skip
        .iter()
        .map(|s| (s.binary.clone(), s.name.clone()))
        .collect();
    let mut left_out: Vec<String> = Vec::new();
    if !skipped.is_empty() {
        left_out.push(any_of(&skipped));
    }
    left_out.extend(reuse.iter().map(|b| format!("binary_id(={b})")));
    let not_skipped = (!left_out.is_empty()).then(|| format!("not ({})", left_out.join(" | ")));
    match (caller, not_skipped) {
        (None, None) => None,
        (Some(c), None) => Some(c.to_string()),
        (None, Some(n)) => Some(n),
        (Some(c), Some(n)) => Some(format!("({c}) & {n}")),
    }
}

/// Where nextest writes the report for a run: a file of ours beside the tool
/// config, named in that config by its absolute path.
///
/// Not left to nextest's own default. Its store directory is `target/nextest`
/// under the workspace and not under `CARGO_TARGET_DIR` or `--target-dir`
/// (nextest 0.9.148, measured), so a report looked for under the target
/// directory the build chose was never there on a machine that shares one, and
/// the run recorded nothing: no timings and no binary that passed. An absolute
/// path in the config is where it goes whatever else is set.
pub fn junit_path(state: &Path) -> PathBuf {
    state.join("junit.xml")
}

pub struct RunSpec<'a> {
    pub mock_dir:     &'a Path,
    pub tool_config:  &'a Path,
    /// The generated cargo config, where a member has an opt level.
    pub cargo_config: Option<&'a Path>,
    pub packages:     &'a [String],
    pub passed:       &'a [String],
    pub filterset:    Option<&'a str>,
    pub test_args:    &'a [String],
}

pub fn command(r: &RunSpec) -> Command {
    let mut cmd = crate::entry::cargo_gate::cargo(r.mock_dir, &["nextest", "run"]);
    cmd.arg("--tool-config-file")
        .arg(format!("mockspace:{}", r.tool_config.display()))
        .arg("--no-fail-fast")
        // A member whose every test is skipped as cached runs nothing, and that
        // is a result rather than an error.
        .arg("--no-tests=warn");
    for p in r.packages {
        cmd.arg("-p").arg(p);
    }
    // Before the caller's own flags, so a `--config` of theirs comes later and
    // wins.
    if let Some(c) = r.cargo_config {
        cmd.arg("--config").arg(c);
    }
    cmd.args(r.passed);
    if let Some(f) = r.filterset {
        cmd.arg("-E").arg(f);
    }
    if !r.test_args.is_empty() {
        cmd.arg("--").args(r.test_args);
    }
    cmd
}

/// Nextest's exit codes that mean every selected test ran: success, and the
/// run finishing with failures. Anything else, a build failure, a setup
/// script, an interrupt, stopped it before the end.
pub fn ran_to_the_end(code: Option<i32>) -> bool {
    // 100 is `TEST_RUN_FAILED` in nextest's `NextestExitCode`.
    matches!(code, Some(0) | Some(100))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::suite::select::SkipWhy;

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
        assert!(t.contains(
            "filter = '(binary_id(=kaski-render) & test(=tests::drawn::a)) | (binary_id(=kaski-web::it) & test(=b))'"
        ), "{t}");
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
        assert_eq!(filterset(None, &[], &[]), None);
        assert_eq!(
            filterset(Some("test(x)"), &[], &[]).as_deref(),
            Some("test(x)")
        );
        assert_eq!(filterset(None, &skip, &[]).as_deref(), Some(not));
        assert_eq!(
            filterset(Some("test(x)"), &skip, &[]),
            Some(format!("(test(x)) & {not}"))
        );
    }

    #[test]
    fn the_filterset_leaves_out_whole_binaries_beside_single_tests() {
        let reuse = vec!["kaski-web::sound_gate".to_string(), "kaski-web".to_string()];
        assert_eq!(
            filterset(None, &[], &reuse).as_deref(),
            Some("not (binary_id(=kaski-web::sound_gate) | binary_id(=kaski-web))")
        );
        let skip = vec![Skip {
            binary: "kaski-render".to_string(),
            name:   "tests::drawn::a".to_string(),
            why:    SkipWhy::Cached,
        }];
        assert_eq!(
            filterset(Some("test(x)"), &skip, &reuse).as_deref(),
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
}
