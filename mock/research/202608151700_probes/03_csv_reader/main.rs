//! Probe: `load_samples_csv` cannot distinguish "field absent or garbled"
//! from "field said zero", and zero is meaningful in every column it
//! defaults.
//!
//! `bench-harness/src/sample.rs:120-140`. `e2e_ns`/`algo_ns` use
//! `p[i].parse().unwrap_or(0.0)`; the appended columns use
//! `p.get(i).and_then(parse).unwrap_or(0)`.
//!
//! NEGATIVE CONTROL: the same reader on a WELL-FORMED row must produce the
//! written values. If it does not, the reader is broken for every input and
//! the corrupted-row result says nothing about corruption specifically.

use mockspace_bench_harness::sample::load_samples_csv;

const HEADER: &str = "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag,instructions,cycles,setup_ns,first_ns,digest";

fn read(name: &str, body: &str) -> Vec<(String, f64, f64, u64)> {
    let p = std::env::temp_dir().join(format!("csvprobe_{name}.csv"));
    std::fs::write(&p, body).unwrap();
    load_samples_csv(&p)
        .unwrap()
        .into_iter()
        .map(|s| (s.variant, s.algo_ns, s.e2e_ns, s.digest))
        .collect()
}

fn main() {
    // ── NEGATIVE CONTROL: well-formed rows round-trip ──
    let good = format!(
        "{HEADER}\n\
         0,0,0,warm,packed,0,110.0,100.0,10.0,1,,,0,0,0.0,0.0,7788\n\
         0,0,0,warm,dense,0,210.0,200.0,10.0,1,,,0,0,0.0,0.0,7788\n"
    );
    let g = read("control", &good);
    println!("CONTROL well-formed rows: {g:?}");
    assert_eq!(g[0].1, 100.0, "CONTROL FAILED: reader loses a good algo_ns");
    assert_eq!(g[0].3, 7788, "CONTROL FAILED: reader loses a good digest");

    // ── Case 1: one timing cell garbled (a truncated write, an editor,
    //    a locale that wrote `100,0`) ──
    let garbled = format!(
        "{HEADER}\n\
         0,0,0,warm,packed,0,110.0,1OO.O,10.0,1,,,0,0,0.0,0.0,7788\n\
         0,0,0,warm,dense,0,210.0,200.0,10.0,1,,,0,0,0.0,0.0,7788\n"
    );
    println!("garbled algo_ns:          {:?}", read("garbled", &garbled));

    // ── Case 2: a pre-digest CSV, which the code comments call out as
    //    supported. Every digest reads 0, so every arm agrees. ──
    let old = "run,pass,cooldown_ms,mode,variant,batch_idx,e2e_ns,algo_ns,bridge_ns,batch_count,score,input_tag\n\
               0,0,0,warm,packed,0,110.0,100.0,10.0,1,,\n\
               0,0,0,warm,dense,0,210.0,200.0,10.0,1,,\n";
    println!("pre-digest CSV:           {:?}", read("old", old));

    // ── Case 3: an arm name containing the delimiter. `write_csv`
    //    (harness.rs:779-798) writes `variant` unquoted. ──
    let comma = format!(
        "{HEADER}\n\
         0,0,0,warm,packed,v2,0,110.0,100.0,10.0,1,,,0,0,0.0,0.0,7788\n"
    );
    println!("arm name with a comma:    {:?}", read("comma", &comma));

    // ── Case 4: a row truncated mid-write (a killed run) ──
    let trunc = format!("{HEADER}\n0,0,0,warm,packed,0,110.0,100.0,10.0,1,,,0,0\n");
    println!("truncated row:            {:?}", read("trunc", &trunc));

    println!();
    println!("Reading: (variant, algo_ns, e2e_ns, digest).");
    println!("A 0.0 ns arm is not a missing measurement, it is the fastest arm.");
}
