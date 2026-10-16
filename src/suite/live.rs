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

/// What nextest lists for these packages, run as the tests themselves will be.
/// `None` when it cannot say (a build that fails, say), and then nothing is
/// dropped: the run reports its own trouble.
pub fn list(mock_dir: &Path, packages: &[String], passed: &[String]) -> Option<Live> {
    let mut cmd =
        crate::entry::cargo_gate::cargo(mock_dir, &["nextest", "list", "--message-format", "json"]);
    for p in packages {
        cmd.arg("-p").arg(p);
    }
    cmd.args(passed);
    let out = cmd.stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
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

    #[test]
    fn what_is_not_a_listing_is_nothing_known() {
        assert_eq!(parse("not json"), None);
        assert_eq!(parse(r#"{"test-count":0}"#), None);
    }
}
