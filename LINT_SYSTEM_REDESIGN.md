# Lint & Agent System Redesign

**Status:** in progress (Phase 2 partially implemented, Phases 6-8 proposed)
**Context:** discovered while integrating mockspace into the saalis project

## Problem

The lint and agent systems have issues that block adoption by
projects other than loimu:

1. Many lints hardcode loimu-specific macros as the "correct" pattern
2. Lint severity was not configurable per-project (now fixed)
3. Agent hooks are copied per-project instead of shipping as builtins
4. Settings JSON is hand-written instead of auto-generated
5. Multiple lints solve the same problem (forbidden type/import) but
   each is a separate hardcoded lint instead of one configurable one
6. No scoped lint configuration (different rules per crate)

## Already Implemented (this session)

### Lint configurability
- `[lints]` section in mockspace.toml with per-gate severity
- `[lints.lint-name]` sub-tables with severity + parameters
- `[lints.lint-name.findings]` per-finding-kind severity
- `LintConfig` struct, `default_severity()` on Lint trait
- `finding_kind` on LintError for granular reporting
- `configure()` method for lint parameterization
- 14 loimu-specific lints default to OFF
- Consumer custom lints via mock/lints/ directory (JIT compiled)

### Bug fixes
- render_agent.rs Unix-only import guarded with #[cfg(unix)]
- bootstrap.rs Windows path + module name validation
- entry.rs process::exit replaced with return ExitCode
- config.rs parse_string_array prefix match fixed
- Severity override preserves per-violation nuance

## Proposed: Phase 6 — Builtin Agent Hooks

These hooks are universal workflow guards. Every mockspace consumer
needs them. They should ship with the mockspace crate, not be
copied into each project's `agent/hooks/`:

### `check-byline.sh`
Enforce commit authorship policy. Block Co-Authored-By in assistant
mode, require it in autonomous mode. Controlled by
`{PROJECT}_AGENT_MODE` env var (auto-derived from `project_name`).

### `mockspace-write-guard.sh`
Phase gate enforcement. Block doc/source edits outside the correct
design round phase. This is the agent-side mirror of the changelist
lints.

### `mockspace-reminder.sh`
Print mockspace rules reminder before tool use on mockspace paths.
Non-blocking context injection.

### `no-yagni-guard.sh`
Flag YAGNI-flavored reasoning in commit messages. Cultural guard.
Configurable: `mockspace.toml` could have `enforce_no_yagni = true`
(default true).

### Implementation
mockspace generates these hooks automatically alongside consumer
hooks. They use `{{PROJECT_NAME}}` and `{{AGENT_MODE_VAR}}`
template variables derived from `mockspace.toml`. No consumer
template needed — mockspace writes them directly to
`.claude/hooks/` and `.github/hooks/`.

Consumer `agent/hooks/*.sh.tmpl` files are ADDITIONAL hooks that
get merged after the builtins.

## Proposed: Phase 7 — Auto-generated Settings

### Problem
`agent/settings/claude.json` and `copilot-hooks.json` are hand-written.
When hooks are added/removed, settings must be manually updated. The
hook-to-tool-matcher wiring is error-prone.

### Solution
mockspace auto-generates settings from discovered hooks. Each hook
template declares its matchers via frontmatter:

```bash
#!/usr/bin/env bash
# @matchers: Write, Edit
# @timeout: 5
```

Or via naming convention:
- `*-guard.sh.tmpl` → matchers: Bash, Write, Edit (blocking)
- `*-reminder.sh.tmpl` → matchers: Bash (context only)
- `check-*.sh.tmpl` → matchers: Bash (commit checks)

mockspace reads these, generates `claude.json` and
`copilot-hooks.json` automatically. No hand-written settings files.

The `agent/settings/` directory becomes optional overrides only.
If `claude.json` exists in `agent/settings/`, it's MERGED with the
auto-generated config (user additions take precedence).

## Proposed: Phase 8 — Unified `forbidden-imports` Lint

### Problem
Multiple lints solve the same problem:
- `no-float` — forbids f32/f64
- `no-bare-vec` — forbids Vec
- `no-box` — forbids Box
- `no-bare-string` — forbids string literals
- `no-bare-pub` — forbids bare pub
- `no-bare-result` — forbids bare Result
- Custom `no-string-in-sdk` — forbids String type
- Custom `no-dyn-trait` — forbids dyn keyword
- Custom `sdk-boundary` — forbids cross-layer deps

These are all "forbidden X in scope Y" with different X and Y.

### Solution
One builtin lint: `forbidden-imports`. Fully data-driven from
`mockspace.toml`. Each rule specifies a scope (crate glob), a
forbidden pattern (type/module/keyword), and a reason.

