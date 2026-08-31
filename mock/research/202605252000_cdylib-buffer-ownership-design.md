# Design: cdylib output buffer ownership

**Date:** 2026-05-25
**Scope:** Resolve PR #212 reviewer Finding F2 (the load-bearing design question for the whole cdylib lint catalogue). Decides who owns the `DiagnosticBatch` output buffer when a cdylib's `LintEvaluateVtable::evaluate` writes diagnostics.
**Parent context:** mockspace round 202605251600's SRC CL (cdylib port). PR #212 attempted to ship the no-todo cdylib with a `static mut DIAG_BUFFER: [MaybeUninit<Diagnostic>; 256]` pattern; reviewer flagged the design as unbacked by the ABI doc and unsafe under arbitrary-time multi-thread dispatch.

## The question

`LintEvaluateVtable::evaluate` returns a `*mut DiagnosticBatch` whose `entries: *const Diagnostic` field points at where? The vtable doc at `viola/mock/crates/viola-plugin-abi/src/vtable.rs:95-110` is silent on call ordering, but the adjacent `DiagnosticBatch` doc at `viola/mock/crates/viola-plugin-abi/src/diagnostic.rs:90-91` already commits:

> "Buffer ownership is plugin-side; the host copies before the next invocation."

That sentence pins both plugin-side ownership and "before the next invocation" serial-call semantics. PR #212's `static mut DIAG_BUFFER` shape is consistent with that doc, not an invention. The reviewer's flag stands on a different axis: the `hilavitkutin-extensions` arbitrary-time linking contract permits parallel dispatch, which conflicts with `DiagnosticBatch`'s implicit serial-call assumption. The contradiction is between two ABI docs in viola-plugin-abi (the per-provider lifetime contract on `DiagnosticBatch`) and the cross-extension contract documented in hilavitkutin-extensions (any extension loads, runs, drops at arbitrary points independent of siblings).

The question is whether to RATIFY the existing v1 commitment (Option A; generalise "before the next invocation" to "across all invocations against this provider" and document the per-provider serial-call rule explicitly), or OVERTURN it (Option B; flip the buffer ownership to host-side so the per-provider serial assumption disappears).

## Three options

### Option A: ABI spec commits to serial-call semantics

Add a clause to `viola-plugin-abi/src/vtable.rs` documenting that `LintEvaluateVtable::evaluate` is called serially per provider. Hosts that dispatch lints in parallel must do so across distinct providers, not concurrent calls to the same provider.

The cdylib's existing `static mut DIAG_BUFFER` pattern works. The host buffers one batch per provider before invoking the next.

**Tradeoffs**: cheap to implement (one doc line). Forecloses host-side parallelism *within* a provider (e.g. multi-file parallel dispatch). The host can still parallelize across distinct providers (multiple lints running in parallel against different files), so the throughput cap is "one core per ported lint" rather than "one core per source file per lint". Adequate for the first port; future workload pressure can revisit.

### Option B: Host owns the output buffer, plugin writes through

Change the vtable signature to take a `*mut Diagnostic` slice the host pre-allocates, with `*mut arvo::USize` for the count:

```rust
#[repr(C)]
pub struct LintEvaluateVtable {
    pub evaluate: unsafe extern "C" fn(
        host_ctx: *mut c_void,
        nam: *const NamPayload,
        lint_config_bytes: *const u8,
        lint_config_len: arvo::USize,
        out_entries: *mut Diagnostic,    // host-allocated; capacity in next slot
        out_capacity: arvo::USize,
        out_len: *mut arvo::USize,
    ) -> AbiStatus,
}
```

The plugin writes up to `out_capacity` entries, sets `*out_len` to the actual count, returns Ok. On overflow, the plugin returns `Internal` with `*out_len` set to the count it would have emitted.

