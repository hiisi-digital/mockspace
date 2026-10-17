//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! The `[test]` table of `mockspace.toml`.
//!
//! Every number here is a setting with a default rather than a constant, since
//! what counts as slow and how many slow tests a machine can take at once are
//! properties of the repository and the machine, not of mockspace.

use std::collections::BTreeMap;

use mockspace_lint_rules::Level;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct TestSettings {
    /// A test whose last run took at least this many seconds is heavy: it runs
    /// in the heavy group, and its passes are cached.
    pub heavy_after_secs: f64,
    /// How many heavy tests run at once.
    pub heavy_threads:    u32,
    /// A test still running after this many seconds is killed and failed.
    pub timeout_secs:     u64,
    /// The commit gate, read as a lint's level is: `off` (the default) and
    /// `info` run nothing, `warn` runs `mock test --cheap` and reports a
    /// failure without blocking, `error` runs it and blocks on a failure.
    pub commit:           Level,
    /// The opt level each named member is built at under `mock test`.
    pub opt_level:        BTreeMap<String, OptLevel>,
}

/// An opt level as cargo reads it: `0` to `3`, `s` or `z`. Parsed once where
/// the table is read, so nothing downstream holds a string it has to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    Zero,
    One,
    Two,
    Three,
    Size,
    SizeMin,
}

impl OptLevel {
    /// Whether cargo spells this level as a string rather than a number.
    pub fn is_word(self) -> bool {
        matches!(self, Self::Size | Self::SizeMin)
    }
}

impl std::fmt::Display for OptLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Zero => "0",
            Self::One => "1",
            Self::Two => "2",
            Self::Three => "3",
            Self::Size => "s",
            Self::SizeMin => "z",
        })
    }
}

impl std::str::FromStr for OptLevel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Ok(match s {
            "0" => Self::Zero,
            "1" => Self::One,
            "2" => Self::Two,
            "3" => Self::Three,
            "s" => Self::Size,
            "z" => Self::SizeMin,
            _ => return Err(()),
        })
    }
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

impl TestSettings {
    /// Whether a commit runs the tests: at `warn` or `error`, never below.
    pub fn commit_runs(&self) -> bool {
        self.commit >= Level::Warn
    }

    /// Whether a failure at the commit blocks it.
    pub fn commit_blocks(&self) -> bool {
        self.commit == Level::Error
    }
}

impl Default for TestSettings {
    fn default() -> Self {
        Self {
            heavy_after_secs: DEFAULT_HEAVY_AFTER_SECS,
            heavy_threads:    DEFAULT_HEAVY_THREADS,
            timeout_secs:     DEFAULT_TIMEOUT_SECS,
            commit:           Level::Pass,
            opt_level:        BTreeMap::new(),
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
    commit:           Option<String>,
    opt_level:        BTreeMap<String, RawOpt>,
}

/// An opt level as written: cargo's own spelling, a number or `"s"` or `"z"`.
#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(serde::Serialize))]
#[serde(untagged)]
pub(crate) enum RawOpt {
    Number(i64),
    Word(String),
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
        let commit = match self.commit.as_deref() {
            None => d.commit,
            Some(s) => {
                Level::from_str_name(s).ok_or_else(|| {
                    format!("[test] commit is a lint level, off, info, warn or error; got {s:?}")
                })?
            },
        };
        let mut opt_level = BTreeMap::new();
        for (member, raw) in self.opt_level {
            let level = match &raw {
                RawOpt::Number(n) => n.to_string(),
                RawOpt::Word(w) => w.clone(),
            };
            let level = level.parse::<OptLevel>().map_err(|()| {
                format!("[test.opt_level] {member} is 0, 1, 2, 3, \"s\" or \"z\"; got {level:?}")
            })?;
            opt_level.insert(member, level);
        }
        Ok(TestSettings {
            heavy_after_secs,
            heavy_threads,
            timeout_secs,
            commit,
            opt_level,
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
            "heavy_after_secs = 2.5\nheavy_threads = 2\ntimeout_secs = 60\ncommit = \"warn\"\n",
        )
        .unwrap();
        assert_eq!(s.heavy_after_secs, 2.5);
        assert_eq!(s.heavy_threads, 2);
        assert_eq!(s.timeout_secs, 60);
        assert_eq!(s.commit, Level::Warn);
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
        assert!(parse("commit = \"always\"\n").is_err());
    }

    /// The commit gate reads as a lint's does: only `warn` and `error` run
    /// anything, and only `error` blocks.
    #[test]
    fn only_warn_and_error_run_at_the_commit_and_only_error_blocks() {
        for (level, runs, blocks) in [
            ("off", false, false),
            ("pass", false, false),
            ("info", false, false),
            ("warn", true, false),
            ("error", true, true),
        ] {
            let s = parse(&format!("commit = \"{level}\"\n")).unwrap();
            assert_eq!(
                (s.commit_runs(), s.commit_blocks()),
                (runs, blocks),
                "{level}"
            );
        }
        assert!(
            !TestSettings::default().commit_runs(),
            "the default runs nothing"
        );
    }

    /// Cargo reads `0` to `3`, `"s"` and `"z"`; the table keeps them as cargo
    /// spells them, so a number and its string are one level.
    #[test]
    fn a_member_carries_its_opt_level() {
        let s = parse("[opt_level]\nkaski-village = 3\nkaski-render = \"s\"\nkaski-web = \"1\"\n")
            .unwrap();
        let want: BTreeMap<String, OptLevel> = [
            ("kaski-village", OptLevel::Three),
            ("kaski-render", OptLevel::Size),
            ("kaski-web", OptLevel::One),
        ]
        .map(|(k, v)| (k.to_string(), v))
        .into();
        assert_eq!(s.opt_level, want);
        assert!(parse("").unwrap().opt_level.is_empty());
    }

    #[test]
    fn an_opt_level_prints_as_cargo_spells_it() {
        for text in ["0", "1", "2", "3", "s", "z"] {
            let level: OptLevel = text.parse().unwrap();
            assert_eq!(level.to_string(), text);
            assert_eq!(level.is_word(), text == "s" || text == "z");
        }
        for bad in ["", "4", "S", "-1", "3\"\n[x]"] {
            assert!(bad.parse::<OptLevel>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn an_opt_level_cargo_would_refuse_is_refused_with_its_member() {
        for bad in ["4", "-1", "\"fast\"", "\"\""] {
            let e = parse(&format!("[opt_level]\nkaski-village = {bad}\n")).unwrap_err();
            assert!(e.contains("kaski-village"), "{bad}: {e}");
        }
    }

    /// A misspelt key is a setting that silently does nothing, which is the
    /// failure a settings table exists to avoid.
    #[test]
    fn an_unknown_key_is_refused() {
        assert!(parse("heavy_after = 3\n").is_err());
    }
}
