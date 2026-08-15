//! Probe: the two keys that decide what every number in the report MEANS
//! both fail open on a typo, and one of them fails open invisibly.
//!
//! `DataSet::with_baseline(name)` (analysis.rs:276) keeps `baseline_idx = 0`
//! when the name is absent. `DataSet::with_floor(name)` (analysis.rs:295)
//! stores any string, and `floor_mean()` returns `None` when it is absent,
//! so `report::generate` silently renders raw ratios instead of
//! floor-differenced ones.
//!
//! NEGATIVE CONTROL, and it is the whole probe: the CORRECT spellings must
//! produce a report that DIFFERS from the typo'd one. If correct and typo'd
//! render identically, the keys do nothing at all and this probe is
//! measuring nothing. Both controls are asserted below and the program
//! aborts if either fails.

use mockspace_bench_harness::analysis::DataSet;
use mockspace_bench_harness::report;
use mockspace_bench_harness::sample::Sample;

fn sample(variant: &str, algo: f64) -> Sample {
    Sample {
        run:          0,
        pass:         0,
        cooldown_ms:  0,
        mode:         "warm".into(),
        variant:      variant.into(),
        e2e_ns:       algo + 10.0,
        algo_ns:      algo,
        bridge_ns:    10.0,
        batch_idx:    0,
        batch_count:  1,
        score:        None,
        input_tag:    None,
        instructions: 0,
        cycles:       0,
        setup_ns:     0.0,
        first_ns:     0.0,
        digest:       0,
    }
}

/// switch = 100 ns, threaded = 50 ns, nullfloor = 20 ns.
/// Floor-differenced against nullfloor, threaded reads (50-20)/(100-20) = 0.375x.
/// Raw, it reads 50/100 = 0.50x. The two are distinguishable by eye.
fn samples() -> Vec<Sample> {
    (0..3)
        .flat_map(|_| {
            [
                sample("switch", 100.0),
                sample("threaded", 50.0),
                sample("nullfloor", 20.0),
            ]
        })
        .collect()
}

fn render(baseline: &str, floor: &str) -> String {
    let s = samples();
    let mut ds = DataSet::from_samples(&s, "warm").with_baseline(baseline);
    ds.meta.normalise_mode = "ratio".into();
    if !floor.is_empty() {
        ds = ds.with_floor(floor);
    }
    report::generate(&ds, "t")
}

/// Pull the `× base` cell for one arm out of the rendered table.
fn ratio_of(md: &str, arm: &str) -> String {
    for line in md.lines() {
        if line.starts_with(&format!("| {arm} |")) && line.matches('|').count() == 7 {
            return line.rsplit('|').nth(1).unwrap_or("?").trim().to_string();
        }
    }
    "<no ratio row>".into()
}

fn baseline_named(md: &str) -> String {
    md.lines()
        .find(|l| l.starts_with("Baseline:"))
        .unwrap_or("<none>")
        .to_string()
}

fn has_floor_note(md: &str) -> bool {
    md.contains("floor-differenced against the")
}

fn main() {
    // ── Case A: everything spelled correctly ──
    let correct = render("switch", "nullfloor");
    // ── Case B: the floor key has a one-character typo ──
    let floor_typo = render("switch", "nulfloor");
    // ── Case C: the baseline key has a one-character typo ──
    let base_typo = render("swtich", "nullfloor");
    // ── Case D: no floor declared at all (the honest raw-ratio case) ──
    let no_floor = render("switch", "");

    let rows = [
        ("A  correct               ", &correct),
        ("B  floor = \"nulfloor\"    ", &floor_typo),
        ("C  baseline = \"swtich\"   ", &base_typo),
        ("D  no floor declared     ", &no_floor),
    ];

    println!("{:<26} {:<28} {:>10} {:>10} {:>10}  floor-note", "case", "report header", "switch", "threaded", "nullfloor");
    for (label, md) in rows {
        println!(
            "{label} {:<28} {:>10} {:>10} {:>10}  {}",
            baseline_named(md),
            ratio_of(md, "switch"),
            ratio_of(md, "threaded"),
            ratio_of(md, "nullfloor"),
            if has_floor_note(md) { "present" } else { "ABSENT" },
        );
    }

    println!();
    println!("B identical to D (typo'd floor is indistinguishable from no floor): {}", floor_typo == no_floor);
    println!("B differs from A anywhere except the ratio digits and the note: {}",
        {
            let strip = |s: &str| s.lines().filter(|l| !l.contains('×') && !l.contains("floor-differenced")).collect::<Vec<_>>().join("\n");
            strip(&floor_typo) != strip(&correct)
        });

    println!();
    // ── NEGATIVE CONTROLS ──
    // If the correct spelling did not change the answer, the keys are inert
    // and every line above is noise.
    assert_ne!(
        ratio_of(&correct, "threaded"),
        ratio_of(&floor_typo, "threaded"),
        "CONTROL FAILED: a correct floor and a typo'd floor render the same \
         ratio, so this probe measures nothing"
    );
    assert!(
        has_floor_note(&correct),
        "CONTROL FAILED: the correct spelling does not emit the note either"
    );
    assert_ne!(
        baseline_named(&correct),
        baseline_named(&render("threaded", "nullfloor")),
        "CONTROL FAILED: with_baseline does not move the baseline at all"
    );
    println!("negative controls: all three passed (the correct spellings do change the report)");
}
