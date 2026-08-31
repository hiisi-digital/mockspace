# Rayon per-document dispatch (#534) design memo

Date: 2026-05-23
Phase: research
Source topic: #534 (Phase 2D-perf, schema memo §15)

## The framing

The lint engine's `LintMode::PerDocument` branch in
`mock/crates/mockspace-rs/src/engine.rs` walks documents
sequentially today. Schema memo §15 calls for rayon parallelism
across the document axis (each lint independently dispatches across
its scoped document set; documents have no cross-document
dependencies within a single lint's `check_document` call).

This memo is the unblock plan; implementation lands when the work
falls under "consumer adoption" pressure or when a benchmark
surfaces the cost on realistic workloads.

## Why it cannot land as a one-line change

`MockspaceDocument: !Sync` AND `!Send` today. An
`assert_sync::<MockspaceDocument>()` probe inside the test suite
fails to compile; the rustc error names a handful of inner types
that don't satisfy the marker:

- `Rc<()>` (single-threaded reference count, not Send and not Sync)
- a Span type carrying a non-thread-safe identity
- a TokenStream type with the same constraint

The exact type names depend on the proc-macro2 build configuration:
in the nightly-bridge path syn re-exports the compiler-internal
`proc_macro::Span` / `proc_macro::TokenStream`, in the stable
fallback it uses `proc_macro2::fallback::Span` etc. The names vary;
the soundness argument is independent of which.

The `!Send` half of this is load-bearing and changes the unblock
analysis below. The original draft of this memo proposed `Mutex`-
wrapping the AST caches as the safe-by-default fallback. That is
wrong: `Mutex<T>: Send + Sync` requires `T: Send`, and the inner
`syn::File` is `!Send`. The Mutex path does not work.

The cached `Option<syn::File>` lives inside
`OnceCell<Option<syn::File>>` from `once_cell::sync` (NOT
`core::cell::OnceCell`, which is `!Sync` regardless of contents).
`once_cell::sync::OnceCell<T>` is `Sync` iff `T: Send + Sync`; here
the contents (`Option<syn::File>`) are not Sync, so the whole
document type loses Sync.

The tree-sitter cache (`OnceCell<Option<tree_sitter::Tree>>`) is a
separate axis. tree-sitter::Tree was historically not Sync; modern
versions may be. The `RwLock<HashMap<StripOpts, Arc<str>>>` for
stripped source IS Sync.

## Unblock paths after the !Send realisation

### Path A (rejected): Mutex-wrap the AST caches

Does not work. `Mutex<T>: Send + Sync` requires `T: Send`. The
inner `syn::File` is `!Send` because of `Rc<()>` and
`proc_macro::Span`. The Mutex wrapper inherits the same constraint
the bare cell had. Documented here for completeness; do not
attempt.

### Path B (still possible): `unsafe impl Send + Sync for MockspaceDocument`

The naive framing "read-only after init is sound" is **wrong** at
the type level. `&T: Send` requires `T: Sync`. If `proc_macro::Span`
holds an `Rc<()>` and the lints' syn read paths ever clone that
Span (or any token containing such a Span), every concurrent
`Rc::clone` race is undefined behaviour. The `!Sync` bound is the
compiler refusing to accept those races; "we only read" is not
enough.

The actual soundness condition for Path B:

- The lints' syn read paths called from `check_document` never
  trigger `Rc::clone`, `Rc::downgrade`, or `Drop` on any inner
  `Span` or `TokenStream`. Pure traversal that reads `&Spanned::span`
  shared-references and never clones them is sound; anything that
  walks via `fold`/`visit` and constructs new Spanned values is not.
- `once_cell::sync::OnceCell` writes happen-before subsequent reads
  via the crate's documented release/acquire ordering, so partial
  initialisation is not visible.
- The syn version pin is locked at a specific minor whose
  audited-clean read-paths are known.

The audit work the impl PR must carry:

1. Enumerate the syn surfaces every lint's `check_document` body
   touches (`mock/crates/mockspace-rs/src/builtins/*.rs` callers
   of `doc.ast()`).
2. For each, confirm the traversal does not call `Span::clone`,
   `TokenStream::clone`, or any wrapper that internally clones.
3. Lock syn to a specific minor version known to satisfy the
   above (a semver-compatible patch could introduce
   interior-mutability changes; the pin protects against drift).
4. Document the impl with a `// SAFETY:` block enumerating the
   audited read paths.

If the audit turns up any `Rc::clone` on the hot path, Path B is
unsound and the work falls back to Path A.

Cost: zero runtime overhead in the happy case; significant audit
work up front.

Benefit: best perf when the audit passes. Risk: a future lint's
`check_document` body could add a syn API call that clones a
non-Sync inner; that breaks soundness silently. Mitigation:
document the audit, gate new lint primitives' read-path changes
behind a re-audit, and consider a miri-based concurrent-read test
that exercises the dispatch loop.

### Path C: reparse-per-worker (Send-free)

The rayon worker receives the document by reference, owns its own
copy of the parsed AST for the duration of its task, drops it
before returning. No sharing across threads at any point. Cost: N
reparses per document where N is the number of lints that touch
the AST.

For a workload of ~50 documents and ~7 AST-hot lints, that is
~350 syn parses per `mock check` vs ~50 today. syn is fast (single
files measured at sub-millisecond on modern hardware for typical
sizes), so the cost may be tolerable; benchmark needed.

This is the only path that does not require `unsafe`. The
implementation reshapes the engine's per-document dispatch loop:
each rayon worker takes a `(lint, document)` pair, parses the
document on its own thread, runs the lint, drops the AST.

### Path D: replace syn with a Send+Sync parser

`tree-sitter::Tree` is already cached in `MockspaceDocument`. If
the lints can be ported off syn entirely (or onto a wrapper that
re-emits a Send+Sync IR), the rayon path opens up.

Out of scope for #534; tracked as a longer-term direction.

## Recommendation

The original Path A is dead. Live options:

- **Path B** (unsafe impl Send + Sync) with the full audit:
  potential best perf, irreducible unsafe debt.
- **Path C** (reparse per worker): no unsafe, real but bounded
  perf cost from re-parsing. Benchmark to validate.
- **Path D** (replace syn): largest scope; tracks as follow-up.

For #534, default to **Path C unless a benchmark shows the reparse
cost dominates**. The benchmark uses bench-harness (#604's
`report_from_csv` chain) on a synthetic 500-document fixture
comparing sequential, Path C, and (if audit clears it) Path B.

This memo's first iteration was wrong about the safety axis (`&T:
Send` requires `T: Sync`) and the send axis (Mutex requires Send).
Both got tightened in this revision. Implementation work remains
gated on the bench numbers; no impl PR should land before that.

## Implementation steps

1. Benchmark first. Bench-harness infrastructure (#604's
   `report_from_csv` chain) compares sequential vs Path C
   (reparse-per-worker) variants on a synthetic 500-document
   fixture. If Path C's reparse cost is bounded (say within 2x of
   the sequential baseline), Path C wins by avoiding unsafe
   entirely.
2. If Path C clears the bench, ship the impl. Reshape the per-
   document loop in `engine.rs::Engine::run` to dispatch
   (lint, doc) pairs in parallel; each worker re-parses the doc
   on its thread before invoking `lint.check_document`. The
   existing OnceCell caches remain single-threaded inside each
   worker.
3. The findings sink is already rayon-ready: the `FindingSink`
   trait declares `Send + Sync` and the default
   `VecFindingSink` impl uses `Mutex<Vec<Finding>>` for
   concurrent push (see
   `mock/crates/mockspace-rs/src/finding_sink.rs`). One fewer
   blocker; the dispatch loop's task closures can capture
   `&sink: &dyn FindingSink` directly. No sink refactor needed.
4. Test plan: the #564 differential e2e tests
   (`check_is_byte_deterministic_run_to_run` +
   `check_findings_are_path_order_independent`) cover the
   regression surface. Run them in a loop (e.g. 100 iterations)
   on the rayon-enabled branch; any nondeterminism surfaces.
5. If the benchmark shows Path C's reparse cost dominating,
   escalate to Path B: run the syn-read-path audit, lock the syn
   version, write the `unsafe impl Send + Sync` with a
   `// SAFETY:` block enumerating the audit results, gate behind
   a miri-aware concurrent-read test.

## Out-of-scope follow-ups

- `LintMode::ProjectScoped` and `TwoPhaseProject` are inherently
  per-project; they do not benefit from per-document rayon. A
  separate axis (parallelise across lints rather than across
  documents) would address them but requires
  `InstantiatedLint: Sync` end-to-end, which carries its own
  analysis.
- The `entries.iter()` outer loop could also rayon-parallelise
  (each lint runs against its own document set independently).
  This composes with per-document parallelism but adds rayon
  scheduling overhead; benchmark-driven decision.

## Cross-references

- `mock/crates/mockspace-rs/src/engine.rs` per-document dispatch
  loop (the comment block above the `dispatch` closure carries the
  short summary and a pointer to this memo).
- `mock/crates/mockspace-rs/src/document.rs` `MockspaceDocument`
  struct (the OnceCells live here).
- `mock/crates/mockspace-cli/tests/e2e_check.rs`
  `check_is_byte_deterministic_run_to_run` +
  `check_findings_are_path_order_independent` (the #564 regression
  surface that catches rayon-introduced nondeterminism).
- Schema design memo §15 (the original perf direction).
- Workspace rule `.claude/rules/no-alloc-no-std-framing.md` (rayon
  brings a thread-pool dependency; mockspace's `Cargo.toml` carries
  it for build infrastructure already, so this does not introduce
  new ecosystem deps to consumers).
