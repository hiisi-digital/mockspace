//! Probe: `Sample::mode` is a `String` over a closed two-element set,
//! and selecting a mode that does not occur is a panic rather than a
//! diagnostic.
//!
//! `Sample::mode` (`bench-harness/src/sample.rs:31`) is documented as
//! carrying `"normal"` / `"batched"`. The code writes and compares
//! `"warm"` / `"cold"` in eleven places
//! (`harness.rs:272,332,376,452,470,572,626`, `driver/mod.rs:241,590,646`).
//! Every public entry point that selects one takes `&str`:
//! `BenchResult::dataset(mode)`, `write_report(.., mode, ..)`,
//! `report_from_csv(.., mode, ..)`.
//!
//! `DataSet::from_samples` filters by that string and `report::generate`
//! opens with `ds.baseline()`, which is `&self.variants[self.baseline_idx]`
//! (`analysis.rs:312-314`) with no emptiness guard.
//!
//! NEGATIVE CONTROLS, stated before the run:
//!   C1 the correct mode MUST render a report naming both arms.
//!      Without it the "typo" result is not about the typo.
//!   C2 the correct mode MUST NOT panic. Same reason.

use mockspace_bench_harness::{DataSet, Sample, generate_report};

fn samples(mode: &str) -> Vec<Sample> {
    let mut out = Vec::new();
    for (variant, algo) in [("alpha", 100.0f64), ("bravo", 200.0)] {
        for batch_idx in 0 .. 8 {
            out.push(Sample {
                mode: mode.into(),
                variant: variant.into(),
                e2e_ns: algo + 10.0,
                algo_ns: algo,
                bridge_ns: 10.0,
                batch_idx,
                batch_count: 1000,
                ..Sample::default()
            });
        }
    }
    out
}

fn try_render(selected: &str) -> Result<String, String> {
    let s = samples("warm");
    std::panic::catch_unwind(move || {
        let ds = DataSet::from_samples(&s, selected);
        generate_report(&ds, "probe")
    })
    .map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "<non-string panic>".into())
    })
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));

    let ok = try_render("warm");
    let mut controls_failed = 0;
    match &ok {
        Ok(md) => {
            let names = md.contains("alpha") && md.contains("bravo");
            println!("C1 correct mode renders both arms : {}", if names { "PASS" } else { controls_failed += 1; "FAIL" });
            println!("C2 correct mode does not panic    : PASS");
        },
        Err(e) => {
            controls_failed += 2;
            println!("C1/C2 FAILED: the correct mode panicked: {e}");
        },
    }
    if controls_failed > 0 {
        println!("\ncontrols failed; findings void.");
        std::process::exit(1);
    }
    println!("\ncontrols pass.\n");

    for typo in ["wamr", "Warm", "cold", "normal", "batched", ""] {
        match try_render(typo) {
            Ok(md) => println!("F  mode={typo:>8?} -> rendered {} bytes", md.len()),
            Err(e) => println!("F  mode={typo:>8?} -> PANIC: {e}"),
        }
    }
    println!(
        "\n`\"normal\"` and `\"batched\"` are the two values the field's own doc\n\
         comment names (sample.rs:28-29). Neither is ever written by the code."
    );
}
