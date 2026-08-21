# The cell record: one type, and everything else a projection

The ratified shape (`202608151340_topic.one-record-and-every-writer-a-projection.md:35-51`)
says one `CellRecord`, written once, where the driver's post-cell block already
runs; every other writer projects from it and invents no field. This settles
the record's exact fields, which are required, what happens to `.bench_cache`,
the runid's granularity, and whether a cell that fails before measuring gets a
record.

Every claim below is either a `file:line` against the branch's own source or a
probe committed in `202608151419_probes/`, run twice: once unpinned (caught a
real blocker) and once on the workspace-pinned toolchain,
`rustc 1.98.0-nightly (57d06900f 2026-05-27)` (`202608151419_probes/toolchain.txt`),
which is `nightly-2026-05-28` per `workspace.md`.

## 0. Gates

**Canon gate.** No `mock/canon/` exists in this repository, confirmed the same
way both prior experts confirmed it (`ls mock/`, no such directory). There is
nothing above the ratified topic-file shape to defend and nothing above this
file either.

**Test gate.** `cargo test --workspace --no-fail-fast` on this branch at
`aa8d45d`: **551 passed, 0 failed, 18 ignored**, reproduced myself via
`grep "test result:"` summed across all 28 suites. This is the third
independent reproduction of this exact figure on this branch (the sweep
expert's probe run, the validation expert's own run, and now this one), which
is worth stating because three independent instances agreeing is the bar this
workspace asks for on a claim, not two.

I read the body of every test in the surface I am designing over.
`bench-harness/src/sample.rs:148-202` (`csv_parses_perf_columns_and_is_backward_compatible`)
establishes the exact tolerant-parser convention I reuse for `schema_v2` below:
new columns append at the end, `p.get(N).unwrap_or(default)` reads them, and a
shorter old row still loads. `bench-harness/src/history.rs` has **zero**
`#[test]`/`mod tests` anywhere in the file or in `tests/smoke.rs` (grep for
both patterns returns nothing in either); `bench-harness/src/cache.rs` and
`bench-harness/src/env.rs` are the same, zero in-file coverage, with `cache.rs`
partially exercised only through `smoke.rs`'s `cache_csv_round_trips`
(`tests/smoke.rs:127-160`), which tests the CSV cache-hit path, not `Cache`'s
dylib-hash-keyed manifest at all. `bench-harness/src/driver/mod.rs` has zero
tests (confirmed independently: `grep -n "mod tests\|#\[test\]"` returns
nothing), matching the validation expert's finding. The one known defect,
`bench-core/src/lib.rs:637-651`, I read directly: it recomputes `abi_hash()`'s
literal folds by hand and asserts the recomputation equals the function,
which cannot fail under any layout change that a matching hand-edit to the
test wouldn't also make. Not mine to fix; noted because I am about to design a
migration (`schema_v2`) for a file (`history.rs`) that has no test coverage of
its read path at all, which the design below has to be honest about rather
than assume away.

## 1. What I found beyond the brief, checked before designing over it

Two things the topic file and the two prior proposals do not name, both
directly load-bearing for the record's design because they are additional,
independent instances of the exact defect class this round exists to close.

### 1a. `DataSetMeta` is a second, unpopulated record, and the report's Methodology section is dead

`analysis.rs:133-149` declares `DataSetMeta`: `passes`, `runs_per_pass`,
`batch_size`, `harness_runs`, `cooldowns_ms`, `master_seed`, `counter_freq`,
`drift_correction`. `DataSet::from_samples` (`analysis.rs:165-265`) always sets
`meta: DataSetMeta::default()` (`analysis.rs:262`), and grepping for every
assignment to `.meta.*` in `bench-harness/src/` (`analysis.rs:690`,
`report.rs:829,859`) shows only `ops_per_call`, `normalise_mode` and
`floor_variant` are ever set after construction. **Nothing in the live driver
path ever sets `passes`, `runs_per_pass`, `batch_size`, `harness_runs`,
`cooldowns_ms`, `master_seed`, or `counter_freq` on a `DataSet`'s meta.**

