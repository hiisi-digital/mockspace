//! Probe: `report::generate` indexes an empty variant list.
//!
//! `report::generate` opens with `let base = ds.baseline();`
//! (`bench-harness/src/report.rs:17`), and `DataSet::baseline` is
//! `&self.variants[self.baseline_idx]` (`analysis.rs:312-314`). Nothing
//! between them checks that `variants` is non-empty. The two guards that do
//! exist (`report.rs:36`, `report.rs:42`) are `len() > 1` and sit AFTER the
//! index.
//!
//! `DataSet::from_samples(samples, mode)` filters by mode, so a samples set
//! whose mode column does not match yields zero variants. Per probe 03 a
//! sheared or garbled CSV row can produce exactly that.
//!
//! NEGATIVE CONTROL: the SAME call with a matching mode must render a report.
//! If that panics too, the probe measures a broken constructor rather than
//! the empty case.
use mockspace_bench_harness::analysis::DataSet;
use mockspace_bench_harness::report;
use mockspace_bench_harness::sample::Sample;

fn sample(variant: &str, mode: &str, algo: f64) -> Sample {
    Sample {
        run: 0, pass: 0, cooldown_ms: 0,
        mode: mode.into(), variant: variant.into(),
        e2e_ns: algo + 10.0, algo_ns: algo, bridge_ns: 10.0,
        batch_idx: 0, batch_count: 1, score: None, input_tag: None,
        instructions: 0, cycles: 0, setup_ns: 0.0, first_ns: 0.0, digest: 0,
    }
}

fn try_generate(label: &str, samples: Vec<Sample>, mode: &str) {
    let r = std::panic::catch_unwind(move || {
        let ds = DataSet::from_samples(&samples, mode);
        let n = ds.variants.len();
        let md = report::generate(&ds, "t");
        (n, md.lines().count())
    });
    match r {
        Ok((n, lines)) => println!("  {label}: Ok, {n} variant(s), {lines} report lines"),
        Err(_) => println!("  {label}: PANICKED"),
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    println!("NEGATIVE CONTROL (mode matches, two arms):");
    try_generate(
        "mode=warm, samples are warm",
        vec![sample("a", "warm", 100.0), sample("b", "warm", 50.0)],
        "warm",
    );
    println!();
    println!("the cases:");
    try_generate(
        "mode=warm, samples say cold",
        vec![sample("a", "cold", 100.0), sample("b", "cold", 50.0)],
        "warm",
    );
    try_generate("no samples at all", vec![], "warm");
    try_generate(
        "mode field garbled (probe 03's shear)",
        vec![sample("a", "", 100.0)],
        "warm",
    );
    println!();
    println!("A regenerated report is `mock bench report`, which reads a committed CSV.");
}
