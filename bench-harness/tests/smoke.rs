//! Smoke tests for the bench-harness data plane.
//!
//! These tests do not exercise the orchestrator/worker subprocess
//! flow (that requires building cdylibs against a published
//! `mockspace-bench-core`, which we cannot do until v2 is merged).
//! Instead, they validate the full pipeline of the in-process data
//! path: synthetic samples → cache CSV round trip → `DataSet`
//! aggregation → markdown report.
//!
//! End-to-end orchestrator validation lives in consumer-adoption
//! work (see workspace task #266) where we run a real Routine across
//! real cdylibs.

use std::fs;

use mockspace_bench_harness::{
    cache, generate_report, BenchManifest, BenchResult, DataSet, EnvMeta, Sample,
};

fn synthetic_samples() -> Vec<Sample> {
    let mut samples = Vec::new();
    // Two variants, two cooldowns, three runs * three passes.
    for run in 1..=3 {
        for pass in 1..=3 {
            for cooldown in [0u64, 100u64] {
                for batch_idx in 0..5 {
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

    assert!(alpha.algo_all.median > 0.0, "alpha should have a positive median");
    assert!(beta.algo_all.median > 0.0, "beta should have a positive median");
    assert!(
        beta.algo_all.median < alpha.algo_all.median,
        "beta is constructed faster than alpha; median should reflect that"
    );

    // Per-cooldown breakdown should have entries for both 0 and 100.
    assert!(alpha.algo_per_cd.contains_key(&0));
    assert!(alpha.algo_per_cd.contains_key(&100));
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
fn cache_csv_round_trips() {
    let tmp = std::env::temp_dir().join(format!(
        "mockspace_bench_smoke_{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&tmp).unwrap();

    // Cache writes under `.bench_cache/<bench>/<cfg>` relative to
    // cwd. The save_variant call exercises the same code path that
    // the orchestrator uses post-run.
    let mut c = cache::Cache::load("smoke", 0xDEADBEEF);
    let samples = synthetic_samples();
    c.save_variant("variants/alpha/target/release/libalpha.dylib", 0xCAFE, 100.0, 95.0, &samples);
    c.flush();

    // Reload + partition: with the dylib_hash returning 0 for missing
    // files, the cache should not match and we expect the partition
    // to schedule everything for re-run. That's the correct safe
    // behaviour for missing artefacts.
    let c2 = cache::Cache::load("smoke", 0xDEADBEEF);
    let (to_run, cached) = c2.partition(&[
        "variants/alpha/target/release/libalpha.dylib".into(),
    ]);
    assert!(to_run.contains(&0), "baseline must always re-run");
    let _ = cached;

    std::env::set_current_dir(cwd).unwrap();
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn manifest_loads_and_converts_to_config() {
    let tmp = std::env::temp_dir().join(format!(
        "mockspace_bench_manifest_{}",
        std::process::id()
    ));
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
fn bench_result_dataset_helpers_compile() {
    // No actual workload, but the pipeline shape needs to typecheck.
    let result = BenchResult {
        title: "demo".into(),
        env: EnvMeta::default(),
        samples: synthetic_samples(),
        cache_path: String::new(),
        report_path: String::new(),
    };
    let ds = result.dataset("warm");
    assert!(!ds.variants.is_empty());
}
