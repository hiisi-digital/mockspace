//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `[test]` table of `mockspace.toml`.
//!
//! Every number here is a setting with a default rather than a constant, since
//! what counts as slow and how many slow tests a machine can take at once are
//! properties of the repository and the machine, not of mockspace.

use serde::Deserialize;

/// What a commit runs before it is linted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnCommit {
    /// Nothing. The default: a commit hook that starts a suite is a choice a
    /// repository makes, not one mockspace makes for it.
    Off,
    /// `mock test --cheap`: the crates a change reaches, without the heavy tests
    /// that are not new.
    Cheap,
    /// `mock test`: the crates a change reaches, heavy tests included unless
    /// cached.
    Changed,
}

impl OnCommit {
    /// The `mock test` arguments a commit runs, or nothing.
    pub fn args(self) -> Option<&'static [&'static str]> {
        match self {
            OnCommit::Off => None,
            OnCommit::Cheap => Some(&["--cheap"]),
            OnCommit::Changed => Some(&[]),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestSettings {
    /// A test whose last run took at least this many seconds is heavy: it runs
    /// in the heavy group, and its passes are cached.
    pub heavy_after_secs: f64,
    /// How many heavy tests run at once.
    pub heavy_threads:    u32,
    /// A test still running after this many seconds is killed and failed.
    pub timeout_secs:     u64,
    pub on_commit:        OnCommit,
}

/// Ten seconds: kaski's drawn tests start a Bevy app on a software Vulkan
/// device and op saw one take about 160 seconds alone, while a test drawing
/// nothing takes well under a second. A stand-in, since no per-test
/// distribution has been taken yet; `research/202610091400_suite-selection.md`
/// says how to take one.
pub const DEFAULT_HEAVY_AFTER_SECS: f64 = 10.0;
/// One at a time. A software Vulkan device already spreads one frame over every
/// core, so a second heavy test beside it mostly contends.
pub const DEFAULT_HEAVY_THREADS: u32 = 1;
/// Fifteen minutes: about five times the slowest drawn test kaski has run
/// alone, so a test is killed for hanging rather than for being slow.
pub const DEFAULT_TIMEOUT_SECS: u64 = 900;

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            heavy_after_secs: DEFAULT_HEAVY_AFTER_SECS,
            heavy_threads:    DEFAULT_HEAVY_THREADS,
            timeout_secs:     DEFAULT_TIMEOUT_SECS,
            on_commit:        OnCommit::Off,
        }
    }
}

/// The table as written. Every key optional, an absent table is the defaults.
#[derive(Debug, Default, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(default, deny_unknown_fields)]
pub(crate) struct RawTest {
    heavy_after_secs: Option<f64>,
    heavy_threads:    Option<u32>,
    timeout_secs:     Option<u64>,
    on_commit:        Option<String>,
}

impl RawTest {
    /// The settings, or what is wrong with the table.
    ///
    /// A value that cannot mean anything is refused rather than clamped: a
    /// zero thread count would never run a heavy test, and a zero timeout would
    /// kill every test, and either would read as a broken suite.
    pub(crate) fn resolve(self) -> Result<TestSettings, String> {
        let d = TestSettings::default();
        let heavy_after_secs = self.heavy_after_secs.unwrap_or(d.heavy_after_secs);
        if !(heavy_after_secs.is_finite() && heavy_after_secs > 0.0) {
            return Err(format!(
                "[test] heavy_after_secs must be a positive number of seconds, got {heavy_after_secs}"
            ));
        }
        let heavy_threads = self.heavy_threads.unwrap_or(d.heavy_threads);
        if heavy_threads == 0 {
            return Err("[test] heavy_threads must be at least 1".to_string());
        }
        let timeout_secs = self.timeout_secs.unwrap_or(d.timeout_secs);
        if timeout_secs == 0 {
            return Err("[test] timeout_secs must be at least 1".to_string());
        }
        let on_commit = match self.on_commit.as_deref() {
            None | Some("off") => OnCommit::Off,
            Some("cheap") => OnCommit::Cheap,
            Some("changed") => OnCommit::Changed,
            Some(other) => {
                return Err(format!(
                    "[test] on_commit is one of off, cheap, changed; got {other:?}"
                ));
            },
        };
        Ok(TestSettings {
            heavy_after_secs,
            heavy_threads,
            timeout_secs,
            on_commit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<TestSettings, String> {
        let raw: RawTest = toml_edit::de::from_str(text).map_err(|e| e.to_string())?;
        raw.resolve()
    }

    #[test]
    fn an_empty_table_is_the_defaults() {
        assert_eq!(parse("").unwrap(), TestSettings::default());
    }

    #[test]
    fn each_key_overrides_its_default_alone() {
        let s = parse(
            "heavy_after_secs = 2.5\nheavy_threads = 2\ntimeout_secs = 60\non_commit = \"cheap\"\n",
        )
        .unwrap();
        assert_eq!(s.heavy_after_secs, 2.5);
        assert_eq!(s.heavy_threads, 2);
        assert_eq!(s.timeout_secs, 60);
        assert_eq!(s.on_commit, OnCommit::Cheap);
    }

    /// An integer is a number of seconds too; a table written `= 10` must not
    /// be refused for want of `.0`.
    #[test]
    fn a_whole_number_threshold_is_accepted() {
        assert_eq!(
            parse("heavy_after_secs = 10\n").unwrap().heavy_after_secs,
            10.0
        );
    }

    #[test]
    fn values_that_cannot_mean_anything_are_refused() {
        assert!(parse("heavy_after_secs = 0\n").is_err());
        assert!(parse("heavy_after_secs = -1\n").is_err());
        assert!(parse("heavy_threads = 0\n").is_err());
        assert!(parse("timeout_secs = 0\n").is_err());
        assert!(parse("on_commit = \"always\"\n").is_err());
    }

    /// A misspelt key is a setting that silently does nothing, which is the
    /// failure a settings table exists to avoid.
    #[test]
    fn an_unknown_key_is_refused() {
        assert!(parse("heavy_after = 3\n").is_err());
    }
}
