---
date: 2026-05-22
phase: research
scope: mockspace-rs Finding shape, Lint trait, cargo mock check --fix flow
status: design-locked
related:
  - mock/research/202605220000_canonical-directive-vocabulary.md
  - mock/research/202605211200_lint-schema-design.md
---

# Auto-fix domain + structured diagnostics

Mockspace gains a unified auto-fix and structured-diagnostic surface. The `Lint` trait gains an optional `fix(...)` method; the `Finding` shape grows hint/help/suggestion fields; `cargo mock check` gains `--fix` / `--fix-dry-run` flags. The auto-fix domain and the diagnostic-shape work share one machinery: a `Fix` recipe is a kind of structured suggestion, and a suggestion that happens to be unambiguously applicable IS an auto-fix.

This memo records the design as locked. Implementation tasks live as #550 (memo), #551 (extend Finding), #552 (Lint::fix trait method), #553 (--fix flow), #554 (audit existing lints for fix candidates).

## What's wrong with the current Finding shape

The current `Finding` carries: severity, lint name, source location, message. That's enough to say "this is wrong"; it's not enough to say "this is wrong and here's specifically what to do." Three concrete gaps:

- No structured **suggestion**: the message string sometimes contains "did you mean X" as prose, sometimes not. No machine-readable hint that a renderer (or `--fix`) can act on.
- No **help text**: rustc-style "for more information, see <reference>" / "this lint exists because <rule>" is delivered in the message body if at all. Multi-line help splits awkwardly.
- No **fix recipe**: even when the lint *knows* the fix (e.g. "replace `Option<T>` with `Maybe<T>` at this byte range"), there's no way to ship that knowledge through to a runner that would apply it.

The fix is to extend `Finding` with three optional fields and add one new type:

```rust
pub struct Finding {
    pub lint_name: String,
    pub severity: Severity,
    pub location: SourceLocation,
    pub message: String,

    // New:
    pub help: Option<String>,
    pub hint: Option<String>,
    pub suggestion: Option<Suggestion>,
}

pub struct Suggestion {
    pub description: String,
    pub fix: Option<Fix>,         // present iff the suggestion is mechanically applicable
}

pub enum Fix {
    /// Replace bytes [start, end) with `replacement`.
    Replace { start: usize, end: usize, replacement: String },

    /// Insert `text` at `position`. Used for adding directives, imports, etc.
    Insert { position: usize, text: String },

    /// Delete bytes [start, end).
    Delete { start: usize, end: usize },

    /// Multiple edits applied atomically.
    Multi(Vec<Fix>),

    /// File-level operation (create, delete, move).
    File(FileOp),
}
```

## The Lint trait gains an optional fix method

```rust
pub trait Lint: Send + Sync {
    fn name(&self) -> &str;
    fn check_document(&self, ctx: &LintContext, doc: &MockspaceDocument, sink: &dyn FindingSink) -> Result<(), LintError>;

    // New, default None: lint declares it has no auto-fix
    fn fix(&self, _ctx: &LintContext, _doc: &MockspaceDocument, _finding: &Finding) -> Option<Fix> {
        None
    }
}
```

Most lints return `None`; the trivially-fixable ones return a structured `Fix`. The check phase populates `finding.suggestion.fix` from this method when emitting findings; the runner consults `--fix` mode to decide whether to apply.

## Trivially-fixable vs needs-judgment

The architectural distinction: a fix is "trivially-fixable" iff applying it produces a result the author would have written. Two examples on opposite ends:

