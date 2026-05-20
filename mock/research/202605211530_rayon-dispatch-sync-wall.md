---
date: 2026-05-21
phase: research
scope: mockspace-rs engine dispatch
status: parked
---

# Rayon dispatch hits a Sync wall in MockspaceDocument

Schema design memo §15 calls for parallel per-document dispatch via
rayon. The implementation attempted on `feat/phase-2d-rayon` did not
compile, surfacing a real soundness wall worth recording before the next
attempt.

## What failed

```rust
project
    .documents_slice()
    .par_iter()
    .try_for_each(dispatch)?;
```

`par_iter` on `&[MockspaceDocument]` requires `MockspaceDocument: Sync`.
The compiler refuses:

```
error[E0599]: the method `par_iter` exists for reference
  `&[MockspaceDocument]`, but its trait bounds were not satisfied
```

## Why

`MockspaceDocument` carries two lazy AST caches:

- `syn_ast_cache: OnceCell<Option<syn::File>>`
- `tree_sitter_cache: OnceCell<Option<tree_sitter::Tree>>`

`OnceCell<T>: Sync` only when `T: Sync`. Both inner types fail:

- `syn::File` contains `proc_macro2` types (`Ident`, `Span`, etc.) that
  use `Rc<...>` for string interning even in fallback (non-proc-macro)
  builds. `Rc<T>` is `!Sync` because its refcount mutates non-atomically
  on clone / drop.
- `tree_sitter::Tree` carries a raw FFI pointer behind a `Send`-marked
  newtype, but its `Sync` status is conditional on the underlying C
  library's thread-safety guarantees and is not advertised in the trait
  bounds.

## Why `unsafe impl Sync` is not sound

Tempting argument: "once initialised, we only read `&syn::File`; reads
don't bump the `Rc` refcount." This is true for our current access
pattern but it's a lie about the type's contract, not a sound proof.

- `Rc<T>: !Sync` is part of the `Rc` API contract. The auto-trait
  system relies on that contract to forbid sharing `&Rc<T>` across
  threads. An `unsafe impl Sync` would override the contract for our
  type while the inner `Rc` still expects single-thread access.
- Any future addition to a `Lint::check_document` impl could clone a
  Span, clone an Ident, or otherwise touch the inner `Rc` refcount.
  Two threads doing that concurrently corrupts the count silently.
- `OnceCell::sync::get_or_init` synchronises first-init. After init,
  the cell hands out `&T`; `T`'s `Sync` contract governs every read
  and every method call. `Rc` fails that contract.

`unsafe impl Sync for MockspaceDocument` would compile and pass on
today's exact access pattern, but the marker promises more than the
type actually delivers. The next code change that touches a `Span`
or `Ident` clone path silently breaks soundness with no compile
error. Future agents reading the marker would reasonably assume the
type is safe to share, which it is not.

## Paths forward (in order of preference)

### A. Pre-extract only thread-safe data per lint

Naive Arc-wrap of the AST does not help: `Arc<T>: Sync` requires
`T: Sync + Send`, and `syn::File: !Sync` for the same `Rc` reason.
Wrapping the cache adds nothing.

The real shape: each lint declares what thread-safe slice of the
document it needs (typically `Arc<[String]>` of identifiers, or a
small index struct), and a pre-pass populates that slice single-
threaded while walking the AST. The slice is then `Sync`, the
parallel dispatch ships only those slices, and the original
`syn::File` stays single-thread-bound for the pre-pass.

This is the right long-term shape but requires a per-primitive
extraction API. Every lint primitive grows a thread-safe slice
descriptor; the engine schedules pre-pass and parallel pass
separately. Significant surgery.

### B. Per-thread re-parse

Hand each thread its own copy of the source bytes; let each thread
re-parse with `syn::parse_str` on demand. Cheap-ish (parse cost is
microseconds per file for small files) but loses the per-doc cache
benefit across lints. Net win depends on lint count and file size.

### C. Lint-level parallelism, not document-level

Instead of `par_iter` over documents per lint, `par_iter` over
*(lint, doc)* pairs grouped by lint. Each lint instance is `Send + Sync`,
so the closure can hold `&dyn Lint`. The problem is the same: each
closure invocation needs `&MockspaceDocument`, which still needs Sync.

### D. Mutex-protected caches

Wrap `OnceCell<Option<syn::File>>` in `Mutex<Option<syn::File>>`. Per
lint-call lock contention. For lints that share docs (and they all do),
threads serialise on the lock during AST access. Defeats parallelism
within a doc's lint set but allows parallelism across docs. Probably the
honest interim fix.

### E. Accept sequential

Bench reality: even at 5000 files and 21 lints, sequential dispatch is
sub-second on warm caches. Parallel dispatch becomes load-bearing only
when consumer codebases dwarf the current workspace (10k+ files) AND
lints become non-trivial in CPU time. Today neither holds.

## Recommendation

Path A (pre-extract immutable thread-safe view per lint) is the right
shape long-term but requires API changes per lint primitive (each lint
defines what thread-safe slice of the doc it needs). Path E (accept
sequential) is the right shape today.

Park task #534 with this note as the artifact. Revisit when:
- A real workload shows sequential dispatch as the bottleneck.
- The lint pack grows past ~50 lints.
- Per-lint check_document goes beyond ~1ms on a realistic doc.

Until then the design memo §15 promise stands as aspirational; the
shipped engine documents the gap inline at the dispatch site.

## What this PR did not do

Nothing committable. The branch is closed without merge. The two
deliverables of the attempt land as:

1. Inline doc comment in `mock/crates/mockspace-rs/src/engine.rs`
   explaining why dispatch is sequential and naming the schema-memo
   intent (separate commit on `dev`).
2. This research note (committed to `dev` directly via a `chore:`
   commit).