**Tradeoffs**: plugin is purely a function over its inputs; no plugin-side state, no aliasing concerns, no serial-call assumption. The host can call the same provider concurrently from multiple threads as long as it passes distinct output buffers. Cost: ABI break (vtable signature changes), which means re-versioning to `viola.lint.evaluate.v2`. Pre-1.0 affords this cleanly per `no-legacy-shims-pre-1.0.md`: v1 deletes outright on v2 ship (no deprecation alias), and the version axis covers the bump. The `DiagnosticBatch` `#[repr(C)]` carrier in `diagnostic.rs:88-98` either deletes (clean per the same rule) or stays for host-side emit paths that don't traverse the v2 vtable. Recommendation under B: delete `DiagnosticBatch` together with v1 since no consumer outside the vtable references it; if a future emit path needs the same shape, it ships under a fresh name.

### Option C: Per-call dynamic alloc (rejected)

Cdylib allocates a `Box<[Diagnostic]>` per call, returns its pointer via `entries`, leaks it to the host. Host copies, drops a callback into the cdylib to free.

Rejected: no_std + no alloc on the cdylib's side forbids `Box`. Even if relaxed, the free callback adds another vtable slot and another lifetime contract.

## Recommendation: Option B

The buffer-ownership question is the same shape as every output-buffer FFI in any C ABI. Industry convention is the host owns the buffer; the plugin writes through it. The arbitrary-time linking contract on the host side reinforces this: the host *should* be free to parallelize, and the plugin should not encode assumptions about call cadence.

The cost is one ABI version bump. The current `viola.lint.evaluate.v1` has zero shipped plugins (no-todo's PR #212 closed without merging). Shipping `viola.lint.evaluate.v2` with the host-owned-buffer shape as the new canonical form, and deprecating v1 at the same time, costs nothing because there's nothing to break. Pre-1.0 affords this exactly.

A v2 ABI is the cleaner solution. The cdylib lint catalogue ports against v2 from the start.

**Op confirmation point**: Option A vs B is the load-bearing call. Option A is one doc-line change to ship; Option B is an ABI version bump + cdylib redesign. Agent's call: B. Option A is defensible if op prefers minimum surface change.

## What this memo does NOT do

- Does not edit `viola-plugin-abi`'s vtable.rs. The ABI change ships in its own slice in viola, against the design this memo settles.
- Does not re-implement the no-todo cdylib. The cdylib ships once the ABI version is settled.
- Does not address the arvo dedup or tree-sitter conflict in mockspace's mock workspace. Those are separate slices (patch table fix).

## Open questions for op

1. **Option A or Option B?** (Agent's call: B, on technical grounds for thread safety and future scalability.)
2. **If B: is the ABI re-version named `viola.lint.evaluate.v2`?** Per task #610 R4's per-lint provider id shape, plugins would export `<pack>.lint.<name>.v2` and the vtable shape behind it is the new host-owned-buffer one. Or does op want a different naming axis (e.g. `viola.lint.evaluate.v1.host-buf`)?
3. **If B: what's a sensible default capacity the host should pre-allocate?** PR #212 used 256 slots per call as the static buffer; the host-owned shape lets the host pick. 256 is a reasonable starting cap (covers typical source files); the host can grow per workload.

## Settle the question, then unblock

Once op confirms (or the agent's B call lands without redirect), the cdylib slice resumes:

1. **viola-side slice**: viola-plugin-abi gains the v2 vtable + provider id, with the new signature.
2. **mockspace-side slice**: cdylib reimplements against v2, drops the static mut buffer, simpler evaluator body.
3. **Patch-table slice in mockspace mock workspace**: independent of the buffer question; needs to land for the cdylib to compile inside the workspace.

## See also

- PR #212 (closed) commit `1acbfc3`: the static-mut buffer attempt; recoverable for reference.
- `viola/mock/crates/viola-plugin-abi/src/vtable.rs:95-110`: the v1 vtable shape.
- `hilavitkutin/mock/crates/hilavitkutin-extensions`: arbitrary-time linking contract.
- `~/Dev/clause-dev/.claude/rules/no-legacy-shims-pre-1.0.md`: justifies the clean v1 to v2 ABI bump without deprecation aliases.
- Mockspace round 202605251600 DOC CL R2 (`fixed-cap 256 slots, Internal on overflow`): the cap value can carry through to v2 unchanged.
