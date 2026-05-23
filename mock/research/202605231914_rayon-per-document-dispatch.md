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

`MockspaceDocument: !Sync` today. The compiler reports three
specific non-Sync members inside `syn::File`:

- `proc_macro::Span` (not Sync)
- `proc_macro::TokenStream` (not Sync)
- `Rc<()>` (single-threaded reference count)

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

## Two unblock paths

### Path A: Mutex-wrap the AST caches

Change `OnceCell<Option<syn::File>>` to
`Mutex<Option<OnceCell<Option<syn::File>>>>` (or a custom dual-state
lock pattern). Every cache hit pays a Mutex lock + unlock pair.

Cost: AST-hot lints (no-bare-numeric, ast-type-position-based,
suppression-meta) call `doc.ast()` once per document. Under rayon
that single call per document also serialises through the Mutex if
multiple threads land on the same document. The latter does not
happen in the natural lint dispatch shape (each lint+document pair
runs once), so contention is low in practice.

Benefit: zero unsafe, mechanically safe, surfaces the cost cleanly
in a profiler.

### Path B: `unsafe impl Sync for MockspaceDocument`

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

## Recommendation

Path A or Path B depending on the syn-read-path audit. Path A
(Mutex-wrap) is the safe default if the audit work is deferred or
turns up any `Rc::clone` call on a Span or TokenStream from the
lints' read paths. Path B (unsafe impl Sync) is the right choice
**only** if the audit confirms no such clone happens, paired with
a syn version pin and a `// SAFETY:` block enumerating the
audited read paths.

Default to Path A. Escalate to Path B with the audit when a
benchmark actually shows the Mutex contention costing real time.

## Implementation steps

1. Default to Path A. Wrap the AST caches in
   `Mutex<OnceCell<...>>` or equivalent. The Mutex path is sound
   by construction and the audit becomes a follow-up optimisation.
2. Wire rayon. Change the per-document loop in
   `engine.rs::Engine::run` to use
   `documents.par_iter().try_for_each(|doc| dispatch(doc))` or
   equivalent.
3. The findings sink (`DiagnosticSink`) needs to be Sync-safe under
   concurrent push. Check the current impl: if it's a Mutex
   already, rayon dispatch composes; if it's not, that becomes the
   next blocker.
4. Test plan: the #564 differential e2e tests
   (`check_is_byte_deterministic_run_to_run` +
   `check_findings_are_path_order_independent`) cover the
   regression surface. Run them in a loop (e.g. 100 iterations) on
   the rayon-enabled branch; any nondeterminism surfaces.
5. Benchmark: bench-harness has the infrastructure (#604's
   `report_from_csv` chain). Wire a variant cdylib that runs
   `Engine::run` against a 500-document synthetic fixture; compare
   sequential vs Path A vs Path B (if the audit clears it). Numbers
   drive the Path A to Path B promotion decision.
6. If benchmarks justify Path B: run the syn-read-path audit
   described above, lock the syn version, swap the Mutex for
   `unsafe impl Sync` with a `// SAFETY:` block enumerating the
   audit results.

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
