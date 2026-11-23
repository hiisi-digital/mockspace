# u64 hash-mix function comparison

2 variants, 40 samples per variant.
Baseline: **bitwise-rotate**

## Highlights

Baseline for all deltas below: **bitwise-rotate**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### Baseline (bitwise-rotate) is the SLOWEST variant; every rival beats it

The declared/defaulted baseline bitwise-rotate has the worst median (25 ns). Every delta is therefore measured against the worst performer, which flatters all rivals and compresses the differences that matter among them (e.g. fastest multiply-xor at 23 ns).

_Why it matters:_ A baseline picked by accident (often the first variant to run / sort) silently skews every comparison. Re-baseline via `[bench.<name>.normalise]` on a representative variant.

### multiply-xor is fastest but the noisiest (CV 12.5%)

multiply-xor wins on median (23 ns) yet has the highest variance (CV 12.5%), while bitwise-rotate is the steadiest (CV 6.7%, 25 ns).

_Why it matters:_ For latency-sensitive or tail-bound paths, the steadier variant can beat the faster-on-average one; weigh peak vs consistency.

### Whole-field spread is below the measurement noise floor

The fastest-to-slowest gap (1 ns) is smaller than the fastest variant's own run-to-run std-dev (3 ns); the ranking is inside the noise.

_Why it matters:_ When the spread is below resolution, any apparent ordering is likely noise; increase work per call before trusting a winner.

### Speed leader multiply-xor vs stability leader bitwise-rotate (+5% speed for 1.9x steadier)

multiply-xor is fastest (23 ns, CV 12.5%); bitwise-rotate gives up 5.4% median for 1.9x lower variance (CV 6.7%).

_Why it matters:_ The pick depends on priority: peak throughput vs predictable latency. Both are defensible; name which the workload needs.

## Key findings

- **Fastest: multiply-xor** at 23.3 ns median (-5.1% vs baseline)
- 1 variant significantly faster than baseline
- Spread: 1.05x (fastest 23.3 ns, slowest 24.6 ns)

## Methodology

| Parameter | Value |
|---|---|
| Workload | u64 hash-mix function comparison |
| Passes | 4 |
| Runs per pass | 1000 |
| Batch size | 100 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x123456789abcdef0 |
| Counter frequency | 2100484574 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| bitwise-rotate | 156ns | 147ns | 127ns | 141ns | 227ns | base |
| multiply-xor | 157ns | 134ns | 127ns | 139ns | 240ns | +0.61% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| bitwise-rotate | 25ns | 23ns | 27ns | base | 0.319 |
| multiply-xor | 24ns | 21ns | 29ns | -3.16% | 0.330 |

## Performance model

- Peak throughput: **0.378 Gops/s** (multiply-xor; best 20% batches)
- Ops per call: 8

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| bitwise-rotate | 0.326 | 86.3% |
| multiply-xor | 0.343 | 90.9% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| bitwise-rotate | 156ns | 156ns | base |
| multiply-xor | 157ns | 157ns | +0.61% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| bitwise-rotate | 25ns | base | --- | [24, 26] | --- | --- | --- | --- |
| multiply-xor | 23ns | -1.5ns (-6.1%) | [-2, -0]ns | [23, 23] | YES | 0.0022 | 0.0022 | 0 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | bitwise-rotate | multiply-xor |
|---|---|---|---|
| 1 | 1 | 27ns | -15.0% |
| 1 | 2 | 26ns | +11.4% |
| 1 | 3 | 23ns | -9.4% |
| 1 | 4 | 24ns | -0.9% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| bitwise-rotate | 0.277 | moderate+ |
| multiply-xor | -0.477 | moderate- |

**Consistency summary:**

- **multiply-xor**: won 3/4, lost 1/4

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| bitwise-rotate | 46.4ns | 25.0ns | 185.4% | HIGH |
| multiply-xor | 46.3ns | 24.2ns | 191.0% | HIGH |

## Distribution (algo ns)

```
bitwise-rotate (n=40, range 23.4-27.4 ns)
     23.4 |########################################
     23.6 |
     23.8 |####
     24.0 |
     24.2 |####
     24.4 |####
     24.6 |####
     24.8 |####
     25.0 |
     25.2 |########
     25.4 |
     25.6 |
     25.8 |####
     26.0 |#############
     26.2 |####
     26.4 |
     26.6 |####
     26.8 |
     27.0 |####
     27.2 |#############
  (8 below, 6 above range)

multiply-xor (n=40, range 21.2-29.2 ns)
     21.2 |#################
     21.6 |
     22.0 |
     22.4 |
     22.8 |
     23.2 |########################################
     23.6 |####
     24.0 |
     24.4 |
     24.8 |
     25.2 |
     25.6 |
     26.0 |
     26.4 |
     26.8 |
     27.2 |
     27.6 |
     28.0 |##
     28.4 |##
     28.8 |###########
  (2 below, 3 above range)

```

## Diagnostics

- **bitwise-rotate**: bridge=180.2% of algo (FFI overhead may distort results)
- **multiply-xor**: bridge=176.8% of algo (FFI overhead may distort results)
