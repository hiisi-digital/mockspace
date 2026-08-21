# Probe 06: an undeclared point is discovered by aborting

The arm ABI is three symbols, and the harness looks up exactly those:
`bench_abi_hash`, `bench_entry`, `bench_name` (`bench-harness/src/harness.rs:122-138`,
declared at `bench-macro/src/lib.rs:321-345`). **None of them reports which
`n` the arm was compiled for.**

`#[bench_variant(..., sizes = [...])]` builds an `n` dispatch table whose
fallback arm is a `panic!` (`bench-macro/src/lib.rs:328-333`). The message is
good and names both remedies. But `bench_entry` is `extern "C"`, so the panic
is a non-unwinding one: the observed result is **SIGABRT, exit 134**, with the
useful line buried under `panic in a function that cannot unwind` and a
seventeen-frame backtrace.

So a manifest whose points list has drifted from an arm's attribute is caught
at the innermost point of the run, in a subprocess, after every arm has been
built, by killing the process.

**And the harness could not have caught it earlier**, because there is no
symbol to ask. That makes this a missing export rather than a missing check.

## Negative control

n=64, the point the arm does declare, must return cleanly. It does (exit 0).
Without that, the abort would be equally consistent with a broken build.

## Scope

`holds for: arm declaring one point, harness point differing, target
macOS aarch64, rustc as pinned by the checkout, arms 1, edition 2024.`
Not varied: platform, arity, the routine form of the attribute.

## Addendum, phase two: this probe also shows the three arm identities diverging

Used in the reconciliation for a claim it was not built for, so recorded here.

The arm in this probe sits in a directory named `arm`, builds to `libarm.dylib`, and exports
`bench_name` = `"only64"`. Three identities, all different:

```
$ nm -gU arm/target/release/libarm.dylib | grep bench
0000000000000888 T _bench_abi_hash
000000000000089c T _bench_entry
0000000000000c00 T _bench_name
$ strings arm/target/release/libarm.dylib | grep -x only64
only64
```

`Sample::variant`'s doc comment (`bench-harness/src/sample.rs:32-36`) says the third is the one that
matters: "the name the variant's cdylib exports through its `bench_name` symbol, not anything derived
from its path. Every grouping downstream keys on this string."

Nothing checks that the three agree.
