# Sketch: no-todo as a viola plugin cdylib

**Date:** 2026-05-25
**Hypothesis:** A mockspace lint shaped like `no-todo` (a single-pattern `ContentRegexLint` instance per `mock/crates/mockspace-rs/src/builtins/content_regex.rs:113`) packages as a cdylib exporting `viola.lint.evaluate.v1` (`viola-plugin-abi/src/vtable.rs:102`) without requiring changes to the landed cdylib boundary, the landed lint substrate vocabulary, or the `viola-plugin-abi` wire shapes. The host-side `ViolaEngine` impl of `mockspace_core::lint::LintEngine` is the only new code path; the cdylib is a thin descriptor + evaluator pair.
**Outcome:** WORKS modulo one open assumption about the `NamPayload` schema (named below).
**Parent memo:** `mock/research/202605251000_lint-catalog-cdylib-boundary.md` (Path A recommendation).
**Status:** sketch. Not committed source. The DOC CL that follows references this sketch by filename.

## The cdylib source shape

A standalone Rust crate at (working name) `mockspace/mock/crates/lints/mockspace-builtin-lints/`. The `Cargo.toml` declares the crate as a `cdylib`:

```toml
[package]
name = "mockspace-builtin-lints"
version = "0.0.0"

[lib]
crate-type = ["cdylib"]

[dependencies]
hilavitkutin-extensions = { path = "../../../../hilavitkutin/mock/crates/hilavitkutin-extensions" }
hilavitkutin-extensions-macros = { path = "../../../../hilavitkutin/mock/crates/hilavitkutin-extensions-macros" }
viola-plugin-abi = { path = "../../../../viola/mock/crates/viola-plugin-abi" }
arvo = { path = "../../../../arvo/mock/crates/arvo" }
```

The crate body declares one provider entry per ported built-in lint. For the no-todo example this is one entry:

```rust
#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

use core::ffi::c_void;
use viola_plugin_abi::{
    AbiStatus, DiagnosticBatch, LintEvaluateVtable, NamPayload,
    PROVIDER_LINT_EVALUATE, ProviderEntry, ProviderId,
};
use hilavitkutin_extensions::{
    ExtensionDescriptor, ExtensionVersion, HOST_ABI_VERSION,
    MAX_DESCRIPTOR_LIST_LEN,
};

/// Provider id for the no-todo lint. Each ported lint gets one provider
/// id of the form `mockspace.lint.<name>.v1`. The provider id is matched
/// at host-side dispatch to pick which lint the descriptor entry runs.
const PROVIDER_NO_TODO: ProviderId =
    ProviderId::from_name("mockspace.lint.no-todo.v1");

/// The vtable static the descriptor entry points at.
static NO_TODO_VTABLE: LintEvaluateVtable = LintEvaluateVtable {
    evaluate: no_todo_evaluate,
};

/// Evaluator fn for the no-todo lint.
///
/// SAFETY: the host upholds the contract documented at
/// `viola-plugin-abi/src/vtable.rs:102`. The pointers are valid and
/// stable for the call's duration.
unsafe extern "C" fn no_todo_evaluate(
    host_ctx: *mut c_void,
    nam: *const NamPayload,
    lint_config_bytes: *const u8,
    lint_config_len: arvo::USize,
    out_batch: *mut DiagnosticBatch,
) -> AbiStatus {
    // 1. Resolve the file source view from the NAM payload.
    //    (See "Open assumption: NamPayload schema" below.)
    let source: &[u8] = unsafe { nam_read_source(nam) };
    let file_path: &[u8] = unsafe { nam_read_path(nam) };

    // 2. Optionally read the lint config. For no-todo today this is
    //    empty (no-todo has no tunable parameters). If a future lint
    //    needs config the cdylib deserialises from the (ptr, len) pair.
    let _config = (lint_config_bytes, lint_config_len);

    // 3. Run the no-todo scan. The body is unchanged from the in-process
    //    pattern at `mockspace-rs/src/builtins/content_regex.rs:124`:
    //    walk the regex, emit one diagnostic per match.
    let pattern = b"TODO";
    let mut offset: usize = 0;
    let mut emitted: u32 = 0;
    while let Some(found) = find_subslice(&source[offset..], pattern) {
        let abs_offset = offset + found;
        let (line, column) = byte_offset_to_line_col(source, abs_offset);
        // SAFETY: host upholds DiagnosticBatch slot capacity.
        let written = unsafe {
            diag_batch_push(
                out_batch,
                file_path,
                line,
                column,
                pattern.len() as u32,
                b"todo found in shipped source",
            )
        };
        if !written {
            // Batch full; surface as a non-fatal status the host knows
            // to retry with a fresh batch (out of scope for this sketch).
            return AbiStatus::Internal;
        }
        emitted = emitted.saturating_add(1);
        offset = abs_offset + pattern.len();
    }
    let _ = (host_ctx, emitted);
    AbiStatus::Ok
}

// Helpers (illustrative; final shape may live in a shared
// viola-plugin-abi extension trait).
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> { unimplemented!() }
fn byte_offset_to_line_col(source: &[u8], offset: usize) -> (u32, u32) { unimplemented!() }
unsafe fn nam_read_source(_nam: *const NamPayload) -> &'static [u8] { unimplemented!() }
unsafe fn nam_read_path(_nam: *const NamPayload) -> &'static [u8] { unimplemented!() }
unsafe fn diag_batch_push(
    _batch: *mut DiagnosticBatch,
    _path: &[u8],
    _line: u32,
    _column: u32,
    _length: u32,
    _message: &[u8],
) -> bool { unimplemented!() }

/// The descriptor exported via `__hilavitkutin_extension_descriptor`.
///
/// One `ProviderEntry` per lint. The entry's `provider_id` is the
/// per-lint id; the `vtable_ptr` points at the lint's vtable static.
///
/// All entries share `PROVIDER_LINT_EVALUATE` as the vtable shape;
/// the per-lint id distinguishes them at host-side dispatch.
#[hilavitkutin_extensions_macros::export_extension]
static MOCKSPACE_BUILTIN_LINTS_DESCRIPTOR: ExtensionDescriptor = ExtensionDescriptor {
    abi_version: HOST_ABI_VERSION,
    extension_version: ExtensionVersion::new(0, 0, 1),
    providers: &[
        ProviderEntry {
            provider_id: PROVIDER_NO_TODO,
            vtable_ptr: &NO_TODO_VTABLE as *const _ as *const core::ffi::c_void,
        },
        // 15 more entries here for the rest of the mockspace built-in pool.
    ],
};
```

