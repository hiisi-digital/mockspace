---
date: 2026-05-22
phase: research
scope: per-lint fix-readiness verdict across the 22 registered catalog entries
status: design-locked
related:
  - mock/research/202605220030_auto-fix-and-structured-diagnostics.md
  - mock/crates/mockspace-rs/src/builtins/registry.rs
  - mock/crates/mockspace-rs/src/fix.rs
---

# Auto-fix catalog audit

The auto-fix design memo (`202605220030`) enumerates three confidence tiers for catalog fix-readiness: definitely fixable, fixable with a confidence threshold, hint-only. This memo runs through every registered catalog entry (22 today, counting the `directive-style-consistency` addition since the design memo named 21) and records a per-lint verdict plus the shape the fix takes when one applies.

The verdicts here drive the per-lint `fix()` impl population that #554 ships in waves. The first wave lands the unambiguous cases inline at `check_document` emit time (no `Lint::fix` override needed; the finding carries `suggestion.fix` directly per the design memo's "lints that already populate Finding::suggestion.fix inline during check_document/check_project do not need to override this method" clause). Later waves populate `Lint::fix` for cases where the fix recipe depends on additional context that the emit-time path does not have.

## Verdict format

Each entry below records:

- **Name**: the registered catalog name.
- **Kind**: the primitive (`content-regex`, `ast-node-position`, `suppression-meta`, etc.) the lint runs on.
- **Verdict**: `auto-fixable` / `threshold` / `hint-only` / `n/a`.
- **Recipe**: when `auto-fixable` or `threshold`, the `Fix` shape to emit. When `hint-only`, the prose hint the lint should populate on `finding.hint`.
- **Notes**: blockers, dependencies, or scope deferrals.

## Catalog walkthrough

### 1. `no-alloc`

- Kind: `content-regex`
- Verdict: hint-only
- Reason: the alternative depends on what the consumer's working with. `Vec<T>` may want `&mut impl Collector<T>` or `Map<K, V, N>` or `[T; N]` depending on context. Cannot mechanise.
- Hint: "use a stack-bounded or trait-bound collection: `&mut impl Collector<T>`, `[T; N]`, `Map<K, V, N>`."

### 2. `no-std`

- Kind: `content-regex`
- Verdict: hint-only
- Reason: replacement depends on the std API touched. `std::collections::HashMap` → `Map<K, V, N>`, `std::vec::Vec` → trait bound, `std::format!` → const-string or stack buffer. Domain-shaped.
- Hint: "this crate targets `#![no_std]`; reach into `notko` / `arvo` / `hilavitkutin` primitives instead."

### 3. `no-dyn-dispatch`

- Kind: `content-regex`
- Verdict: hint-only
- Reason: `dyn Trait` removal forces a monomorphisation decision the lint cannot infer. The call site may need to become `impl Trait` (single-type) or generic-bounded (multi-type fan-out) depending on intent.
- Hint: "monomorphise via `impl Trait` or `<T: Trait>`; the substrate ships zero `dyn` boundaries."

### 4. `no-runtime-spawn`

- Kind: `content-regex`
- Verdict: hint-only
- Reason: thread/task spawn replacement is workload-shaped. The right answer is "rewrite as a `WorkUnit` plus scheduler dispatch"; that is a refactor, not a substitution.
- Hint: "model parallel work as a `WorkUnit` and dispatch via the scheduler; do not spawn threads inside the engine."

### 5. `no-runtime-registration`

- Kind: `content-regex`
- Verdict: hint-only
- Reason: `inventory::submit!` / `linkme::distributed_slice` removal requires moving registration to compile-time via the catalog or builder pattern. Mechanical substitution would produce broken code.
- Hint: "register at compile time via the catalog (`CatalogEntry`) or `SchedulerBuilder::with`; do not use runtime registration sinks."

### 6. `no-bare-numeric`

- Kind: `ast-type-position`
- Verdict: threshold (deferred)
- Reason: bare `u32` → `Uint32` is the common case but width may want `USize` (if the value is a count/index), `Cap<N>` (if bounded), or a domain alias. Auto-fix only when source already imports a specific arvo numeric and the surrounding context (e.g. consts named `MAX_FOO`) suggests the right one.
- Recipe (when applicable): `Fix::Replace { start, end, replacement: "Uint32" }` where `start`/`end` cover the bare ident.
- Notes: defer to a later wave that adds a heuristics layer reading nearby imports + ident names. Hint applies in the meantime.
- Hint: "use an arvo numeric: `Uint32`, `Uint8`, `USize`, `Cap<N>`, or a domain alias."

### 7. `no-bare-string`

