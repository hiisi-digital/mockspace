//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Every test of a run, timed and spooled.
//!
//! The selection cache in [`super::history`] keeps only the heavy tests and
//! rewrites a timing when it moves a lot. This keeps every test's timing from
//! every run as its own row, in a shard per run, so a series builds up rather
//! than a last value. The row type is the bench harness's
//! ([`mockspace_bench_harness::test_sample`]), the context comes from its
//! [`mockspace_bench_harness::EnvMeta`], and nothing here decides anything: it
//! only records.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use mockspace_bench_harness::EnvMeta;
use mockspace_bench_harness::test_sample::{self, TestSample, VERSION};

use super::junit::{Case, Outcome, Work, package_of};

/// What one run knows about itself, the same for every row it writes.
pub struct RunFacts<'a> {
    pub env:            &'a EnvMeta,
    pub run:            &'a str,
    pub commit:         &'a str,
    pub flavour:        &'a str,
    pub profile:        &'a str,
    pub test_threads:   u64,
    pub load_1m_before: Option<f64>,
    pub load_1m_after:  Option<f64>,
}

/// The rows of a run: one per case, every case, at the precision JUnit gives.
/// A test with a work line carries its unit and count; one without carries
/// neither, and is read as plain seconds.
pub fn rows(
    facts: &RunFacts<'_>,
    cases: &[Case],
    work: &[Work],
    tree_fp: &dyn Fn(&str) -> Option<String>,
) -> Vec<TestSample> {
    let by_test: BTreeMap<(&str, &str), &Work> = work
        .iter()
        .map(|w| ((w.binary.as_str(), w.name.as_str()), w))
        .collect();
    cases
        .iter()
        .map(|c| {
            let w = by_test.get(&(c.binary.as_str(), c.name.as_str()));
            TestSample {
                v:              VERSION,
                ts:             facts.env.timestamp,
                run:            facts.run.to_string(),
                commit:         facts.commit.to_string(),
                tree_fp:        tree_fp(&c.binary).unwrap_or_default(),
                bin:            c.binary.clone(),
                test:           c.name.clone(),
                member:         package_of(&c.binary).to_string(),
                flavour:        facts.flavour.to_string(),
                secs:           c.secs,
                outcome:        match c.outcome {
                    Outcome::Pass => "pass",
                    Outcome::Fail => "fail",
                    Outcome::Skipped => "skipped",
                }
                .to_string(),
                machine_class:  facts.env.machine_class.clone(),
                profile:        facts.profile.to_string(),
                test_threads:   facts.test_threads,
                load_1m_before: facts.load_1m_before,
                load_1m_after:  facts.load_1m_after,
                cpus_online:    facts.env.cpus_online,
                work_unit:      w.map(|w| w.unit.clone()),
                work_n:         w.map(|w| w.n),
            }
        })
        .collect()
}

/// Write a run's rows as one new shard under `spool` and return its path.
///
/// The shard is written to a temp name beside it and hard-linked into place, so a reader never
/// sees half a shard, and a name already taken is an error rather than an
/// overwrite. Nothing is written for no rows.
pub fn spool(spool: &Path, rows: &[TestSample]) -> std::io::Result<Option<PathBuf>> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let path = test_sample::shard_path(spool, first.ts, &first.machine_class, &first.run);
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("shard {} already exists", path.display()),
        ));
    }
    let dir = path.parent().expect("a shard path has a parent");
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("ndjson.tmp");
    let written = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        for r in rows {
            serde_json::to_writer(&mut f, r).map_err(std::io::Error::other)?;
            f.write_all(b"\n")?;
        }
        f.sync_all()?;
        // A hard link fails when the name is taken, where a rename would replace it.
        std::fs::hard_link(&tmp, &path)
    })();
    let _ = std::fs::remove_file(&tmp);
    written?;
    Ok(Some(path))
}

/// A run id unique across machines of one class: the time to the nanosecond, the
/// process id and a hash of the working directory, since containers reuse low pids.
pub fn run_id(ts: u64) -> String {
    use std::hash::{Hash, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::env::current_dir().ok().hash(&mut h);
    std::process::id().hash(&mut h);
    format!("{ts}-{nanos:09}-{:08x}-{n}", h.finish() as u32)
}

/// The test threads a run used: nextest's `-j`, `--jobs` or `--test-threads` in any
/// of its spellings (`-j N`, `-jN`, `-j=N`, `--jobs N`, `--jobs=N`, and the same for
/// `--test-threads`), else `env` (`NEXTEST_TEST_THREADS`), else the cpus. A negative
/// count means the cpus less that many, and `num-cpus` the cpus.
pub fn test_threads(passed: &[String], env: Option<&str>, cpus: u64) -> u64 {
    let count = |v: &str| -> Option<u64> {
        let v = v.trim();
        if v == "num-cpus" {
            return Some(cpus.max(1));
        }
        match v.parse::<i64>().ok()? {
            0 => None,
            n if n > 0 => Some(n as u64),
            n => Some(cpus.saturating_sub(n.unsigned_abs()).max(1)),
        }
    };
    let mut it = passed.iter();
    while let Some(a) = it.next() {
        let value = match a.as_str() {
            "-j" | "--jobs" | "--test-threads" => it.next().map(String::as_str),
            _ => ["--test-threads=", "--jobs=", "-j=", "-j"]
                .iter()
                .find_map(|p| a.strip_prefix(p)),
        };
        if let Some(n) = value.and_then(count) {
            return n;
        }
    }
    env.and_then(count).unwrap_or(cpus)
}

/// Where the spool lives under the test state directory.
pub const SPOOL_DIR: &str = "timings-spool";

/// What a run feeds the spool with, besides its cases.
pub struct Run<'a> {
    pub state:       &'a Path,
    pub flavour:     &'a str,
    pub profile:     &'a str,
    pub passed:      &'a [String],
    pub report:      &'a str,
    pub load_before: Option<f64>,
    pub threads_env: Option<&'a str>,
    pub commit:      &'a str,
    pub fingerprint: Option<&'a super::fingerprint::Fingerprints>,
}

/// Time and spool a finished run: every case of the report, with the machine
/// class, the load before and after, and any work lines. A failure to write is
/// a note on stderr and never the run's result.
pub fn record_run(run: &Run<'_>, cases: &[Case], work_marker: &str) {
    let env = mockspace_bench_harness::env::collect_run_env();
    let id = run_id(env.timestamp);
        let facts = RunFacts {
        env:            &env,
        run:            &id,
        commit:         run.commit,
        flavour:        run.flavour,
        profile:        run.profile,
        test_threads:   test_threads(run.passed, run.threads_env, env.cpus_online),
        load_1m_before: run.load_before,
        load_1m_after:  mockspace_bench_harness::env::load_1m(),
    };
    let work = super::junit::work_lines(run.report, work_marker);
    let fp = |bin: &str| run.fingerprint.and_then(|f| f.of_binary(bin).cloned());
    let rows = rows(&facts, cases, &work, &fp);
    if let Err(e) = spool(&run.state.join(SPOOL_DIR), &rows) {
        eprintln!("note: could not spool this run's test timings: {e}");
    }
}

#[cfg(test)]
#[path = "timings_tests.rs"]
mod tests;
