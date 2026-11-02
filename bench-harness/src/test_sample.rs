//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! One row per test per run: the test-side sibling of [`crate::Sample`].
//!
//! A bench [`crate::Sample`] is a per-batch record whose csv columns are a
//! contract between four readers, so a test's timing is not squeezed into it.
//! This row carries what a test run knows (the test, its binary, wall seconds,
//! outcome) with the context the bench side keeps in [`crate::EnvMeta`]
//! (machine class, load, cpus), so a bench and a test feed one analysis.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The row layout version. A reader refuses a row of a version it does not know.
pub const VERSION: u64 = 1;

/// One test's timing in one run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TestSample {
    pub v:              u64,
    /// Unix seconds at the run's end.
    pub ts:             u64,
    /// The run's id, unique per run on a machine class.
    pub run:            String,
    pub commit:         String,
    /// The test binary's fingerprint, empty when none could be taken.
    pub tree_fp:        String,
    pub bin:            String,
    pub test:           String,
    pub member:         String,
    pub flavour:        String,
    /// Wall seconds as the JUnit report gives them (milliseconds resolution).
    pub secs:           f64,
    /// `pass`, `fail` or `skipped`.
    pub outcome:        String,
    pub machine_class:  String,
    pub profile:        String,
    pub test_threads:   u64,
    pub load_1m_before: Option<f64>,
    pub load_1m_after:  Option<f64>,
    pub cpus_online:    u64,
    /// What the test counted, `None` when it printed no work line.
    pub work_unit:      Option<String>,
    pub work_n:         Option<u64>,
}

/// `yyyy-mm` of a unix timestamp, UTC.
#[must_use]
pub fn year_month(ts: u64) -> String {
    // Days since 1970-01-01 to a civil date (Hinnant's algorithm).
    let z = (ts / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

/// `<root>/samples/<yyyy-mm>/<machine-class>/<run>.ndjson`: one shard per run,
/// so two writers never write the same file.
#[must_use]
pub fn shard_path(root: &Path, ts: u64, machine_class: &str, run: &str) -> PathBuf {
    root.join("samples")
        .join(year_month(ts))
        .join(machine_class)
        .join(format!("{run}.ndjson"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_month_is_read_off_the_timestamp_in_utc() {
        assert_eq!(year_month(0), "1970-01");
        assert_eq!(year_month(1_798_761_599), "2026-12"); // 2026-12-31T23:59:59Z
        assert_eq!(year_month(1_798_761_600), "2027-01");
        assert_eq!(year_month(1_709_251_199), "2024-02"); // leap day's last second
    }

    #[test]
    fn a_shard_is_per_run_so_two_runs_never_share_a_file() {
        let a = shard_path(Path::new("/r"), 0, "c", "run-1");
        let b = shard_path(Path::new("/r"), 0, "c", "run-2");
        assert_ne!(a, b);
        assert_eq!(a, Path::new("/r/samples/1970-01/c/run-1.ndjson"));
    }
}
