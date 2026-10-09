//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! What each test did, read back from the JUnit report nextest writes.
//!
//! JUnit is nextest's stable machine-readable result; its libtest-json output
//! is still marked experimental. The report's shape is fixed and small, one
//! `<testcase name classname time>` per test that ran, with a `<failure>`,
//! `<error>` or `<skipped>` child when it did not pass, so it is read with a
//! scanner over that shape rather than a general XML parser.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Fail,
    /// Ignored, or otherwise reported as not run.
    Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// The test binary, as nextest names it: the package for its library's
    /// unit tests, `package::name` for an integration test or a binary.
    pub binary:  String,
    pub name:    String,
    pub secs:    f64,
    pub outcome: Outcome,
}

impl Case {
    /// The package a binary belongs to.
    pub fn package(&self) -> &str {
        package_of(&self.binary)
    }
}

pub fn package_of(binary_id: &str) -> &str {
    binary_id.split("::").next().unwrap_or(binary_id)
}

pub fn parse(xml: &str) -> Vec<Case> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<testcase") {
        rest = &rest[at + "<testcase".len() ..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let tag = &rest[.. tag_end];
        let self_closing = tag.ends_with('/');
        let body = if self_closing {
            ""
        } else {
            let close = rest.find("</testcase>").unwrap_or(rest.len());
            &rest[tag_end .. close]
        };
        let outcome = if body.contains("<failure") || body.contains("<error") {
            Outcome::Fail
        } else if body.contains("<skipped") {
            Outcome::Skipped
        } else {
            Outcome::Pass
        };
        if let (Some(name), Some(binary)) = (attr(tag, "name"), attr(tag, "classname")) {
            out.push(Case {
                binary,
                name,
                secs: attr(tag, "time")
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0.0),
                outcome,
            });
        }
        rest = &rest[tag_end ..];
    }
    out
}

/// One attribute's value, unescaped.
fn attr(tag: &str, key: &str) -> Option<String> {
    let needle = format!(" {key}=\"");
    let start = tag.find(&needle)? + needle.len();
    let len = tag[start ..].find('"')?;
    Some(unescape(&tag[start .. start + len]))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cut from what nextest 0.9.148 wrote for a four-test crate with one
    /// failure, the failure's backtrace shortened.
    const REPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="4" skipped="1" failures="1" errors="0">
    <testsuite name="nxprobe" tests="3" skipped="0" errors="0" failures="1">
        <testcase name="tests::quick" classname="nxprobe" timestamp="2026-10-09T14:13:04.129+00:00" time="0.012"/>
        <testcase name="tests::fails" classname="nxprobe" timestamp="2026-10-09T14:13:04.129+00:00" time="0.208">
            <failure message="thread &apos;tests::fails&apos; panicked at src/lib.rs:6:26" type="test failure with exit code 101">thread &apos;tests::fails&apos; panicked
&lt;fn() as FnOnce&gt;::call_once</failure>
            <system-out></system-out>
        </testcase>
        <testcase name="tests::slow_one" classname="nxprobe" timestamp="2026-10-09T14:13:04.130+00:00" time="1.512">
            <system-out>a &lt;testcase name=&quot;not_a_case&quot;&gt; printed by the test</system-out>
        </testcase>
    </testsuite>
    <testsuite name="nxprobe::it" tests="1" skipped="1" errors="0" failures="0">
        <testcase name="ignored_one" classname="nxprobe::it" time="0.000">
            <skipped/>
        </testcase>
    </testsuite>
</testsuites>"#;

    #[test]
    fn every_case_with_its_time_and_outcome() {
        let cases = parse(REPORT);
        let got: Vec<(&str, &str, f64, Outcome)> = cases
            .iter()
            .map(|c| (c.binary.as_str(), c.name.as_str(), c.secs, c.outcome))
            .collect();
        assert_eq!(got, vec![
            ("nxprobe", "tests::quick", 0.012, Outcome::Pass),
            ("nxprobe", "tests::fails", 0.208, Outcome::Fail),
            ("nxprobe", "tests::slow_one", 1.512, Outcome::Pass),
            ("nxprobe::it", "ignored_one", 0.0, Outcome::Skipped),
        ]);
    }

    /// A test printing something shaped like a case is not one: its output is
    /// escaped inside `<system-out>`, so no `<testcase` appears in it.
    #[test]
    fn a_tests_own_output_is_not_read_as_a_case() {
        assert!(parse(REPORT).iter().all(|c| c.name != "not_a_case"));
    }

    #[test]
    fn a_binary_belongs_to_the_package_before_its_separator() {
        assert_eq!(package_of("kaski-render"), "kaski-render");
        assert_eq!(package_of("kaski-web::drawn"), "kaski-web");
        assert_eq!(package_of("kaski-web::bin/page"), "kaski-web");
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert!(parse("").is_empty());
        assert!(parse("<testsuites></testsuites>").is_empty());
    }
}
