# git checkout with and without the data refs' monitor

4 variants, 60 samples per variant.
Baseline: **none_dev**

## Highlights

Baseline for all deltas below: **none_dev**. (Deltas are paired `variant - baseline` medians; `*` marks a CI that excludes zero.)

### none_old dominates: 32% faster than the next best (none_dev)

none_old (64.24 ms) leads none_dev (84.79 ms) by 32%, a clear separation rather than a photo finish. CV 26.9%.

_Why it matters:_ A dominant, well-separated winner is a safe default pick for this workload shape.

### monitor_old is an outlier: 2.0x slower than the field

monitor_old (128.91 ms) is 2.0x the fastest (64.24 ms), well off the pack.

_Why it matters:_ A >2x outlier is almost never the right choice; if it is intentional (e.g. it buys correctness), say so explicitly.

### monitor_old shows alternating (throttle bounce) (autocorr -0.67)

monitor_old's per-pass series has lag-1 autocorrelation -0.67, indicating alternating (throttle bounce). Its timing may not be at steady state.

_Why it matters:_ Autocorrelated samples violate the independence the CIs assume; the interval is optimistic until the drift is warmed out or cooled down.

### none_old is inconsistent: worst-20% is 1.9x its best-20%

none_old's best 20% of batches run at 53.66 ms but its worst 20% at 99.90 ms (1.9x) - a bimodal or bursty profile the median hides.

_Why it matters:_ A fat tail matters for latency budgets even when the median looks fine; a steadier variant may serve better under load.

## Key findings

- **Fastest: none_old** at 64238720.2 ns median (-24.2% vs baseline)
- 2 variants significantly slower than baseline
- Spread: 2.01x (fastest 64238720.2 ns, slowest 128913356.1 ns)

## Methodology

| Parameter | Value |
|---|---|
| Workload | git checkout with and without the data refs' monitor |
| Passes | 3 |
| Runs per pass | 20 |
| Batch size | 1 |
| Harness runs | 1 |
| Cooldown schedule | 0ms |
| Master seed | 0x18ea898a53fa6277 |
| Counter frequency | 2100421012 Hz |
| Drift correction | none |

## End-to-end (all cooldowns combined)

| Variant | mean | median | best 20% | mid 60% | worst 20% | Δ mean |
|---|---|---|---|---|---|---|
| monitor_dev | 85482306ns | 88367763ns | 63549656ns | 83928990ns | 112074905ns | +9.68% |
| monitor_old | 147364579ns | 128918127ns | 115893201ns | 135634528ns | 214026108ns | +89.08% |
| none_dev | 77937988ns | 84794393ns | 56028983ns | 77423640ns | 101390039ns | base |
| none_old | 71129107ns | 64243775ns | 53666173ns | 67359363ns | 99901270ns | -8.74% |

## Function-under-test only (all cooldowns combined)

| Variant | mean | best 20% | worst 20% | Δ mean | throughput (Gops/s) |
|---|---|---|---|---|---|
| monitor_dev | 85477775ns | 63545207ns | 112070476ns | +9.68% | 0.000 |
| monitor_old | 147359227ns | 115888604ns | 214020307ns | +89.08% | 0.000 |
| none_dev | 77932940ns | 56024125ns | 101383550ns | base | 0.000 |
| none_old | 71124609ns | 53661527ns | 99896702ns | -8.74% | 0.000 |

## Performance model

- Peak throughput: **0.000 Gops/s** (none_old; best 20% batches)
- Ops per call: 1

| Variant | Gops/s (median) | % of peak |
|---|---|---|
| monitor_dev | 0.000 | 60.7% |
| monitor_old | 0.000 | 41.6% |
| none_dev | 0.000 | 63.3% |
| none_old | 0.000 | 83.5% |

## Per-cooldown breakdown (e2e mean)

| Variant | 0ms | avg | Δ avg |
|---|---|---|---|
| monitor_dev | 85482306ns | 85482306ns | +9.68% |
| monitor_old | 147364579ns | 147364579ns | +89.08% |
| none_dev | 77937988ns | 77937988ns | base |
| none_old | 71129107ns | 71129107ns | -8.74% |

## Statistical comparison (algo, 95% bootstrap CI)

| Variant | median | Δ median | Δ CI | 95% CI | sig? | adj. p | sign p | ties |
|---|---|---|---|---|---|---|---|---|
| none_dev | 84789248ns | base | --- | [65403159, 89391358] | --- | --- | --- | --- |
| monitor_dev | 88363520ns | +8480529.3ns (+10.0%) | [+5779118, +10635184]ns | [71007228, 92010315] | YES | 0.0001 | 0.0000 | 0 |
| monitor_old | 128913356ns | +65236871.0ns (+76.9%) | [+57823395, +71613371]ns | [123861248, 133137297] | YES | 0.0000 | 0.0000 | 0 |
| none_old | 64238720ns | no significant difference | [-8412224, +237800]ns | [59329737, 72875880] | no | 0.0925 | 0.0925 | 0 |

