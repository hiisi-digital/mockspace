# What the gate actually costs, and why I am not writing a cache for it

Ad-hoc quick spike, `202608222000_probes/what_the_gate_costs.sh`, run against
kamu. **Not a bench**: no harness, no arms, no competitors, no artifact trail.
It answers "is this worth optimising at all" and nothing finer. Re-run it rather
than trusting the numbers below, that is what it is there for.

## The numbers

| what | cost |
|---|---|
| whole warm gate, `cargo mock check` | 1.0 to 1.2s |
| the cargo spawn inside it, warm | 0.03s |
| fresh tree, empty cache | 17.8s, 20 crates, 59 files in `deps/`, 47 MB |
| fresh tree, seeded shared cache | 16.4s |

## So no fingerprint cache

The plan going in was to fingerprint everything that feeds the cdylib, stamp it
beside the artifact, and skip the cargo spawn when nothing moved. Then I measured
the spawn and it is **30 milliseconds** out of a full second. Writing a cache to
save 3% of a gate, where the cache is the exact mechanism whose invalidation bug
we just spent a PR fixing, is a terrible trade and I am not doing it. Cargo's own
freshness check is already the cache and it is already fast.

Note to self and anyone else: I nearly built this before measuring it. The whole
reason it looked worth building is that the cold case is genuinely slow, and I
carried that feeling straight across to the warm case without checking. Measure
the case you are actually optimising.

## What the shared cache is worth, which is less than I said

I wrote in a commit message that the cdylib graph is 159 crates and 121 MB and
that pinning `--target-dir` was what saturated the machine. Both wrong and worth
saying so plainly.

**159 was files in `deps/`, not crates.** The real graph is 20 crates, 59 files,
47 MB for a cold build. 121 MB was kamu's accumulated gen target dir, which has
more in it than one cold build produces.

**And the cache saves about 8%, not the machine.** 17.8s cold against 16.4s with
a seeded cache. Almost all of it is the seven project crates plus lint-rules,
whose fingerprints differ per tree path, and a shared cache cannot help those.
The artifact-path fix is still right, on the stale-artifact ground it was
actually about. It is not the performance win I dressed it up as.

## What was actually eating the machine

Spotlight. Two throwaway build trees under `/private/tmp` with no
`.metadata_never_index` put four `mdworker_shared` and `mds` on the cpu at once,
and `mds_stores` was still at 180% cleaning up after I deleted them. kamu had a
marker in `mock/target/` already, put there by hand, which is the tell: the
engine generates the tree, so the engine should mark it, and no project should
have to know the desktop indexer exists. That is `build_dir::target_dir` now.

The other half was the check script in kamu building a fresh tree per arm at
cargo's default parallelism, six of them fanned out. Already fixed there: one
tree, reused, children capped.

## What is still open

A project that wants to prove its own gate fires has to copy the tree, and pays
16 to 18s for it even warm, because the tool crates are path deps and a new path
is a new fingerprint. That is the wrong shape: the arms only ever change
`mock/registry/`, which does not feed the cdylib at all, so nothing should need
rebuilding.

The engine has no way to run the gate against a different registry root without
relocating the whole mock dir (`registry/load.rs` derives it from `mock_dir`).
Giving it one would make "plant the defect, watch it fire" cheap for every
project, which matters because nine of kamu's previous twenty-four checks could
not detect the defect they named and nobody found out until somebody mutated
them. Not designing that here; it wants its own round.
