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


/// A work count a test reported on its stderr: what it did, so a timing can be
/// read per unit of work rather than as plain seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub binary: String,
    pub name:   String,
    /// What was counted, such as `ticks` or `beings`.
    pub unit:   String,
    pub n:      u64,
}

/// The work counts in a report, one per test that printed a well-formed
/// `<marker> <unit> <n>` line.
///
/// The marker is the caller's, so the scanner names no project. A line whose
/// count is not a number is ignored, and the test's case is still returned by
/// [`parse`]: a bad work line costs the count, never the timing. A test that
/// prints the line twice reports its last one.
pub fn work_lines(xml: &str, marker: &str) -> Vec<Work> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<testcase") {
        rest = &rest[at + "<testcase".len() ..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let tag = &rest[.. tag_end];
        let body = if tag.ends_with('/') {
            ""
        } else {
            &rest[tag_end .. rest.find("</testcase>").unwrap_or(rest.len())]
        };
        // A flaky pass nests the failed attempts' stderr inside `<flakyFailure>` ahead of
        // the testcase's own, so the last `<system-err>` is the passing attempt's. A test
        // that fails every retry has the first attempt's stderr and time at testcase
        // level, which is still a consistent pair.
        let found = body.rfind("<system-err>").map(|s| {
            let err = &body[s + "<system-err>".len() ..];
            unescape(&err[.. err.find("</system-err>").unwrap_or(err.len())])
        });
        let last = found.as_deref().and_then(|err| {
            // The last line carrying the marker stands; if it is malformed the
            // test has no count, rather than an older line's.
            let line = err.lines().rev().find(|l| l.split_whitespace().next() == Some(marker))?;
            let mut w = line.split_whitespace().skip(1);
            let unit = w.next()?.to_string();
            let n = w.next()?.parse().ok()?;
            w.next().is_none().then_some((unit, n))
        });
        if let (Some((unit, n)), Some(name), Some(binary)) =
            (last, attr(tag, "name"), attr(tag, "classname"))
        {
            out.push(Work { binary, name, unit, n });
        }
        rest = &rest[tag_end ..];
    }
    out
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

    const WORKED: &str = r#"<testsuites>
    <testsuite name="sim" tests="4">
        <testcase name="walks" classname="sim" time="3.250">
            <system-out>PROJ-WORK ignored_in_out 9</system-out>
            <system-err>noise
PROJ-WORK ticks 1200
</system-err>
        </testcase>
        <testcase name="bad_count" classname="sim" time="0.500">
            <system-err>PROJ-WORK ticks abc</system-err>
        </testcase>
        <testcase name="plain" classname="sim" time="0.100"/>
        <testcase name="twice" classname="sim" time="1.000">
            <system-err>PROJ-WORK beings 5
PROJ-WORK beings 7 &amp; more</system-err>
        </testcase>
    </testsuite>
</testsuites>"#;

    #[test]
    fn a_work_line_on_stderr_gives_the_count_for_its_test() {
        let w = work_lines(WORKED, "PROJ-WORK");
        assert_eq!(w, vec![Work {
            binary: "sim".into(),
            name:   "walks".into(),
            unit:   "ticks".into(),
            n:      1200,
        }]);
    }

    /// `PROJ-WORK ticks abc` yields no count, and the case is still read, so
    /// the sample is recorded with plain seconds.
    #[test]
    fn a_work_line_that_is_not_a_number_loses_the_count_and_never_the_timing() {
        assert!(work_lines(WORKED, "PROJ-WORK").iter().all(|w| w.name != "bad_count"));
        let cases = parse(WORKED);
        let bad = cases.iter().find(|c| c.name == "bad_count").unwrap();
        assert_eq!(bad.secs, 0.5);
        assert!(cases.iter().any(|c| c.name == "plain" && c.secs == 0.1));
    }

    /// Stdout is not the channel, a line with extra words is not the shape,
    /// and another marker is another project's.
    #[test]
    fn only_the_exact_shape_on_stderr_under_the_callers_marker_counts() {
        let names = |m: &str| -> Vec<String> {
            work_lines(WORKED, m).into_iter().map(|w| w.name).collect()
        };
        assert_eq!(names("OTHER-WORK"), Vec::<String>::new());
        assert!(!names("PROJ-WORK").contains(&"twice".to_string()));
    }

    /// nextest nests the earlier attempts' stderr inside `<flakyFailure>` ahead of
    /// the testcase's own, and the count belongs to the last attempt.
    #[test]
    fn a_flaky_tests_count_is_the_final_attempts_not_the_first() {
        let xml = r#"<testsuites><testsuite name="s">
        <testcase name="flaky" classname="s" time="2.000">
            <flakyFailure message="x" type="test failure">
                <system-err>PROJ-WORK ticks 1</system-err>
            </flakyFailure>
            <system-err>PROJ-WORK ticks 500</system-err>
        </testcase>
        </testsuite></testsuites>"#;
        let w = work_lines(xml, "PROJ-WORK");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].n, 500);
    }
}
