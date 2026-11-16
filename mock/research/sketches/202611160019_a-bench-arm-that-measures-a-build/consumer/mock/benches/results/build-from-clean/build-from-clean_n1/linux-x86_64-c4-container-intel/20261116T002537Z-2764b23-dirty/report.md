# build-from-clean: a fixture workspace of four crates built from an empty target directory

1 variants, 3 samples per variant.
Baseline: **from_clean**

## Methodology

| Parameter | Value |
|---|---|
| Workload | build-from-clean: a fixture workspace of four crates built from an empty target directory |
| Passes | 3 |
| Runs per pass | 1 |
| Batch size | 1 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x6d346275696c6401 |
| Counter frequency | 2300370926 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| from_clean | 122620466ns | 126145740ns | 114080837ns | -0ns | 122620466ns | base |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| from_clean | 122613465ns | 114073133ns | 122613465ns | base | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (from_clean; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| from_clean | 0.000 | 90.4% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| from_clean | 122620466ns | 122620466ns | base |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| from_clean | 126138775ns | base | --- | [114073133, 127628488] | --- | --- | --- | --- |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | from_clean |
|---|---|---|
| 1 | 1 | 126138775ns |
| 1 | 2 | 114073133ns |
| 1 | 3 | 127628488ns |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| from_clean | -0.660 | HIGH- (thermal bounce) |

**Consistency summary:**


## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| from_clean | 2668159.1ns | 122613465.3ns | 2.2% |  |

## Distribution (algo ns)

```
from_clean (n=3, range 114073133.1-122613465.3 ns)
  114073133.1 |########################################
  114500149.7 |
  114927166.3 |
  115354182.9 |
  115781199.5 |
  116208216.1 |
  116635232.8 |
  117062249.4 |
  117489266.0 |
  117916282.6 |
  118343299.2 |
  118770315.8 |
  119197332.4 |
  119624349.0 |
  120051365.6 |
  120478382.2 |
  120905398.8 |
  121332415.4 |
  121759432.0 |
  122186448.7 |
  (0 below, 2 above range)

```
