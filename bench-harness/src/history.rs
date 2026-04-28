//! Historical trend tracking and regression detection.
//!
//! Append-only log of benchmark results: each run appends a record
//! with timestamp, git commit, variant, N, mode, median, CI bounds.
//! [`detect_regressions`] reads the log and flags entries where the
//! current CI does not overlap the historical baseline.

use std::io::{BufRead, Write};

use crate::error::BenchError;

const HISTORY_DIR: &str = ".bench_history";

/// One historical data point.
#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub git_commit: String,
    pub benchmark: String,
    pub variant: String,
    pub n: usize,
    pub mode: String,
    pub median_ns: f64,
    pub ci_lo_ns: f64,
    pub ci_hi_ns: f64,
}

const SCHEMA_HEADER: &str =
    "# schema_v1\ttimestamp\tgit_commit\tbenchmark\tvariant\tn\tmode\tmedian_ns\tci_lo_ns\tci_hi_ns";

/// Append entries to the history log. Writes the schema header if
/// the file is new or empty.
pub fn append(benchmark: &str, entries: &[HistoryEntry]) -> Result<(), BenchError> {
    std::fs::create_dir_all(HISTORY_DIR)
        .map_err(|e| BenchError::io("creating history dir", e))?;
    let path = format!("{}/{}.tsv", HISTORY_DIR, benchmark);

    let is_new = std::fs::metadata(&path).map(|m| m.len() == 0).unwrap_or(true);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| BenchError::io("opening history file", e))?;

    if is_new {
        writeln!(file, "{}", SCHEMA_HEADER)
            .map_err(|e| BenchError::io("writing history schema header", e))?;
    }

    for e in entries {
        writeln!(
            file,
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{:.1}\t{:.1}",
            e.timestamp, e.git_commit, e.benchmark, e.variant,
            e.n, e.mode, e.median_ns, e.ci_lo_ns, e.ci_hi_ns
        )
        .map_err(|e| BenchError::io("writing history entry", e))?;
    }
    Ok(())
}

/// Load all history for a benchmark. Missing log file yields an
/// empty vector (not an error). Comment / header lines are skipped.
pub fn load(benchmark: &str) -> Vec<HistoryEntry> {
    let path = format!("{}/{}.tsv", HISTORY_DIR, benchmark);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in std::io::BufReader::new(file).lines().flatten() {
        if line.starts_with('#') {
            continue;
        }
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() >= 9 {
            entries.push(HistoryEntry {
                timestamp: p[0].parse().unwrap_or(0),
                git_commit: p[1].to_string(),
                benchmark: p[2].to_string(),
                variant: p[3].to_string(),
                n: p[4].parse().unwrap_or(0),
                mode: p[5].to_string(),
                median_ns: p[6].parse().unwrap_or(0.0),
                ci_lo_ns: p[7].parse().unwrap_or(0.0),
                ci_hi_ns: p[8].parse().unwrap_or(0.0),
            });
        }
    }
    entries
}

/// Detect regressions against a rolling window of the last 5
/// historical entries.
pub fn detect_regressions(
    current: &[HistoryEntry],
    historical: &[HistoryEntry],
) -> Vec<(String, String, f64, bool)> {
    detect_regressions_window(current, historical, 5)
}

/// Detect regressions with an explicit rolling-window size.
///
/// For each `current` entry, the last `window_k` historical entries
/// for the same `(variant, mode, n)` are collected (older than
/// current's timestamp). The historical baseline is the median of
/// their medians; the historical CI upper bound is the median of
/// their CI uppers. A regression is flagged when current
/// `ci_lo > historical_ci_hi_median` (the new CI lies entirely above
/// the historical CI).
///
/// Returns a list of `(variant, mode, pct_change, regressed)`.
pub fn detect_regressions_window(
    current: &[HistoryEntry],
    historical: &[HistoryEntry],
    window_k: usize,
) -> Vec<(String, String, f64, bool)> {
    let mut results = Vec::new();

    for curr in current {
        let mut prev_entries: Vec<&HistoryEntry> = historical
            .iter()
            .filter(|h| {
                h.variant == curr.variant
                    && h.mode == curr.mode
                    && h.n == curr.n
                    && h.timestamp < curr.timestamp
            })
            .collect();
        prev_entries.sort_by_key(|h| h.timestamp);
        let k = window_k.min(prev_entries.len());
        if k == 0 {
            continue;
        }
        let window = &prev_entries[prev_entries.len() - k..];

        let mut hist_medians: Vec<f64> = window.iter().map(|h| h.median_ns).collect();
        hist_medians.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = hist_medians.len();
        let hist_median = if n % 2 == 0 {
            (hist_medians[n / 2 - 1] + hist_medians[n / 2]) / 2.0
        } else {
            hist_medians[n / 2]
        };

        let mut hist_ci_hi: Vec<f64> = window.iter().map(|h| h.ci_hi_ns).collect();
        hist_ci_hi.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let hist_ci_hi_median = if n % 2 == 0 {
            (hist_ci_hi[n / 2 - 1] + hist_ci_hi[n / 2]) / 2.0
        } else {
            hist_ci_hi[n / 2]
        };

        let pct = if hist_median > 0.0 {
            ((curr.median_ns - hist_median) / hist_median) * 100.0
        } else {
            0.0
        };

        let regressed = curr.ci_lo_ns > hist_ci_hi_median;

        results.push((curr.variant.clone(), curr.mode.clone(), pct, regressed));
    }

    results
}

/// Get the current short git commit hash for the consumer's working
/// directory. Returns `"unknown"` outside a git tree.
pub fn git_commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// Current timestamp as Unix epoch seconds.
pub fn timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
