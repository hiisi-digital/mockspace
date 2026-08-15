# Probe 08: the precedence matrix, exercised

The sibling derivation named `compose_composed_member` as the largest surface
either of us read without exercising: 127 lines of hand-written precedence
resolving a sweep's declaration against its member's, with `merge_timing`
doing five more knobs and `for_size` applying the root's last. This exercises
it, over **both** member forms, on `origin/dev` (which now carries PR #21).

## Shape

Per field, four cases: declared nowhere, only below, only above, both. Per
timing knob, all eight combinations of root/member/inner.

## Negative controls, and one of them is the whole point

- **C1** D, L and H pairwise distinct. A bool has two inhabitants so H equals
  D; there the control becomes that `both` still returns H, which proves an
  explicit `Some(false)` is distinguished from `None` rather than swallowed
  by `unwrap_or`. That is the real `Option<bool>` hazard and it is checked.
- **C2** declaring only at the lower level must move the value. If it does
  not, that level is inert and "the higher level wins" is vacuously true.
- **C3** the same for the upper level.

## The instrument's own control, which is the reason to believe it

Run against `origin/feat/bench-consolidation`, the tree **before** the
sections-form timing fix, it reports exactly one failure and it is that
defect:

```
== sections: declaring ONE knob must not move the other four ==
  got : passes=8 runs_per_pass=50000 batch_size=5000 harness_runs=3 cooldowns_ms=[0, 100, 600]
  want: passes=8 runs_per_pass=77   batch_size=777  harness_runs=7 cooldowns_ms=[7]
```

`output-prefix-instrument-control.txt`. Against `origin/dev` every case
passes: `output-dev-postfix.txt`.

## The finding about how to test this surface

**The eight-combination timing matrix passes on the broken tree.** Look at the
sections rows in the pre-fix output: `-M-=8`, `RM-=8`, all correct. The matrix
varies one knob at a time, so the member declares the knob being read, and the
reset of the *other four* is invisible to it.

Only the cross-knob case sees it: root declares all five, member declares one,
the other four must stay at the root's. A per-knob matrix, however exhaustive
in its own dimension, is structurally blind to a defect that lives across
knobs. That is worth stating because a reviewer looking at a full 8-case
matrix would reasonably call the surface covered.

## Results on dev

Every field resolves inner-over-member, every timing knob resolves
inner-over-member-over-root, and one-knob isolation holds in both forms.
Nothing here is a defect report; the surface is correct as of `origin/dev`.

## One divergence between the forms, found by the scaffold rather than looked for

`workload` is **required** in the sections form (`BenchSection`, no serde
default) and **defaulted** in the composed form (`ComposedBench`,
`#[serde(default = "default_workload")]`). A section moved from one form to
the other without adding `workload` fails to parse:

```
TOML parse error at line 3, column 1
  |
3 | [bench.s]
  | ^^^^^^^^^
missing field `workload`
```

The sibling's F5 table records the two types' declarations correctly and does
not draw this consequence. It is one more instance of its point rather than a
new one.

## Scope

```
holds for: branches = { origin/dev @93f51bf, origin/feat/bench-consolidation },
           forms = { composed, sections }, fields = 8, timing knobs = 5,
           levels = { root, member, sweep-or-section }, threads = 1,
           host = darwin/aarch64
```

Not varied: root `[bench.*]` sections colliding with member keys (refused by
`insert_composed`, untested here), per-point `variants` overrides, nested
members, `exclude` patterns.

## Reproduction

```
cargo run -q     # repoint Cargo.toml's path dep first
```
Exit 0 means every case and every control passed.