- Kind: `ast-type-position`
- Verdict: threshold (deferred)
- Reason: `String` → `Str` works when source already imports `hilavitkutin_str::Str`; without that import, the fix needs `Fix::Multi { fixes: [Fix::Insert { use_decl }, Fix::Replace { String → Str }] }`. The import-detection logic is non-trivial; deferred.
- Recipe (when applicable): `Fix::Multi { [Fix::Insert { position: after_use_block, text: "use hilavitkutin_str::Str;\n" }, Fix::Replace { start, end, replacement: "Str" }] }`.
- Hint: "use `hilavitkutin_str::Str` for owned strings; `&'static str` for compile-time string literals."

### 8. `no-bare-option`

- Kind: `ast-type-position`
- Verdict: hint-only
- Reason: `Option<T>` → `Maybe<T>` is the common but not universal substitute. Hot-path-infallible call sites want `Just<T>`; error-bearing call sites want `Outcome<T, E>`. The lint cannot infer call-site intent.
- Hint: "use `notko::Maybe<T>` (common), `notko::Just<T>` (hot-path infallible), or `notko::Outcome<T, E>` (error-bearing)."

### 9. `no-bare-result`

- Kind: `ast-type-position`
- Verdict: hint-only
- Reason: parallel to `no-bare-option`. `Result<T, E>` → `Outcome<T, E>` is the right answer for fallible-API call sites; `Just<T>` is right when the error path is unreachable. Lint cannot infer.
- Hint: "use `notko::Outcome<T, E>` for fallible APIs; `notko::Just<T>` when the error path is statically unreachable."

### 10. `no-public-raw-field`

- Kind: `ast-node-position`
- Verdict: hint-only
- Reason: the right newtype name is domain-specific. `pub width: u32` may want `pub width: Width(Uint32)` or `pub width: USize` or a fully-named domain alias. No mechanical answer.
- Hint: "wrap public struct fields in a domain newtype that names the invariant being carried."

### 11. `no-vec-in-trait-sig`

- Kind: `ast-node-position`
- Verdict: hint-only
- Reason: `Vec<T>` in a trait signature wants `&[T]`, `&mut impl Collector<T>`, `impl IntoIterator<Item = T>`, or `impl Push<T>` depending on owned-vs-borrowed-vs-emit semantics the trait author intended. Lint cannot infer.
- Hint: "trait signatures take trait bounds, not owned collections: `impl IntoIterator<Item = T>`, `&mut impl Collector<T>`, or `&[T]`."

### 12. `strategy-marker-required`

- Kind: `ast-node-position`
- Verdict: hint-only
- Reason: which strategy (`Hot` / `Warm` / `Cold` / `Precise`) is the right default depends on the type's intended use. Auto-defaulting to one would silently bias the API.
- Hint: "add an explicit `S: Strategy` parameter (default `Hot` for development, `Cold` for storage-heavy, `Precise` for overflow-sensitive)."

### 13. `trait-first-signatures`

- Kind: `ast-node-position`
- Verdict: hint-only
- Reason: the right trait bound depends on the function's semantics. Cannot mechanise.
- Hint: "name a trait bound (`impl IntoIterator<Item = T>`, `impl ByteSink`, etc.) rather than a concrete container."

### 14. `writing-style`

- Kind: `content-regex`
- Verdict: **auto-fixable** (em-dash case in Wave 1; `leverage`/`utilize` are Wave 1.5 candidates)
- Reason: the em-dash → period substitution is unambiguous per the workspace's writing-style rule. Most marketing-word and filler patterns are advisory (the replacement depends on what the writer meant), but two marketing words have unambiguous one-word substitutes per `vocabulary.md`: `leverage` → `use` and `utilize` → `use`. The current `content-regex` schema cannot express a per-alternation `replace_with` because the `[[patterns]]` table groups the marketing alternations into one regex; promoting these two words to their own `[[patterns]]` entries with `replace_with = "use"` would land them in a follow-up wave without schema change.
- Recipe (em-dash pattern, Wave 1): `Fix::Replace { start: match.start, end: match.end, replacement: "." }`.
- Recipe (`leverage` / `utilize`, Wave 1.5): split each from the current marketing-word alternation into its own `[[patterns]]` entry with `replace_with = "use"`.
- **First fix-impl wave lands the em-dash case.**

### 15. `lint-allow-requires-task-id`

- Kind: `suppression-meta`
- Verdict: threshold (deferred to second wave)
- Reason: inserting a `tracked: #?` placeholder when missing is structurally mechanical (the `#?` value still needs a human, but the keyword + placeholder lands deterministically). Requires the suppression scope's byte-range, which is currently surfaced as a line/column span; promoting to byte offsets is a small but tracked extension.
- Recipe (planned): `Fix::Insert { position: directive_end_byte, text: " tracked: #?" }` placed before the closing `)` or after the reason argument.
- Notes: requires `SuppressionScope` to carry byte offsets through the directive extraction path. Tracked separately as a follow-up.