- **Trivial**: em-dash → period (or comma, etc per writing-style.md). The character substitution is deterministic; no human judgment needed. Auto-fix applies and the result is what the author would have written.
- **Non-trivial**: a bare `Option<T>` in a public API position should become `Maybe<T>`. But maybe the right answer is actually `Just<T>` (hot-path infallible) or `Outcome<T, E>` (if there's an associated error). The lint can suggest `Maybe<T>` as the most common case, but the suggestion is *advice* not *fix*.

The split:
- Returns `Fix::Replace { ..., replacement: "Maybe<T>" }`: lint asserts this is the right answer
- Returns `None` but populates `finding.hint = "consider Maybe<T>, Just<T>, or Outcome<T, E>"`: lint provides advice without committing

`--fix` mode applies the former, skips the latter. `--fix-dry-run` shows what would be applied without writing files. The default `cargo mock check` shows everything but applies nothing.

## Candidates for auto-fix in the existing catalog

Audit of the 21 currently-registered catalog entries (see #554 for the full pass), highest-confidence candidates first:

**Definitely auto-fixable**:
- `writing-style`: em-dash → period, double-hyphen `--` → em-dash candidates (the inverse), trailing whitespace removal, banned hype words → suggested replacements where unambiguous
- `directive-style-consistency` (new from #548): comment ↔ attribute form for languages with both surfaces
- `lint-allow-requires-task-id`: insert `tracked: #NNN` placeholder when missing (requires human to resolve the `#NNN` value, but the structural fix is mechanical)

**Auto-fixable with confidence threshold**:
- `no-bare-numeric` where the context unambiguously suggests one width: bare `u32` in a function returning a domain type already typed as `Uint32` → suggest `Uint32`. Where width is ambiguous, suggest the most common arvo equivalent as a hint, no fix.
- `no-bare-string` where source already imports `Str`: suggest replacement. Where `Str` is not imported, suggest adding the import + replacement as a multi-edit fix.

**Hint/help only, no fix**:
- `no-bare-option`, `no-bare-result`: the right substitute (`Maybe`/`Just`/`Outcome`) depends on call-site semantics the lint can't infer
- `no-public-raw-field`: the right newtype name depends on domain
- `strategy-marker-required`: which strategy (`Hot`/`Warm`/`Cold`/`Precise`) depends on intended use
- `no-vec-in-trait-sig`: the right replacement (`Collector`, `Push`, `BulkPush`) depends on trait semantics
- `trait-first-signatures`: needs human judgment on which trait bound to add

**File-level operations** (less common but well-supported):
- A future "missing-required-tier-file" lint could `Fix::File(FileOp::Create { path, content })` to scaffold a missing `BACKLOG.md.tmpl` or similar

## CLI flow

```bash
cargo mock check                  # default: report only, exit code reflects findings
cargo mock check --fix            # apply all trivially-fixable findings, write files, re-run check to verify
cargo mock check --fix-dry-run    # print the diff that --fix would apply, no writes
cargo mock check --fix=<lints>    # apply fixes only for the named lints
```

Important properties:

- **Atomicity**: all fixes from one `--fix` run are applied in one batch; if any fix conflicts (overlapping byte ranges across two findings), the runner reports a conflict and skips the conflicting fixes, applying the rest.
- **Verification**: after `--fix` applies fixes, the runner re-runs the lint pass to confirm the fixes resolved their findings and did not introduce new violations. Net-new findings post-fix are reported as warnings ("--fix introduced N new findings; review").
- **Backup**: `--fix` writes a unified diff to `.git/mockspace/fix-<timestamp>.diff` before applying, so revert is one `git apply -R`. Optional `--no-backup` to skip.
- **Interactive mode** (future, not v2 Phase 2): `--fix-interactive` walks fixes one at a time with a `y/n/q` prompt. Out of scope for the initial implementation; track as follow-up.

## Renderer integration

A future "human-readable diagnostic renderer" (presumably part of the v2 Phase 3 render pipeline #482) consumes the extended `Finding` shape:

```
error[no-bare-numeric]: bare `u32` in public API position
  --> mock/crates/foo/src/lib.rs:42:18
   |
42 |     pub fn count() -> u32 {
   |                       ^^^ replace with arvo numeric type
   |
   = hint: consider `Uint32`, `USize`, or a domain alias
   = help: arvo is the workspace's exclusive numeric substrate; bare primitives are forbidden in pub API per .claude/rules/no-bare-primitives.md
   = suggestion: pub fn count() -> Uint32 {
                                   ^^^^^^
                 (apply with --fix=no-bare-numeric)
```

The renderer reads `finding.message`, `finding.hint`, `finding.help`, `finding.suggestion.description`, `finding.suggestion.fix`. Each is independently optional; the lint chooses which to populate. The render output adapts gracefully.

For machine consumers (CI, LSP), the same Finding emits as JSON with the same field structure. No information loss.

## Validation pass after --fix

Critical: `--fix` MUST re-run the lint pass and confirm:

1. The findings it intended to fix are now resolved
2. No net-new findings appeared

If (1) fails, the fix is buggy. Emit a structured error pointing to the original finding plus the post-fix state. If (2) holds, report the net-new findings as warnings ("the fix for X surfaced N new violations; review needed"). If both (1) and (2) hold, exit success.

This catches subtle bugs in fix logic early. A fix that produces syntactically valid but semantically wrong code (`Option<T>` → `Maybe<T>` when the call site actually needed `Just<T>`) will sometimes surface as a downstream lint firing post-fix; the verification pass makes the cascade visible instead of hidden.

## Diagnostic identifiers + URI scheme integration

Each lint's name is the diagnostic ID (`no-bare-numeric`, etc). The renderer surfaces this in brackets `error[no-bare-numeric]`. Future enhancement: every diagnostic emits a stable URL/URI for documentation:

```
error[no-bare-numeric]: ...
  = info: mock://@/export/lint-docs/no-bare-numeric
```

The `mock://@/export/lint-docs/<name>` resolves to first-party lint documentation embedded in the mockspace binary. Lint packs can publish their own docs at `mock://ext/<pack>/export/lint-docs/<name>`. This parallels the preset URI scheme settled in the same morning session (preset infrastructure tasks #536-#542; URI form `mock://ext/<pkg>/export/lint-preset/<name>` per §27/§29 of the v2 spec) and uses the same export infrastructure.

Not required for the initial implementation; track as follow-up under the v2 Phase 3 render pipeline (#482).

## Integration with the directive vocabulary work

The directive vocabulary work (#119, #186, #543-#549) and the auto-fix work are independent but composable. Several directive-related findings ship with `Fix` recipes:

- `directive-style-consistency` finding has `Fix::Replace` to swap comment ↔ attribute form
- A future "deprecated-primitive-introductions-table" lint that fires on `[primitive-introductions]` in mockspace.toml ships a `Fix::Multi` recipe that removes the TOML table AND emits `lint:introduces` directives at the right source sites (where unambiguous; otherwise the migration tool from #549 takes over)
- `lint:allow` missing `tracked: #N` ships a `Fix::Insert` recipe that adds the placeholder

The two designs do not depend on each other for landing order; both can ship in parallel.

## Cross-references

- `mock/research/202605220000_canonical-directive-vocabulary.md`: companion memo on the directive surface.
- `mock/research/202605211200_lint-schema-design.md`: the catalog work this extends.
- `mock/crates/mockspace-rs/src/lint.rs`: Lint trait definition (gains `fix` method).
- `mock/crates/mockspace-core/src/lint.rs`: Finding type definition (gains hint/help/suggestion fields).

## Tasks

#550 (this memo as a tracked task), #551 (Finding extension), #552 (Lint::fix trait method), #553 (--fix flow + flags), #554 (audit existing 21 catalog entries for fix candidates).
