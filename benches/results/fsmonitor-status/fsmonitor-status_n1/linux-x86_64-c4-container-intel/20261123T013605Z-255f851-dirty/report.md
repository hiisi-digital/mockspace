# git status with and without the data refs' monitor

4 variants, 60 samples per variant.
Baseline: **none_dev**

## Highlights

Baseline for all deltas below: **none_dev**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### none_dev dominates: 41% faster than the next best (none_old)

none_dev (3.11 ms) leads none_old (4.37 ms) by 41%, a clear separation rather than a photo finish. CV 21.2%.

_Why it matters:_ A dominant, well-separated winner is a safe default pick for this workload shape.

### monitor_old is an outlier: 21.2x slower than the field

monitor_old (65.86 ms) is 21.2x the fastest (3.11 ms), well off the pack.

_Why it matters:_ A >2x outlier is almost never the right choice; if it is intentional (e.g. it buys correctness), say so explicitly.

### monitor_old shows alternating (throttle bounce) (autocorr -0.64)

monitor_old's per-pass series has lag-1 autocorrelation -0.64, indicating alternating (throttle bounce). Its timing may not be at steady state.

_Why it matters:_ Autocorrelated samples violate the independence the CIs assume; the interval is optimistic until the drift is warmed out or cooled down.

### No variant beats the baseline (none_dev)

The baseline none_dev is the fastest (3.11 ms median); no rival improves on it (all deltas are >= 0).

_Why it matters:_ When nothing beats the baseline, the current choice stands; the contenders cost speed for whatever else they buy.

### Two tiers: {none_dev, none_old, monitor_dev} vs {monitor_old} (656% apart)

The field splits into a fast tier {none_dev, none_old, monitor_dev} and a slow tier {monitor_old} with a 656% jump between them - a qualitative difference, not a gradient.

_Why it matters:_ A tier split usually reflects a mechanism boundary (branchless vs branch, cached vs not); the tier, not the exact rank, is the finding.

### Wide spread: slowest is 21.2x the fastest

Fastest none_dev (3.11 ms) to slowest monitor_old (65.86 ms): 21.2x. The strategy choice matters a lot for this workload.

_Why it matters:_ A wide field means the strategy is load-bearing here; getting it right (or wrong) has large consequences.

### none_old is inconsistent: worst-20% is 1.9x its best-20%

none_old's best 20% of batches run at 3.16 ms but its worst 20% at 5.91 ms (1.9x) - a bimodal or bursty profile the median hides.

_Why it matters:_ A fat tail matters for latency budgets even when the median looks fine; a steadier variant may serve better under load.

## Key findings

- **Baseline (none_dev) is the fastest** at 3108423.3 ns median
- 3 variants significantly slower than baseline
- Spread: 21.19x (fastest 3108423.3 ns, slowest 65864894.9 ns)

## Methodology

