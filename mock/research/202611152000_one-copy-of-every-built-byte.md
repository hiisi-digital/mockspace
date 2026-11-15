# One copy of every built byte: what cargo offers mockspace

The part of a workspace-wide reading that is about mockspace. The whole of it, with the plan, the measurements, the probe and the table of third-party tools, is in the clause-dev workspace under `.shared/research/202611152000_one-copy-of-every-built-byte/` (`README.md`, `cargo-and-rustc-options.md`, `third-party-tools.md`, `sketches/cargo-sharing-probe/`), and every source is saved there under `.data/fetched/` by the hash given here. Fetched and checked on 15 November 2026 against the pin `nightly-2026-05-28`, cargo 1.98.0-nightly (fbb61be30 2026-05-26). This is v1 throughout, the code under `src/` and `cargo-mock/`.

Op asked that one of each built part be on disk and that sharing and cleanup happen inside the harness with nobody doing them by hand. Mockspace is where most of the second copies are made, because it builds three things into directories of its own: the engine the launcher starts, the generated lint cdylib, and the bench driver with its arms.

## What cargo gives for it

`build.build-dir` (`CARGO_BUILD_BUILD_DIR`) is the directory for intermediate artifacts, apart from `target-dir`, which then holds only final ones (Cargo Book, Configuration and Build Cache, 51789a6532a8 and 443474bb4343, read). On the pin it needs no flag, and two builds with a target directory each and one build directory compiled a crate once and kept one rlib (probe line 11, checked). So a mockspace build that must find its output at a path of its own can keep that path and still compile nothing the consumer's workspace already compiled.

Cargo tells two builds of one crate apart by feature set, `RUSTFLAGS`, profile and toolchain, and not by where the checkout sits (probe lines 1 to 10, checked). A generated crate built with flags or a toolchain other than the consumer's therefore shares nothing with it, whatever directory it builds into. `resolver.feature-unification = "workspace"` under `-Z feature-unification` makes a `-p` build ask the same features of a dependency as the whole workspace does (Unstable Features, 2398f93b7ed0, read; the flag is on the pin, its effect on a real consumer not yet checked), which matters to `cargo mock test`, since it builds member by member.

Cargo cleans its global cache by itself and never a target directory (2398f93b7ed0, and the 2023 announcement 2c458c742038, read). What mockspace writes outside cargo, the verdict store and the generated crates, is mockspace's own to remove.

## What it means in this tree

The lint cdylib's target is resolved in `src/custom_lints/build.rs` (the target root is marked at `:231`) and the bench targets in `src/bench_gen.rs:422` and `:464`, both through `crate::build_dir`, which is mockspace's own module and not cargo's setting. The fix is for these and the launcher to pass the consumer's build directory to cargo and keep only a final directory of their own; plan items M1, M2 and M5 in the workspace file.

`--generate-only` already renders the agent files with no `cargo check` and no lints (`src/entry/dispatch.rs:983`, `:1094`, `:1169`, `:1724`). Homma's `agent regen` runs a bare `cargo mock` and so checks the whole consumer; the change is on homma's side, item H4, and nothing is owed here beyond keeping the mode.

## The third-party tools that touch mockspace

| Tool | What it does | Last release | Verdict for mockspace | Sources |
|---|---|---|---|---|
| cargo-hakari 0.9.39 | Generates a workspace-hack crate so members ask the same features of shared dependencies | 2026-09-19 | Held in reserve behind cargo's own feature unification, which needs no generated crate | 95ef5799fc5d, 986d77e9385d |
| cargo-shear 1.14.1 | Finds and fixes unused dependencies without building | 2026-11-13 | A candidate asked tool for a pack, like deny; later | d94fa044f23f |
| sccache 0.19.0 | Compiler wrapper with its own cache | 2026-10-24 | Not adopted: a second store, and it caches no cdylib, bin or proc macro, which is what mockspace generates | 160bc047ccfd, efc65855d154 |
| cargo-nextest 0.9.152 | Test runner with archives of built tests | 2026-11-13 | Not adopted: the suite runner and test history are mockspace's own | 0d33d7d6729b |

holds for: cargo 1.98.0-nightly (fbb61be30 2026-05-26) where a line says checked, the nightly Cargo Book and the crates.io records of 15 November 2026 where it says read; mockspace v1 at `origin/dev` of that day.
