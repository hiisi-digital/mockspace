//! Tests for the bench-harness data plane: synthetic samples through the cache
//! CSV round trip, `DataSet` aggregation, and the markdown report.
//!
//! The orchestrator and worker subprocess flow is not exercised here. Driving
//! it needs variant cdylibs built against `mockspace-bench-core`, which is a
//! build step rather than a test fixture; `benches/` is where that path runs.

use std::fs;

use mockspace_bench_harness::{
    BenchManifest,
    BenchResult,
    DataSet,
    EnvMeta,
    Sample,
    cache,
    generate_report,
};

fn synthetic_samples() -> Vec<Sample> {
    let mut samples = Vec::new();
    // Two variants, two cooldowns, three runs * three passes.
    for run in 1 ..= 3 {
        for pass in 1 ..= 3 {
            for cooldown in [0u64, 100u64] {
                for batch_idx in 0 .. 5 {
                    samples.push(Sample {
                        run,
                        pass,
                        cooldown_ms: cooldown,
                        mode: "warm".into(),
                        variant: "alpha".into(),
                        e2e_ns: 120.0 + (batch_idx as f64) * 0.5,
                        algo_ns: 100.0 + (batch_idx as f64) * 0.3,
                        bridge_ns: 20.0,
                        batch_idx,
                        batch_count: 100,
                        score: Some(42.0),
                        input_tag: Some(0),
                        instructions: 0,
                        cycles: 0,
                        setup_ns: 0.0,
                        first_ns: 0.0,
                        digest: 0,
                    });
                    samples.push(Sample {
                        run,
                        pass,
                        cooldown_ms: cooldown,
                        mode: "warm".into(),
                        variant: "beta".into(),
                        e2e_ns: 95.0 + (batch_idx as f64) * 0.4,
                        algo_ns: 80.0 + (batch_idx as f64) * 0.2,
                        bridge_ns: 15.0,
                        batch_idx,
                        batch_count: 100,
                        score: Some(40.0),
                        input_tag: Some(0),
                        instructions: 0,
                        cycles: 0,
                        setup_ns: 0.0,
                        first_ns: 0.0,
                        digest: 0,
                    });
                }
            }
        }
    }
    samples
}

#[test]
fn dataset_aggregates_per_variant() {
    let samples = synthetic_samples();
    let ds = DataSet::from_samples(&samples, "warm");

    assert_eq!(ds.variants.len(), 2, "expected two variants in dataset");
    let alpha = ds.variants.iter().find(|v| v.name == "alpha").unwrap();
    let beta = ds.variants.iter().find(|v| v.name == "beta").unwrap();

    // The fixture makes every quintile exact, so assert the values rather than
    // that they are positive. 3 runs x 3 passes x 2 cooldowns x 5 batches = 90
    // samples per variant, eighteen each of five distinct algo times.
    assert_eq!(alpha.algo_all.count, 90);
    assert_eq!(beta.algo_all.count, 90);
    let close = |a: f64, b: f64, what: &str| {
        assert!((a - b).abs() < 1e-9, "{what}: {a} vs {b}");
    };
    // alpha algo: 100.0, 100.3, 100.6, 100.9, 101.2, eighteen of each.
    close(alpha.algo_all.median, 100.6, "alpha median");
    close(alpha.algo_all.min, 100.0, "alpha min");
    close(alpha.algo_all.max, 101.2, "alpha max");
    close(alpha.algo_all.best_20pct, 100.0, "alpha best 20%");
    close(alpha.algo_all.worst_20pct, 101.2, "alpha worst 20%");
    close(alpha.algo_all.mid_60pct, 100.6, "alpha mid 60%");
    close(alpha.algo_all.mean, 100.6, "alpha mean");
    // beta algo: 80.0, 80.2, 80.4, 80.6, 80.8.
    close(beta.algo_all.median, 80.4, "beta median");
    close(beta.algo_all.best_20pct, 80.0, "beta best 20%");
    close(beta.algo_all.worst_20pct, 80.8, "beta worst 20%");

    // bridge_ns is a constant per variant in the fixture, so its spread is the
    // check that the per-variant partition did not mix the two arms.
    close(alpha.bridge_all.median, 20.0, "alpha bridge");
    close(beta.bridge_all.median, 15.0, "beta bridge");
    close(alpha.bridge_all.std_dev, 0.0, "alpha bridge is constant");

    // Per-cooldown breakdown: both cohorts present, each holding half.
    assert_eq!(
        alpha.algo_per_cd.len(),
        2,
        "one entry per declared cooldown"
    );
    assert_eq!(alpha.algo_per_cd[&0].count, 45);
    assert_eq!(alpha.algo_per_cd[&100].count, 45);
    // The nonstop series is one value per (run, pass) at cooldown 0: three runs
    // of three passes, not the 45 batches those passes hold.
    assert_eq!(alpha.nonstop_per_pass.len(), 9);
    assert_eq!(
        alpha
            .nonstop_per_pass
            .iter()
            .map(|(k, _)| *k)
            .collect::<Vec<_>>(),
        vec![(1, 1), (1, 2), (1, 3), (2, 1), (2, 2), (2, 3), (3, 1), (3, 2), (3, 3)]
    );
    // Each pass holds five batches at 100.0, 100.3, 100.6, 100.9, 101.2.
    for (k, v) in &alpha.nonstop_per_pass {
        close(*v, 100.6, &format!("pass {k:?} median"));
    }

    // Scores and tags come through, and the tag index is named.
    assert_eq!(alpha.scores.len(), 90);
    assert_eq!(alpha.algo_per_tag[&0].count, 90);
    assert_eq!(ds.tag_names.get(&0).map(String::as_str), Some("tag-0"));

    // The baseline defaults to the first variant by name, which is what every
    // delta in the report is measured against.
    assert_eq!(ds.baseline().name, "alpha");
}