### 16. `directive-style-consistency`

- Kind: bespoke (PR #59)
- Verdict: threshold (deferred to second wave)
- Reason: swapping comment ↔ attribute form for languages that support both is mechanical (the directive content is identical, only the surrounding syntax differs). Recipe needs each language's attribute-form template hardcoded; ships once the AST-form parser side of #545 lands.
- Recipe (planned): `Fix::Replace { start: directive_start, end: directive_end, replacement: <language-specific attribute form> }`.
- Notes: blocks on #545 (language-extension trait + Rust attribute alias parser). Will land alongside that.

### 17. `no-bare-vec`

- Kind: bespoke
- Verdict: hint-only
- Reason: parallel to `no-vec-in-trait-sig`. The replacement is context-shaped.
- Hint: "use a stack-bounded collection or trait bound: `Map<K, V, N>`, `&mut impl Collector<T>`, `&[T]`."

### 18. `no-manual-id`

- Kind: bespoke
- Verdict: hint-only
- Reason: replacing a hand-rolled ID counter with a typed `Id<T>` newtype requires choosing the underlying width and tagged type, both domain-shaped.
- Hint: "use the workspace's `Id<T>` / `NodeId` / `UnitId` / `SlotId` family rather than a hand-rolled `usize` counter."

### 19. `no-manual-impl`

- Kind: bespoke
- Verdict: hint-only
- Reason: removing a hand-rolled impl of a substrate trait (e.g. open-coded `Iterator::next`) requires understanding the iterator's underlying state machine. Refactor, not substitution.
- Hint: "use the substrate's pre-built `Iterator` / `FromIterator` / `IntoIterator` adapter (see `notko::iter`)."

### 20. `no-adhoc-framework`

- Kind: bespoke
- Verdict: hint-only
- Reason: replacing an ad-hoc framework pattern (manual `dyn` table, hand-rolled scheduler) with the workspace's primitives is a refactor.
- Hint: "use `hilavitkutin`'s scheduler + WorkUnit + Resource / Column model; do not roll your own dispatch."

### 21. `registrable-completeness`

- Kind: bespoke
- Verdict: hint-only
- Reason: a registration missing a required field (per-extension-point shape) cannot be filled in mechanically; the consumer must decide the value.
- Hint: "complete the registration with the named missing field; consult the extension-point doc."

### 22. `deprecation-comparison`

- Kind: bespoke
- Verdict: hint-only
- Reason: design-CL lint; the fix is editing the deprecated-CL file by hand to track what the active CL added or dropped. Process work, not source-code mechanisation.
- Hint: "edit the deprecated CL to record the diff against the active CL."

## Summary

| Verdict | Count | Lints |
|---|---|---|
| auto-fixable | 1 | `writing-style` (em-dash only) |
| threshold (deferred) | 4 | `no-bare-numeric`, `no-bare-string`, `lint-allow-requires-task-id`, `directive-style-consistency` |
| hint-only | 17 | all others |

The first fix-impl wave (PR following this memo) lands the `writing-style` em-dash recipe via a new `replace_with: Option<String>` field on `ContentPattern`. Threshold-tier lints land in subsequent waves as their prerequisite plumbing arrives (byte-offset spans for suppression scopes, language-extension trait for directive form-swap, import-detection heuristics for the bare-type substitutes).

Hint-only entries populate `Finding::hint` with the prose from the verdict; no `Fix` is produced. That work is mechanical and lands as a sweep across the catalog independently of the fix recipes; tracked as a follow-up so this audit's load-bearing claim (which lints can ship a fix) stays separable from the prose-quality improvements (which all 22 entries get).

## Wave plan

- **Wave 1** (#554, this branch): research memo + `writing-style` em-dash fix via `ContentPattern::replace_with`. Lands the pattern that #560's CLI consumes.
- **Wave 2** (follow-up): suppression-scope byte-offset surface + `lint-allow-requires-task-id` fix recipe via `Lint::fix` override.
- **Wave 3** (post #545): `directive-style-consistency` form-swap recipe alongside the attribute-alias parser.
- **Wave 4** (post import-detection): `no-bare-string` and `no-bare-numeric` threshold fixes for the unambiguous import cases.

## Cross-references

- `mock/research/202605220030_auto-fix-and-structured-diagnostics.md`: the parent design memo this audit executes.
- `mock/crates/mockspace-rs/src/fix.rs`: the planner/applier the fixes feed into.
- `mock/crates/mockspace-rs/src/builtins/registry.rs`: the catalog this audit walks.
- `mock/crates/mockspace-rs/src/builtins/content_regex.rs`: the primitive Wave 1 extends.
