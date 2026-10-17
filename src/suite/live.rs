//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The tests that exist now, as opposed to the ones the history remembers.
//!
//! History outlives the code it timed: a test is renamed, a binary is deleted.
//! Nextest rejects a config override naming a binary it does not have, so a
//! remembered test that is gone must not reach the generated config, and must
//! not be kept for the next run either.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Each test binary nextest knows, by binary id, with its test names.
pub type Live = BTreeMap<String, BTreeSet<String>>;

/// Reads `cargo nextest list --message-format json`.
pub fn parse(json: &str) -> Option<Live> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let suites = v.get("rust-suites")?.as_object()?;
    Some(
        suites
            .iter()
            .map(|(id, s)| {
                let tests = s
                    .get("testcases")
                    .and_then(|t| t.as_object())
                    .map(|t| t.keys().cloned().collect())
                    .unwrap_or_default();
                (id.clone(), tests)
            })
            .collect(),
    )
}

/// Flags that choose what is built or listed, and take no value.
const BUILD_BARE: &[&str] = &[
    "--all-features",
    "--no-default-features",
    "--release",
    "-r",
    "--locked",
    "--offline",
    "--frozen",
    "--lib",
    "--bins",
    "--tests",
    "--examples",
    "--benches",
    "--all-targets",
];

/// Flags that choose what is built or listed, and take a value.
const BUILD_VALUED: &[&str] = &[
    "--features",
    "-F",
    "-Z",
    "--cargo-profile",
    "--target",
    "--target-dir",
    "--manifest-path",
    "--config",
    "--bin",
    "--test",
    "--example",
    "--bench",
];

/// The short flags among those, which clap also reads with the value attached,
/// as `-Ffoo`.
const BUILD_ATTACHED: &[&str] = &["-F", "-Z"];

/// The part of what the caller passed that `nextest list` accepts: the build
/// and selection flags. The flags that only shape a run (`--no-capture`,
/// `--retries`, `-j`, `--no-fail-fast`, `--failure-output`, `--status-level`)
/// make it exit 2, so they stay out and are not listed one by one.
pub fn list_args(passed: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = passed.iter();
    while let Some(a) = it.next() {
        if BUILD_BARE.contains(&a.as_str()) {
            out.push(a.clone());
        } else if BUILD_VALUED.contains(&a.as_str()) {
            out.push(a.clone());
            out.extend(it.next().cloned());
        } else if a
            .split_once('=')
            .is_some_and(|(flag, _)| BUILD_VALUED.contains(&flag))
            || BUILD_ATTACHED
                .iter()
                .any(|f| a.strip_prefix(f).is_some_and(|v| !v.is_empty()))
        {
            out.push(a.clone());
        }
    }
    out
}

/// The arguments `nextest list` gets after its subcommand: the packages, the
/// generated cargo config, and the build flags the caller passed. The config
/// comes before the caller's flags, so a `--config` of theirs wins, and it is
/// what makes the listing build as the run will.
pub fn list_command_args(
    packages: &[String],
    passed: &[String],
    cargo_config: Option<&Path>,
) -> Vec<String> {
    let mut out = Vec::new();
    for p in packages {
        out.extend(["-p".to_string(), p.clone()]);
    }
    if let Some(c) = cargo_config {
        out.extend(["--config".to_string(), c.display().to_string()]);
    }
    out.extend(list_args(passed));
    out
}

/// What nextest lists for these packages, built as the tests themselves will
/// be. `None` when it cannot say (a build that fails, say), and then nothing
/// is dropped: the run reports its own trouble. Its stderr is the user's to
/// read, the build's progress and, when it refuses, why.
pub fn list(
    mock_dir: &Path,
    packages: &[String],
    passed: &[String],
    cargo_config: Option<&Path>,
) -> Option<Live> {
    let mut cmd =
        crate::entry::cargo_gate::cargo(mock_dir, &["nextest", "list", "--message-format", "json"]);
    cmd.args(list_command_args(packages, passed, cargo_config));
    let out = match cmd.stderr(std::process::Stdio::inherit()).output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("note: could not ask nextest which tests exist: {e}; none are dropped");
            return None;
        },
    };
    if !out.status.success() {
        eprintln!(
            "note: `cargo nextest list` failed ({}); tests that are gone are not dropped this time",
            out.status
        );
        return None;
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_listing_gives_each_binary_its_tests() {
        let json = r#"{"test-count":3,"rust-suites":{
            "kaski-village":{"binary-id":"kaski-village","testcases":{"a":{},"b":{}}},
            "kaski-village::it":{"binary-id":"kaski-village::it","testcases":{"c":{}}},
            "kaski-village::example/empty":{"binary-id":"kaski-village::example/empty","testcases":{}}}}"#;
        let live = parse(json).unwrap();
        assert_eq!(live.len(), 3);
        assert_eq!(
            live["kaski-village"],
            BTreeSet::from(["a".to_string(), "b".to_string()])
        );
        assert!(live["kaski-village::example/empty"].is_empty());
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn listing_is_given_the_build_flags_and_none_that_only_shape_a_run() {
        let passed = words(
            "--features editor --release --no-capture --retries 2 -j 4 --test-threads 2 \
             --no-fail-fast --failure-output final --status-level fail --target-dir=out \
             --cargo-profile dev --locked",
        );
        assert_eq!(
            list_args(&passed),
            words("--features editor --release --target-dir=out --cargo-profile dev --locked")
        );
        assert!(list_args(&words("--no-capture --retries 2")).is_empty());
    }

    /// Each valued build flag, in every way clap reads a value: the next word,
    /// `=`, and for the short flags the value attached.
    #[test]
    fn every_separated_and_attached_form_of_a_valued_flag_is_listed() {
        for long in BUILD_VALUED.iter().filter(|f| f.starts_with("--")) {
            let sep = words(&format!("{long} v"));
            assert_eq!(list_args(&sep), sep, "{long} v");
            let eq = words(&format!("{long}=v"));
            assert_eq!(list_args(&eq), eq, "{long}=v");
        }
        for short in ["-F", "-Z"] {
            for form in [format!("{short} v"), format!("{short}=v"), format!("{short}v")] {
                let passed = words(&form);
                assert_eq!(list_args(&passed), passed, "{form}");
            }
        }
    }

    #[test]
    fn the_listing_builds_with_the_generated_config_before_the_callers_flags() {
        let config = Path::new("/w/mock/target/state/cargo.toml");
        let args = list_command_args(
            &words("a b"),
            &words("--config x=1 --release"),
            Some(config),
        );
        assert_eq!(
            args,
            words("-p a -p b --config /w/mock/target/state/cargo.toml --config x=1 --release")
        );
        assert_eq!(
            list_command_args(&words("a"), &words("--release"), None),
            words("-p a --release")
        );
    }

    #[test]
    fn an_attached_feature_does_not_swallow_the_word_after_it() {
        let passed = words("-Ffoo --no-capture -Zunstable-options --retries 2 --release");
        assert_eq!(
            list_args(&passed),
            words("-Ffoo -Zunstable-options --release")
        );
        let passed = words("-F foo --no-capture -Z unstable-options --retries 2");
        assert_eq!(list_args(&passed), words("-F foo -Z unstable-options"));
    }

    #[test]
    fn what_is_not_a_listing_is_nothing_known() {
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"test-count":0}"#), None);
    }
}