The per-lint provider id (`mockspace.lint.no-todo.v1`) is distinct from `PROVIDER_LINT_EVALUATE` (`viola.lint.evaluate.v1`). The vtable shape behind both is the same `LintEvaluateVtable`; the provider id is what the host matches when picking which lint to invoke.

This is a refinement of the parent memo's recommendation. The original framing was "exports `viola.lint.evaluate.v1`"; the actual shape is "exports N per-lint provider ids, all of vtable shape `LintEvaluateVtable`". This matches viola's existing convention where a runner plugin exports `viola.runner.execute_scope.v1` (one provider id, one vtable). A multi-lint cdylib exports N per-lint provider ids, each pointing at a distinct evaluator function, all sharing the `LintEvaluateVtable` shape.

## The host-side dispatch shape

The `ViolaEngine` impl of `mockspace_core::lint::LintEngine` (working name; lives in viola as `viola-mockspace-engine`). Implements `run` by routing every lint through viola's existing plugin host.

```rust
pub struct ViolaEngine {
    host: ExtensionHost,
    // ...
}

impl LintEngine for ViolaEngine {
    const HASH_ALGORITHM: HashAlgorithm = HashAlgorithm::Sha256;
    type Config = ViolaEngineConfig;

    fn run<'a, P: Project + ?Sized>(
        &self,
        project: &'a P,
        config: &'a Self::Config,
        run_surface: RunSurface,
        gate: Gate,
    ) -> Result<Vec<Finding>, LintError> {
        let mut findings = Vec::new();

        // 1. For each document in the project, build a NamPayload.
        for doc in project.documents() {
            let nam = self.build_nam_payload_for(doc);

            // 2. For each configured lint, look up the per-lint provider id
            //    in the loaded plugins and invoke its evaluate fn.
            for (lint_name, lint_severity) in config.iter_active_lints(gate) {
                let provider_id = self.resolve_lint_provider_id(lint_name);
                let Some(handle) = self.host.find_provider(provider_id) else {
                    continue;
                };

                let vtable = unsafe {
                    &*(handle.vtable_ptr() as *const LintEvaluateVtable)
                };

                let cfg_bytes = config.serialize_lint_config(lint_name);
                let mut batch = DiagnosticBatch::empty();

                let status = unsafe {
                    (vtable.evaluate)(
                        self.host_ctx_for(lint_name),
                        &nam as *const NamPayload,
                        cfg_bytes.as_ptr(),
                        arvo::USize(cfg_bytes.len()),
                        &mut batch as *mut DiagnosticBatch,
                    )
                };

                if !matches!(status, AbiStatus::Ok) {
                    return Err(LintError::Plugin {
                        lint_name: lint_name.to_string(),
                        status_code: status as u32,
                    });
                }

                // 3. Translate the DiagnosticBatch into Findings, applying
                //    severity from the engine config and the suppression
                //    map from the substrate.
                for diag in batch.iter() {
                    findings.push(translate_diag_to_finding(
                        diag,
                        lint_name,
                        lint_severity,
                        doc,
                    ));
                }
            }
        }

        // 4. Apply the substrate's SuppressionMap before returning.
        Ok(apply_suppressions(findings, project.suppressions()))
    }
}
```

Three observations on this body.

