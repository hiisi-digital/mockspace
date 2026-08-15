# Bench Ergonomics Survey: What Consumers Copy, What the Tool Should Own

A survey of every mockspace bench consumer in the clause-dev workspace, a count
of what each one hand-writes that the tool could generate or infer, and a
proposal for the consolidation the maintainer asked for: a `bench.toml`-driven
default where mockspace owns the driver binary the way it already owns the
custom-lint collect lib, with the consumer lib entrypoint entirely optional.
This is a proposal, not a decision; the maintainer ratifies.

The survey covers input ergonomics (manifest, driver, variants) and output
layout (the flat artifact roots, `results/`, `.bench_history/`), per the
follow-up direction. Line references are against the workspace clones as of
2026-08-15; consumer paths are written `<repo>/mock/benches/...`.

## 1. The survey

Five bench trees exist. Four consumers (`arvo`, `hilavitkutin`, `kirjo`,
`vehje`, each at `<repo>/mock/benches/`) plus mockspace's own self-bench at
`mockspace/benches/` (repo root, because mockspace's `mock/` is v2-only).

| tree | bench.toml lines | `[bench.*]` sections | variant refs (total / unique) | driver src lines | variant crates | root entries | flat root artifacts (csv+meta+findings) | uses library driver |
|---|---|---|---|---|---|---|---|---|
| arvo | 3018 | 49 | 1305 / 81 | 672 | 94 | 768 | 254+254+254 | no |
| hilavitkutin | 773 | 29 | 365 / 82 | 311 (+355 disasm_5check +8 lib) | 110 | 444 | 144+144+144 | no |
| vehje | 4955 | 337 (180 merged names) | 5170 / 820 | 51 | 902 | 77 | 0 (uses `results/`, 220 subdirs) | yes |
| kirjo | 50 | 1 | 4 / 2 | 60 | 2 | 7 | 0 (uses `results/`) | yes |
| mockspace (self) | 28 | 1 | 2 / 2 | 207 | 2 | 6 | 1+1 flat | no |

The split is clean and it is chronological. `vehje` and `kirjo` sit on
`mockspace_bench_harness::driver::drive` and are already close to the intended
shape: a 51-line and a 60-line `main.rs` that is registrations only. `arvo`,
`hilavitkutin`, and mockspace's own self-bench predate the driver and each
carries a private copy of the loop the driver was extracted to kill; the
driver's own module docs name the drift classes exactly
(`bench-harness/src/driver/mod.rs:4-9`: "hand-grown size whitelists, hardcoded
may-differ name lists, stale helper snapshots").

### 1a. bench.toml: 257 of 258 per-size variant lists carry no information

The dominant manifest shape in the three big trees is the array-of-tables
form, one `[[bench.<name>.sizes]]` block per N, each repeating the full
variant list:

- arvo repeats variant paths 1305 times for 81 distinct variants across 256
  `[[sizes]]` blocks (zero short names; every entry is a
  `"../target/release/<libstem>"` path, e.g. `arvo/mock/benches/bench.toml:6-32`,
  where the same two paths are copied into four consecutive size blocks).
- vehje repeats 5170 entries for 820 distinct variants across 1132 blocks.
- hilavitkutin repeats 365 entries for 82 distinct variants across 128 blocks.

Parsed and counted: **of the 258 benches using this form, exactly one (in
arvo) has a variant set that actually differs between its sizes.** For the
other 257 the repetition is pure copy maintenance. The concise form already
exists and already parses: bench-level `variants = [...]` plus a plain integer
`sizes = [64, 256]` array (`bench-harness/src/config.rs:84-90`, `de_sizes` at
`config.rs:213-235`, per-size override at `config.rs:358`). The redundancy is
not a missing feature; it is three trees written before or without the concise
form, kept consistent by hand ever since.

Second redundancy, same file: a reader must keep the manifest's size list
consistent with the driver binary's `byte_routine_dispatch!(sizes = [...])`
declaration by hand. The same numbers are written in `bench.toml` and in
`main.rs` (`vehje/mock/benches/src/main.rs:42-49` against every `sizes` array
in `vehje/mock/benches/bench.toml`), and the only reconciliation is a runtime
error (`driver/mod.rs:136-145`).

Third: variant naming is inconsistent per tree, and the manifest is coupled to
the build layout. hilavitkutin and vehje use short names and
`variants/<dir>/target/release/<stem>` paths; arvo uses
`../target/release/<libstem>` paths that point into `mock/target/release`, a
location that is an accident of workspace membership. When arvo's mock
workspace `members` list was emptied, every bench silently broke
("current package believes it's in a workspace when it's not"), which
`arvo/mock/Cargo.toml`'s own comment documents at length, and the tree is now
split-brained: 12 dylibs in `mock/target/release` from the members era, 90
variant-local `target/` dirs from the exclusion era, while `bench.toml` still
points all 1305 entries at the former.

Fourth: one variant has up to four names. In arvo, directory
`variants/bitpack-aligned-rand/`, package `bench-bitpack-aligned-rand`, lib
`bench_bitpack_aligned_rand`, and the manifest refers to the lib stem
(`arvo/mock/benches/variants/bitpack-aligned-rand/Cargo.toml:2,8`). Short-name
resolution assumes dir name == lib name (`config.rs:244-267`), which arvo's
tree violates throughout, so arvo cannot use short names at all without a
rename pass.

### 1b. The driver binary: two consumers still own a dead copy of the loop

Against `STARTER_BIN_MAIN` (`src/bench.rs:511-565`, 55 lines) and the library
driver (`bench-harness/src/driver/`, 603+281+100+80 lines):

- **arvo, 672 lines.** Lines 16-201 are a hand-rolled manifest loop that the
  library driver wholly subsumes (selection, report-only, seed, csv+findings
  writing), minus features it lacks (no preflight, no staging, no history, no
  summary, no duplicate-name guard). Lines 597-672 are the worker dispatch that
  `drive_worker` subsumes. Lines 206-595 are a 360-arm
  `(name, n) => routine_bridge!(Type<KEY>)` match table where the `n` in each
  arm's pattern must equal the const argument by hand, and where the only real
  information per bench is one type name. The file also carries, as comments,
  the archaeology of what hand-rolling cost: the double dylib-prefix bug that
  made every bench fail with `TIMEOUT/<load-fail>` (`main.rs:76-82`), the
  discovery that its loop never called `validate` (`main.rs:134-144`), and the
  discovery that it never called the disasm duplicate check (`main.rs:150-157`).
  Every one of those is already wired in the library driver
  (`driver/mod.rs:376-435, 391, 455-474`).
- **hilavitkutin, 311 lines** plus an 8-line lib and 355 lines of genuinely
  custom disasm_5check. The main is the same dead loop, plus
  `shape_variant_path` (`main.rs:251-260`) re-implementing
  `resolve_variant_path`'s suffix logic (the duplication that produced arvo's
  `liblibfoo.dylib.dylib` class), plus the hardcoded 13-name `may_differ` list
  (`main.rs:213-216`) duplicating a per-bench manifest flag that already exists
  (`config.rs:96`). The 18-arm `ByteRoutine` ladder (`main.rs:217-243`) is
  byte-for-byte what `byte_routine_dispatch!` generates.
- **mockspace's own self-bench, 207 lines**, is the same pre-driver loop with
  flat outputs. The reference implementation does not use its own current
  machinery, which is worth fixing for its own sake: it is what a reader copies.
- **vehje, 51 lines and kirjo, 60 lines** are the target shape: workload
  builder, `routine_for` hook, one `byte_routine_dispatch!`. Of vehje's 51
  lines, the only content that is genuinely vehje's is the workload stage mix
  and the 16-number size list, roughly 10 lines.

Boilerplate fraction, counted against what the library already provides: arvo
about 280 of 672 lines are a strictly worse copy of library code, and the
360-arm table is mechanically compressible about 10 to 1 (see section 4).
hilavitkutin about 250 of 311. vehje and kirjo about 40 of each 51 and 60 (the
identical scaffold around the 10 real lines).

### 1c. Variant crates: one datum per manifest, three dep signatures per tree

1064 variant `src/lib.rs` files across the workspace use `#[bench_variant]`;
only 2 hand-write the `no_mangle` exports. The lib.rs side is already thin:
the measured code plus one attribute. The manifests are the copy mass:

- vehje's 902 variant `Cargo.toml`s: two sampled at random are byte-identical
  after substituting the crate name. Across all 902 there are exactly three
  dependency signatures (594 core+macro+matrix+`vehje-bench-carrier`, 291
  core+macro, 15 core+macro+shared). Each 17-line manifest carries one datum
  (its name) plus membership in one of three dep groups.
- hilavitkutin: 79 of its ~110 variant manifests are exactly 21 lines.
- The mockspace git pin is copied per-manifest workspace-wide: 1787 manifests
  pin `rev = "98031e43..."`, 471 pin `rev = "49ff5f55..."`, 229 `branch = "dev"`,
  177 `ssh://...branch = "dev"`. Four different specs for one dependency, and a
  pin bump is a 2600-file sed.
- The shared-crate pattern: arvo has 13 `*-shared` rlib crates under
  `variants/` (listed at `arvo/mock/benches/Cargo.toml` dependencies) that are
  not arms; vehje has `vehje-bench-carrier`. The tree distinguishes an arm from
  a shared dep only by `crate-type = ["cdylib"]` versus rlib, and by a `-shared`
  naming convention nobody enforces.
- The `[workspace]` header trap: vehje's 902 manifests all start with
  `[workspace]` (learned the hard way); the starter template
  (`src/bench.rs:602-620`) still does not emit it, so `mock bench add` in any
  repo whose mock workspace does not explicitly exclude `benches/` scaffolds a
  crate that either gets captured by the outer workspace (target dir moves,
  profile is ignored) or refuses to build. The same trap is documented for the
  lint cdylib (`src/custom_lints.rs:108-111`) and in `arvo/mock/Cargo.toml`,
  where it silently broke every bench in the repository.
- The profile: PR #19 measured that none of arvo's 94 variant manifests nor its
  bench root had an effective `[profile.release]`, so ninety variants built at
  `lto = false, codegen-units = 16` against a documented promise of fat LTO and
  one codegen unit. The profile lines exist in every manifest and do nothing
  whenever a workspace root wins.

### 1d. Naming and the mapping question, measured

Can a formalised structure recover the bench-to-arms mapping without a listing?
Measured per tree, comparing each bench name against its arm directory stems:

| tree | (bench, arm) pairs | arm stem strictly prefixed by bench name | arms used by more than one bench |
|---|---|---|---|
| vehje | 820 | 619 loose / not reliable | 0 of 820 |
| hilavitkutin | 83 | 0 | 1 of 82 |
| arvo | 246 | 0 | 36 of 81 |

The finding the design has to respect: **for the existing trees the mapping is
not recoverable from the directory structure. It exists only in `bench.toml`.**
arvo reuses 36 of its 81 arms across benches (shared `Case<KEY>` arms swept by
several sections), so no per-bench nesting can represent it without
duplication. vehje, the newest tree, shares nothing across benches, which says
new trees naturally partition and a nested convention is viable as a default
for them. Autodiscovery of the mapping is therefore an opt-in convention for
new benches, never the migration story for old ones.

## 2. The proposal: mirror `mock/lints`, precisely where the analogy holds

The lints precedent (`src/custom_lints.rs`): the consumer drops `.rs` files in
`mock/lints/`; mockspace generates a collect crate under
`mock/target/mockspace-lints/`, builds it, loads it. The consumer writes only
what is theirs.

Where the analogy holds: the generated crate, the tool-owned build, the
generated manifest (with the `[workspace]` header the generator already knows
to emit), the pin-matched dependency spec injected at generation time, the
"nothing to do" fast path when the consumer has no custom code.

Where it breaks, and the design must not pretend otherwise: lints cross the
boundary as `Box<dyn Lint>` behind one C symbol, so the consumer's code can
live in a separately built cdylib. Bench routines cannot. A `RoutineSpec`
bridge is a monomorphised generic (`routine_bridge!(Case<80003>)`); it must be
compiled *into* the driver binary and into its `--worker` subprocess. So the
bench shape is not "consumer cdylib loaded by tool binary" but "tool-generated
binary crate that optionally depends on a consumer rlib by path", which is
exactly the mechanism `bench-matrix` already validated: the sibling crate needs
the cell's path, never its body, and per-crate fat LTO inlines across the
boundary identically (`bench-matrix/src/lib.rs:24-36`). Both mechanisms needed
are therefore already shipped and proven in this repository; nothing here is
speculative.

### The generated driver

`mock bench run` stops requiring `mock/benches/Cargo.toml` + `src/main.rs`.
Instead it generates `mock/target/mockspace-bench/` (idempotent, content-keyed,
like the lint crate) containing a `main.rs` that is today's `STARTER_BIN_MAIN`
with three slots filled from `bench.toml`:

1. `byte_routine_dispatch!(out = <dispatch.out>, sizes = [<union>])` where the
   size list defaults to the union of every `sizes` array in the manifest.
   This deletes the manifest/binary size-list dual maintenance outright; the
   numbers exist in one place and the macro invocation is generated from them.
   `out` defaults to 8, which is what all five trees use today, overridable in
   a new `[dispatch]` section.
2. The workload, generated from a declarative `[workload.<name>]` section:
   `stages = ["algo_call", "scalar_work 48", "graph_work 32", "heavy_memory 384",
   "branch_work 24", "light_scalar"]`. Checked expressible: every workload in
   all five trees is a composition of exactly these six harness-exported stage
   constructors with integer arguments and nothing else
   (`hilavitkutin .../main.rs:193-206`, `vehje .../main.rs:17-30`,
   `arvo .../main.rs:50-53`, starter `src/bench.rs:526-546`). A default
   `realistic` and `default` program ship built in.
3. The consumer hooks. If `mock/benches/src/lib.rs` exists, the generated crate
   depends on it by path and calls its exported `routine_for` (and
   `build_workload` if exported), else compiles in `|_| None`. Presence of the
   lib is the opt-in; no config key needed.

A consumer with only byte-shaped benches (kirjo, vehje as of today) then owns
**zero Rust in the driver path**: `bench.toml`, `variants/*/src/lib.rs`, done.
That is the maintainer's "single lib entrypoint, and even that optional",
delivered literally.

The consumer's own full `Cargo.toml` + `src/main.rs`, where present, remains
the escape hatch: if `mock/benches/Cargo.toml` declares a `[[bin]]`, `mock
bench run` builds and runs it exactly as today (with PR #19's name resolution).
Nothing existing breaks on day one; the generated path activates only when the
bin crate is absent, and migration is deleting files rather than writing them.

### What the toml owns and what the lib owns

The toml owns everything declarative: benches, titles, workloads (by name or
by declared stage list), sizes, variants, seeds, timing, `may_differ`,
`required`, `threaded`, `normalise`, `[dispatch]`, `[docgen]`. The lib owns
the two things a manifest cannot express, and they are real:

1. **Custom routines.** Anything whose input is not plain bytes: arvo's typed
   `Case<KEY>` / `Contend<KEY>` / `FootprintColumn<N>` bridges, graph and
   spectral shapes. This is irreducible; a monomorphisation cannot be named in
   TOML. But its cost should collapse: a `routine_table!` macro in bench-core,
   `routine_table! { "bitpack-carrier-width" => CarrierColumn[16384, 131072, 1048576, ...], ... }`,
   expanding to today's match arms, turns arvo's 360 hand-written arms
   (`arvo .../main.rs:227-590`) into roughly 40 lines and removes the
   pattern-n-equals-const-argument hand invariant, since the macro writes both
   from one number. Straightforward `macro_rules!` over literal lists; same
   expansion class as `byte_routine_dispatch!`.
2. **Custom workload programs and post-run analysis** beyond the declarative
   stage grammar: hilavitkutin's disasm_5check pass with its exit-code policy
   (`hilavitkutin .../main.rs:141-183`) is the live example. The
   `DriverRegistry` should grow an optional `post_bench` hook so that survives
   migration onto the driver instead of forcing hilavitkutin to keep its whole
   hand-rolled loop for one feature. The `NormaliseSection` TODO already
   anticipates exactly this hook shape (`config.rs:137-144`).

### Variant autodiscovery and generation

Formalised structure, default for the generated path:

- `variants/<name>/` containing only `src/lib.rs` (no `Cargo.toml`): mockspace
  generates the manifest at build time (name from the directory, `[workspace]`
  header, cdylib crate-type, pin-matched bench-core/macro deps, shared deps
  from an optional `[variants] deps = ["shared/foo"]` or per-variant
  `[variants.<name>] deps = [...]` in bench.toml), builds it with the tool-owned
  target dir and PR #19's `--config` profile pins. This is the lints analogy
  applied exactly: the consumer writes the measured code and one attribute.
- `variants/<name>/` containing a `Cargo.toml`: used as-is, built as today.
  Every one of the existing 1064 variants keeps working unmodified.
- Shared rlib crates move to `shared/<name>/` in new trees; existing
  `variants/*-shared/` are recognised by the mechanical test that already
  distinguishes them (no cdylib crate-type) and skipped as arms.
- Arm-versus-shared discovery therefore never guesses: cdylib crate-type or
  presence of `#[bench_variant]`/`bench_entry` in a manifest-less lib.rs.

Bench-to-arm mapping stays in `bench.toml` (section 1d showed the structure
cannot carry it for existing trees). Two ergonomic reductions land instead:

- The concise form becomes the documented and scaffolded default, and
  `mock bench lint` (or the doc-regen pass) warns when every `[[sizes]]` block
  of a bench repeats one variant list, naming the collapse. That is a
  mechanical rewrite the tool can offer as `mock bench fmt`: arvo 3018 lines
  to roughly 450, vehje 4955 to roughly 1400, hilavitkutin 773 to roughly 250,
  with the one genuinely varying arvo bench keeping its per-size form.
- For new benches only, `variants = "auto"` on a bench section resolves to the
  directory listing of `variants/<bench-name>/*/` (nested per-bench arms). New
  trees partition cleanly (vehje: 0 of 820 arms shared); old trees never have
  to adopt it.

### Migration per consumer, costed

| consumer | to get onto the generated driver | cost |
|---|---|---|
| kirjo | delete `Cargo.toml` + 60-line `main.rs`; sizes union already matches | minutes |
| vehje | delete `Cargo.toml` + 51-line `main.rs`; move the stage mix into `[workload.realistic]` | minutes; 902 variant manifests optionally deleted later for generated ones (mechanical, or left as-is) |
| mockspace self-bench | replace the 207-line main with the generated path; becomes the reference | small, and overdue independently |
| hilavitkutin | needs the `post_bench` hook first (disasm_5check); then delete the loop, keep the manifest `may_differ` flags that already exist, drop the name list | the hook, then hours |
| arvo | needs `routine_table!`; keep a lib.rs with the table; rename or alias the four-names-per-variant scheme if short names are wanted; `mock bench fmt` collapses the manifest | the macro, then a day; the tree is currently unbuildable against the deleted `arvo` crate anyway (`arvo/mock/benches/Cargo.toml:11-24`), so this waits for the canon rebuild and should be the shape it rebuilds into |

A tree that does not match the convention keeps working through the whole
plan: the `[[bin]]` escape hatch preserves hand-rolled drivers, explicit
variant listing preserves non-conventional layouts, and per-size variant lists
remain valid for the case that needs them.

## 3. bench.toml before and after, on a real bench

Before, `arvo/mock/benches/bench.toml:1-32`, verbatim shape (32 lines):

```toml
[bench.fnv1a-vs-xxhash3]
title = "ContentHash algorithms: FNV1a vs xxHash3"
workload = "default"
master_seed = 0x1234_5678_9ABC_DEF0

[[bench.fnv1a-vs-xxhash3.sizes]]
n = 64
variants = [
    "../target/release/fnv1a",
    "../target/release/xxhash3",
]

[[bench.fnv1a-vs-xxhash3.sizes]]
n = 256
variants = [
    "../target/release/fnv1a",
    "../target/release/xxhash3",
]

# ... two more identical blocks for 1024 and 4096 ...
```

After (6 lines, parseable by the shipped `config.rs` today except that short
names additionally require the tool-owned build layout so dir name resolves):

```toml
[bench.fnv1a-vs-xxhash3]
title = "ContentHash algorithms: FNV1a vs xxHash3"
workload = "default"
master_seed = 0x1234_5678_9ABC_DEF0
variants = ["fnv1a", "xxhash3"]
sizes = [64, 256, 1024, 4096]
```

And the whole-file additions that replace the consumer's `main.rs`:

```toml
[dispatch]
out = 8            # sizes default to the union of every bench's sizes

[workload.default]
stages = ["algo_call", "light_scalar"]
```

## 4. The tool owns the build, which is what makes names mean anything

Three build facts should become tool guarantees rather than per-consumer
conventions, because each has already failed silently at least once:

1. **Where variants build.** `mock bench run` passes `--target-dir` explicitly
   for variants and driver (into `mock/benches/target/` or a tool dir), so
   short-name resolution (`variants/<name>/target/release/lib<name>`) is true
   by construction instead of true only when workspace membership happens to
   cooperate. This retires the arvo class: 1305 `../target/release` couplings,
   the members-deletion breakage, and the 12-versus-90 split-brain output dirs.
2. **The release profile.** PR #19's `--config` pins; already done, keep it.
3. **The generated manifests' `[workspace]` header**, and fix the starter
   variant template to emit it too (`src/bench.rs:602`), since the escape
   hatch keeps scaffolding real manifests.

## 5. Error messages: what fails unhelpfully today

Catalogued with the failure a reader actually experiences:

1. **Hardcoded binary name.** Built everything, then
   `no bench binary found in <dir>; expected 'benches' or 'benches.exe'`
   (`src/bench.rs:328-337`) when the package is named `arvo-benches`. Fixed by
   PR #19. The generated driver makes it moot on the default path (the tool
   names the crate); PR #19's manifest read remains load-bearing for the
   escape hatch. This proposal complements PR #19 and does not supersede it.
2. **Silent profile loss.** Not an error at all: ninety variants built at the
   wrong profile with no signal. PR #19 fixes the profile; the residual
   principle is that framework guarantees must not live in consumer manifests,
   which section 4 generalises.
3. **dlopen failure surfaced as a timeout.** A missing or double-prefixed
   dylib prints `TIMEOUT\t<load-fail>` from the worker
   (`bench-harness/src/harness.rs:211-220`) and the run continues. arvo lived
   through a stretch where every bench failed this way before anyone found the
   cause (`arvo .../main.rs:76-82`). The worker's stderr does carry
   `dlopen failed: <e>`, but the coordinator's per-variant line should say
   "could not load `<path>`: <dlerror>. The file
   {exists but is not loadable | does not exist; nearest existing sibling is
   <candidate>}" instead of a timeout-shaped row. The driver's preflight
   (`driver/mod.rs:252-274`) already catches the does-not-exist half for
   driver users; the pre-driver trees have no preflight, which is one more
   migration argument, and the exists-but-unloadable half (ABI hash mismatch,
   wrong arch) still needs the loud path.
4. **Filtered runs silently skip out-of-tree builds.** `mock bench run <name>`
   maps names to `variants/` dirs and deliberately ignores any other entry
   shape (`src/bench.rs:199-213`), so in a path-style tree like arvo it builds
   nothing and runs whatever stale dylibs exist, with no warning. Should warn:
   "bench `<name>`'s entries are paths outside `variants/`; nothing was rebuilt
   for them".
5. **Unknown keys in bench.toml are silently ignored.** No
   `deny_unknown_fields` on the serde shapes (`config.rs:60-121`), so a typo
   (`may-differ`, `require`, `varaints`) parses clean and the flag silently
   stays default. In a measurement tool a silently-defaulted `may_differ` is a
   wrong-answer generator. Deny unknown fields, name the nearest valid key.
6. **`for_size` errors do not list what exists.** "bench `X` not found in
   manifest" (`config.rs:345-349`) versus the driver's selection error, which
   lists available names (`driver/mod.rs:213-218`). The library error should
   carry the list too; pre-driver trees route through it.
7. **`mock bench init` refuses an existing tree with no upgrade path.**
   `"delete the directory or pick a different scaffolding strategy"`
   (`src/bench.rs:344-351`). With a generated driver this should become
   `mock bench init` scaffolding only what is missing, and a `--force` for the
   starter files, since re-running init is exactly what a consumer upgrading
   to new conventions will do.
8. **Stale init instructions.** `next steps` says "edit src/main.rs: replace
   IdentityAdd with your Routine" (`src/bench.rs:375`) and the README template
   repeats it (`src/bench.rs:678`); the starter main has contained no
   `IdentityAdd` since the driver extraction. Cheap fix, real confusion.
9. **Duplicate exported `bench_name` merging.** The driver catches and
   explains this well (`driver/mod.rs:455-474`); the pre-driver trees get the
   silently-merged medians it describes. Migration inherits the good error.

## 6. Output layout: the artifact tree

### What exists, counted

arvo's `mock/benches/` root holds 768 entries: 254 `.csv` + 254 `.meta.json` +
254 `_findings.md`, flat, interleaved with `Cargo.toml`, `bench.toml`, `src/`,
`variants/`, `target/`. hilavitkutin's holds 444 (144 of each), plus four
nonconforming side trees: `runs/run1`, `runs/run2` (hand-made snapshot copies
of result triples, a manual version scheme), `engine_vs_std/` (a standalone
crate timing with `Instant::now()`, `engine_vs_std/src/lib.rs:35,187`, no
harness dependency: under the workspace's own naming rule that is an ad-hoc
spike living inside `benches/`), `resource_storage/` and `asm_gate_fixtures/`
(harness-consuming and fixture trees respectively). vehje's root holds 39
directories of which about 25 are standalone bench-adjacent projects
(`carrier/` and `scale-runner/` harness-consuming, `disasm-probe/` and
`carrier-runtime/` not), plus the conforming `results/` (220 bench subdirs),
`.bench_history/` (1422 tracked TSVs), `src/`, `variants/`.

The naming convention (`<bench>_n<size>.csv`, `.meta.json`, `_findings.md`)
carries exactly the information a directory could: the flat layout encodes
nothing.

### What the tool already owns, which is most of the answer

The library driver already writes `results/<bench>/<bench>_n<n>.{csv,md}` with
meta alongside (`driver/mod.rs:54,152-163`, `harness.rs:73-78`), stages
in-flight runs and quarantines crash-borne trees into `results/void/<runid>/`
(`driver/staging.rs`), defers history appends until promotion, and appends
`.bench_history/<benchmark>.tsv` (`history.rs:15`), which docgen and
regression detection read (`src/bench_docs.rs:12-15`). **The flat mess is not
a missing design; it is the two pre-driver trees never having been moved onto
the design.** Migrating arvo and hilavitkutin to the generated driver ends
flat output the day it lands, and the tool then owns the layout as a guarantee
exactly as the follow-up asks: the driver, generated by mockspace, is the only
writer.

### History and live results: two mechanisms, deliberately

They should both survive, and stay separate. `results/` is the latest
evidence: full sample CSVs, environment meta, rendered findings, the thing a
design document cites, overwritten per run under transactional staging.
`.bench_history/` is the append-only per-benchmark time series (timestamp,
commit, median, CI), the input to regression detection and docgen, never
overwritten. One is a snapshot with provenance, the other a ledger; collapsing
them would either bloat the ledger with samples or strip the snapshot of them.
What should die is the third, informal mechanism: hand-made `runs/run1` copies,
which the ledger plus git history of `results/` already subsume. Keep both
directory names as-is; vehje and kirjo already committed trees depend on them,
and `feedback_bench_history_is_tracked` already pins `.bench_history` as
tracked.

### Migration of committed flat artifacts, and the citation constraint

Checked rather than assumed: committed documents citing flat artifact paths by
name. arvo: 10 files under `mock/research/` cite 5 distinct flat artifacts
(e.g. `mock/benches/bitpack-carrier-width_n16384.csv`,
`mock/benches/satfold-const-gate_n10000_findings.md`). hilavitkutin: zero
path-shaped citations found for the flat pattern. vehje: 5 citations, all
already `results/`-shaped. The arvo citing files are closed panel records,
which this workspace does not edit.

So the migration is: new runs never write flat (comes free with the driver);
existing flat artifacts are moved by a one-shot `mock bench migrate` that
relocates `<bench>_n<size>.*` into `results/<bench>/` via `git mv`, except
paths on a keep-list the consumer passes (the 5 cited ones for arvo, derivable
by the same grep run here). The mapping is name-preserving and fully
mechanical, so even a missed citation is recoverable by construction: the
filename alone determines the destination. The 5 kept files cost nothing and
honour the constraint that a tidier layout must not break a citation. For
hilavitkutin all 432 flat files can move. The side trees (`runs/`,
`engine_vs_std/`, the vehje standalone projects) are not the tool's to move;
they get named in the conventions doc as out-of-convention, with
`engine_vs_std`-style timing loops pointed at the harness or at
`mock/research/sketches/` where the workspace's own rules put them.

### Root convention after the pass

`mock/benches/` contains exactly: `bench.toml`, optional `src/lib.rs`
(hooks) or full escape-hatch crate, `variants/`, optional `shared/`,
`results/`, `.bench_history/`, `README.md`, `target/` (ignored). `mock bench
lint` warns on anything else at the root, so the convention is observed by the
tool rather than by each consumer's discipline.

## 7. What this does not change, and why

- **The harness measurement core.** Cdylib-per-variant isolation, subprocess
  workers, hardware counters, validation, disasm dup check, analysis,
  staging, history, docgen. This survey found consumers hand-rolling *around*
  it, never a reason to change *it*. Its newest features (preflight,
  duplicate-name guard, quarantine) are precisely what the laggard trees lack.
- **`bench.toml`'s existing vocabulary and semantics.** `[bench.*]`, `title`,
  `workload`, `master_seed` (including the string-for-u64 shape), `sizes` in
  both forms, `may_differ`, `required`, `threaded`, `[timing]` with per-bench
  override merge, `[bench.*.normalise]`, `[docgen]`. Every proposal above is
  additive (`[dispatch]`, `[workload.*]`, `[variants.*]`, `"auto"`); nothing
  parsed today stops parsing. The per-size variant form stays for the one
  bench in 258 that needs it.
- **`#[bench_variant]` and the variant lib.rs shape.** 1064 files, one
  attribute each, measured code otherwise. Already right.
- **`bench-matrix`.** A distinct, opinionated layer with its own validated
  discipline; the generated driver sits beside it, not over it.
- **`results/` and `.bench_history/` names and shapes.** Committed trees and
  the tracked-history rule depend on them.
- **Committed evidence cited by closed panels.** Grandfathered in place per
  the keep-list; conventions apply forward.
- **PR #19.** Complemented, not superseded: its manifest-name resolution and
  `--config` profile pins are load-bearing for the escape hatch and for the
  interim, and its follow-up note is this document.

## Appendix: raw counts

Gathered 2026-08-15 from the workspace clones (read-only). Variant reference
counts by `grep -o` over quoted strings per manifest grammar; per-size
uniformity and mapping recoverability by `tomllib` parse; file counts by `ls`;
citation counts by `grep -rlE 'benches/[a-z0-9-]+_n[0-9]+'` over
`mock/research` and `mock/design_rounds` per repo. Key figures: arvo 1305/81
refs, 256 size blocks, 254 flat triples, 672-line driver, 94 variants, 0
short names; hilavitkutin 365/82, 128 blocks, 144 flat triples, 311+355+8
lines, 110 variants; vehje 5170/820, 1132 blocks, 51 lines, 902 variants,
`results/` 220 dirs, `.bench_history` 1422 TSVs; kirjo 4/2, 60 lines, 2
variants; mockspace self 2/2, 207 lines. Per-size variant lists uniform for
257 of 258 array-of-tables benches. Arms shared across benches: arvo 36/81,
hilavitkutin 1/82, vehje 0/820. Variant manifests: 3 dep signatures across
vehje's 902; four distinct mockspace dep specs across 2664 manifest lines
workspace-wide. `#[bench_variant]` users: 1064; hand-rolled exports: 2.
