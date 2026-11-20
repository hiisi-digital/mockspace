# u64 hash-mix function comparison

2 variants, 40 samples per variant.
Baseline: **bitwise-rotate**

## Highlights

Baseline for all deltas below: **bitwise-rotate**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### Baseline (bitwise-rotate) is the SLOWEST variant; every rival beats it

The declared/defaulted baseline bitwise-rotate has the worst median (5 ns). Every delta is therefore measured against the worst performer, which flatters all rivals and compresses the differences that matter among them (e.g. fastest multiply-xor at 4 ns).

_Why it matters:_ A baseline picked by accident (often the first variant to run / sort) silently skews every comparison. Re-baseline via `[bench.<name>.normalise]` on a representative variant.

### multiply-xor is fastest but the noisiest (CV 63.9%)

multiply-xor wins on median (4 ns) yet has the highest variance (CV 63.9%), while bitwise-rotate is the steadiest (CV 45.6%, 5 ns).

_Why it matters:_ For latency-sensitive or tail-bound paths, the steadier variant can beat the faster-on-average one; weigh peak vs consistency.

### Whole-field spread is below the measurement noise floor

The fastest-to-slowest gap (1 ns) is smaller than the fastest variant's own run-to-run std-dev (3 ns); the ranking is inside the noise.

_Why it matters:_ When the spread is below resolution, any apparent ordering is likely noise; increase work per call before trusting a winner.

### multiply-xor is inconsistent: worst-20% is 4.0x its best-20%

multiply-xor's best 20% of batches run at 2 ns but its worst 20% at 9 ns (4.0x) - a bimodal or bursty profile the median hides.

_Why it matters:_ A fat tail matters for latency budgets even when the median looks fine; a steadier variant may serve better under load.

## Key findings

- **Fastest: multiply-xor** at 4.2 ns median (-16.0% vs baseline)
- Spread: 1.19x (fastest 4.2 ns, slowest 5.0 ns)

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| bitwise-rotate | 256ns | 252ns | 196ns | 245ns | 350ns | base |
| multiply-xor | 229ns | 198ns | 163ns | 210ns | 352ns | -10.60% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| bitwise-rotate | 5ns | 3ns | 9ns | base | 0.183 |
| multiply-xor | 5ns | 2ns | 9ns | -8.58% | 0.201 |

## Performance model

- Peak throughput: **0.435 Gops/s** (multiply-xor; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| bitwise-rotate | 0.200 | 46.0% |
| multiply-xor | 0.238 | 54.8% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| bitwise-rotate | 256ns | 256ns | base |
| multiply-xor | 229ns | 229ns | -10.60% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| bitwise-rotate | 5ns | base | --- | [4, 6] | --- | --- | --- | --- |
| multiply-xor | 4ns | no significant difference | [-1, +0]ns | [4, 5] | no | 0.1433 | 0.1433 | 2 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | bitwise-rotate | multiply-xor |
|---|---|---|---|
| 1 | 1 | 8ns | -13.3% |
| 1 | 2 | 5ns | -30.0% |
| 1 | 3 | 5ns | -4.3% |
| 1 | 4 | 4ns | +13.5% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| bitwise-rotate | 0.056 | ok |
| multiply-xor | -0.315 | moderate- |

**Consistency summary:**

- **multiply-xor**: won 3/4, lost 1/4

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| bitwise-rotate | 1.6ns | 5.5ns | 28.9% | HIGH |
| multiply-xor | 1.4ns | 5.0ns | 27.2% | HIGH |

## Distribution (algo ns)

```
bitwise-rotate (n=40, range 2.8-9.1 ns)
      2.8 |################
      3.1 |################################
      3.4 |################################
      3.7 |
      4.1 |########
      4.4 |########################################
      4.7 |################
      5.0 |
      5.3 |########################################
      5.6 |
      6.0 |################
      6.3 |
      6.6 |########################
      6.9 |
      7.2 |
      7.5 |
      7.8 |################
      8.2 |################
      8.5 |
      8.8 |########
  (3 below, 4 above range)

multiply-xor (n=40, range 2.3-9.2 ns)
      2.3 |####################
      2.6 |####################
      3.0 |##########################
      3.3 |
      3.7 |#############
      4.0 |########################################
      4.4 |######
      4.7 |
      5.1 |##########################
      5.4 |
      5.8 |##########################
      6.1 |#############
      6.4 |######
      6.8 |
      7.1 |
      7.5 |
      7.8 |#############
      8.2 |######
      8.5 |
      8.9 |
  (4 below, 3 above range)

```

## Diagnostics

- **bitwise-rotate**: CV=41.8% (high variance, measurements may be unstable)
- **bitwise-rotate**: worst_20/best_20 = 3.3x (possible bimodal distribution)
- **bitwise-rotate**: bridge=34.0% of algo (FFI overhead may distort results)
- **multiply-xor**: CV=53.8% (high variance, measurements may be unstable)
- **multiply-xor**: worst_20/best_20 = 4.0x (possible bimodal distribution)
- **multiply-xor**: bridge=28.6% of algo (FFI overhead may distort results)
