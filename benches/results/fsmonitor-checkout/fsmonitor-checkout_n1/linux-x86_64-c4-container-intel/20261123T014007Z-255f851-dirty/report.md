# git checkout with and without the data refs' monitor

4 variants, 60 samples per variant.
Baseline: **none_dev**

## Highlights

Baseline for all deltas below: **none_dev**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### monitor_old is an outlier: 2.1x slower than the field

monitor_old (125.93 ms) is 2.1x the fastest (61.02 ms), well off the pack.

_Why it matters:_ A >2x outlier is almost never the right choice; if it is intentional (e.g. it buys correctness), say so explicitly.

### none_old is fastest but the noisiest (CV 30.7%)

none_old wins on median (61.02 ms) yet has the highest variance (CV 30.7%), while monitor_old is the steadiest (CV 29.4%, 125.93 ms).

_Why it matters:_ For latency-sensitive or tail-bound paths, the steadier variant can beat the faster-on-average one; weigh peak vs consistency.

### monitor_dev shows alternating (throttle bounce) (autocorr -0.67)

monitor_dev's per-pass series has lag-1 autocorrelation -0.67, indicating alternating (throttle bounce). Its timing may not be at steady state.

_Why it matters:_ Autocorrelated samples violate the independence the CIs assume; the interval is optimistic until the drift is warmed out or cooled down.

### Two tiers: {none_old, none_dev, monitor_dev} vs {monitor_old} (83% apart)

The field splits into a fast tier {none_old, none_dev, monitor_dev} and a slow tier {monitor_old} with a 83% jump between them - a qualitative difference, not a gradient.

_Why it matters:_ A tier split usually reflects a mechanism boundary (branchless vs branch, cached vs not); the tier, not the exact rank, is the finding.

### none_dev is inconsistent: worst-20% is 1.9x its best-20%

none_dev's best 20% of batches run at 50.97 ms but its worst 20% at 97.47 ms (1.9x) - a bimodal or bursty profile the median hides.

_Why it matters:_ A fat tail matters for latency budgets even when the median looks fine; a steadier variant may serve better under load.

## Key findings

- **Fastest: none_old** at 61022579.4 ns median (-3.0% vs baseline)
- 2 variants significantly slower than baseline
- Spread: 2.06x (fastest 61022579.4 ns, slowest 125932691.9 ns)

## Methodology

