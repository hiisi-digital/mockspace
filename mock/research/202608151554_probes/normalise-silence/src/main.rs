//! Probe: the three declared analysis roles are free strings, and a
//! wrong one is silent.
//!
//! The roles are `baseline`, `floor` and `delta`/`mode`, declared in
//! `bench.toml` (`bench-harness/src/config.rs:262-272` flattened, or
//! `:310-326` as the `[normalise]` table) and carried to the report as
//! `Option<String>` / `String`.
//!
//! What is asserted:
//!
//!   1. `with_baseline("typo")` is a no-op. The report normalises
//!      against whatever `baseline_idx` already held.
//!      (`analysis.rs:276-281`: the `if let Some` has no else.)
//!   2. `with_floor("typo")` is a no-op.
//!      (`analysis.rs:295` doc: "ignored by the reporter".)
//!   3. Of the four documented `mode` values, only `"ratio"` is
//!      observable. `"subtract"`, `"percent"`, `"none"` and any typo
//!      all render one byte-identical report.
//!      (`report.rs:196`: `== "ratio"` is the only read.)
//!
//! NEGATIVE CONTROLS, stated before the run, each of which must FAIL
//! the silence assertion, or the instrument is measuring nothing:
//!
//!   C1. `with_baseline("<a real arm>")` MUST change the report.
//!       Without this, assertion 1 is satisfied by a renderer that
//!       ignores the baseline entirely.
//!   C2. `with_floor("<a real arm>")` under `mode = "ratio"` MUST
//!       change the report. Same reason.
//!   C3. `mode = "ratio"` MUST change the report. Without this,
//!       assertion 3 is satisfied by a renderer that ignores `mode`
//!       entirely, which is a different (weaker) finding.

use mockspace_bench_harness::{DataSet, Sample, generate_report};

/// Three arms with clearly separated means so any baseline or floor
/// change moves a rendered number.
fn samples() -> Vec<Sample> {
    let mut out = Vec::new();
    for (variant, algo) in [("alpha", 100.0f64), ("bravo", 200.0), ("charlie", 400.0)] {
        for batch_idx in 0 .. 24 {
            out.push(Sample {
                run: 0,
                pass: batch_idx / 8,
                cooldown_ms: 0,
                mode: "warm".into(),
                variant: variant.into(),
                e2e_ns: algo + 10.0,
                algo_ns: algo + (batch_idx as f64 % 3.0),
                bridge_ns: 10.0,
                batch_idx,
                batch_count: 1000,
                ..Sample::default()
            });
        }
    }
    out
}

fn render(baseline: Option<&str>, floor: Option<&str>, mode: Option<&str>) -> String {
    let mut ds = DataSet::from_samples(&samples(), "warm");
    if let Some(b) = baseline {
        ds = ds.with_baseline(b);
    }
    if let Some(f) = floor {
        ds = ds.with_floor(f);
    }
    if let Some(m) = mode {
        ds = ds.with_normalise_mode(m);
    }
    generate_report(&ds, "probe")
}

fn main() {
    let mut failed_controls = 0usize;

    // ── negative controls first ──────────────────────────────────
    let default_base = render(None, None, None);
    let real_base = render(Some("charlie"), None, None);
    println!(
        "C1 real baseline changes the report: {}",
        if real_base != default_base { "PASS" } else { failed_controls += 1; "FAIL" }
    );

    let ratio_no_floor = render(Some("charlie"), None, Some("ratio"));
    let ratio_real_floor = render(Some("charlie"), Some("alpha"), Some("ratio"));
    println!(
        "C2 real floor changes the report: {}",
        if ratio_real_floor != ratio_no_floor { "PASS" } else { failed_controls += 1; "FAIL" }
    );

    let subtract = render(Some("charlie"), None, Some("subtract"));
    println!(
        "C3 mode=ratio changes the report: {}",
        if ratio_no_floor != subtract { "PASS" } else { failed_controls += 1; "FAIL" }
    );

    if failed_controls > 0 {
        println!("\n{failed_controls} CONTROL(S) FAILED: the findings below are void.");
        std::process::exit(1);
    }
    println!("\nall controls pass; the instrument can see a role change.\n");

    // ── the findings ─────────────────────────────────────────────
    let typo_base = render(Some("charley"), None, None);
    println!(
        "F1 baseline=\"charley\" (typo) == no baseline at all: {}",
        typo_base == default_base
    );

    let typo_floor = render(Some("charlie"), Some("alfa"), Some("ratio"));
    println!(
        "F2 floor=\"alfa\" (typo) == no floor at all:           {}",
        typo_floor == ratio_no_floor
    );

    for m in ["percent", "none", "percnt", "", "RATIO", "banana"] {
        let r = render(Some("charlie"), None, Some(m));
        println!("F3 mode={m:>8?} == mode=\"subtract\":              {}", r == subtract);
    }

    // How wrong the silent answer is, in the rendered number, so the
    // cost is a quantity rather than an adjective.
    let head = |s: &str| -> String {
        s.lines()
            .filter(|l| l.starts_with("| alpha") || l.starts_with("| charlie"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    println!("\n-- rows under the DECLARED baseline `charlie` --\n{}", head(&real_base));
    println!("\n-- rows under the TYPOED baseline `charley` --\n{}", head(&typo_base));
}
