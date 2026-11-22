# next-frame asymmetry: clean skip vs value-swap cone vs plan band

3 variants, 18 samples per variant.
Baseline: **sb_none**

## Highlights

Baseline for all deltas below: **sb_none**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### sb_none dominates: 25% faster than the next best (sb_plan)

sb_none (10.02 us) leads sb_plan (12.53 us) by 25%, a clear separation rather than a photo finish. CV 1.8%.

_Why it matters:_ A dominant, well-separated winner is a safe default pick for this workload shape.

### No variant beats the baseline (sb_none)

The baseline sb_none is the fastest (10.02 us median); no rival improves on it (all deltas are >= 0).

_Why it matters:_ When nothing beats the baseline, the current choice stands; the contenders cost speed for whatever else they buy.

## Key findings

- **Baseline (sb_none) is the fastest** at 10022.9 ns median
- 2 variants significantly slower than baseline
- Spread: 1.78x (fastest 10022.9 ns, slowest 17822.9 ns)

## Methodology

| Parameter | Value |
|---|---|
| Workload | next-frame asymmetry: clean skip vs value-swap cone vs plan band |
| Passes | 3 |
| Runs per pass | 30 |
| Batch size | 10 |
| Harness runs | 2 |
| Cooldown schedule | 0ms |
| Master seed | 0x51c3510700000002 |
| Counter frequency | 24000000 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| sb_none | 12636ns | 12596ns | 12093ns | 12512ns | 13094ns | base |
| sb_plan | 14991ns | 14894ns | 14488ns | 14850ns | 15454ns | +18.63% |
| sb_value | 20261ns | 20115ns | 19915ns | 20104ns | 20669ns | +60.34% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| sb_none | 10095ns | 9944ns | 10272ns | base | 0.006 |
| sb_plan | 12620ns | 12306ns | 12955ns | +25.02% | 0.005 |
| sb_value | 17816ns | 17617ns | 17971ns | +76.49% | 0.004 |

## Performance model

- Peak throughput: **0.006 Gops/s** (sb_none; best 20% batches)
- Ops per call: 64

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| sb_none | 0.006 | 99.2% |
| sb_plan | 0.005 | 79.4% |
| sb_value | 0.004 | 55.8% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| sb_none | 12636ns | 12636ns | base |
| sb_plan | 14991ns | 14991ns | +18.63% |
| sb_value | 20261ns | 20261ns | +60.34% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| sb_none | 10023ns | base | --- | [9977, 10127] | --- | --- | --- | --- |
| sb_plan | 12531ns | +2499.9ns (+24.9%) | [+2444, +2556]ns | [12438, 12667] | YES | 0.0000 | 0.0000 | 0 |
| sb_value | 17823ns | +7779.2ns (+77.6%) | [+7619, +7854]ns | [17700, 17915] | YES | 0.0000 | 0.0000 | 0 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | sb_none | sb_plan | sb_value |
|---|---|---|---|---|
| 1 | 1 | 9979ns | +25.1% | +79.7% |
| 1 | 2 | 10146ns | +24.7% | +75.6% |
| 1 | 3 | 10121ns | +25.1% | +76.9% |
| 2 | 1 | 9975ns | +24.7% | +79.7% |
| 2 | 2 | 10021ns | +24.5% | +78.0% |
| 2 | 3 | 10133ns | +25.6% | +73.8% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| sb_none | -0.205 | moderate- |
| sb_plan | -0.187 | ok |
| sb_value | 0.031 | ok |

**Consistency summary:**

- **sb_plan**: won 0/6, lost 6/6
- **sb_value**: won 0/6, lost 6/6

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| sb_none | 351504.4ns | 10094.7ns | 3482.1% | HIGH |
| sb_plan | 354672.2ns | 12620.4ns | 2810.3% | HIGH |
| sb_value | 352623.6ns | 17816.2ns | 1979.2% | HIGH |

## Distribution (algo ns)

```
sb_none (n=18, range 9944.5-10272.2 ns)
   9944.5 |##########
   9960.9 |####################
   9977.2 |##########
   9993.6 |
  10010.0 |########################################
  10026.4 |
  10042.8 |
  10059.2 |
  10075.6 |
  10091.9 |
  10108.3 |##############################
  10124.7 |##########
  10141.1 |##########
  10157.5 |
  10173.9 |
  10190.3 |
  10206.7 |##########
  10223.0 |
  10239.4 |
  10255.8 |
  (2 below, 2 above range)

sb_plan (n=18, range 12305.5-12954.9 ns)
  12305.5 |##########################
  12338.0 |#############
  12370.5 |
  12402.9 |
  12435.4 |##########################
  12467.9 |########################################
  12500.3 |
  12532.8 |
  12565.3 |#############
  12597.7 |#############
  12630.2 |##########################
  12662.7 |#############
  12695.1 |#############
  12727.6 |
  12760.1 |
  12792.5 |
  12825.0 |
  12857.5 |
  12889.9 |#############
  12922.4 |
  (1 below, 2 above range)

sb_value (n=18, range 17616.7-17970.8 ns)
  17616.7 |
  17634.4 |########################################
  17652.1 |####################
  17669.8 |
  17687.5 |
  17705.2 |
  17722.9 |####################
  17740.6 |
  17758.3 |####################
  17776.0 |
  17793.8 |####################
  17811.5 |####################
  17829.2 |####################
  17846.9 |####################
  17864.6 |
  17882.3 |
  17900.0 |########################################
  17917.7 |########################################
  17935.4 |####################
  17953.1 |
  (2 below, 2 above range)

```

## Diagnostics

- **sb_none**: bridge=3488.4% of algo (FFI overhead may distort results)
- **sb_plan**: bridge=2828.3% of algo (FFI overhead may distort results)
- **sb_value**: bridge=1975.3% of algo (FFI overhead may distort results)