The consequence is not cosmetic. `report.rs:116-118`: `if m.passes > 0 ||
m.harness_runs > 0 { md.push_str("## Methodology\n\n"); ... }`. Since `meta`
is always the zero default, **this condition is always false and the entire
Methodology section (parameter table, master seed, cooldown schedule, counter
frequency) has never once rendered in a live run.** It is dead code that reads
as a feature. `report.rs:37` also reads `ds.meta.master_seed` (always `0`) for
`crate::summary::summarise`, while `driver/mod.rs:506` correctly passes the
real `config.master_seed` to the *stdout* highlight, so the stdout summary and
the findings.md report currently disagree about which run produced them,
silently, on every bench with more than one variant.

This is the strongest available evidence for the ratified rule. `DataSetMeta`
is exactly a writer that invented its own field-carrying struct instead of
projecting from the config and result already in scope, and it rotted to
permanently-empty the moment nobody kept it in sync by hand. It is what
happens when the shape this round is fixing is left unfixed one layer down.

### 1b. The build-profile override is already forcing one profile, and it forces it by a mechanism `harness.rs` cannot see

`src/bench.rs:268-275` (`PROFILE_ARGS`, already on this branch): every `cargo
build` for every variant crate and the bench binary runs with `--config
profile.release.opt-level=3 --config profile.release.lto="fat" --config
profile.release.codegen-units=1`, unconditionally, which is a CLI-level
override that wins over whatever a crate's own `[profile.release]` says. This
is what makes `harness.rs:54`'s hardcoded string accurate today (the topic's
"on `dev` it happens to be accurate" claim, `202608151340_topic...:29`,
confirmed).

