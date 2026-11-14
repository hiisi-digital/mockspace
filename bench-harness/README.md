# `mockspace-bench-harness`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> The side of the mockspace benches that loads the variants, runs them and reads what they did: isolation, validation, timing, statistics and the findings.

</div>

Each variant of a bench is its own cdylib built against `mockspace-bench-core`, and this crate is what loads them. Every variant runs in a worker subprocess of its own, is checked against the others on the same seeded input before anything is timed, and is then timed over several passes with cooldowns between them. What comes out is a CSV of every sample, a metadata file, an entry in an append-only history and a `findings.md` with the medians, bootstrap intervals on them and on the paired differences, a sign test and the corrections for testing many pairs at once. A mismatch in validation stops the bench, because two variants that compute different things cannot be compared on speed.

Most consumers never call it directly. `cargo mock bench run` generates a driver from `bench.toml` and the driver calls into here. A member with a driver of its own uses `driver::drive` with its routines registered, and `staging::StagedBuild` to refuse running anything but a build staged into a directory no other build writes into.

## Usage

The crate is not on crates.io, so a consumer takes it from git, at the same revision as the `mockspace-bench-core` its variants link:

```toml
[dependencies]
mockspace-bench-harness = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev" }
```

A driver of its own registers how a bench section maps to a routine and hands over:

```rust
use mockspace_bench_harness::driver::{DriverRegistry, drive};

fn main() -> std::process::ExitCode {
    drive(&DriverRegistry {
        build_workload,
        routine_for,
        byte_dispatch,
    })
}
```

## Extras

### Cargo features

| Feature | Default | Effect |
|---|---|---|
| `perf-counters` | off | Reads hardware performance counters where the platform allows it, `kperf` on macOS and `perf_event_open` on Linux. |
| `stage-link` | off | `StagedBuild::link_into`, pointing a member's `target` at its staged build for the length of a run, on the targets `build.rs` names. |

## Support

Whether you use this project, have learned something from it, or just like it, please consider supporting it by buying me a coffee, so I can dedicate more time on open-source projects like this :)

<a href="https://buymeacoffee.com/orgrinrt" target="_blank"><img src="https://www.buymeacoffee.com/assets/img/custom_images/orange_img.png" alt="Buy Me A Coffee" style="height: auto !important;width: auto !important;" ></a>

## License

> The project is licensed under the Mozilla Public License 2.0.

`SPDX-License-Identifier: MPL-2.0`

> You can check out the full license [here](https://github.com/hiisi-digital/mockspace/blob/dev/LICENSE)
