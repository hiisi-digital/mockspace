# build-without-clean: the control, the same fixture built without removing the target directory

1 variants, 3 samples per variant.
Baseline: **without_clean**

## Methodology

| Parameter | Value |
|---|---|
| Workload | build-without-clean: the control, the same fixture built without removing the target directory |
| Passes | 3 |
| Runs per pass | 1 |
| Batch size | 1 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x6d346275696c6402 |
| Counter frequency | 2300277632 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| without_clean | 22587160ns | 22582515ns | 19910000ns | -0ns | 22587160ns | base |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| without_clean | 22578077ns | 19898883ns | 22578077ns | base | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (without_clean; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| without_clean | 0.000 | 88.1% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| without_clean | 22587160ns | 22587160ns | base |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| without_clean | 22574424ns | base | --- | [19898883, 25260924] | --- | --- | --- | --- |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | without_clean |
|---|---|---|
| 1 | 1 | 22574424ns |
| 1 | 2 | 19898883ns |
| 1 | 3 | 25260924ns |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| without_clean | -0.499 | moderate- |

**Consistency summary:**


## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| without_clean | 49658.7ns | 22578077.0ns | 0.2% |  |

## Distribution (algo ns)

```
without_clean (n=3, range 19898883.1-22578077.0 ns)
  19898883.1 |########################################
  20032842.8 |
  20166802.5 |
  20300762.2 |
  20434721.9 |
  20568681.6 |
  20702641.3 |
  20836601.0 |
  20970560.7 |
  21104520.4 |
  21238480.1 |
  21372439.7 |
  21506399.4 |
  21640359.1 |
  21774318.8 |
  21908278.5 |
  22042238.2 |
  22176197.9 |
  22310157.6 |
  22444117.3 |########################################
  (0 below, 1 above range)

```
