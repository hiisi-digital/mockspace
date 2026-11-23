# Bench Results Index

Generated from the runs under `results/`, one directory per run at `<bench>/<cell>/<machine class>/<run>/`, and regenerated from them on every run. Each run holds its samples, meta, report and summary.

## fsmonitor-checkout/fsmonitor-checkout_n1

### linux-x86_64-c4-container-intel, 2 run(s)

Latest: `20261123T015028Z-cbb0a98`

| variant | median | vs best |
|---|---|---|
| monitor_dev | 88.36 ms | 1.38x |
| monitor_old | 128.91 ms | 2.01x |
| none_dev | 84.79 ms | 1.32x |
| none_old | 64.24 ms | 1.00x |

- [20261123T014007Z-255f851-dirty](fsmonitor-checkout/fsmonitor-checkout_n1/linux-x86_64-c4-container-intel/20261123T014007Z-255f851-dirty/report.md)
- [20261123T015028Z-cbb0a98](fsmonitor-checkout/fsmonitor-checkout_n1/linux-x86_64-c4-container-intel/20261123T015028Z-cbb0a98/report.md)

## fsmonitor-status/fsmonitor-status_n1

### linux-x86_64-c4-container-intel, 2 run(s)

Latest: `20261123T014653Z-cbb0a98`

| variant | median | vs best |
|---|---|---|
| monitor_dev | 8.36 ms | 2.55x |
| monitor_old | 68.07 ms | 20.75x |
| none_dev | 3.28 ms | 1.00x |
| none_old | 3.90 ms | 1.19x |

- [20261123T013605Z-255f851-dirty](fsmonitor-status/fsmonitor-status_n1/linux-x86_64-c4-container-intel/20261123T013605Z-255f851-dirty/report.md)
- [20261123T014653Z-cbb0a98](fsmonitor-status/fsmonitor-status_n1/linux-x86_64-c4-container-intel/20261123T014653Z-cbb0a98/report.md)

## hash-mix/hash-mix_n1

### macos-aarch64-c8-bare-apple, 1 run(s)

- [20261115T135851Z-816a4da](hash-mix/hash-mix_n1/macos-aarch64-c8-bare-apple/20261115T135851Z-816a4da/report.md)

## hash-mix/hash-mix_n8

### linux-x86_64-c4-container-intel, 1 run(s)

Latest: `20261123T014511Z-4fcdf0d-dirty`

| variant | median | vs best |
|---|---|---|
| bitwise-rotate | 25 ns | 1.06x |
| multiply-xor | 23 ns | 1.00x |

- [20261123T014511Z-4fcdf0d-dirty](hash-mix/hash-mix_n8/linux-x86_64-c4-container-intel/20261123T014511Z-4fcdf0d-dirty/report.md)