First, the loop structure is one outer iteration over documents (mockspace's notion) and one inner iteration over configured lints. This matches the in-process `MockspaceEngine` shape; the only difference is the inner dispatch goes through a vtable pointer instead of a `dyn Lint` call.

Second, the `ViolaEngine` owns the `ExtensionHost`. The lint config from `mockspace.toml` (or eventually `viola.toml` per the locked sequence) becomes the `ViolaEngineConfig` that names which provider ids to invoke and at what severity.

Third, the suppression-map application stays at the substrate layer. The cdylib does not know about `// lint:allow(...)` comments; the host filters before returning.

## Open assumption: NamPayload schema

The cdylib's `nam_read_source` and `nam_read_path` helpers assume the NAM v1.x schema includes per-file `source: BytesRef` and `path: BytesRef` accessors. The landed `NamPayload` is the opaque-carrier shape (`viola-plugin-abi/src/nam.rs:34`): `(NamVersion, data: *const c_void, len: arvo::USize)`. The schema behind `data` is deferred to a later minor revision per the file's module doc.

Two paths to close this assumption:

1. **Define a minimal NAM schema for the lint cdylib slice.** Even a single-file schema (one entry: path, language, source bytes) suffices for the first round of ported lints. Lints that need parsed structure (no-duplicate-fn, undocumented-type) come in a later slice with a richer schema.

2. **Sidestep NAM for content-regex lints.** Pass raw source through a sibling provider id (`viola.lint.evaluate-raw.v1`) whose vtable takes `(source_bytes, source_len, path_bytes, path_len)` directly instead of `*const NamPayload`. This avoids speccing NAM at all for content-shaped lints but adds a second vtable shape to the boundary.

The Path A recommendation in the parent memo leans toward option 1: settle a minimal NAM schema in the DOC CL slice, port lints against it, expand the schema as later lint families need richer structure. Option 2 fragments the cdylib protocol; option 1 keeps it singular at the cost of one schema commit.

This sketch leaves the assumption open. The DOC CL slice that follows picks the resolution.

## Other open questions surfaced by the sketch

- **DiagnosticBatch capacity.** The cdylib's `diag_batch_push` returns `false` when the host-provided slot is full. The wire shape today (per `viola-plugin-abi/src/diagnostic.rs`) is a plugin-owned output buffer. Buffer-full handling for a multi-emit lint is a real concern; the in-process `Vec<Finding>` shape doesn't have it. Subsequent slices decide whether the cdylib retries with a fresh batch, errors, or whether the batch grows.

- **Per-lint host_ctx.** The vtable's first arg is `host_ctx: *mut c_void`. Today viola passes its own host context; the sketch leaves the field as `self.host_ctx_for(lint_name)`. Whether mockspace lints need a different host context shape (project state, design-round file paths, etc.) than viola lints is an open question. Defaulting to viola's shape keeps one host_ctx convention.

- **Per-lint provider id naming.** The sketch uses `mockspace.lint.<name>.v1`. Viola uses `viola.lint.evaluate.v1` as a single id. Two viable shapes: (a) one provider id per lint (this sketch's shape); (b) one provider id per pack (the cdylib's lints are dispatched by an internal lint-name parameter passed alongside the call). Shape (b) is closer to viola's current convention; shape (a) lets the host enumerate lints from the descriptor without invoking each. The DOC CL slice settles this.

## What the sketch closes

- The cdylib boundary shape (descriptor, vtable, evaluator) is sufficient to host a content-regex-style lint without changes to the landed boundary contract.
- The host-side `ViolaEngine::run` shape is a direct translation of the in-process `MockspaceEngine::run` with one vtable-pointer dispatch step where the in-process version has a `dyn Lint` call.
- The substrate's `LintEngine` trait does not need changes; `ViolaEngine` is one more impl of it alongside `MockspaceEngine`.
- The substrate's `SuppressionMap` filtering applies host-side after cdylib dispatch; cdylibs are oblivious to suppression.

## What the sketch defers

- The NAM payload schema for lints that read raw source.
- The DiagnosticBatch buffer-full retry contract.
- The per-lint host_ctx convention.
- The per-lint vs per-pack provider id convention.
- The cdylib-load entry point in the host (when does `ViolaEngine` discover plugin paths? `mockspace.toml`? `viola.toml`?).

Each is named explicitly so the DOC CL slice carries them as bullet items the lock criteria address.

## Next step

The DOC CL slice in mockspace specifies:

1. Path A as the directional commitment (with this sketch named as the feasibility artefact).
2. The four open questions above each settled with a concrete choice and rationale.
3. The first-port lint (no-todo) end-to-end naming the cdylib crate location, the vtable shape (per the resolved per-lint vs per-pack question), the NAM schema slot used (per the resolved schema question), and the test harness.

After the DOC CL locks, the SRC CL ports no-todo against the locked spec. Subsequent rounds port the other 15 built-ins and the 17 stack-lints against the same pattern.

## See also

- `mock/research/202605251000_lint-catalog-cdylib-boundary.md` (parent memo).
- `hilavitkutin/mock/research/202605232100_workunit-cdylib-boundary.md` (boundary recommendation).
- `mock/crates/mockspace-rs/src/builtins/content_regex.rs:113` (the in-process `ContentRegexLint` body the cdylib evaluator translates).
- `viola/mock/crates/viola-plugin-abi/src/vtable.rs:102` (the `LintEvaluateVtable` the cdylib targets).
- `viola/mock/crates/viola-plugin-abi/src/nam.rs:34` (the opaque NAM carrier with deferred schema).
