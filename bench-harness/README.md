# `mockspace-bench-harness`

<div align="center" style="text-align: center;">

[![GitHub Stars](https://img.shields.io/github/stars/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/stargazers)
[![GitHub Issues](https://img.shields.io/github/issues/hiisi-digital/mockspace.svg)](https://github.com/hiisi-digital/mockspace/issues)
![License](https://img.shields.io/github/license/hiisi-digital/mockspace?color=%23009689)

> The side of the mockspace benches that loads the variants, runs them and reads what they did: isolation, validation, timing, statistics and the findings.

</div>

Each variant of a bench is its own cdylib built against `mockspace-bench-core`, and this crate is what loads them. Every variant runs in a worker subprocess of its own, is checked against the others on the same seeded input before anything is timed, and is then timed over several passes with cooldowns between them. What comes out is one directory per run, `results/<bench>/<sweep>_n<point>/<machine class>/<run>/`, holding a CSV of every sample, a metadata file, a summary of the medians that is that run's piece of the history, and a report with the medians, bootstrap intervals on them and on the paired differences, a sign test and the corrections for testing many pairs at once. A mismatch in validation stops the bench, because two variants that compute different things cannot be compared on speed.

Consumers never call it directly. `cargo mock bench run` generates the driver from `bench.toml`, and that driver, which calls `driver::drive_spec`, is the only runner there is: a bench tree with a driver of its own is refused. A run is never replaced by a later one, and runs taken on different machines sit beside each other, so a cell's history is read across its runs on the same class of machine.

## Usage

The crate is not on crates.io, so a consumer takes it from git, at the same revision as the `mockspace-bench-core` its variants link:

```toml
[dependencies]
mockspace-bench-harness = { git = "https://github.com/hiisi-digital/mockspace", branch = "dev" }
```

What a bench needs beyond its `bench.toml` files goes in the tree's hooks library, `src/lib.rs` beside the root `bench.toml`, which the generated driver compiles in:

```rust
use mockspace_bench_harness::driver::Hooks;

pub fn hooks() -> Hooks {
    Hooks {
        routine_for: Some(routine_for),
        ..Hooks::default()
    }
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