#[test]
fn a_dataset_holds_only_the_mode_it_was_asked_for() {
    // The fixture is entirely warm. Asking for cold must produce an empty
    // dataset rather than the warm samples relabelled, because `write_report`
    // takes the mode as a string and a silent fallthrough would report warm
    // numbers under a cold heading.
    let samples = synthetic_samples();
    let result = BenchResult {
        title: "demo".into(),
        env: EnvMeta::default(),
        samples,
        cache_path: String::new(),
        report_path: String::new(),
    };
    let warm = result.dataset("warm");
    assert_eq!(warm.variants.len(), 2);
    assert!(warm.variants.iter().all(|v| v.algo_all.count == 90));

    let cold = result.dataset("cold");
    assert!(
        cold.variants.is_empty(),
        "the fixture has no cold samples, so a cold dataset holds nothing"
    );

    let nonsense = result.dataset("there-is-no-such-mode");
    assert!(
        nonsense.variants.is_empty(),
        "an unknown mode matches nothing"
    );
}

#[test]
fn report_renders_expected_sections() {
    let samples = synthetic_samples();
    let ds = DataSet::from_samples(&samples, "warm");

    let md = generate_report(&ds, "smoke-test");

    // Spot-check the sections produced by Round 6's generator.
    for needle in [
        "# smoke-test",
        "## Key findings",
        "## End-to-end (all cooldowns combined)",
        "## Function-under-test only",
        "## Per-cooldown breakdown",
        "## Statistical comparison",
        "## Bridge overhead per variant",
        "## Distribution (algo ns)",
    ] {
        assert!(md.contains(needle), "report missing section `{needle}`");
    }
}

