# Probe 05: what of the public surface anything calls

Classifies every module-level `pub fn`/`struct`/`enum` in the four bench
crates by whether anything outside its declaring file references it, searching
the framework and the four consumer bench trees.

Both negative controls pass: `drive` classifies CALLED with 7 consumer call
sites, a fabricated name classifies UNREFERENCED.

## What it proves, and what it does not

**It proves reach by name.** It cannot see a type used through method calls or
type inference. `ProgramBuilder` (`workload.rs`) classifies REEXPORT-ONLY and
is genuinely used: kirjo's driver reaches it as the closure argument of
`w.program(|b| ...)` (`kirjo/mock/benches/src/main.rs:19-31`) without naming
it. A config section reachable only as a field of an exported struct
(`DispatchSection`, `DocgenSection`, `NormaliseSection`) classifies
UNREFERENCED for the same reason and is likewise fine.

So the 38 non-CALLED rows are **not** 38 dead items, and the file must not be
cited as though they were.

## The one row that was checked by hand and holds

**`bench-harness/src/cache.rs`, 463 lines, has no caller.** All seven of its
module-level public items classify REEXPORT-ONLY, and a direct grep for
`cache::` across the framework and all four consumers returns only a doc
comment at `config.rs:779` and the `pub use` at `lib.rs:66`. Nothing in the
driver, the harness or any consumer constructs a `Cache`, calls `dylib_hash`,
or calls `consensus_drift`.

That matters twice over. The round's changelist already defers "wiring the
cache's skip-rerun system into the driver", so the module is intended rather
than abandoned. But while it is unreached it carries a **second, drifting copy
of the samples CSV codec**: `cache.rs:400-431` is line-for-line
`sample.rs:108-141` apart from the signature, an empty-line guard and an
inverted condition, and `cache.rs:433` duplicates `harness.rs:772`.

## Reproduction

```
FW=<path to a feat/bench-consolidation checkout> bash run.sh
```