```toml
[lints.forbidden-imports]
commit = "warn"
build = "error"
push = "error"

[[lints.forbidden-imports.rules]]
scope = "{prefix}-primitives"
forbidden = ["std::*", "alloc::*"]
reason = "primitives is #![no_std] with zero deps"

[[lints.forbidden-imports.rules]]
scope = "{prefix}-sdk"
forbidden = ["std::*"]
reason = "SDK is #![no_std] + alloc"

[[lints.forbidden-imports.rules]]
scope = "{prefix}-sdk"
forbidden = ["String"]
reason = "use Str (Cow<'static, str>) instead"

[[lints.forbidden-imports.rules]]
scope = "*"
forbidden = ["dyn *"]
reason = "use repr(C) descriptors or monomorphization"

[[lints.forbidden-imports.rules]]
scope = "{prefix}-connector-*"
forbidden = ["{prefix}_core::*", "{prefix}_scheduler::*"]
reason = "extensions depend on {prefix}-sdk only"

[[lints.forbidden-imports.rules]]
scope = "*"
forbidden = ["f32", "f64"]
reason = "use arvo fixed-point types"
enabled = false  # off until arvo is integrated
```

`{prefix}` expands to `crate_prefix` from mockspace.toml.

Scope uses glob matching against crate directory names.
Forbidden patterns match against:
- Type annotations (tree-sitter type nodes)
- Use statements (tree-sitter use_declaration nodes)
- Module paths in qualified types

This replaces: `no-float`, `no-bare-vec`, `no-box`, `no-bare-result`,
`no-bare-pub` (partially), and all the custom per-project forbidden-type
lints. Each becomes a config entry instead of a separate lint implementation.

The existing individual lints remain for backwards compat but are
deprecated in favor of `forbidden-imports` rules.

### no-std as a special case

`#![no_std]` enforcement is common enough to warrant first-class
support. The `forbidden-imports` rule with `scope` + `forbidden = ["std::*"]`
handles it, but mockspace could also auto-detect `#![no_std]` in
a crate's lib.rs and automatically forbid `std::` imports without
explicit config.

## Proposed: Phase 9 — Builtin Agent Templates

### Problem
Rules, skills, preamble, postamble, and workflow docs are copied
per project and manually adapted. All three consumers (loimu,
polka-dots, saalis) have near-identical versions with only
project-specific names changed.

### Solution
mockspace ships builtin versions of universal agent content.
Consumer templates are ADDITIONAL, merged after builtins.

**Builtin rules** (generated by mockspace, no consumer template needed):
- `generated-agent-rules.md.tmpl` — STOP guard on .claude/.github
- `generated-docs.md.tmpl` — STOP guard on docs/
- `generated.md.tmpl` — reminder that agent files are auto-generated
- `mock-workspace.md.tmpl` — 3 doc layers, design rounds, validation
- `readmes.md.tmpl` — per-crate README template rules

**Builtin skills** (generated by mockspace):
- `design-round` — design round workflow
- `mockup-workflow` — step-by-step mockspace workflow
- `real-code-guard` — prevents editing real crates in design phase

**Builtin PREAMBLE/POSTAMBLE** (generated with `{{project_name}}`):
The 6 core mockspace rules are universal. mockspace generates them.
Consumer `PREAMBLE.md.tmpl` is merged AFTER the builtin (for
project-specific additions like loimu's string rule or saalis's
YAGNI rule).

**Builtin WORKFLOW section**:
The design round phases, naming conventions, and `cargo mock`
commands are mockspace's domain. mockspace generates this section.
Consumer `WORKFLOW.md.tmpl` provides project-specific workflow
additions (e.g., code conventions, branch workflow).

### What stays consumer-only
- `MAIN.md.tmpl` — project identity, architecture rules
- `DESIGN.md.tmpl` — project architecture
- `PRINCIPLES.md.tmpl` — project principles
- `implementation.md.tmpl` — project code rules
- `cargo.md.tmpl` — project dep rules
- `writing-style.md.tmpl` — project style with examples
- `commit-style` skill — project commit examples
- `lint-rules` skill — project lint names
- Project-specific skills and hooks

## Proposed: Phase 10 — Replace Hand-rolled Parser with `toml` Crate

### Problem
The hand-rolled TOML parser in config.rs doesn't handle:
- Inline arrays of tables (`rules = [{ ... }, { ... }]`)
- Multi-line inline tables
- Prefix key matching bugs
- Complex nested sections

### Solution
Add `toml` and `serde` (with derive) as dependencies.
Define `RawConfig` as a serde-deserializable struct.
Replace all `parse_*` functions with `toml::from_str`.

This enables the clean forbidden-imports format:
```toml
rules = [
    { scope = "*", forbidden = "dyn *", reason = "use repr(C)" },
]
```

## Implementation Order

1. ✅ Lint configurability (done)
2. ✅ Bug fixes (done)
3. ✅ Consumer custom lints (done)
4. ✅ Builtin agent hooks (done — Phase 6)
5. ✅ Auto-generated settings (done — Phase 7)
6. ✅ Unified forbidden-imports lint (done — Phase 8)
7. Replace parser with toml crate (Phase 10)
8. Builtin agent templates (Phase 9)
9. Deprecate individual forbidden-type lints

## Code Review Findings (remaining)

### Minor (from review, not yet fixed)
- `render_design.rs` — `now_rfc3339` uses `date` command, Windows
- `bootstrap.rs` — `is_active` fragile string contains
- `no_bare_pub.rs` — brace-depth miscounts in strings/comments
- `design_doc_source_mismatch.rs` — substring match false negatives
- `render_design.rs` — unwrap() on fs::read_dir in generation
