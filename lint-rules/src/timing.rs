//--------------------------------------------------------------------------------------------------
// Copyright (c) 2026                   orgrinrt                 ort@hiisi.digital
// SPDX-License-Identifier: MPL-2.0     https://mozilla.org/MPL/2.0        contact@hiisi.digital
//--------------------------------------------------------------------------------------------------

//! Where a gate run spends its time, kept only when asked for.
//!
//! `MOCKSPACE_TIMINGS=1` makes the runners record how long each lint took and the engine
//! record how long each phase took, and the engine prints the table once at the end of a run.
//! Without the variable nothing is recorded and nothing is printed, so the measurement costs a
//! gate run nothing it did not already cost.
//!
//! The table is the measurement `research/` quotes: a cost is attributed to the lint or phase
//! that incurred it, never estimated from the total.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The variable that turns recording on.
pub const ENV: &str = "MOCKSPACE_TIMINGS";

static TABLE: Mutex<BTreeMap<String, (Duration, u64)>> = Mutex::new(BTreeMap::new());

/// Whether recording is on for this process.
#[must_use]
pub fn enabled() -> bool {
    use std::sync::OnceLock;
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os(ENV).is_some_and(|v| !v.is_empty() && v != "0"))
}

/// Add `spent` under `label`. Records whatever it is given, so a test can exercise the table
/// without the environment; callers in the runners go through [`time`], which checks [`enabled`].
pub fn record(label: &str, spent: Duration) {
    let mut table = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let slot = table.entry(label.to_string()).or_insert((Duration::ZERO, 0));
    slot.0 += spent;
    slot.1 += 1;
}

/// Run `f`, and record its wall time under `label` when recording is on.
pub fn time<T>(label: &str, f: impl FnOnce() -> T) -> T {
    if !enabled() {
        return f();
    }
    let start = Instant::now();
    let out = f();
    record(label, start.elapsed());
    out
}

/// The recorded table, slowest first, one row per label: milliseconds, calls, label.
#[must_use]
pub fn report() -> String {
    let table = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    let mut rows: Vec<(&String, &(Duration, u64))> = table.iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    let mut out = String::from("--- timings (ms, calls, label) ---\n");
    for (label, (spent, calls)) in rows {
        out.push_str(&format!("{:>10.1} {:>6} {label}\n", spent.as_secs_f64() * 1000.0, calls));
    }
    out
}

/// Empty the table. Only the tests need it.
#[cfg(test)]
pub(crate) fn reset() {
    TABLE.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    // The table is process-wide, so the cases that read it share one test and run in order.
    #[test]
    fn the_report_is_slowest_first_and_sums_repeated_labels() {
        reset();
        record("fast", Duration::from_millis(2));
        record("slow", Duration::from_millis(40));
        record("slow", Duration::from_millis(60));
        let text = report();
        let slow = text.find("slow").expect("slow is listed");
        let fast = text.find("fast").expect("fast is listed");
        assert!(slow < fast, "slowest first: {text}");
        assert!(text.contains("100.0      2 slow"), "two calls summed to 100 ms: {text}");
    }

    #[test]
    fn nothing_is_recorded_while_it_is_off() {
        // The suite never sets the variable, so `time` must be the identity and leave no row.
        assert!(!enabled(), "{ENV} must not be set under the test runner");
        let out = time("never-recorded", || 7);
        assert_eq!(out, 7);
        assert!(!report().contains("never-recorded"));
    }
}
