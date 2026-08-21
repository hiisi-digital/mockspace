# Probe 04: the per-arm target directory, and the one parameter that fixes it

`bench_tree::arm_target_dir` (`bench-harness/src/tree.rs:590-595`) gives every
arm its own `target/mock-arms/<bench>/<arm>`, and `cargo_build_at`
(`src/bench.rs:508-534`) passes it as `--target-dir` on a per-arm cargo
invocation. Every arm therefore recompiles every dependency it shares with its
siblings.

On disk in arvo today, under the pre-PR21 equivalent shape: **90 per-variant
target directories totalling 2.4 GB**, each carrying the same six rlibs
(`mockspace_bench_core`, `syn`, `quote`, `proc_macro2`, `unicode_ident`, the
bench's support crate). Counted with
`ls -d variants/*/target | wc -l`, `du -sh variants`, and
`find variants/<v>/target -name '*.rlib'`.

## The obvious fix is wrong, and this probe is what killed it

Putting the arms in one cargo workspace compiles the shared crate **once** and
that single compile is the contamination: arm-a asks for the support crate's
`fast` feature, arm-b and arm-c do not, and feature unification hands all three
the same rlib. Arm-b would then be measured against code it did not ask for.
That is the real reason per-arm isolation exists, and it is control (3).

## The fix that survives it

Keep one cargo invocation per arm. Give them **one shared `--target-dir`**.
Cargo then shares an artifact only when the fingerprint matches, so arms with
identical feature resolution share and arms that differ get their own build:

| shape | compilations of the shared crate, 3 arms, 2 feature sets |
|---|---|
| A one workspace, one invocation | 1 (wrong: b and c get a's feature) |
| B per-arm invocation, one shared target dir | **2** (correct and shared) |
| C per-arm invocation, per-arm target dir (today) | 3 |

Isolation is unaffected under B: `nm` shows no arm resolving `shared::common`
across the artifact boundary; each cdylib remains a closed linkage unit under
fat LTO.

## Scope of the claim

`holds for: cargo 1.9x on macOS aarch64, crate-type cdylib, profile
release+lto=fat+codegen-units=1, dependency graph a path dependency with a
feature axis, arms any, invocation one-per-arm.` Not varied: linker, target
triple, registry dependencies, build scripts, `-Z` flags, parallelism.

## Reproduction

```
bash run.sh
```

Exit status 0 means all three controls passed. Wall-clock time is deliberately
not reported: that would be a measurement, it was not taken on the mockspace
bench harness, and under this workspace's rules it could not be called one.
The counts above are counts of compilation units, not timings.
