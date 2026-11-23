# git status with and without the data refs' monitor

4 variants, 60 samples per variant.
Baseline: **none_dev**

## Highlights

Baseline for all deltas below: **none_dev**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### none_dev dominates: 19% faster than the next best (none_old)

none_dev (3.28 ms) leads none_old (3.90 ms) by 19%, a clear separation rather than a photo finish. CV 17.7%.

_Why it matters:_ A dominant, well-separated winner is a safe default pick for this workload shape.

### monitor_old is an outlier: 20.8x slower than the field

monitor_old (68.07 ms) is 20.8x the fastest (3.28 ms), well off the pack.

_Why it matters:_ A >2x outlier is almost never the right choice; if it is intentional (e.g. it buys correctness), say so explicitly.

### monitor_old shows alternating (throttle bounce) (autocorr -0.66)

monitor_old's per-pass series has lag-1 autocorrelation -0.66, indicating alternating (throttle bounce). Its timing may not be at steady state.

_Why it matters:_ Autocorrelated samples violate the independence the CIs assume; the interval is optimistic until the drift is warmed out or cooled down.

### No variant beats the baseline (none_dev)

The baseline none_dev is the fastest (3.28 ms median); no rival improves on it (all deltas are >= 0).

_Why it matters:_ When nothing beats the baseline, the current choice stands; the contenders cost speed for whatever else they buy.

### Two tiers: {none_dev, none_old, monitor_dev} vs {monitor_old} (714% apart)

The field splits into a fast tier {none_dev, none_old, monitor_dev} and a slow tier {monitor_old} with a 714% jump between them - a qualitative difference, not a gradient.

_Why it matters:_ A tier split usually reflects a mechanism boundary (branchless vs branch, cached vs not); the tier, not the exact rank, is the finding.

### Wide spread: slowest is 20.8x the fastest

Fastest none_dev (3.28 ms) to slowest monitor_old (68.07 ms): 20.8x. The strategy choice matters a lot for this workload.

_Why it matters:_ A wide field means the strategy is load-bearing here; getting it right (or wrong) has large consequences.

### none_old is inconsistent: worst-20% is 1.8x its best-20%

none_old's best 20% of batches run at 3.09 ms but its worst 20% at 5.64 ms (1.8x) - a bimodal or bursty profile the median hides.

_Why it matters:_ A fat tail matters for latency budgets even when the median looks fine; a steadier variant may serve better under load.

## Key findings

- **Baseline (none_dev) is the fastest** at 3279726.0 ns median
- 3 variants significantly slower than baseline
- Spread: 20.75x (fastest 3279726.0 ns, slowest 68069591.8 ns)

## Methodology

