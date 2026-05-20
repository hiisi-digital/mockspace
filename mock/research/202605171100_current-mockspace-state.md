# Current mockspace state — pre-redesign snapshot

**Status:** research / current-state inventory
**Authored:** 2026-05-17
**Companion to:** [202605171033_ref-based-mockspace-redesign.md](./202605171033_ref-based-mockspace-redesign.md)

> Captures mockspace as it stands today, before the ref-based redesign. The
> purpose: explicit inventory of what mockspace currently does, which patterns
> are load-bearing and worth preserving, which are tied to the
> `mock/`-in-`refs/heads/*` assumption and need replacement, and where pending
> design tasks map into the redesign.
>
> Sourced from Explore-agent investigation 2026-05-17 plus context from
> pending tasks tracked in the clause-dev workspace session.

## Table of contents

1. [Tool architecture today](#tool-architecture-today)
2. [State machine and lifecycle](#state-machine-and-lifecycle)
3. [Render pipeline](#render-pipeline)
4. [Hook system](#hook-system)
5. [Lint engine](#lint-engine)
6. [No index, no registry: filesystem scanning is the truth](#no-index-no-registry-filesystem-scanning-is-the-truth)
7. [Consumer adoption inventory](#consumer-adoption-inventory)
8. [Patterns to preserve in the redesign](#patterns-to-preserve-in-the-redesign)
9. [Patterns that need replacement](#patterns-that-need-replacement)
10. [Pending mockspace tasks: context map](#pending-mockspace-tasks-context-map)
11. [Migration risk inventory](#migration-risk-inventory)

## Tool architecture today

Mockspace is a multi-crate Rust workspace at `~/Dev/clause-dev/mockspace/`.

**Core crates:**

- **`mockspace`** (root, library + binary) — entry point `src/main.rs`,
  implements the `cargo mock` CLI. Exports modules:
  - `bootstrap` — proxy crate + hook generation, `bootstrap_from_buildscript()`, `activate()`, `deactivate()`.
  - `design_round` — `lock`, `unlock`, `deprecate`, `close` subcommands.
  - `config` — `mockspace.toml` parsing.
  - `lint` — lint execution pipeline.
  - `render`, `render_agent`, `render_design`, `render_md` — template rendering for docs, agent files, dep graphs.
- **`mockspace-lint-rules`** — AST-based lints via tree-sitter (Rust grammar). 30+ lints across categories: changelist immutability, gate enforcement, design-doc-vs-source matching, deprecation chain integrity, file size, etc.
- **`bench-core`**, **`bench-harness`**, **`bench-macro`**, **`benches`** — v2 bench harness ported from polka-dots (tasks #269-#280).

**Extracted library crates** (PR #18, commit `f3fd2eb`, task #444):

- **`mockspace-config`** at `mock/crates/mockspace-config/` — serde-backed parser for `mockspace.toml`. Exports `Config` + `IntoMockspaceConfig` bridge. **Already language-agnostic.**
- **`mockspace-template`** at `mock/crates/mockspace-template/` — minijinja-based template rendering engine for agent file generation. **Already language-agnostic.**

These two extracted libs are the substrate that homma already consumes
(see workspace tasks #445-#447). They survive the redesign without
modification.

**No standalone `mockspace` binary today.** The tool is invoked as
`cargo mock` via a cargo alias installed by the bootstrap. The redesign's
language-agnostic `mock` standalone binary doesn't exist yet.

## State machine and lifecycle

Five-phase design-round workflow today:

```
TOPIC → DOC → DRAFT → IMPL → CLOSED
```

The `Phase` enum (in `mockspace_lint_rules::changelist_helpers`) labels:

| Internal name | Display label | Trigger condition |
|---|---|---|
| `Topic` | TOPIC | Only topic files exist in the round dir |
| `Doc` | DOC | An unlocked doc CL exists |
| `SrcPlan` | DRAFT | Doc CL is locked, no src CL yet |
| `Src` | IMPL | Both doc CL locked AND unlocked src CL exists |
| `Done` | CLOSED | Both CLs locked |

**On-disk encoding via filename suffix:**

- Topic files: `<YYYYMMDDHHMM>_topic.<name>.md`
- Changelist files: `<YYYYMMDDHHMM>_changelist.{doc|src}.{md|lock.md|deprecated.md}`
  - `.md` = active (editable)
  - `.lock.md` = locked (frozen forever)
  - `.deprecated.md` = superseded by newer CL; stays as audit trail

**Phase detection is filesystem-driven.** `Phase::current_phase()` lists files in `mock/design_rounds/` and matches against the filename pattern.

**State-changing operations:**

- `cargo mock lock` — rename active CL to `.lock.md`; phase advances.
- `cargo mock unlock` — destructive: reverts source-side state, marks src CL as deprecated, unlocks doc CL. Used when a round needs reworking after locking.
- `cargo mock deprecate` — mark an unlocked CL as `.deprecated.md`, creating a chain that a successor CL replaces.
- `cargo mock close` — archive a completed CLOSED-phase round into a dated subdirectory.

No central state file. State is entirely emergent from filenames.

## Render pipeline

Three rendering destinations today:

### 1. Repo-root `docs/` tree

- `docs/DESIGN.md` ← `mock/DESIGN.md.tmpl`
- `docs/PRINCIPLES.md` ← `mock/PRINCIPLES.md.tmpl`
- `docs/WORKFLOW.md` ← `mock/WORKFLOW.md.tmpl`
- `docs/STRUCTURE.md` + `.dot` graph file ← parsed from crate source via `render` module

These render at every `cargo mock` invocation that touches relevant inputs. Output committed to `refs/heads/dev`.

### 2. Per-crate `crates/<crate>/README.md`

Crate-level READMEs render from `mock/crates/<crate>/README.md.tmpl`. Per consumer convention these ship to crates.io via `cargo publish`.

### 3. Agent integration files

Two parallel sets of agent files generated from `mock/agent/` templates:

**`.claude/` (Claude Code):**
- `.claude/CLAUDE.md` — top-level agent instructions
- `.claude/rules/*.md` — per-rule files (rule-per-file with `description` + `paths` front-matter for scoped activation)
- `.claude/skills/*/SKILL.md` — invocable skills
- `.claude/hooks/*.sh` — PreToolUse, pre-commit, pre-push hook scripts
- `.claude/settings.json` — merged from templates plus per-repo overrides

**`.github/` (GitHub Copilot):**
- `.github/copilot-instructions.md` — Claude analog
- `.github/instructions/*.instructions.md` — rule analogs
- `.github/skills/*/SKILL.md` — same skill content, different platform
- `.github/hooks/*.sh` — Copilot hook equivalents

Both platforms render from the same source-of-truth templates in `mock/agent/`. The `render_agent` module handles the platform-specific output shape.

## Hook system

**Generation flow (entered via `bootstrap_from_buildscript()`):**

1. Consumer's `build.rs` calls `mockspace::bootstrap::bootstrap_from_buildscript()`.
2. Mockspace captures its own `CARGO_MANIFEST_DIR` at compile time. This is how the proxy crate finds the mockspace source.
3. A proxy crate is generated at `target/mockspace-proxy/Cargo.toml` (git-ignored). This proxy contains a machine-specific path to mockspace.
4. Hooks are generated at `<mock_dir>/target/hooks/` (also git-ignored). The hook scripts source the user's `.git/hooks/*` (if any) first, then run mockspace validation.
5. Activation is explicit: `git config core.hooksPath <mock_dir>/target/hooks/`. The bootstrap function in newer mockspace also auto-activates (per task #229, completed).

**Hook names** are defined as `HOOK_NAMES` constant in bootstrap: pre-commit, pre-push.

**Custom lint loading** uses two mechanisms:

1. In-tree lint files under `<mock_dir>/lints/*.rs` (each file exports `pub fn lint()`).
2. External lint-pack crates declared in `[lint-crates]` in `mockspace.toml`, exposing `pub fn lints()` + `pub fn cross_lints()`.

The generated proxy crate pulls these in as normal Cargo dependencies via dylib loading.

## Lint engine

**AST-based via tree-sitter** with the Rust grammar pinned in.

**Lint architecture:**

- `Lint` trait: per-file lints, single-pass AST walk.
- `CrossCrateLint` trait: lints that span multiple crates' ASTs in one pass.
- Each lint carries metadata: name, description, default severity.

**Severity gate model:**

Each lint has three configurable severity levels in `mockspace.toml`:

```toml
[lints.no-bare-numeric]
commit = "error"     # blocks pre-commit
build = "warn"       # warns on cargo build / check
push = "error"       # blocks pre-push
```

Gate downgrades are workspace-level human-overseer-only (rule in `~/Dev/clause-dev/.claude/rules/mockspace-toml-edits.md`).

**Lint families today (workspace-relevant subset):**

- Bare-primitive bans: `no-bare-numeric`, `no-bare-string`, `no-bare-option`, `no-bare-result`.
- API discipline: `no-vec-in-trait-sig`, `no-public-raw-field`, `strategy-marker-required`.
- Heap discipline: `no-alloc`, `no-std`, `no-dyn-dispatch`, `no-runtime-spawn`, `no-runtime-registration`.
- Workspace discipline: `arvo-types-only` (semantic-alias nudge), `trait-first-signatures`.
- Mockspace internal: changelist-immutability, gate enforcement, design-doc-vs-source mismatch detection.

## No index, no registry: filesystem scanning is the truth

This deserves its own section because it's the single biggest mismatch with the redesign.

**There is no central state file.** No `state.toml`, no `rounds.json`, no
registry of rounds. Every query about workflow state walks the filesystem:

- "What rounds exist?" → list subdirectories of `mock/design_rounds/`.
- "What's the current phase?" → `Phase::current_phase()` matches filename patterns in the round directory.
- "Is this round closed?" → check for the absence of an active CL plus existence of locked CLs in the dated archive.
- "Is this CL deprecated?" → filename ends in `.deprecated.md`.

This works because filenames are deterministic and a single round directory has a small, bounded set of files. It does NOT scale to multi-round queries ("which rounds are still active?", "what's the global state?") without scanning every directory.

The redesign's `refs/mock/index` ref is the explicit answer to this. The index ref is the registry that's currently emergent; with refs, registry becomes load-bearing.

## Consumer adoption inventory

Spot-check of design-round adoption across consumer repos (as of 2026-05-17):

| Repo | Closed rounds | Notes |
|---|---|---|
| `arvo` | 61 | Most rounds; extensive design history through substrate const-trait redesign |
| `hilavitkutin` | 82 | Deepest iteration; engine megaround, kit-trait split, multi-pass design |
| `vehje` | 27 | Newer, less history (clause→vehje rename was recent) |
| `notko` | (low; foundation primitives, fewer rounds needed) | |
| `viola` | (mid; recent Rust port, design rounds via #359 etc.) | |
| `homma` | 0 today | New repo; mockspace consumer via lib crates (#445); no design rounds yet |

The migration burden scales with closed-round count. Arvo and hilavitkutin will be the heaviest migrations. Notko and homma are lightest.

**Consumer `mockspace.toml` shape today** (every consumer):

```toml
[lints.<name>]
commit = "<sev>"
build = "<sev>"
push = "<sev>"

[lint-crates]
# external lint pack references

[domain.<name>]
# domain-specific macro definitions for the render layer

[crates.<name>]
color = "<hex>"
layer = "<int>"
# per-crate metadata for STRUCTURE.md dep graph rendering
```

Per-consumer divergence is mostly in lint severity tuning and domain-specific configuration. The schema itself is uniform.

## Patterns to preserve in the redesign

The Explore-agent investigation flagged six load-bearing patterns. All survive the redesign:

### 1. Lint engine abstraction

Tree-sitter-based AST walks with per-lint severity gates. The `Lint` and `CrossCrateLint` traits are generic and not Rust-specific (the tree-sitter grammar IS Rust-specific, but the engine itself isn't). The redesign keeps the lint engine in mockspace core; the Rust grammar (and Rust-specific lints) factor to `mockspace-rs`.

### 2. Extracted lib crates (mockspace-config + mockspace-template)

Already language-agnostic. Already consumed by homma. Survive as-is. The redesign's mockspace core builds on top of these.

### 3. Marker-file-as-state idiom (the mechanic, not the specific implementation)

The filename-suffix convention (`.md`, `.lock.md`, `.deprecated.md`) as
state encoding is a useful pattern: state changes are `git mv` operations,
producing a single atomic commit per transition. Bash scripts can check
state without parsing TOML.

The redesign keeps this idiom but applies it differently:

- Round phase marker: `phase.<phase>` file at round ref root.
- Task state marker: `state.<state>` file at task ref root.

**The specific 5-phase TOPIC → DOC → DRAFT → IMPL → CLOSED model is NOT
preserved.** TBPED replaces it with the 6-phase TOPIC → PLAN(DOC) →
APPLY(DOC) → PLAN(SRC) → APPLY(SRC) → DONE model with `plan/apply/finish`
verbs. See "Patterns that need replacement" for details. The current
lock/unlock semantics specifically are the bad-DX pain (BUG_LOCK_SEMANTICS,
#227) that TBPED is correcting.

### 4. Hook platform abstraction

Separating Claude and Copilot hook generation with platform-specific helpers is clean. The scope-checking logic in PR #10's Phase 2 (homma's aggregation) demonstrated this pattern works at workspace level too. Survives the redesign; rendering targets move (hooks land in local-only render destination, not committed to `refs/heads/*`).

### 5. Bootstrap idempotence

The `ensure_generated_hooks()` pattern with version-bumping and stale-check survives. The redesign replaces "bootstrap proxy crate" with "fetch harness ref + worktree it", which has the same idempotence properties.

### 6. Config-driven lint severity

Per-lint commit/build/push gates allow consumers to tune without modifying the lint pack. Workspace-level rule keeps severity downgrades human-overseer-only. Survives without change.

## Patterns that need replacement

Five patterns are tied to the current "`mock/`-in-`refs/heads/*`" assumption and need explicit redesign treatment:

### 1. `mock/` directory layout assumption

All paths in the codebase assume `mock/` exists as a tracked subdirectory of the parent worktree. Bootstrap path computation, design-round directory walking, lint context construction, config search, render-target resolution, all assume this.

Migration: rewrite path computation to work against `.mock/` (the harness worktree) plus nested worktrees for round/task/research refs. The agnostic core uses worktree-relative paths from `.mock/`.

### 2. Hardcoded Rust toolchain integration

Several pieces are Rust-only by design today:

- Tree-sitter parser pinned to `tree-sitter-rust`.
- `cargo mock` alias generation.
- `.cargo/config.toml` modification.
- Proc-macro crate concept (Rust-specific lint configuration).
- `.rs` file extension assumption in custom-lint loader.
- `Cargo.toml` dependency parsing.

Migration: factor to `mockspace-rs`. The agnostic core has plugin points where Rust support attaches. Other-language adapters (`mockspace-ts` future) plug into the same points.

### 3. Proxy crate generation mechanism

`target/mockspace-proxy/Cargo.toml` injected at bootstrap is the Rust-specific mechanism resolving mockspace's source location for proxy compilation. Language-agnostic redesign needs a different registration:

Options: env var pointing at mockspace install, config file in `.mock/`, symlink. The simplest: mockspace is a standalone binary installed once on `PATH`. Bootstrap then doesn't need to find mockspace's source; it just executes the binary.

The `mockspace-rs` crate retains the proxy mechanism as a Rust-cargo convenience for build-time invocation. Standalone `mockspace` binary is the primary entry point for any other context.

### 4. Git hook activation via `core.hooksPath`

Git-specific. For now this is fine (mockspace is fundamentally tied to git refs). Worth flagging as a constraint for documentation: mockspace assumes git. Non-git-VCS support is out of scope.

### 5. Filesystem-driven phase detection

Phase emerges from filename patterns in the round directory. Works, but doesn't generalise to:

- Multiple concurrent rounds (would need to scan each round dir).
- Queries across closed and open rounds simultaneously.
- Programmatic access without filesystem traversal.

The redesign replaces emergent-from-filesystem detection with `refs/mock/index` registry. Each round's status is a field in its index descriptor. Filesystem still carries the canonical truth (phase marker file inside the round ref), but the index is the queryable cache. Mockspace CLI maintains the index transparently.

### 6. The 5-phase TOPIC → CLOSED state machine itself

The existing model: TOPIC → DOC → DRAFT → IMPL → CLOSED with `lock` /
`unlock` / `deprecate` / `close` verbs. This is **bad DX and pain to
work with** (see BUG_LOCK_SEMANTICS, task #227 which already corrected
one specific bug here). The lock-suffix model is backwards from
intuition: "locking" a CL means it's frozen and the next phase opens.
Users expect "lock" to mean "we're done, lock it in" with subsequent
edits forbidden, not "transition to the next planning phase".

TBPED (`docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md`) replaces
this entirely with:

- Six phases: TOPIC, PLAN(DOC), APPLY(DOC), PLAN(SRC), APPLY(SRC), DONE.
- Three forward verbs: `mock phase plan` / `mock phase apply` / `mock phase finish`.
- One backward verb: `mock phase replan` (always deprecating,
  surface-precise restoration).

Tasks #236, #237, #238 cover the implementation.

The storage redesign (this doc's companion) preserves the marker-file
idiom for state encoding but uses TBPED's 6-phase vocabulary, not the
current 5-phase one.

### 7. Branch as first-class workflow object

The current model implicitly ties workflow state to the current git
branch (since `mock/` is in `refs/heads/*`, switching branches switches
workflow state). TBPED's #240 explicitly demotes branch from
first-class to ambient integration context. One active workflow area,
not branch-nested storage trees. Branch is recorded in archive closure
metadata for readability, not used as a separate identity layer.

The storage redesign reinforces this: the harness ref is independent of
source branches. The active round ref is one specific round, not
per-branch.

## Pending mockspace tasks: context map

Pending mockspace-related tasks in the clause-dev workspace, grouped by how they fold into the redesign.

### Workflow gate cluster (#231, #243)

`#231` is the substrate readiness gate (blocks all substrate work). `#243` is the mockspace workflow gate (blocks substrate readiness work). The redesign IS the mockspace workflow gate's main payload.

### Tasks-as-refs (#234, #235, #202, #233)

- **#234** (Namespace task tree under mock/tasks/ + ref resolution + identity lints) — proposes a `mock/tasks/` directory tree. Redesign: tasks are orphan refs under `refs/mock/task/<slug>`. Index handles registration; `mock://task/<slug>` URI handles resolution.
- **#235** (mock task command family — create/close/move) — `mock task new`, `mock task start`, `mock task close` operate on task refs. Sub-task transitions edit `meta.toml` `[steps.*]`.
- **#202** (Canonicalize TaskCreate + bundle tasks into mockspace mock/tasks/) — folds into the same task-as-ref design. The TaskCreate tool's tasks (workspace-session-scoped, like the ones tracked in `MEMORY.md`) become a different namespace, but the principle (durable structured task storage) is the same. May be worth folding agent-session TaskCreate output into mockspace tasks as an integration step.
- **#233** (Issue model + workflow-aware diagnostic context) — Issues collapse into the task ref namespace (no separate "issue" concept; everything trackable is a task). Diagnostics emitted by lints/hooks carry workflow context.

### Phase machine cluster (#236, #238)

- **#236** (Phase state machine + plan/apply/finish transitions) — original framing had plan/apply/finish; redesign keeps DOC/SRC phases with the marker-file convention. The transitions are still phase advance commands. The "plan/apply/finish" framing folds into "phase doc / phase src / lock / close" naturally.
- **#238** (mock phase replan = always deprecating, restore phase-owned surfaces only) — Replan deprecates the current manifest in the round ref, restores phase-owned surfaces (the source-side files that were claimed) to round-open state for THOSE files only. This survives the redesign as a `mock replan` command operating on the round ref and the source-side feature branch.

### Manifest cluster (#237, #318)

- **#237** (Manifest model — rename of changelist — + task ref linkage + seal-time snapshot) — the redesign's `manifest.doc.toml` + `manifest.src.toml` IS the manifest model. Task ref linkage via `[[change]] task = "mock://task/..."`. Seal-time snapshot is the lock event.
- **#318** (cl-claim-vs-source-mismatch lint + structured CL claim grammar) — verifier runs on manifest lock. Each `[[change]]` has a `verification` command that runs against source-side branch tip; mismatch blocks lock. The "structured CL claim grammar" IS the manifest's `[[change]]` TOML shape.

### Status and discovery cluster (#241, #242, #240)

- **#241** (mock status as primary entrypoint + guidance-oriented diagnostics) — `mock status` reads the index, shows current round, current phase, recent activity, pending tasks, suggests next action. Becomes the canonical CLI entry for "what's going on".
- **#242** (Reorganize mockspace docs + write summary starting-round doc) — Documentation refresh. Redesign updates this naturally; new design doc + WORKFLOW.md.tmpl in harness.
- **#240** (One active workspace + branch demoted to context) — Aligns with redesign's "one active round by default" assumption. Branch on the source side is contextual; round on the mock side is primary.

### Template and rendering cluster (#88, #43, #245, #246)

- **#88** (workspace-aware template fragment includes) — template engine supports include directives. mockspace-template already does this via minijinja. Survives as-is; redesign just keeps the engine in core.
- **#43** (Formalise BACKLOG.md.tmpl convention in mockspace) — template-convention task. Redesign accommodates by including BACKLOG.md.tmpl as a recognised template shape.
- **#245, #246** (auto-inject AI responsibility notice + canonical workflow description into rendered WORKFLOW.md + PRINCIPLES.md) — template-time render features. The harness ref carries the AI-responsibility text + canonical workflow text as a fragment; templates include it via the standard include mechanism.

### Lint and hook cluster (#54, #186, #289, #40)

- **#54** (Scope pre-push lint to changed crates) — hook-policy change. Harness hook scripts compute changed files via git diff against the remote tip; lint only those. No architectural change.
- **#186** (Unify lint:allow token naming) — internal naming consistency. Carries over.
- **#289** (Pre-commit cargo check should not block) — hook-policy change in harness. Pre-commit relaxed; build/push stay strict.
- **#40** (Explicit project-root hook scoping and workspace aggregation) — project-root scoping is intrinsic to per-project ref isolation (each project's hooks live on its own harness ref). Workspace aggregation is homma's job (already shipped in PR #10).

### Misc (#42, #444, #224, #203)

- **#42** (Tighten cargo mock check to verify CL fulfillment + tests + lints) — `mock check` becomes a verification command. After redesign: checks current round's state across all surfaces (manifest claims, tests pass, lints clean). Maps to existing `cargo mock check` + manifest-verifier from #318.
- **#444** (Extract reusable internals as lib crates) — **DONE.** Survives the redesign as-is.
- **#224** (Self-hosted mockspace — mockspace uses mockspace) — already true in spirit (mockspace has its own `mock/`). Under redesign, mockspace's design rounds live on its own `refs/mock/round/*`. Validated last per the open-question.
- **#203** (cargo mock ci feature — forge-agnostic CI orchestrator) — separate feature, not load-bearing for the redesign. Lands later.

### Dropped

- **#239** (Epoch concept) — DROPPED. Release tags carry the archival-boundary function. See redesign doc's "Open questions" section.

## Migration risk inventory

Things that look risky or non-obvious during migration:

### R1: Proxy-crate-based bootstrap breaks before standalone binary exists

If we ship the standalone `mockspace` binary and the redesign's worktree-based bootstrap as a single change, but a consumer's `build.rs` still calls `bootstrap_from_buildscript()`, the proxy crate machinery is gone and the build breaks.

Mitigation: phased migration. `mockspace-rs` retains the proxy-crate bootstrap for backward compatibility during transition. Consumers opt into the new flow per repo. Once all four consumer repos migrate, the legacy bootstrap is removed.

### R2: Per-repo `mockspace.toml` shape changes

Consumer `mockspace.toml` shapes today have `[lints.*]`, `[lint-crates]`, `[domain.*]`, `[crates.*]`. Redesign introduces `[externals.*]`, `[forge]`, `[refs]` sections. Consumers need to migrate their configs.

Mitigation: keep all existing sections; new sections are additive. A consumer with only the existing sections continues working. New sections become required only when consuming new features (external refs, forge integration).

### R3: Existing closed rounds carry historical content patterns

Today's closed rounds have `_topic.<name>.md`, `_changelist.doc.lock.md`, `_changelist.src.lock.md` files in dated subdirectories. Migrating each to an orphan ref preserves the content but the file naming/structure changes (timestamp prefix becomes ref name; filename pattern becomes flat).

Mitigation: migration script. Each closed round's dated subdirectory becomes one orphan ref with the files at root. The translation is mechanical:

```
mock/design_rounds/202605131900/                  →  refs/mock/round/202605131900-extracted-lib-crates/
├── 202605131900_topic.extracted-lib-crates.md     →  topic.extracted-lib-crates.md
├── 202605131900_changelist.doc.lock.md            →  manifest.doc.toml (or legacy doc.md if no migration done)
├── 202605131900_changelist.src.lock.md            →  manifest.src.toml (or legacy src.md)
                                                   +  phase.closed (new marker)
                                                   +  round.toml (synthesised; minimal, just slug+title)
```

If the changelist files don't translate cleanly to structured TOML, keep them as `legacy.doc.md` / `legacy.src.md` and skip the structured manifest. The historical content is preserved; future rounds use structured manifests.

### R4: Hook activation across repos

Today's hooks live at `<mock_dir>/target/hooks/`. Activation is `git config core.hooksPath ...`. Under redesign, hooks live in the harness ref's `hooks/` (or templates rendered to a local-only path). Activation changes.

Mitigation: bootstrap (standalone binary or `build.rs`) handles activation. Consumer doesn't need to know. Migration path: on first `mock init` in a redesigned repo, activate new hook path; deactivate old `target/hooks/`.

### R5: Workspace-level `.claude/` aggregation expects per-repo `.claude/`

Homma's PR #10 Phase 2 aggregation reads each per-repo `.claude/rules/*.md` and `.claude/hooks/*.sh`. Today these are rendered into the repo's `refs/heads/dev` tree. Under redesign, they render to local-only (target 2 in the redesign's rendering pipeline). Homma's aggregation needs to read from the local-only location, not from `refs/heads/*` tree.

Mitigation: aggregation updates to read from the resolved render path. The path is per-repo configurable (could be `.claude/rules/`, could be something else). Homma queries each repo's mockspace for the resolved path.

### R6: Closed-round filename convention vs ref-name convention

Today: `<YYYYMMDDHHMM>_topic.<name>.md` (timestamp + name in filename). Under redesign: ref name is `<YYYYMMDDHHMM>-<short-name>` (timestamp + name in ref slug; files inside are flat). Slight naming convention divergence (`_topic.<name>` becomes part of filename inside the ref vs. part of the ref name).

Mitigation: migration script translates. Convention shift is documented. New rounds use new convention.

### R7: Loss of "default-clone shows mock/" educational signal

Today, an outsider cloning the repo sees `mock/` and the design rounds. They CAN learn about mockspace by reading those files. Under redesign, default clone shows zero mockspace surface. The educational pathway is gone.

Mitigation: cleaner README on `refs/heads/main` that mentions mockspace exists, points at an opt-in. Mockspace participation is a deliberate decision, not an osmotic one.

This is also the design goal, not a bug.

## Cross-references

- [`docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md`](../../docs/research/TASKS_BRANCHES_PHASES_EPOCHS_DESIGN.md) (TBPED) — **the authoritative workflow redesign spec.** Defines the 6-phase model, plan/apply/finish/replan verbs, task model, manifest model, branch demotion. The companion storage redesign (this doc's sibling) layers on top of TBPED, not in conflict with it.
- [202605171033_ref-based-mockspace-redesign.md](./202605171033_ref-based-mockspace-redesign.md) — companion design doc for the ref-based storage layer.
- `docs/research/LINT_SYSTEM_REDESIGN.md` — companion to TBPED for lint engine + diagnostic shape redesign (referenced by #233).
- `docs/research/BUG_LOCK_SEMANTICS.md` — the bug record that motivates TBPED's replacement of lock/unlock with plan/apply/finish (closed as #227).
- `~/Dev/clause-dev/.claude/rules/mockspace-toml-edits.md` — workspace rule on severity downgrades.
- Closed round `202605131900-extracted-lib-crates` — the lib-crate extraction that produced `mockspace-config` + `mockspace-template`.
- Tasks in workspace MEMORY.md `#231-#243` (workflow gate cluster) — the pending workflow tasks TBPED defines; the storage redesign incorporates without reframing them.
