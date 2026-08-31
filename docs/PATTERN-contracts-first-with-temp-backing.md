# Pattern: contracts-first with a temporary backing

A repeating shape across this workspace: a future runtime is on the way, but consumer crates need its shape today. Rather than blocking, or rather than shipping a concrete one-shot that gets rewritten later, the pattern is:

1. **Read the eventual spec.** Identify the trait surface, the typestate, the assoc-type wiring the eventual backing expects.
2. **Author those traits in the consumer crate today.** Sized to match the eventual spec. Public surface is the trait, never the concrete type.
3. **Wire consumer code against traits via generics or assoc types.** Internals bubble up through generic parameters; consumers code against the trait.
4. **Ship a temporary backing.** Concrete shape depends on the cross-boundary contract: `Box<dyn Trait>` when the consumer statically links Rust, a cdylib adapter that wraps a loaded `unsafe extern "C" fn` vtable when the consumer ships as a cdylib. The temporary backing satisfies the trait so production consumers can use it today.
5. **When the real backing arrives, swap it at the app-startup wiring point.** Trait surface stays. Consumer code unchanged. Migration is mechanical.

## Choosing the temporary backing

Two cases dominate. Pick by the boundary the consumer crosses to reach the host.

### In-process Rust consumer

Consumer is a Rust crate linked into the host binary at build time (`rlib` or `dylib`). The Rust sidecar metadata, the `.init_array` ctor pattern (`inventory::submit!`), and the type system info all cross the boundary; the host sees the consumer's static-init effects.

- Temporary backing: `Box<dyn Trait>` registered via `inventory::submit!` at static-init.
- Migration: when the future runtime arrives, the engine adds a second dispatch path; the in-process trait surface remains for built-ins and static-linked consumers.

### Cross-language / cdylib consumer

Consumer ships as `crate-type = ["cdylib"]` (or another non-Rust language entirely). The cdylib strips Rust-specific machinery for C ABI compatibility. The `.init_array` ctor runs in the cdylib's address space and does NOT populate the host's static registries. Inventory does not work here. `libloading` + `dlsym` + `extern "C" fn` vtables are the only viable contract.

- Temporary backing: a host-side adapter that loads the cdylib via `libloading::Library::new`, dlsyms named symbols, validates an ABI hash, and implements the trait by marshalling calls through the loaded vtable.
- Wire shapes: `#[repr(C)]` structs only. No `String`, no `Vec<T>`, no Rust-owned references across the boundary. `(*const u8, usize)` carriers, plugin-owned static memory.
- ABI hash: a const FNV-1a (or similar) computed from the wire layout, exported by both sides as a named symbol, checked at load.
- Migration: when the future runtime arrives, the engine swaps its bespoke symbol-based loader for the runtime's full descriptor protocol (e.g. viola's `ExtensionDescriptor` over hilavitkutin-extensions). The vtable shape and the consumer's cdylib do not change if the vtable was authored to mirror the eventual runtime's vtable.

## Workspace precedents

The mockspace stack runs the contracts-first pattern in several places.

**Engine layer (hilavitkutin).** The `WorkUnit` / `Resource` / `Column` / `AccessSet` traits are authored against the eventual scheduler shape. Hilavitkutin's scheduler implementation is the real backing; consumer apps see only the trait surface. In-process Rust boundary.

**Bench harness.** Variant cdylibs export three named symbols (`bench_abi_hash`, `bench_name`, `bench_entry`) per `bench-core::{AbiHashFn, BenchNameFn, BenchEntryFn}`. The host dlsyms each via `libloading`, checks the FNV-1a ABI hash, dispatches the entry function. Wire shape is `#[repr(C)] FfiBenchCall`. This is the workspace's canonical cdylib-loading pattern. See `bench-core/src/lib.rs:320-358` and `bench-harness/src/harness.rs:90-120`.

**Viola plugin ABI.** Viola plugins are cdylibs exporting a descriptor via the `DESCRIPTOR_SYMBOL` constant from `hilavitkutin-extensions`. The descriptor points at provider entries; each provider id maps to a `#[repr(C)]` vtable (e.g. `LintEvaluateVtable` with `evaluate: unsafe extern "C" fn(host_ctx, nam, lint_config_bytes, lint_config_len, out_batch) -> AbiStatus`). When viola becomes mockspace's lint runtime, mockspace lints loaded through mockspace's symbol-based loader migrate by adopting the descriptor protocol; the vtable shapes and wire format are designed today to match viola's so the cdylibs themselves do not need to change.

**Lint surface (mockspace-rs, in flight).** Consumer-authored lints have two boundaries:
- Static-link Rust consumers register `Box<dyn Lint>` via `inventory::submit!{CatalogEntry { ... }}`. This is the in-process path; works for built-ins and for Rust consumers shipped as rlib/dylib.
- cdylib consumers ship a `.so`/`.dylib` exporting the mockspace lint ABI (named symbols per the bench-harness precedent; vtable shape matching viola's `LintEvaluateVtable`). A host-side cdylib adapter loads them and exposes them through the same `Box<dyn Lint>` surface the engine already dispatches against. Same trait, different backing.

## Why this over the alternatives

- **Versus deferring:** consumer work would be blocked indefinitely. The future backing is by definition not on the critical path; locking the contract is what unblocks downstream.
- **Versus shipping a concrete one-shot:** every consumer codes against a name that gets renamed when the real backing arrives. Mechanical churn at every call site instead of a single startup-wiring swap.
- **Versus copying the eventual spec verbatim and stubbing it out:** the trait surface needs to be production-shaped now, with a backing that actually works. Stubs that panic are not the same as a working temporary backing.

## The rule of thumb

Before authoring a new abstraction for a consumer crate, ask:

> Is there a known eventual backing for this shape? (viola, hilavitkutin scheduler, persistence spine, etc.)

If yes, read the eventual spec and author the trait surface against it now. Decide which boundary the consumer will cross (in-process Rust or cdylib) and ship the matching temporary backing. Document the migration path in a memo (per this file's shape) so future agents know which assoc-type wiring point or which loader to flip when the real backing lands.

If no, then the contracts-first pattern does not apply: just write the concrete type for the consumer's actual need.

## What this is NOT

- This pattern does not justify writing speculative trait surfaces "just in case" a future backing arrives. The eventual backing must have a real specification, with a real timeline, that the trait surface targets.
- This pattern does not bless `Box<dyn>` in hot paths or in `no_std` boundary code unless the runtime polymorphism is genuinely needed. The temporary backing is a means to an end; if the eventual backing is statically dispatched (typestate generics, assoc types on a real type), the migration removes the `Box<dyn>` entirely.
- This pattern does not blur the in-process / cdylib boundary. The two cases have different temporary backings because the boundaries themselves are different. Conflating them (e.g. authoring an in-process Rust trait and assuming `inventory::submit!` reaches across a cdylib boundary) is the exact mistake the workspace's bench-harness precedent guards against.
- This pattern does not replace a design round. The trait surface and its migration path need to live somewhere durable (this file's pattern, or a per-crate `DESIGN.md.tmpl` section).