| Parameter | Value |
|---|---|
| Workload | git status with and without the data refs' monitor |
| Passes | 3 |
| Runs per pass | 20 |
| Batch size | 1 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x18ea89584d50af6d |
| Counter frequency | 2100406678 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| monitor_dev | 8646432ns | 8363443ns | 7743788ns | 8385755ns | 10331109ns | +166.79% |
| monitor_old | 79631999ns | 68074888ns | 60708727ns | 76922344ns | 106684236ns | +2357.06% |
| none_dev | 3240947ns | 3281801ns | 2468086ns | 3224317ns | 4063698ns | base |
| none_old | 4142635ns | 3906617ns | 3092832ns | 3991786ns | 5644986ns | +27.82% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| monitor_dev | 8643498ns | 7741144ns | 10328524ns | +166.86% | 0.000 |
| monitor_old | 79625464ns | 60703959ns | 106679769ns | +2358.37% | 0.000 |
| none_dev | 3238957ns | 2466359ns | 4061655ns | base | 0.000 |
| none_old | 4140406ns | 3090911ns | 5642700ns | +27.83% | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (none_dev; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| monitor_dev | 0.000 | 29.5% |
| monitor_old | 0.000 | 3.6% |
| none_dev | 0.000 | 75.2% |
| none_old | 0.000 | 63.2% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| monitor_dev | 8646432ns | 8646432ns | +166.79% |
| monitor_old | 79631999ns | 79631999ns | +2357.06% |
| none_dev | 3240947ns | 3240947ns | base |
| none_old | 4142635ns | 4142635ns | +27.82% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| none_dev | 3279726ns | base | --- | [3050476, 3463431] | --- | --- | --- | --- |
| monitor_dev | 8361207ns | +5117689.1ns (+156.0%) | [+5022168, +5440319]ns | [8272020, 8485663] | YES | 0.0000 | 0.0000 | 0 |
| monitor_old | 68069592ns | +65109762.2ns (+1985.2%) | [+63390570, +88272873]ns | [66201848, 92692720] | YES | 0.0000 | 0.0000 | 0 |
| none_old | 3904071ns | +810446.4ns (+24.7%) | [+609832, +1084346]ns | [3454146, 4436217] | YES | 0.0000 | 0.0000 | 0 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | none_dev | monitor_dev | monitor_old | none_old |
|---|---|---|---|---|---|
| 1 | 1 | 3568642ns | +133.9% | +1823.0% | +38.1% |
| 1 | 2 | 3463078ns | +146.9% | +2648.9% | +3.2% |
| 1 | 3 | 2553238ns | +223.1% | +2468.6% | +26.3% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| monitor_dev | -0.602 | HIGH- (thermal bounce) |
| monitor_old | -0.661 | HIGH- (thermal bounce) |
| none_dev | -0.115 | ok |
| none_old | -0.069 | ok |

**Consistency summary:**

- **monitor_dev**: won 0/3, lost 3/3
- **monitor_old**: won 0/3, lost 3/3
- **none_old**: won 0/3, lost 3/3

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| monitor_dev | 652.8ns | 8643497.9ns | 0.0% |  |
| monitor_old | 1194.5ns | 79625464.1ns | 0.0% |  |
| none_dev | 538.9ns | 3238956.9ns | 0.0% |  |
| none_old | 495.6ns | 4140406.3ns | 0.0% |  |

## Distribution (algo ns)

```
monitor_dev (n=60, range 7741144.1-10328523.8 ns)
  7741144.1 |######
  7870513.1 |#############
  7999882.0 |##########
  8129251.0 |####################
  8258620.0 |########################################
  8387989.0 |#######################
  8517358.0 |#############
  8646727.0 |##########
  8776095.9 |######
  8905464.9 |###
  9034833.9 |###
  9164202.9 |######
  9293571.9 |
  9422940.9 |######
  9552309.9 |###
  9681678.8 |
  9811047.8 |
  9940416.8 |
  10069785.8 |
  10199154.8 |
  (6 below, 4 above range)

monitor_old (n=60, range 60703959.3-106679768.6 ns)
  60703959.3 |####################
  63002749.8 |####################################
  65301540.2 |########################################
  67600330.7 |################
  69899121.1 |########
  72197911.6 |####
  74496702.1 |
  76795492.5 |
  79094283.0 |
  81393073.5 |
  83691863.9 |
  85990654.4 |
  88289444.9 |####
  90588235.3 |
  92887025.8 |########
  95185816.2 |############
  97484606.7 |############
  99783397.2 |####################
  102082187.6 |####################
  104380978.1 |####
  (5 below, 4 above range)

none_dev (n=60, range 2466359.0-4061655.2 ns)
  2466359.0 |####################
  2546123.8 |####################
  2625888.6 |########################################
  2705653.4 |
  2785418.3 |
  2865183.1 |#############
  2944947.9 |#############
  3024712.7 |#############
  3104477.5 |#############
  3184242.3 |####################
  3264007.1 |####################
  3343771.9 |####################
  3423536.7 |#################################
  3503301.5 |####################
  3583066.3 |#############
  3662831.1 |####################
  3742595.9 |####################
  3822360.7 |######
  3902125.5 |######
  3981890.4 |#############
  (6 below, 5 above range)

none_old (n=60, range 3090910.6-5642700.3 ns)
  3090910.6 |########################################
  3218500.1 |###############
  3346089.6 |##############################
  3473679.1 |####################
  3601268.6 |#####
  3728858.0 |##########
  3856447.5 |
  3984037.0 |##########
  4111626.5 |
  4239216.0 |###############
  4366805.5 |###############
  4494395.0 |###############
  4621984.4 |#####
  4749573.9 |##############################
  4877163.4 |
  5004752.9 |##########
  5132342.4 |#####
  5259931.9 |#####
  5387521.4 |
  5515110.8 |
  (6 below, 8 above range)

```

## Diagnostics

- **monitor_old**: CV=23.7% (high variance, measurements may be unstable)
- **none_old**: CV=23.1% (high variance, measurements may be unstable)