Two things follow that the topic did not have to hand. First, no `[build]` or
per-bench profile-override config surface exists anywhere in `config.rs`
(grepped for `BuildSection`, `[build]`, `opt_level`: zero hits), so the "open
PR" the topic warns about is not yet buildable against anything in this repo;
it is future work, and the record's job is to not need touching again once it
lands. Second, and this is the part that decides the mechanism: **the process
that knows the real profile (`src/bench.rs`, the `mockspace` CLI's build step)
is a different crate and a different process from the one that writes the
record (`bench-harness`, invoked later as the compiled consumer binary).**
There is no shared memory between them. A future per-bench override cannot be
read back by re-deriving it inside `bench-harness`, because `bench-harness`
never sees the `cargo build` invocation at all.

So the fix this round has to specify is a **handoff**, not a read. The build
step is the only truthful source and has to write the resolved profile down
at build time, beside the artifact it produced, for the harness to read back
later. Section 3's `build.profile` field says exactly this and marks it as
the one field in this design that needs work outside `bench-harness` to be
made fully honest; everything else here is `bench-harness`-internal.

### 1c. `--report-only` regeneration already discards the thing this round is trying to make load-bearing

`lib.rs:222-238` (`report_from_csv_for_routine`, what `--report-only` calls,
`driver/mod.rs:343-361`): it loads the CSV and builds a `BenchResult` with
`env: env::EnvMeta::default()`. **It never opens `.meta.json`.** The file that
already exists beside the CSV it does read is never touched. This means
`--report-only` regeneration today always renders with a blank environment,
even though the real one is sitting on disk. Section 6 fixes this as a direct
consequence of giving the record a stable, readable home; it was not asked for
by the brief and it falls out of the design for free.

### 1d. `.meta.json` has zero readers anywhere in the repository

`grep -rn "meta_json_path\|env_meta_to_json"` across `bench-harness/src/` and
`src/`: every hit is inside `harness.rs` itself (the writer). No reader exists.
This matters for the migration question in section 5: **the record can change
format and filename extension with zero compatibility cost**, because nothing
currently depends on the file's shape or even its existence.

## 2. What `.bench_cache` actually is today, checked before answering the topic's question about it

The topic's inventory (`...:20`) says `.bench_cache` "knows the dylib hash and
nothing else knows it." True as far as it goes, but I checked what else is
true of the module before designing around it, because the answer changes
depending on whether it is live machinery or a stranded function.

`grep -rn "Cache::\|DEFAULT_CACHE_ROOT" bench-harness/src/ src/` (excluding
`cache.rs` itself): the only other hits are `lib.rs:68` (the re-export) and
`tests/smoke.rs:138,153` (`cache::Cache::load("smoke", ...)`, exercising the
CSV-cache-hit path in isolation). **`driver/mod.rs` never calls `Cache::load`,
`Cache::partition`, or any save method.** The incremental skip-rerun mechanism
(manifest.tsv, dylib-hash-gated cache hits, drift-corrected merge) that `Cache`
implements is not wired into the live orchestrator path at all. I also checked
the one workspace consumer that mentions it: `hilavitkutin/mock/benches/resource_storage/bench.toml:28`
is a comment describing the mechanism, not a call site.

So `.bench_cache`'s `Cache` struct, as a system, is currently dead code from
the driver's point of view. What is not dead is the free function underneath
it: `cache::dylib_hash(path: &str) -> u64` (`cache.rs:39-45`), a pure FNV-1a
hash of the file's bytes, called today only from inside `Cache::partition`.
That function is the one fact the topic is right to say nothing else computes,
and it is trivially reusable on its own, with no dependency on the rest of the
module.

**Answer: the record calls `cache::dylib_hash(path)` directly**, once per
surviving arm, in the driver's post-cell block, to populate
`build.arm_dylib_hash`. The `Cache` skip-rerun system is untouched: it is an
independent, currently-unwired, opt-in mechanism, and nothing in this design
depends on its behaviour or changes it. I would not wire it into `drive()` as
part of this round; whether the incremental-cache feature should exist at all,
live, is a separate design question this topic was not asked to settle, and
bolting it on as a side effect of building the record would be exactly the
kind of scope creep the brief warns against.

## 3. The record

```rust
/// One measurement: identity, the config that produced it, the build
/// that produced the arms, the environment it ran in, what actually
/// executed, and what the numbers say. Written once per cell, in the
/// driver's post-cell block, at the same path `.meta.json` used
/// (`<bench>_n<point>.meta.toml`; see section 7 for the extension
/// change). Every other writer (CSV header context, findings.md,
/// INDEX.md, the history projection) reads this rather than holding
/// its own copy.
pub struct CellRecord {
    pub identity:    Identity,
    pub config:      Config,
    pub build:       Build,
    pub environment: Environment,
    pub execution:   Execution,
    /// `None` for a cell that produced zero measured arms (section 6).
    /// `Some` and non-empty otherwise; `Some` and empty cannot occur
    /// (an arm either has an analysis or it was dropped, and dropped
    /// arms are in `execution.arms_dropped`, not here).
    pub analysis:    Option<BTreeMap<String, ArmAnalysis>>,
}

pub struct Identity {
    /// The literal `[bench.<key>]` manifest key AND the bench identity.
    /// See the note below on why this is one field, not the two the
    /// ratified list names.
    pub manifest_key: String,
    pub title:        String,
    /// This invocation's runid; see section 4 for why it is per
    /// invocation, not per cell.
    pub runid:        String,
    /// Every axis, swept and held, keyed by name. Required, and
    /// required to be non-empty (see the required/optional table).
    /// Today this is exactly `{"n": AxisPoint { value: n, label: None }}`,
    /// because the shipped driver has no axis concept beyond `n`. Once
    /// the sweep topic's multi-axis work lands, additional entries
    /// arrive under this same map with no shape change here: this is
    /// the forcing constraint from `202608151351_what-a-sweep-carries.md:483-501`,
    /// satisfied by making the container open rather than by
    /// pre-guessing the sweep vocabulary.
    pub axes:         BTreeMap<String, AxisPoint>,
}

pub struct AxisPoint {
    pub value: i64,
    /// The symbolic name for this value, when the axis declares one
    /// (`nc = "small"` mapping to `8192`, per the sweep expert's worked
    /// TOML example). `None` for a bare numeric axis (`n`, `w`, `d`
    /// today).
    pub label: Option<String>,
}

pub struct Config {
    /// The resolved seed actually used. Never the manifest's `0`
    /// ("fresh random") sentinel: this is what `driver/mod.rs:366-374`
    /// computes when the manifest declares `0`, or the manifest's own
    /// value otherwise. This is the field the topic names as currently
    /// living nowhere but stderr (`driver/mod.rs:373`).
    pub resolved_seed: u64,
    pub workload:       String,
    pub passes:          usize,
    pub runs_per_pass:   usize,
    pub batch_size:      usize,
    pub harness_runs:    usize,
    pub cooldowns_ms:    Vec<u64>,
    pub batch_k:         usize,
    pub max_call_us:     Option<u64>,
    pub threaded:        bool,
    /// Statistical/validation budgets (`HarnessTuning`): validation
    /// seed count, determinism-check seed count, quality seed count,
    /// bootstrap iteration count. Cheap to carry (already a small
    /// resolved struct on `BenchConfig.tuning`) and load-bearing for
    /// exact CI reproduction.
    pub tuning:          HarnessTuning,
    /// The arms the manifest declared for this cell, before
    /// validation. Compare against `execution.arms_measured` plus
    /// `execution.arms_dropped` to see what validation actually did.
    pub arms_declared:   Vec<String>,
    pub normalise_baseline: Option<String>,
    pub normalise_mode:     Option<String>,
    pub normalise_floor:    Option<String>,
}

pub struct Build {
    /// The build profile actually used to compile these arms.
    /// `Some` only when the build step recorded it (section 1b); the
    /// harness process cannot derive this itself, so `None` is the
    /// honest value until the handoff described there exists. This is
    /// the ONE field in this record that is allowed to be absent for
    /// a reason other than "the cell failed before reaching it": a
    /// hand-built variant compiled outside `cargo mock bench build`
    /// has no handoff to read.
    pub profile:        Option<String>,
    /// Best-effort: the variant crate's own declared `[dependencies]`,
    /// read from its `Cargo.toml` located by convention from the
    /// dylib path. `None` when the convention does not resolve (an
    /// unconventional consumer layout). Not authoritative (a
    /// `Cargo.lock`-resolved version would be); flagged as such rather
    /// than guessed. See section 9: I did not price reading
    /// `Cargo.lock` per arm and would not claim it is free.
    pub dep_spec:        BTreeMap<String, Option<String>>,
    /// Per surviving arm, `cache::dylib_hash(path)` (section 2).
    /// Populated for every arm in `execution.arms_measured`; a dropped
    /// arm is not hashed (nothing was run, and the hash of a dylib
    /// that was never loaded tells a reader nothing the identity in
    /// `execution.arms_dropped` doesn't already).
    pub arm_dylib_hash:  BTreeMap<String, u64>,
    /// `env!("CARGO_PKG_VERSION")` of `mockspace-bench-harness` at
    /// compile time. Unlike `profile`, this needs no handoff: it is a
    /// fact about the exact binary writing this record, true by
    /// construction, with no drift risk the way a re-derived or
    /// hand-copied constant has. See the caution in section 8.
    pub framework_version: String,
}

pub struct Environment {
    pub cpu:          String,
    pub os:           String,
    pub rustc:        String,
    pub git_commit:   String,
    pub timestamp:    u64,
    pub counter_freq: u64,
}

pub struct Execution {
    pub arms_measured: Vec<String>,
    pub arms_dropped:  Vec<DroppedArm>,
    /// From `disasm::check_duplicates` (section 8's signature change):
    /// duplicate `.text`-section pairs and paths whose disassembly
    /// could not be extracted. `None` when fewer than two variants
    /// (the check does not run, `disasm.rs:306-308`).
    pub disasm:        Option<DisasmOutcome>,
    /// Owned by the validation topic; not mine to shape further than
    /// giving it a home. `202608151356_validation-semantics.md:650-678`
    /// already specifies exactly this content (which check ran and
    /// against what plan, per-arm structural failures with seed and
    /// reason, per-arm cross-variant mismatches with seed/baseline/
    /// error, a per-cell not per-bench unverified tag, and whether
    /// `required` escalated). I place it here and take whatever shape
    /// that topic settles on.
    pub validation:    Option<ValidationOutcome>,
}

pub struct DroppedArm {
    pub arm:    String,
    pub reason: String,
}

pub struct ArmAnalysis {
    pub median_ns: f64,
    pub ci_lo_ns:  f64,
    pub ci_hi_ns:  f64,
    /// Set once `history::detect_regressions` has run for this arm
    /// against its rolling window.
    pub regression: bool,
}
```

### Why `manifest_key` absorbs the ratified list's `bench` and `manifest key` as one field

The topic's ratified identity list (`...:39-40`) names both `bench` and
`manifest key` alongside `sweep` and `point`. I checked whether the shipped
driver has two distinct strings here before giving them two fields.

`BenchManifest.bench: HashMap<String, BenchSection>` (`config.rs:53`) is keyed
on exactly one string, and `BenchConfig.bench_name` (`config.rs:499`) carries
that same string through `for_size`. The sweep expert's own finding
(`202608151351_what-a-sweep-carries.md:67-72`) establishes that a `bench-matrix`
`MatrixDecl` expands into **one `[bench.*]` section per sweep value**
(`generate.rs:113-124`), which means today's `bench_name`, at the driver's
level, already **is** what the sweep vocabulary calls a sweep (`width-l1`),
not a family. There is no shipped concept of a family/bench grouping one level
above that. Giving the record two fields today, `bench` and `manifest_key`,
would mean inventing a value for one of them with nothing to derive it from,
which is exactly the "field that lies" failure this round exists to close,
just introduced fresh instead of fixed.

So today the record carries one field, `manifest_key`, doing both jobs: it is
simultaneously the literal TOML lookup key and the only identity string that
exists. If the sweep topic's family/sweep split lands, this is the seam:
`manifest_key` keeps meaning "the literal section key" and a new field
(`family`, or `sweep` depending on what that topic settles) is added
alongside it, required once that mechanism exists, absent until it does. I am
not adding that field speculatively now, for the same reason the sweep
expert's own file did not decide the vocabulary: it is not this topic's call
and a field with no way to populate it is worse than no field.

## 4. Required versus optional, and why

The judgement asked for. The rule I applied: **a field is required unless
there exists at least one real, in-scope cell for which no honest value can be
computed.** "Honest" excludes placeholders; a required field that would
otherwise need a placeholder is the tell that it should be optional instead.

**Required, unconditionally, on every `CellRecord` including one for a cell
that failed before measuring (section 6):** `identity.manifest_key`,
`identity.title`, `identity.runid`, `identity.axes` (non-empty),
`config.resolved_seed`, `config.workload`, all of `config`'s timing fields,
`config.tuning`, `config.arms_declared`, `build.framework_version`,
every field of `environment`, `execution.arms_measured` (may be empty),
`execution.arms_dropped` (may be empty). All of these are known the instant a
`BenchConfig` exists and the seed is resolved, which happens before
validation and before any measurement, so there is no cell in the loop for
which they are unavailable.

**This is where the topic's caution bites hardest, and I checked it rather
than asserted it.** An identity field with no honest value anywhere is a
defect to fix (as `manifest_key`'s scope note above did), not a field to make
optional; making it optional would just relocate the lying to a different
place, a `None` that always reads as "not applicable" when it actually means
"this shipped design has no way to know." I found no such field inside
`identity` once `manifest_key` was collapsed to one string, which is why
`identity` has zero optional fields.

**Optional, with a stated reason each:**

- `build.profile: Option<String>` (section 1b): absent when the build step's
  handoff does not exist yet, or when a variant was hand-built outside `cargo
  mock bench build`. Not fixable inside `bench-harness` alone.
- `build.dep_spec` per-arm entries: absent when the crate-root-from-dylib-path
  convention does not resolve. Best-effort by design (section 9).
- `execution.disasm`: absent structurally when fewer than two variants exist
  to compare (`disasm.rs:306-308`); there is nothing to be honest about
  because the check does not run, not because a value went uncomputed.
- `execution.validation`: absent for a single-variant cell for the same
  structural reason (`validation::validate` requires two variants,
  `driver/mod.rs:431-435`), and its populated shape is not mine to finish
  designing.
- `analysis`: `None` whole-record when zero arms were measured (section 6).
  Never partially populated: an arm either has an entry or it is in
  `execution.arms_dropped`, so there is no "some arms analysed, others
  silently missing" state to represent.

## 5. Wire format: TOML, not the hand-rolled JSON string, and why I checked before deciding

`harness.rs:42-62`'s own doc comment states the constraint plainly: "no
serde_json dependency." That was a proportionate choice for `EnvMeta`'s six
flat strings, hand-interpolated with manual escaping. It does not scale to a
record with nested structs, `BTreeMap`s, and `Option`s: hand-rolling that
correctly is exactly the kind of copy-paste-prone code that produces a lying
field, the same class as `build_profile`'s current hardcoded literal, just
moved into the serializer instead of the value.

`toml = "0.8"` is already a dependency of `bench-harness` (`Cargo.toml:19`),
already used for `BenchManifest::load`, and `serde` derive is already the
established pattern for every structured type in this crate (`Sample`,
`BenchResult`, `BenchManifest`, `BenchSection`). Adopting it for the record
costs nothing new and matches the crate's own convention rather than adding
one.

**I checked this compiles and round-trips before proposing it, and the first
attempt found a real blocker.** `202608151419_probes/toml_roundtrip_main.rs`,
run first with a plain `u64` field for a dylib hash: `toml`'s serializer
refuses it, `serialize: OutOfRange(Some("u64"))`
(`202608151419_probes/toml_roundtrip_first_failure.out`). TOML integers are
signed 64-bit; there is no unsigned 64-bit type in the format, and dylib
hashes routinely have the high bit set. This is not a new problem for this
codebase: `config.rs:213-235` (`de_seed`) already solves exactly this for
`master_seed`, encoding it as a hex string precisely because "TOML 1.0 caps
integer literals at `i64::MAX`" (`config.rs:93-96`). The fix is the same
pattern applied to every raw-u64 field the record carries (`resolved_seed`,
`arm_dylib_hash`'s values): a small `serde(with = "hex_u64")` module,
serialize as `"0x...18-hex-digits"`, parse back with
`u64::from_str_radix`. Re-run with the fix
(`202608151419_probes/toml_roundtrip.out`, on the pinned nightly): round-trips
`u64::MAX` and a high-bit-set hash exactly, a record missing a required field
is refused at parse time (`TomlError { message: "missing field
\`manifest_key\`", ... }`, the negative control), and an empty (but present)
`axes` map deserializes fine, confirming that "axes must be non-empty" is a
rule the record's own validation has to enforce; the format does not give it
for free.

**The filename extension changes, `.meta.json` to `.meta.toml`.** The topic
says the record "replaces `.meta.json` at the same path as a superset"
(`...:39`); I read "same path" as same stem and same directory, and the
extension as the one thing worth changing, because writing TOML content under
a `.json` name would be actively misleading, and because section 1d already
established there is zero reader anywhere in the repository to break. This is
a decision I am making, not deferring: same directory, same file-per-cell
granularity, only the trailing three characters move.

## 6. Is a record written for a cell that failed before measuring? Yes, and here is what that requires in the driver.

The topic's own ratified text already answers this: "the record exists
whatever the verdict was, so nothing is lost" (`...:55`). That is not open for
me to relitigate; my job is to say precisely what it costs to keep true, which
the topic left to this file.

I traced every `continue` and every early `return` in the per-cell body of
`drive()` (`driver/mod.rs:310-556`) and classified each against whether the
identity/config sections are already resolved at that point:

| where | what happens today | identity/config known? | record today |
|---|---|---|---|
| `resolve_routine` Err (`:328-334`) | `return ExitCode::FAILURE`, whole run aborts | yes (`config` exists) | none, and no later cell in the manifest gets one either |
| validation drops every variant, `config.variant_paths.is_empty()` (`:423-430`) | `continue`, this cell only | yes | none |
| `run_orchestrator` Err (`:438-447`) | `continue`, this cell only | yes | none |
| variant name-collision (`:460-473`) | `return ExitCode::FAILURE`, whole run aborts | yes, plus partial `result` | none |
| CSV/report write IO error (`:475-478`, `:497-500`) | `return ExitCode::FAILURE`, whole run aborts | yes, plus full `result` | none |

Every path that can produce a `CellRecord` at all already has `config`
resolved, because `resolve_routine` is the first thing that can fail and it
already takes a fully-built `BenchConfig`. So **the record is always
constructible** the moment any of these paths is reached; today's zero-record
outcome is a gap in what the driver does with that fact, not a gap in what it
knows.

**One reordering is required to make the answer clean, and I am naming it
rather than treating it as free.** `output_paths`/`create_dir_all`
(`driver/mod.rs:337-341`) currently run *after* `resolve_routine` (`:328`), so
a `resolve_routine` failure has nowhere to write a record without first
creating the directory that step normally creates. Moving `output_paths` and
`create_dir_all` before `resolve_routine` is a pure reordering (path
arithmetic and a directory create; neither depends on the resolved routine)
and it means every failure path in the table above can write a record into
the same, already-staged, per-bench directory the CSV would have used.

**Consequence for the two hard-abort paths (`resolve_routine`,
name-collision).** These correctly abort the whole run today: a routine that
cannot be resolved or two variants sharing a name are manifest-level defects,
not properties of one cell, and letting the run continue past them would be
wrong regardless of this design. What changes is only that the *one* cell that
revealed the defect gets a record before the process exits, so the failure is
durable and greppable instead of living only in the stderr line that scrolled
past. The record's `execution.arms_measured` is empty, `execution.arms_dropped`
carries one synthetic entry naming the failure (`"(all arms): routine could
not be resolved: <reason>"` or the name-collision message), and `analysis` is
`None`.

**Consequence for the two soft-skip paths (validation-empties-out,
orchestrator Err).** These already `continue` past this cell to the next one,
so nothing about the run's control flow changes. What changes is that before
the `continue`, the driver writes the record: `execution.arms_dropped` carries
the real per-arm reasons (already computed and currently discarded, per the
validation expert's finding at `driver/mod.rs:377-419`, or the orchestrator's
`BenchError` for the second path), `execution.arms_measured` is empty, and
`analysis` is `None`.

**So `analysis: Option<...>` being `None` is not a special case bolted on for
this question; it is the natural consequence of `analysis` being keyed on
measured arms in the first place.** Zero measured arms is zero entries in a
map that was always going to be built that way, which is the sign the shape
in section 3 was right before this question was asked, not adjusted to answer
it.

## 7. `schema_v2`: additive columns, not a migration, and I checked why the existing pattern already decides this

`history.rs:31` (`SCHEMA_HEADER`) writes `# schema_v1\t...` on file creation,
but `load_in` (`:89-117`) **never reads or checks that header string at
all**; it is skipped by the same `if line.starts_with('#') { continue; }` that
skips any comment line. Versioning in the history TSV today is purely
documentary, not enforced, which I checked rather than assumed by reading the
whole parse loop. The load function's actual compatibility mechanism is a
minimum-length positional check, `if p.len() >= 9`, the same tolerant pattern
`sample.rs`'s CSV loader already uses and already has a test for
(`sample.rs:148-202`, section 0).

Given that, the cheapest, already-proven-pattern answer is: **`schema_v2`
appends columns at the end and changes nothing about the existing nine.**
`runid` and the build columns the topic already names (`...:49`) go on the
end: `runid`, `dylib_hash` (a JSON-array-free flat string; TSV has no nested
structure, so this is the one place I would encode it as a semicolon-joined
`arm=hash` list rather than reach for a structured format inside a
line-oriented log, matching the file's own grain), `build_profile`. A
`schema_v1` row loaded by the updated `load_in` (extended to `p.get(9..)`
the same way `sample.rs` extends past its old boundary) simply has those three
fields default to empty/absent, exactly as an old CSV's `setup_ns`/`digest`
already default to `0.0`/`0` today. **No rewrite of committed `.tsv` files,
no migration pass, no version-detection branch**: the same file, the same
loader, three more columns, read positionally with defaults for anything
shorter. The header line changes to `# schema_v2\t...` for a human skimming
the file, and stays cosmetic for the parser, exactly as `schema_v1`'s header
already is.

I want to flag plainly, given section 0: **this design is being proposed
against a read path with zero test coverage.** The tolerant-parsing pattern I
am relying on is proven correct for `sample.rs`'s CSV loader, which has a
test naming exactly this scenario (old/perf/new column counts). No equivalent
test exists for `history.rs`'s TSV loader. Whoever implements `schema_v2`
should write that test first (a three-row fixture: `schema_v1` nine columns,
`schema_v2` twelve, a truncated row) before trusting that the same pattern
holds here by analogy alone.

## 8. The runid: per invocation, reusing the mechanism that already exists

`driver/staging.rs:87-99` (`create_stage_root`) already computes a runid,
`<timestamp>-<pid>`, exactly once per `drive()` invocation, and it is already
well-tested (four tests in that file cover quarantine and promotion against
it). The topic's complaint (`...:21`, "the runid appears in no other
artifact, so nothing ties a run together") is that this value stops at the
filesystem staging layer and never reaches any artifact.

**Answer: keep it per invocation, and thread the same string into every
`CellRecord` written during that invocation.** This is the smaller change
(`create_stage_root` needs to also return the runid string alongside the
`PathBuf` it already returns, a one-line addition, rather than a new
generator), and it is the more useful grain: a per-cell runid would answer
"when did this specific measurement happen" (already answered by
`environment.timestamp`) while losing "which cells ran together in one
invocation of `cargo mock bench run`," which is exactly the correlation the
topic is asking for and which nothing else in the record provides. For
`--report-only` (no new measurement, no new `stage_root`, section 1c): no new
runid is minted and no new record is written; the existing record on disk is
read back, runid included, unchanged.

## 9. What I would not change

**The samples CSV's seventeen columns, cdylib isolation, and the artifact
trail generally.** Named as load-bearing in the brief; I found nothing in
this design that touches any of it. The record sits beside the CSV, not
inside it.

**`Cache`'s skip-rerun mechanism, as a system.** Section 2. It is unwired
dead code today; this design reuses its one live primitive
(`dylib_hash`) and does not decide whether the mechanism itself should ever
be wired in.

**The staging/promotion transaction (`driver/staging.rs`).** The record's
per-bench directory is the same one `output_paths` already computes under
`stage_root`, so it is promoted or quarantined by the existing
crash-safety mechanism for free, with no change to `staging.rs` beyond the
one-line runid-return addition in section 8.

**`ValidationPlan`, `CrossVariant`, and everything about what a mismatch
means.** Owned by the validation topic; I gave the eventual verdict a field
(`execution.validation`) and nothing more.

**`bench-matrix`'s generator, `MatrixDecl`, and the sweep vocabulary.** Owned
by the sweep topic; `identity.axes` is deliberately shaped to receive whatever
that topic decides without needing to change again, per the forcing
constraint it names.

## 10. What I could not settle

- **`build.dep_spec`'s cost.** I did not price reading a variant crate's
  `Cargo.toml` (or, more accurately, `Cargo.lock`) per record write. It is
  marked optional and best-effort rather than benched; whoever implements it
  should measure before assuming it is free, per this workspace's own
  discipline about pricing claims.
- **The exact reason string format for `execution.arms_dropped`.** I gave it
  a type (`{arm, reason}`) and reused the driver's existing free-text reason
  strings; whether these should become a closed enum once the validation
  topic settles its own failure taxonomy is that topic's call, not mine, and
  I did not want to pre-empt it by inventing a taxonomy here.
- **Whether `build.profile`'s handoff file should be per-variant-crate or
  per-invocation.** Today `PROFILE_ARGS` is uniform across a whole build
  (section 1b), so a single per-invocation handoff would be sufficient right
  now; once a per-bench override lands it may need to be per-arm. I named the
  requirement (the build step must persist what it actually passed, beside
  what it built) and left the granularity to whoever designs that landing,
  since it is genuinely their unresolved surface, not mine.
- **Whether `Config.tuning`'s `bootstrap_iterations` is worth carrying given
  `config.rs:578-581`'s own comment that it is "currently informational: the
  analysis module reads from a const."** I carried it anyway on the
  reasoning that the record should describe the resolved config even where a
  known gap means that config is not yet fully honoured; I did not verify
  whether wiring it end-to-end (task #281 per that comment) is in scope for
  this round.
