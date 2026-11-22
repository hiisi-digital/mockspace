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
| Counter frequency | 2300326470 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| from_clean | 141282012ns | 142463254ns | 136850928ns | -0ns | 141282012ns | base |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| from_clean | 141274139ns | 136843668ns | 141274139ns | base | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (from_clean; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| from_clean | 0.000 | 96.1% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| from_clean | 141282012ns | 141282012ns | base |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| from_clean | 142454351ns | base | --- | [136843668, 144524398] | --- | --- | --- | --- |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | from_clean |
|---|---|---|
| 1 | 1 | 144524398ns |
| 1 | 2 | 142454351ns |
| 1 | 3 | 136843668ns |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| from_clean | -0.044 | ok |

**Consistency summary:**


## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| from_clean | 2152468.6ns | 141274138.8ns | 1.5% |  |

## Distribution (algo ns)

```
from_clean (n=3, range 136843667.7-141274138.8 ns)
  136843667.7 |########################################
  137065191.3 |
  137286714.8 |
  137508238.4 |
  137729761.9 |
  137951285.5 |
  138172809.0 |
  138394332.6 |
  138615856.1 |
  138837379.7 |
  139058903.2 |
  139280426.8 |
  139501950.4 |
  139723473.9 |
  139944997.5 |
  140166521.0 |
  140388044.6 |
  140609568.1 |
  140831091.7 |
  141052615.2 |
  (0 below, 2 above range)

```
