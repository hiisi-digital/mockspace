//! Probe: a zero-timing sample is a valid measurement to every stage
//! downstream of the worker parser.
//!
//! WHY THIS MATTERS. The worker-to-orchestrator wire is a positional
//! TSV. The writer emits 12 or 13 columns
//! (`bench-harness/src/harness.rs:490-499`). The reader accepts any
//! line with `parts.len() >= 9` (`harness.rs:721`) and parses every
//! field with `.parse().unwrap_or(<default>)`
//! (`harness.rs:728-739`). So a line that is short, or carries one
//! unparseable field, is not dropped: it becomes a Sample whose
//! timings are `0.0` and whose `digest` is `0`.
//!
//! This probe does not test the parser, which is an inline loop with
//! no function boundary and therefore has no test anywhere. It tests
//! the half that is reachable: what the rest of the pipeline does
//! with the Sample that parser produces.
//!
//! NEGATIVE CONTROLS, stated before the run:
//!   C1 the clean dataset MUST report a mean near 100ns. If the
//!      instrument cannot read the mean, nothing below means anything.
//!   C2 dropping the same three samples entirely (rather than zeroing
//!      them) MUST NOT move the mean materially. This separates "the
//!      zeros did damage" from "any change of sample count does".

use mockspace_bench_harness::{DataSet, Sample};

fn s(variant: &str, algo: f64, idx: usize) -> Sample {
    Sample {
        mode: "warm".into(),
        variant: variant.into(),
        e2e_ns: algo + 10.0,
        algo_ns: algo,
        bridge_ns: 10.0,
        batch_idx: idx,
        batch_count: 1000,
        digest: 0xDEAD_BEEF,
        ..Sample::default()
    }
}

/// What `harness.rs:728-739` produces from a line that is short or has
/// one unparseable field: the defaults, verbatim.
fn short_line_sample(variant: &str, idx: usize) -> Sample {
    Sample {
        mode: "warm".into(),
        variant: variant.into(),
        e2e_ns: 0.0,   // parts[3].parse().unwrap_or(0.0)
        algo_ns: 0.0,  // parts[4].parse().unwrap_or(0.0)
        bridge_ns: 0.0,
        batch_idx: idx,
        batch_count: 0,
        digest: 0, // parts.get(11)...unwrap_or(0)  <- outside the >= 9 guard
        ..Sample::default()
    }
}

fn mean_of(ds: &DataSet, name: &str) -> f64 {
    ds.variants.iter().find(|v| v.name == name).map(|v| v.algo_all.mean).unwrap_or(f64::NAN)
}

fn main() {
    let clean: Vec<Sample> = (0 .. 20).map(|i| s("alpha", 100.0, i)).collect();

    let mut zeroed = clean.clone();
    zeroed.truncate(17);
    zeroed.extend((17 .. 20).map(|i| short_line_sample("alpha", i)));

    let dropped: Vec<Sample> = clean.iter().take(17).cloned().collect();

    let m_clean = mean_of(&DataSet::from_samples(&clean, "warm"), "alpha");
    let m_zero = mean_of(&DataSet::from_samples(&zeroed, "warm"), "alpha");
    let m_drop = mean_of(&DataSet::from_samples(&dropped, "warm"), "alpha");

    let mut failed = 0;
    let c1 = (m_clean - 100.0).abs() < 1.0;
    println!("C1 clean mean reads ~100ns              : {} ({m_clean:.2})", if c1 { "PASS" } else { failed += 1; "FAIL" });
    let c2 = (m_drop - m_clean).abs() < 1.0;
    println!("C2 dropping 3 of 20 does not move it    : {} ({m_drop:.2})", if c2 { "PASS" } else { failed += 1; "FAIL" });
    if failed > 0 {
        println!("\ncontrols failed; findings void.");
        std::process::exit(1);
    }
    println!("\ncontrols pass.\n");

    println!("F1 3 of 20 samples arriving as short lines:");
    println!("   reported mean {m_zero:.2}ns against the true {m_clean:.2}ns  ({:+.1}%)",
             (m_zero - m_clean) / m_clean * 100.0);
    println!("   the run reports {} samples either way", DataSet::from_samples(&zeroed, "warm")
             .variants.iter().find(|v| v.name == "alpha").unwrap().algo_all.count);

    let ds = DataSet::from_samples(&zeroed, "warm");
    let v = ds.variants.iter().find(|v| v.name == "alpha").unwrap();
    println!("\nF2 nothing in the analysis names the zeros:");
    println!("   mean {:.2}  best20% {:.2}  worst20% {:.2}  count {}",
             v.algo_all.mean, v.algo_all.best_20pct, v.algo_all.worst_20pct, v.algo_all.count);

    println!("\nF3 the digest column sits OUTSIDE the `parts.len() >= 9` guard.");
    println!("   A short line yields digest = 0 for every arm, so two arms that");
    println!("   never reported a digest compare equal. Change 1 of this round");
    println!("   makes that comparison load-bearing for the first time.");
}