| Parameter | Value |
|---|---|
| Workload | git checkout with and without the data refs' monitor |
| Passes | 3 |
| Runs per pass | 20 |
| Batch size | 1 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x18ea88f9a5151819 |
| Counter frequency | 2100470732 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| monitor_dev | 82889753ns | 68772648ns | 60376784ns | 81455193ns | 109706402ns | +13.28% |
| monitor_old | 135077300ns | 125937216ns | 112352437ns | 125586086ns | 186275807ns | +84.59% |
| none_dev | 73175386ns | 62886019ns | 50974027ns | 72474563ns | 97479214ns | base |
| none_old | 71311046ns | 61027918ns | 53927712ns | 67191538ns | 101052906ns | -2.55% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| monitor_dev | 82884742ns | 60371749ns | 109700994ns | +13.28% | 0.000 |
| monitor_old | 135072490ns | 112347819ns | 186270908ns | +84.60% | 0.000 |
| none_dev | 73169080ns | 50969276ns | 97468339ns | base | 0.000 |
| none_old | 71306361ns | 53922965ns | 101048233ns | -2.55% | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (none_dev; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| monitor_dev | 0.000 | 74.1% |
| monitor_old | 0.000 | 40.5% |
| none_dev | 0.000 | 81.1% |
| none_old | 0.000 | 83.5% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| monitor_dev | 82889753ns | 82889753ns | +13.28% |
| monitor_old | 135077300ns | 135077300ns | +84.59% |
| none_dev | 73175386ns | 73175386ns | base |
| none_old | 71311046ns | 71311046ns | -2.55% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| none_dev | 62881645ns | base | --- | [60023268, 86318646] | --- | --- | --- | --- |
| monitor_dev | 68767723ns | +9550324.6ns (+15.2%) | [+5784753, +11602835]ns | [65855765, 100884056] | YES | 0.0000 | 0.0000 | 0 |
| monitor_old | 125932692ns | +62800911.0ns (+99.9%) | [+58554376, +67888864]ns | [122047319, 128641118] | YES | 0.0000 | 0.0000 | 0 |
| none_old | 61022579ns | no significant difference | [-1453945, +3849315]ns | [59380548, 65298779] | no | 0.8974 | 0.8974 | 0 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | none_dev | monitor_dev | monitor_old | none_old |
|---|---|---|---|---|---|
| 1 | 1 | 61429785ns | +11.5% | +107.4% | +0.6% |
| 1 | 2 | 75198838ns | +33.9% | +68.4% | -22.2% |
| 1 | 3 | 83826770ns | -20.0% | +40.8% | -26.0% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| monitor_dev | -0.666 | HIGH- (thermal bounce) |
| monitor_old | -0.128 | ok |
| none_dev | -0.012 | ok |
| none_old | -0.665 | HIGH- (thermal bounce) |

**Consistency summary:**

- **monitor_dev**: won 1/3, lost 2/3
- **monitor_old**: won 0/3, lost 3/3
- **none_old**: won 2/3, lost 1/3

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| monitor_dev | 1159.6ns | 82884741.5ns | 0.0% |  |
| monitor_old | 1142.1ns | 135072490.3ns | 0.0% |  |
| none_dev | 1124.4ns | 73169080.1ns | 0.0% |  |
| none_old | 1292.9ns | 71306361.1ns | 0.0% |  |

## Distribution (algo ns)

```
monitor_dev (n=60, range 60371748.7-109700993.6 ns)
  60371748.7 |########################
  62838211.0 |########################################
  65304673.2 |################
  67771135.5 |############################
  70237597.7 |####
  72704059.9 |
  75170522.2 |
  77636984.4 |
  80103446.7 |
  82569908.9 |
  85036371.2 |
  87502833.4 |
  89969295.6 |
  92435757.9 |
  94902220.1 |####
  97368682.4 |############
  99835144.6 |########
  102301606.9 |################
  104768069.1 |############################
  107234531.3 |################
  (5 below, 6 above range)

monitor_old (n=60, range 112347819.5-186270908.2 ns)
  112347819.5 |###################################
  116043973.9 |##############################
  119740128.3 |##############################
  123436282.8 |###################################
  127132437.2 |########################################
  130828591.6 |###################################
  134524746.1 |#####
  138220900.5 |#####
  141917054.9 |#####
  145613209.4 |##########
  149309363.8 |
  153005518.3 |
  156701672.7 |
  160397827.1 |
  164093981.6 |
  167790136.0 |
  171486290.4 |#####
  175182444.9 |##########
  178878599.3 |
  182574753.7 |##########
  (6 below, 3 above range)

none_dev (n=60, range 50969276.1-97468338.9 ns)
  50969276.1 |######################
  53294229.2 |#################
  55619182.4 |#################
  57944135.5 |########################################
  60269088.7 |##################################
  62594041.8 |###########
  64918994.9 |
  67243948.1 |#####
  69568901.2 |
  71893854.3 |
  74218807.5 |
  76543760.6 |
  78868713.8 |
  81193666.9 |######################
  83518620.0 |#####
  85843573.2 |#################
  88168526.3 |###########
  90493479.5 |#################
  92818432.6 |#################
  95143385.7 |########################################
  (6 below, 5 above range)

none_old (n=60, range 53922964.7-101048233.5 ns)
  53922964.7 |###############################
  56279228.1 |###################################
  58635491.6 |########################################
  60991755.0 |###############################
  63348018.5 |########
  65704281.9 |####
  68060545.3 |
  70416808.8 |####
  72773072.2 |####
  75129335.7 |
  77485599.1 |
  79841862.5 |
  82198126.0 |
  84554389.4 |
  86910652.9 |####
  89266916.3 |
  91623179.7 |######################
  93979443.2 |########
  96335706.6 |########
  98691970.1 |######################
  (5 below, 4 above range)

```

## Diagnostics

- **monitor_dev**: CV=25.4% (high variance, measurements may be unstable)
- **monitor_old**: CV=27.4% (high variance, measurements may be unstable)
- **none_dev**: CV=25.4% (high variance, measurements may be unstable)
- **none_old**: CV=26.2% (high variance, measurements may be unstable)