## Per-pass consistency (nonstop algo, Δ vs baseline)

| Run | Pass | none_dev | monitor_dev | monitor_old | none_old |
|---|---|---|---|---|---|
| 1 | 1 | 88298818ns | -0.4% | +38.3% | -17.5% |
| 1 | 2 | 68704627ns | +35.2% | +158.7% | -13.0% |
| 1 | 3 | 75035166ns | -7.2% | +61.5% | -22.4% |

**Autocorrelation (lag-1) per-pass series:**

| Variant | r₁ | note |
|---|---|---|
| monitor_dev | -0.294 | moderate- |
| monitor_old | -0.667 | HIGH- (thermal bounce) |
| none_dev | -0.373 | moderate- |
| none_old | -0.115 | ok |

**Consistency summary:**

- **monitor_dev**: won 2/3, lost 1/3
- **monitor_old**: won 0/3, lost 3/3
- **none_old**: won 3/3, lost 0/3

## Bridge overhead per variant

| Variant | mean bridge | algo mean | bridge % | flag |
|---|---|---|---|---|
| monitor_dev | 1112.2ns | 85477775.2ns | 0.0% |  |
| monitor_old | 1277.0ns | 147359227.3ns | 0.0% |  |
| none_dev | 1155.2ns | 77932939.9ns | 0.0% |  |
| none_old | 1094.9ns | 71124608.9ns | 0.0% |  |

## Distribution (algo ns)

```
monitor_dev (n=60, range 63545207.0-112070476.1 ns)
  63545207.0 |########################################
  65971470.5 |########################################
  68397733.9 |########################################
  70823997.4 |########################
  73250260.8 |
  75676524.3 |########
  78102787.7 |########
  80529051.2 |
  82955314.7 |########
  85381578.1 |################
  87807841.6 |########################################
  90234105.0 |################################
  92660368.5 |################################
  95086631.9 |########################
  97512895.4 |################
  99939158.8 |########
  102365422.3 |########
  104791685.7 |################
  107217949.2 |########
  109644212.6 |################
  (6 below, 6 above range)

monitor_old (n=60, range 115888604.0-214020306.7 ns)
  115888604.0 |########################################
  120795189.1 |##############################
  125701774.3 |################
  130608359.4 |#######################
  135514944.5 |###
  140421529.7 |
  145328114.8 |
  150234699.9 |###
  155141285.1 |
  160047870.2 |##########
  164954455.3 |###
  169861040.5 |
  174767625.6 |###
  179674210.7 |#############
  184580795.9 |######
  189487381.0 |######
  194393966.1 |
  199300551.3 |
  204207136.4 |###
  209113721.5 |
  (6 below, 5 above range)

none_dev (n=60, range 56024125.2-101383549.7 ns)
  56024125.2 |########################################
  58292096.4 |###
  60560067.6 |##########
  62828038.8 |######
  65096010.1 |##########
  67363981.3 |######
  69631952.5 |###
  71899923.8 |
  74167895.0 |###
  76435866.2 |
  78703837.4 |
  80971808.7 |
  83239779.9 |######
  85507751.1 |##########
  87775722.4 |####################
  90043693.6 |#############
  92311664.8 |################
  94579636.0 |###
  96847607.3 |##########
  99115578.5 |######
  (4 below, 5 above range)

none_old (n=60, range 53661527.2-99896702.2 ns)
  53661527.2 |################
  55973285.9 |########################################
  58285044.7 |####################
  60596803.4 |####################
  62908562.2 |
  65220320.9 |####
  67532079.7 |################
  69843838.4 |####
  72155597.2 |########################
  74467355.9 |####
  76779114.7 |########
  79090873.4 |
  81402632.2 |
  83714391.0 |
  86026149.7 |
  88337908.5 |####
  90649667.2 |####
  92961426.0 |####
  95273184.7 |################
  97584943.5 |############
  (6 below, 5 above range)

```

## Diagnostics

- **monitor_dev**: CV=21.0% (high variance, measurements may be unstable)
- **monitor_old**: CV=27.2% (high variance, measurements may be unstable)
- **none_dev**: CV=23.0% (high variance, measurements may be unstable)
- **none_old**: CV=24.3% (high variance, measurements may be unstable)