#[test]
fn a_cached_variant_is_reloaded_and_a_changed_one_is_rerun() {
    // What the previous shape of this test did: wrote a cache entry for a dylib
    // path that does not exist, so `dylib_hash` returned 0, the hash never
    // matched, the reload path was never entered, and the only assertion was
    // that index 0 is in the re-run list, which `partition` pushes
    // unconditionally on its first line. It was named for a round trip and
    // completed none. It also `set_current_dir`, which is process-global in a
    // test binary whose tests run on parallel threads.
    let tmp = std::env::temp_dir().join(format!("mockspace_bench_cache_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    // A real file, so `dylib_hash` returns a real hash rather than its
    // could-not-read zero.
    let base = tmp.join("libalpha.dylib");
    let cached = tmp.join("libbeta.dylib");
    fs::write(&base, b"not really a dylib, but it is on disk").unwrap();
    fs::write(&cached, b"nor is this one").unwrap();
    let base_s = base.display().to_string();
    let cached_s = cached.display().to_string();
    let cached_hash = cache::dylib_hash(&cached_s);
    assert_ne!(
        cached_hash, 0,
        "dylib_hash returns 0 only when it cannot read"
    );

    let root = tmp.join("cacheroot");
    let samples = synthetic_samples();
    {
        let mut c = cache::Cache::load_in(&root, "smoke", 0xDEADBEEF);
        c.save_variant(&cached_s, cached_hash, 100.0, 95.0, &samples);
        c.flush();
    }

    // Reload from disk and partition. Index 0 always re-runs; index 1 matches
    // its recorded hash, so its samples come back off the CSV.
    let c2 = cache::Cache::load_in(&root, "smoke", 0xDEADBEEF);
    let (to_run, hits) = c2.partition(&[base_s.clone(), cached_s.clone()]);
    assert!(to_run.contains(&0), "the baseline always re-runs");
    assert_eq!(hits.len(), 1, "the recorded variant is a hit");
    assert_eq!(
        hits[0].samples.len(),
        samples.len(),
        "every sample came back off the cached csv"
    );
    assert_eq!(hits[0].global_mean_warm, 100.0);
    assert_eq!(hits[0].global_mean_cold, 95.0);
    assert!(
        to_run.contains(&1),
        "the first hit is also re-run as the drift control, which is what gives \
         consensus_drift two points to compare"
    );
    // The columns survived the manifest and the CSV, not just the row count.
    let a = &samples[0];
    let b = &hits[0].samples[0];
    assert_eq!(b.variant, a.variant);
    assert_eq!(b.run, a.run);
    assert_eq!(b.mode, a.mode);
    assert_eq!(b.batch_count, a.batch_count);
    assert!((b.algo_ns - a.algo_ns).abs() < 0.05);

    // Change the file and the entry stops matching.
    fs::write(&cached, b"a different build").unwrap();
    let c3 = cache::Cache::load_in(&root, "smoke", 0xDEADBEEF);
    let (to_run2, hits2) = c3.partition(&[base_s, cached_s]);
    assert!(hits2.is_empty(), "a changed dylib is not a hit");
    assert_eq!(to_run2, vec![0, 1], "both re-run");

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn manifest_loads_and_converts_to_config() {
    let tmp = std::env::temp_dir().join(format!("mockspace_bench_manifest_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let manifest_text = r#"
[bench.demo]
title = "Demo bench"
workload = "default"
master_seed = 0x1234_5678_9ABC_DEF0

[[bench.demo.sizes]]
n = 64
variants = ["variants/x/target/release/libx.dylib"]

[timing]
passes = 2
runs_per_pass = 100
batch_size = 10
harness_runs = 1
cooldowns_ms = [0, 100]
"#;
    let manifest_path = tmp.join("bench.toml");
    fs::write(&manifest_path, manifest_text).unwrap();

    let manifest = BenchManifest::load(&manifest_path).unwrap();
    assert_eq!(manifest.bench.len(), 1, "expected one bench entry");
    let demo = manifest.bench.get("demo").expect("demo entry present");
    assert_eq!(demo.sizes.len(), 1);

    let cfg = manifest.for_size("demo", 0, &tmp).unwrap();
    assert_eq!(cfg.bench_name, "demo");
    assert_eq!(cfg.n, 64);
    assert_eq!(cfg.cooldowns_ms, vec![0, 100]);
    assert_eq!(cfg.variant_paths.len(), 1);
    assert!(cfg.variant_paths[0].ends_with("variants/x/target/release/libx.dylib"));

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn report_from_csv_round_trip_emits_findings_md() {
    // #604: CSV cache to findings.md generator. Round-trip
    // synthetic samples through `write_csv` then
    // `report_from_csv`; assert the resulting markdown is non-
    // empty and carries the title from the call.
    use mockspace_bench_harness::{report_from_csv, write_csv};
    let tmp = std::env::temp_dir().join(format!(
        "mockspace_bench_csv_to_findings_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("create tempdir");
    let csv_path = tmp.join("samples.csv");
    let findings_path = tmp.join("findings.md");
    let result = BenchResult {
        title:       "round-trip-demo".into(),
        env:         EnvMeta::default(),
        samples:     synthetic_samples(),
        cache_path:  String::new(),
        report_path: String::new(),
    };
    write_csv(&result, csv_path.to_str().expect("utf-8 path"))
        .expect("write csv from synthetic result");
    report_from_csv(
        &csv_path,
        findings_path.to_str().expect("utf-8 path"),
        "warm",
        "round-trip-demo",
    )
    .expect("regenerate findings.md from csv");
    let md = fs::read_to_string(&findings_path).expect("read regenerated findings.md");
    assert!(
        md.contains("round-trip-demo"),
        "expected title in regenerated findings.md; got:\n{md}"
    );
    assert!(
        md.contains("alpha") && md.contains("beta"),
        "expected both variant names in tables; got:\n{md}"
    );
    let _ = fs::remove_dir_all(&tmp);
}