| Parameter | Value |
|---|---|
| Workload | git status with and without the data refs' monitor |
| Passes | 3 |
| Runs per pass | 20 |
| Batch size | 1 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x18ea88c178b0e875 |
| Counter frequency | 2100468460 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| monitor_dev | 8747718ns | 8713953ns | 7661831ns | 8618548ns | 10221115ns | +178.39% |
| monitor_old | 77573266ns | 65869103ns | 57295222ns | 75812626ns | 103133229ns | +2368.73% |
| none_dev | 3142231ns | 3110268ns | 2426027ns | 3042144ns | 4158696ns | base |
| none_old | 4492049ns | 4375621ns | 3160312ns | 4463504ns | 5909421ns | +42.96% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| monitor_dev | 8744744ns | 7658993ns | 10217962ns | +178.53% | 0.000 |
| monitor_old | 77568589ns | 57290797ns | 103127655ns | +2370.68% | 0.000 |
| none_dev | 3139558ns | 2423805ns | 4156390ns | base | 0.000 |
| none_old | 4489571ns | 3157971ns | 5906667ns | +43.00% | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (none_dev; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| monitor_dev | 0.000 | 27.8% |
| monitor_old | 0.000 | 3.7% |
| none_dev | 0.000 | 78.0% |
| none_old | 0.000 | 55.4% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| monitor_dev | 8747718ns | 8747718ns | +178.39% |
| monitor_old | 77573266ns | 77573266ns | +2368.73% |
| none_dev | 3142231ns | 3142231ns | base |
| none_old | 4492049ns | 4492049ns | +42.96% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| none_dev | 3108423ns | base | --- | [2719631, 3310444] | --- | --- | --- | --- |
| monitor_dev | 8711010ns | +5538444.6ns (+178.2%) | [+5221806, +5939923]ns | [8206694, 8967934] | YES | 0.0000 | 0.0000 | 0 |
| monitor_old | 65864895ns | +63120366.6ns (+2030.6%) | [+59307528, +87855232]ns | [62251026, 91092399] | YES | 0.0000 | 0.0000 | 0 |
| none_old | 4373149ns | +1302717.1ns (+41.9%) | [+1019431, +1832011]ns | [4092131, 4841763] | YES | 0.0000 | 0.0000 | 0 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | none_dev | monitor_dev | monitor_old | none_old |
|---|---|---|---|---|---|
| 1 | 1 | 2626792ns | +246.6% | +2449.8% | +66.5% |
| 1 | 2 | 2887642ns | +175.4% | +3002.6% | +17.0% |
| 1 | 3 | 3484636ns | +152.3% | +1659.9% | +48.7% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| monitor_dev | -0.621 | HIGH- (thermal bounce) |
| monitor_old | -0.643 | HIGH- (thermal bounce) |
| none_dev | -0.032 | ok |
| none_old | -0.533 | HIGH- (thermal bounce) |

**Consistency summary:**

- **monitor_dev**: won 0/3, lost 3/3
- **monitor_old**: won 0/3, lost 3/3
- **none_old**: won 0/3, lost 3/3

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| monitor_dev | 706.9ns | 8744744.3ns | 0.0% |  |
| monitor_old | 1311.7ns | 77568589.5ns | 0.0% |  |
| none_dev | 485.8ns | 3139558.3ns | 0.0% |  |
| none_old | 567.4ns | 4489570.7ns | 0.0% |  |

## Distribution (algo ns)

```
monitor_dev (n=60, range 7658992.9-10217961.5 ns)
  7658992.9 |###############
  7786941.3 |########################################
  7914889.7 |###############
  8042838.2 |#####
  8170786.6 |####################
  8298735.0 |#####
  8426683.5 |###############
  8554631.9 |#####
  8682580.3 |##############################
  8810528.8 |##########
  8938477.2 |##########
  9066425.6 |#########################
  9194374.1 |#####
  9322322.5 |##########
  9450270.9 |##########
  9578219.3 |
  9706167.8 |####################
  9834116.2 |#####
  9962064.6 |##########
  10090013.1 |
  (5 below, 4 above range)

monitor_old (n=60, range 57290796.6-103127654.7 ns)
  57290796.6 |########################################
  59582639.5 |####################
  61874482.4 |################
  64166325.3 |#############
  66458168.2 |###
  68750011.1 |
  71041854.0 |
  73333697.0 |
  75625539.9 |
  77917382.8 |
  80209225.7 |
  82501068.6 |
  84792911.5 |
  87084754.4 |######
  89376597.3 |################
  91668440.2 |################
  93960283.1 |######
  96252126.0 |######
  98543968.9 |################
  100835811.8 |###
  (4 below, 6 above range)

none_dev (n=60, range 2423804.6-4156389.7 ns)
  2423804.6 |##################################
  2510433.9 |######################
  2597063.2 |########################################
  2683692.4 |#################
  2770321.7 |
  2856950.9 |#################
  2943580.2 |#####
  3030209.4 |#####
  3116838.7 |############################
  3203467.9 |###########
  3290097.2 |######################
  3376726.4 |#################
  3463355.7 |#################
  3549984.9 |
  3636614.2 |############################
  3723243.4 |
  3809872.7 |#####
  3896501.9 |#####
  3983131.2 |
  4069760.5 |#####
  (5 below, 5 above range)

none_old (n=60, range 3157970.6-5906666.7 ns)
  3157970.6 |########################
  3295405.4 |########################################
  3432840.2 |########
  3570275.0 |########
  3707709.8 |################################
  3845144.6 |################
  3982579.4 |################################
  4120014.2 |################
  4257449.0 |################
  4394883.8 |########################################
  4532318.7 |
  4669753.5 |########
  4807188.3 |################
  4944623.1 |################
  5082057.9 |################
  5219492.7 |########
  5356927.5 |################
  5494362.3 |################################
  5631797.1 |########################
  5769231.9 |################################
  (6 below, 4 above range)

```

## Diagnostics

- **monitor_old**: CV=24.3% (high variance, measurements may be unstable)
- **none_dev**: CV=21.0% (high variance, measurements may be unstable)
- **none_old**: CV=22.5% (high variance, measurements may be unstable)
