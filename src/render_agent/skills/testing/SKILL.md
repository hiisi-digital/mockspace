# Testing

`cargo mock test` is the test command. It runs each tree mockspace owns separately, reruns only what moved since it last passed, and records what passed in `{mock_dir}/test-history/`. A bare `cargo test` in `{mock_dir}/` reaches the members only.

## The trees

- Members of the mock workspace: under cargo-nextest where installed, else `cargo test` (`--plain` forces it and reaches doc tests).
- Tool crates under `{mock_dir}/tools/`.
- Bench crates under `{mock_dir}/benches/`: run only when their inputs moved since they last passed, or with `--benches`, `--full` or `--no-cache`. With `-p` they are left out unless `--benches` is given. `cargo mock bench test` runs them on their own.
- The generated lint crate carrying `{mock_dir}/lints/*.rs`; `cargo mock check` generates it.

## What reruns

- A member runs when its fingerprint moved: its own files, the paths outside it its sources name as `../` literals, the same for members compiled into its tests, and the lockfile, toolchain and cargo configuration.
- Inside a member the unit is the test binary. Each `tests/<name>.rs` and `tests/<name>/main.rs` is built from its own file, the modules it declares, the files its string literals name inside the member, and the member's library and dependencies. A binary that passed at unmoved inputs is left out (`reuse`).
- Shared, and so moving every binary of the member: the library and its unit tests, the manifest, the build script, and any file under `tests/` no binary claims.
- Wide, and so rebuilt on every claimed file of the member: a binary that reads the environment (`env!`, `option_env!`, `std::env`) or `file!()`, declares a module inside a macro or an inline module, or does not parse.
- A filter after `--` or an `-E` runs but records nothing as passed whole.

## Organising tests so selection works

- One concern per integration test binary. A change to one concern then reruns one binary.
- Shared helpers are a module each binary declares: `tests/common/mod.rs` and `mod common;` in the binary. A loose file under `tests/` nobody declares is shared and moves everything.
- Name a fixture by a literal path relative to the member or the test file (`"tests/fixtures/x.toml"`). A path built from `env!`, `std::env` or `file!()` makes the binary wide.
- A read outside the member is a `../` literal, so it is fingerprinted.
- Unit tests in the library rerun with every library change; keep the slow ones in integration binaries.
- `cargo mock test-units [<member>...]` reports each member's binaries, which are wide, and the files under `tests/` no binary claims. It changes nothing.

## Heavy tests

- A test that took `[test] heavy_after_secs` (10) or longer last time is heavy. Heavy tests run `[test] heavy_threads` (1) at a time, and one that passed at its binary's current fingerprint is skipped as cached.
- Every test is killed after `[test] timeout_secs` (900). The run ends with the slowest tests.

## Flags

- `--full` runs every member asked for, every binary. `--no-cache` also runs cached heavy tests and left-out binaries. Run `--full --no-cache` once at the end of a piece of work and once after merging the trunk.
- `--cheap` defers every heavy test that has passed before; deferred members stay owing them.
- `-p <member>` and `--workspace` choose members as in cargo.
- `--debug=<crate,...>` builds those with symbols and runs them; `--no-debug` gives none. Members build without debug info otherwise, except a member whose tests failed last time.
- Everything else after `test` goes to cargo: `cargo mock test --release`, `cargo mock test -- --nocapture`.
- `cargo mock help test` is the full text.

## Configuration

- `[test]` in `mockspace.toml`: `heavy_after_secs`, `heavy_threads`, `timeout_secs`, `commit` (warn runs `--cheap` at commit, error also blocks, off and info run nothing), `debug`, `lean`.
- `[test.opt_level]` maps a member to an opt level (0 to 3, `"s"`, `"z"`).
- `[test.history]` keeps the history on a git ref instead of the tree.

## The history

- `{mock_dir}/test-history/<flavour>/<member>.json`, committed with the work so a fresh clone starts warm. Never hand-edited. `--features x` has its own flavour.
- A merge or a refused commit touching it: the `test-history` skill.
