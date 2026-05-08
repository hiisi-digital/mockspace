# Lint System Redesign

**Status:** proposed  
**Scope:** `mockspace` lint/runtime architecture redesign  
**Audience:** `mockspace` maintainers, consumer repos, external reviewers  
**Supersedes:** earlier redesign notes in this file that are now largely implemented or obsolete

---

## Purpose

This document resets the lint-system redesign around a new architectural conclusion:

> `mockspace` should stop evolving a bespoke lint runtime and instead bundle and delegate to **Viola** as its primary lint engine.

In this model:

- **`mockspace`** owns workflow orchestration, bootstrap, hooks, repo-state detection, and context generation
- **Viola** owns plugin hosting/runtime execution, grammar-driven extraction, and issue aggregation
- **all execution surfaces are plugins behind one unified plugin ABI** (runners and lints included)
- **grammar plugins** produce one **normalized, versioned structure model** consumed by all lints
- **Rust-native lints** remain first-class through the same plugin ABI so existing Rust lint packs do not need to be thrown away
- the longer-term option of a **Rust core under the existing Viola ecosystem** remains open, but does not block the immediate redesign

This is not a side experiment or optional add-on. It is the proposed target architecture.

---

## Executive summary

The current `mockspace` lint system already behaves like an approximation of a convention-lint runtime:

- some tree-sitter-driven checks
- some line/path/git heuristics
- some workflow/path gating
- some ad hoc message generation
- some config-driven rules
- some agent/hook integration

Viola is a better fit for this problem shape than continuing to grow `mockspace`'s bespoke lint engine.

### Proposed direction

`mockspace` should:

1. **bundle a dedicated Viola-based lint runner** into `target/`
2. **skip `viola-cli`** and call the Viola library directly through a `mockspace`-owned runner
3. **compute workflow/repo state in Rust**
4. **pass that state into Viola** as structured context
5. **run grammar extraction and lint packs through Viola** using one normalized structure contract
6. **keep Rust-native lints viable** through the same stable unified plugin ABI so existing `mock/lints/` and external Rust lint crates can continue to exist as Rust

### Preferred near-term execution model

The preferred initial packaging model is:

- **Deno runtime**
- **self-contained bundled binary**
- generated and shipped by `mockspace`
- stored in `target/` alongside other generated artifacts such as `mockspace-proxy`

### Not the preferred near-term model

The redesign does **not** currently prefer:

- depending on a generic `viola-cli`
- keeping `mockspace` as the primary bespoke lint runtime
- treating Viola only as an optional sidecar
- forcing all Rust-native lints to be rewritten immediately into TypeScript
- making WASM or Rust-core migration a prerequisite for adopting Viola

---

## Why this document replaces the earlier redesign

Earlier versions of this file focused on problems that were real at the time, but many of those proposed changes are now already implemented or no longer central:

- builtin agent hooks
- auto-generated settings
- lint configurability
- config-driven forbidden-imports
- builtin agent templates
- parser modernization with `serde` / `toml_edit`

Those were useful steps, but they do not answer the bigger architectural question now in front of `mockspace`:

> Should `mockspace` continue growing its own lint runtime, or should it adopt a more general, more sophisticated lint engine and become the workflow/orchestration layer around it?

This document answers: **adopt Viola**.

---

## Problem statement

The current `mockspace` lint system has become an awkward hybrid of:

- Rust AST-based linting
- ad hoc source scanning
- workflow-state gating
- git-change heuristics
- shell-based agent/hook blocking
- mixed message-generation strategies
- a growing configuration surface in `mockspace.toml`

This creates several problems.

### P1 — `mockspace` is reinventing a lint runtime

The current system already approximates a convention-lint runtime, but in a fragmented way:

- one set of structures for Rust lints
- another set of shell hook decisions
- another set of bootstrap messages
- duplicated git/path/phase inference
- no clean, general issue model shared across all enforcement surfaces

### P2 — the current lint layer is less sophisticated than the target abstraction

The current Rust-based lint infrastructure should not be over-romanticized as "the serious parser" in contrast to Viola.

The reality is:

- some lints use tree-sitter
- some lints inspect lines directly
- some lints shell out to git
- some hook logic reimplements policy in bash
- some messages are composed inline with little structured metadata

This is not a mature unified lint engine. It is a partial in-house approximation.

### P3 — lint concerns are bloating `mockspace.toml`

As more lint power gets added to `mockspace`, `mockspace.toml` risks becoming a dumping ground for:

- workflow config
- repo config
- bootstrap config
- project metadata
- lint rule config
- lint pack config
- per-rule parameters
- reporting policies

That is too much responsibility for one config surface.

### P4 — native Rust lint packs are valuable and should not be discarded

Consumer repos already rely on Rust-native lint packs and `mock/lints/`-style extensibility.

Examples include:

- per-repo Rust lints in `mock/lints/`
- external Rust lint crates such as `mockspace-hilavitkutin-stack-lints`

Those represent real accumulated value. A redesign that requires rewriting everything into TypeScript immediately would create avoidable migration pain.

### P5 — diagnostics remain too fragmented

Today the system has no single coherent architecture for:

- issue generation
- report severity classification
- blocking wrapper messages
- contextual remediation
- environment-aware message shaping
- workflow-aware guidance

That is why diagnostics feel inconsistent and why hook-side blocking is often blunter than it should be.

---

## Design goals

### G1 — adopt Viola as the primary lint runtime

The redesign should make Viola the main engine for lint execution rather than growing a more bespoke `mockspace` lint engine.

### G2 — keep `mockspace` as the orchestrator

`mockspace` must remain the owner of:

- workflow state
- bootstrap/build integration
- git hook generation
- `cargo mock` entrypoint
- target artifact generation
- context export to the lint engine

### G3 — preserve Rust-native lint investment

The redesign must provide a path to keep:

- current `mock/lints/` extensibility
- external Rust lint crates
- Rust-native authoring for teams that want it

### G4 — reduce config sprawl in `mockspace.toml`

Lint rule and lint-pack configuration should move toward Viola-owned config, while `mockspace.toml` keeps workflow/project/bootstrap concerns.

### G5 — support workflow-aware convention linting

The new system must support not only code conventions, but also workflow conventions such as:

- phase gating
- archived/frozen surface protection
- source/doc window semantics
- branch/workspace/process guardrails

### G6 — improve diagnostics structurally, not cosmetically

The redesign should produce better messages because the runtime has a better issue model and better context handoff, not because strings were manually polished in many unrelated places.

### G7 — keep the user-facing interface stable

Users should still primarily experience this through:

- `cargo mock`
- git hooks
- generated agent integrations
- `mockspace`-owned bootstrap and orchestration

Not through needing to understand or invoke raw Viola components directly.

### G8 — keep future Rust-core evolution possible

The redesign should not foreclose a later evolution where the Viola core itself becomes Rust-backed while preserving the existing TS/Deno ecosystem.

### G9 — enforce one unified plugin ABI

Runners and lints must use the same plugin contract and lifecycle (with role-specific capabilities), not separate per-kind ABIs.

### G10 — enforce one normalized grammar output model

Grammar plugins may differ internally, but they must emit one stable, versioned structural model that all lints consume.

---

## Non-goals

This redesign does **not** require, in its first phase:

- replacing all existing Rust-native lints immediately
- rewriting all lint packs into TypeScript
- using `viola-cli` as a user-facing requirement
- implementing WASM-first integration
- rewriting Viola core in Rust before adopting it
- eliminating all Rust lint code from `mockspace` on day one
- solving every future multi-language linting need immediately

---

## Current-state review

This section exists to ground the redesign in the current codebase rather than idealized assumptions.

### `mockspace` today

`mockspace` currently provides:

- a Rust lint runner
- per-crate and cross-crate lint execution
- severity gates for commit/build/push
- config-driven lint behavior
- custom lint loading
- hook generation
- bootstrap and proxy generation
- workflow-sensitive lints
- agent hook generation and auto-generated settings

The target redesign keeps this orchestration role but moves execution behind a unified plugin ABI and normalized grammar structure model.

But the lint architecture remains split across several layers:

- Rust lint runtime
- shell hook enforcement
- bootstrap messaging
- workflow commands
- generated agent guards

### `mockspace-hilavitkutin-stack-lints`

The external lint crate demonstrates that the current ecosystem already values:

- reusable external lint packs
- domain-specific lint distribution
- Rust-native authoring
- shared conventions across multiple repos

That crate should be treated as a first-class migration stakeholder.

### Viola today

The locally cloned Viola stack shows:

- a plugin-based convention lint runtime
- a crawler/extraction model
- a structured issue model
- grammar packages
- script-lint plugin support
- builder/config rule classification
- Deno-first runtime and packaging assumptions

It is not merely a toy. It is a serious fit for `mockspace`'s actual lint problem.

### Important reframing

The key conclusion is not:

> `mockspace` has "real parser lints" while Viola is "just convention linting"

The more accurate conclusion is:

> `mockspace` has been building an ad hoc convention-lint/runtime system in Rust; Viola is a stronger abstraction for that same class of problem.

---

## Proposed architecture

## High-level split of responsibilities

### `mockspace` owns

- `cargo mock` entrypoint and UX
- git hook generation and activation
- bootstrap/build integration
- repo root and mock root discovery
- workflow state detection
- validation-surface detection
- changed-file provenance gathering
- generation of target-side artifacts
- rendering and wrapping of workflow-oriented block output when appropriate

### Viola owns

- codebase crawling
- grammar-driven extraction
- lint registration and composition
- lint execution
- issue generation
- lint configuration layering
- generic report classification
- plugin ecosystem

### Shared boundary

The bridge between them should be explicit and versioned.

`mockspace` should not expect Viola to rediscover all workflow state heuristically.  
Viola should not force `mockspace` to reinvent issue/lint execution.

---

### Preferred execution model

### Dedicated `mockspace`-owned Viola runner host

`mockspace` should not depend on a generic public CLI shape.

Instead, it should bundle a dedicated host that:

- loads runner and lint plugins through one unified plugin ABI
- imports the Viola library directly
- imports grammar plugins
- imports the `mockspace` Viola lint pack
- imports optional additional project/domain packs
- accepts structured context from `mockspace`
- executes configured runner scope exactly once
- emits a normalized, versioned structure model
- runs configured lints in one concurrent pass over that model
- emits structured issues/results back to `mockspace`

### Why not `viola-cli`

A generic CLI boundary is the wrong layer here because `mockspace` needs:

- tightly controlled context injection
- controlled artifact generation
- predictable versioning
- workflow-specific composition
- deeper integration than "run a generic CLI with a config file"

The correct abstraction is library-level integration through a `mockspace`-owned runner.

---

## Packaging model

### Preferred near-term model: bundled Deno binary

The preferred first implementation is:

- keep Viola as Deno-first
- compile a self-contained runner binary
- bundle it into `target/`
- manage it through `mockspace` bootstrap/build logic

This provides:

- no user-installed Deno requirement
- reproducible local artifact management
- alignment with existing `target/`-side generation patterns
- strong control over the exact runtime version `mockspace` delegates to

### Likely artifact shape

Conceptually:

- `target/mockspace-proxy/`
- `target/mockspace-viola/runner`
- generated config/context sidecars as needed

Exact paths can be refined later.

### Why this is preferred over WASM-first

WASM linking is conceptually attractive, but likely expands scope too early:

- runtime-host boundary gets more complex
- filesystem/grammar/config loading gets harder
- Deno-first assumptions would likely need portability work
- this risks turning adoption into a runtime-porting project

So WASM should remain a possible future option, not the initial target.

---

## Context bridge: `mockspace` → Viola

The success of this redesign depends on a clean context contract.

### Principle

`mockspace` should compute workflow/repo state once and pass it to Viola in a stable form.  
Viola lints should consume that context rather than reproducing fragile git/path/phase heuristics independently.

### Context layers

#### Layer 1 — environment facts

Useful for simple conditional behavior:

- validation surface (`pre-commit`, `pre-push`, `cli`, `agent-pretool`, etc.)
- CI vs local
- phase summary
- mock root / repo root
- scoped/doc-only mode
- project name / crate prefix as needed

#### Layer 2 — structured context sidecar

Needed for richer workflow-aware lints:

- current workflow phase
- active/locked/deprecated artifacts
- changed files with provenance:
  - staged
  - unstaged
  - untracked
- archive/frozen-path metadata
- maybe current epoch / active manifest info in future models
- maybe computed status summaries for diagnostics

This sidecar should be versioned and explicitly owned by `mockspace`.

### Why both layers matter

Environment variables are easy for coarse selection and report shaping.

Structured sidecars are necessary for correctness, richer messaging, and future extensibility.

---

## Grammar model for Viola

Grammar packages are core enabling deliverables.

### Required outcome

Each `viola` grammar package (including Rust) must expose:

- language-specific source matching
- language-specific extraction queries/transforms
- output mapped into the same normalized, versioned structural model

This keeps grammar internals language-specific while keeping lint consumption language-agnostic.

### Minimum expectation

It must support enough extraction to express most current `mockspace` and domain lints, including things like:

- functions and signatures
- imports/use statements
- type references
- docs/comments where appropriate
- string literals and identifiers where appropriate
- file-level data and source locations

### Important clarification

"Write a grammar plugin" is essential, but not sufficient.

The real minimum viable architecture also requires:

- the bundled runner host
- the context bridge
- the normalized structure contract definition and versioning policy
- the `mockspace` plugin pack
- unified plugin ABI support for both runner and lint roles

---

## `mockspace` Viola plugin pack

A dedicated `mockspace` pack should define the current workflow/convention rules in Viola terms.

### This pack should own

- workflow/path/phase convention lints
- repo/process convention lints
- diagnostics metadata specific to `mockspace`
- issue kinds for workflow-oriented blocks and warnings
- lint defaults appropriate for `mockspace`

### This pack should not own

- bootstrap itself
- hook generation itself
- repo-state discovery itself
- cargo/build integration itself

Those remain `mockspace` responsibilities.

### Relationship to current Rust lints

Initially, the current Rust lint implementation can be treated as migration-era source material.

Over time, logic should move toward:

- pure Viola-native lints where appropriate
- Rust-native plugins via the native bridge where that is the right authoring surface

---

## Unified plugin ABI (runner + lint)

This is a first-class design requirement.

The redesign should not force all lints into TypeScript or shell, and it should not special-case Rust at core level.

### Goal

Allow Viola to execute runner and lint plugins through one stable plugin contract so that:

- existing `mock/lints/` ecosystems can survive
- external Rust lint crates remain viable
- teams that prefer Rust authoring are not punished
- TS/Deno ecosystems remain intact via a Deno runner plugin preset
- `mockspace-hilavitkutin-stack-lints` and similar crates have a credible migration path

### Why this belongs in Viola itself

This should not remain purely a `mockspace`-local hack.

A general-purpose convention lint runtime benefits from being able to host native lints.

So the redesign should assume a modest Viola extension for native plugin support rather than treating it as only an internal mockspace concern.

---

## Native plugin loading modes

There should be two conceptual modes, with one preferred initially.

### Mode A — out-of-process native lint binaries

This should be the preferred first mode.

A native Rust lint pack can be compiled as a standalone executable that:

- receives a stable input payload
- returns a stable result payload
- does not require unstable in-process ABI guarantees
- works well across platforms
- is easier to version and debug

This is analogous in spirit to how `viola-script-lints` already proves the value of a process-boundary plugin protocol.

### Mode B — in-process dynamic libraries

This is desirable as a later optimization path.

A native Rust lint pack could be loaded through a stable ABI from a dylib, but this should be treated as:

- more complex
- more sensitive to ABI/version concerns
- potentially valuable for performance and tighter integration
- not the necessary first step

### Recommendation

Design the plugin contract so it is **transport-agnostic**:

- the same logical contract can be hosted by a binary plugin or a dylib plugin
- start with binary plugins
- add dylib hosting later if it proves worthwhile

---

## Stable unified plugin contract

The core contract should be explicit, versioned, and small.

### What the contract must cover

#### Plugin manifest
A plugin should declare:

- plugin name / version
- plugin role(s): runner, lint (or both)
- capability IDs it exposes
- issue kinds/catalog metadata (for lint role)
- data requirements
- config schema or config expectations
- normalized structure model version compatibility
- runtime compatibility/version info

#### Input model
A plugin should receive:

- normalized structure model (for lint role) or execution scope + grammar config (for runner role)
- relevant `mockspace` workflow context
- plugin config
- execution metadata such as validation surface

#### Output model
A plugin should return:

- normalized structure model (runner role)
- issues (lint role)
- optional plugin-level warnings/errors
- optional diagnostics metadata
- success/failure state

### Strong recommendation

The stable contract should be data-oriented and serializable.

That means JSON- or schema-shaped payloads are the right conceptual base even if a faster transport is later introduced.

This keeps:

- binary mode easy
- dylib mode possible
- debugging tractable
- compatibility auditable

---

## Plugin requirements model

A key design opportunity is to avoid overfeeding plugins.

### Desired behavior

Plugins should declare requirements against the normalized structure model and workflow context, such as:

- node categories required
- file/path metadata required
- raw content requirement (if needed)
- workflow context fields needed

The host can then materialize only the required projections of data.

### Why this matters

This preserves the strengths of Viola's current model:

- single-pass configured runner execution
- shared extraction into one normalized model
- multiple lints over one extracted dataset
- no unnecessary duplicate parsing by every lint

All plugins should benefit from the same architecture, not bypass it.

---

## Rust plugin SDK

There should be a dedicated Rust SDK for writing native Viola plugins.

### Responsibilities of the SDK

- define the stable contract types
- expose plugin manifest helpers
- expose issue construction helpers
- provide config deserialization helpers
- provide data-projection types matching Viola's extracted model
- provide optional compatibility helpers for migration from current `mockspace` lint APIs

### Why this matters

Without a proper SDK, native plugin support becomes:

- fragile
- hard to version
- hard to adopt
- too bespoke to be a credible replacement for current Rust lint authoring

### Migration help

This SDK should intentionally make it easy to port existing Rust lint code by offering familiar ergonomics where possible.

---

## Migration path for existing Rust-native lints

A successful redesign must offer a credible staged migration.

### Existing `mock/lints/` directories

These should have a path such as:

1. continue to work through current transitional infrastructure
2. gain a route to compile into native Viola plugin binaries
3. optionally later support dylib mode
4. eventually converge on the new native plugin SDK

### External Rust lint crates

Crates like `mockspace-hilavitkutin-stack-lints` should be able to move toward:

- a Rust-native Viola plugin crate
- or a dual-published transitional form during migration

### Important principle

The redesign should minimize "rewrite pressure."

Rust authoring is not a defect to be eradicated.
It is a supported first-class path that needs a better runtime beneath it.

---

## Issue model and diagnostics

Viola already has a better issue abstraction than the current ad hoc `mockspace` lint output.

That should be treated as a strategic asset.

### Useful existing Viola issue features

The current issue model already supports concepts such as:

- issue kind
- source location
- message
- confidence
- suggestion
- optional context

That is a better foundation than today's mixed string-generation approach.

### What `mockspace` needs beyond generic issue fields

For workflow-aware diagnostics, we should standardize additional context such as:

- current phase
- validation surface
- why blocked
- safest next step
- recovery path
- maybe related artifact names
- maybe structured change provenance

### Recommended split

#### Viola/plugin layer should produce
- structured issues
- issue context
- suggestions/remediation hints
- confidence/metadata

#### `mockspace` should still own
- top-level blocking wrapper when the user is in a `cargo mock` / hook workflow
- workflow-specific framing around lint failures
- integration-specific summaries if needed

This yields a clean split:
- Viola generates better issue content
- `mockspace` provides workflow-facing presentation and orchestration

---

## Configuration split

A major motivation for this redesign is to keep `mockspace.toml` from becoming a dumping ground.

### `mockspace.toml` should keep

- project/workflow/bootstrap config
- generation/install config
- repo metadata such as project name / crate prefix
- config relevant to workflow and orchestration
- maybe references to lint runtime assets or override entrypoints

### Viola config should own

- lint packs
- lint rule config
- report levels
- issue classification rules
- lint options
- rule composition and filtering

### Migration-friendly approach

`mockspace` should likely be able to:

- generate default Viola config fragments
- merge them with user overrides
- keep beginner ergonomics strong
- avoid forcing users into hand-authoring large TS config unless they want to

Exact format can evolve later, but the ownership split should be decided now.

---

## Relationship to current `mockspace.toml` lint config

Current `[lints]` support was a useful evolutionary step, but should now be treated as transitional architecture.

That means:

- current config mechanisms may continue for compatibility
- but the direction should move toward Viola-owned lint config
- future `mockspace.toml` growth should be resisted when the concern is fundamentally lint-runtime configuration

---

## Hooks and agent integration

Bundling Viola does **not** mean giving up `mockspace`'s strong hook and agent posture.

### Hooks remain `mockspace`-owned

`mockspace` should continue generating and owning:

- git hooks
- activation/deactivation flow
- hook bootstrapping
- target artifact management

### Agent integrations remain `mockspace`-owned

`mockspace` should continue owning:

- generated agent hooks
- tool matcher wiring
- phase-aware write guards
- reminder/context hooks
- project/workflow-facing guidance

### But lint execution beneath them changes

Instead of calling `mockspace`'s bespoke Rust lint runtime directly, those surfaces should ultimately delegate to the bundled Viola runner through `mockspace`.

---

## Native bridge and `viola-script-lints`

The script-lints package is highly relevant to this redesign.

### Why it matters

It proves that Viola already has a useful pattern for external lint execution:

- out-of-process plugins
- stable input/output expectations
- issue JSON
- plugin discovery and configuration

### Strategic implication

Native Rust lint support should learn from this rather than inventing a completely unrelated model.

Conceptually:

- `viola-script-lints` validates the process-boundary plugin pattern
- native Rust lint support can apply the same pattern with a stronger typed contract and better performance

This is one reason out-of-process native binaries are the right first plugin-hosting mode.

---

## Longer-term option: Rust core under the Viola ecosystem

This is worth discussing, but should not block the main redesign.

### The idea

Over time, Viola's core runtime could move toward a Rust implementation while preserving the existing TS/Deno ecosystem shape.

That would allow:

- TS ecosystem continuity
- stronger performance
- stronger native portability
- a more robust compiled core
- continued package/config authoring in the existing ecosystem

### What this would look like conceptually

- Rust core engine
- prebuilt binaries for major platforms
- existing TS/Deno packages become wrappers/adapters/config surfaces over the Rust core
- same plugin/config concepts remain available to the existing ecosystem

### Why this is attractive

It would align well with:

- performance goals
- native distribution goals
- Rust plugin interoperability
- long-term robustness

### Why it should not block the redesign

Because the immediate question is not "what should Viola be in five years?"

It is:

> how should `mockspace` redesign its lint system now?

The answer can still be:

- adopt bundled Deno-first Viola now
- keep Rust-core evolution explicitly open
- avoid tying present adoption to future runtime-porting work

---

## Alternatives considered

### Alternative A — continue evolving the bespoke Rust lint engine

Rejected as the primary direction.

Reasons:

- duplicates effort already better captured by Viola
- keeps lint/runtime concerns too entangled with `mockspace`
- preserves config sprawl pressure
- does not improve ecosystem story
- does not naturally solve native vs TS lint authoring concerns

### Alternative B — use `viola-cli` as the integration boundary

Rejected as the preferred direction.

Reasons:

- too generic for `mockspace`'s needs
- weaker control over context handoff
- weaker artifact/version ownership
- wrong abstraction boundary

### Alternative C — force all lints into TypeScript immediately

Rejected.

Reasons:

- discards real Rust-native lint investment
- creates unnecessary migration pain
- alienates existing external lint-pack users
- not required for Viola adoption

### Alternative D — go WASM-first

Deferred.

Reasons:

- too much portability/runtime work too early
- greater integration complexity
- not needed to realize most of the architecture win

### Alternative E — rewrite Viola core in Rust first, then adopt it

Deferred.

Reasons:

- valuable long-term idea
- wrong near-term dependency order
- would slow needed redesign progress

---

## Migration strategy

A credible phased rollout should look something like this.

### Phase 1 — architecture decision and context contract

- decide the ownership split
- specify the `mockspace` → Viola context bridge
- define the bundled runner model
- define native plugin strategy at the design level

### Phase 2 — bundled Viola runner

- create the dedicated runner
- bundle it into `target/`
- let `mockspace` orchestrate it
- avoid `viola-cli`

### Phase 3 — Rust grammar package

- implement the Rust grammar package for Viola
- validate extraction quality against current needs

### Phase 4 — `mockspace` Viola pack

- port or re-express key `mockspace` workflow/convention lints in Viola-native form where appropriate
- establish issue kinds and reporting model

### Phase 5 — native Rust plugin bridge

- implement the stable native plugin contract
- start with out-of-process binary plugins
- provide Rust SDK

### Phase 6 — migrate current Rust lint investment

- migrate `mock/lints/`-style Rust extensions
- migrate external lint crates
- maintain compatibility shims as needed

### Phase 7 — rationalize old Rust lint runtime

- remove or minimize bespoke Rust lint runtime pieces that are no longer strategic
- keep only the pieces necessary for orchestration, bootstrap, or migration compatibility

### Phase 8 — optional deeper evolution

- consider dylib hosting
- consider a Rust-backed Viola core
- consider tighter in-process integration if still justified

---

## Risks and tradeoffs

### R1 — split-brain period during migration

There will likely be a period where:

- some lints are still in the old Rust path
- some are Viola-native
- some are Rust-native plugins under the new bridge

This is manageable, but should be acknowledged.

### R2 — native plugin ABI design is easy to get wrong

If the native bridge is underspecified or unstable, it will create more churn than it removes.

This is why a versioned, data-oriented contract and SDK are important.

### R3 — Rust grammar work is nontrivial

The Rust grammar package is a real deliverable and must not be hand-waved.

### R4 — config split can become confusing if not curated

If both `mockspace.toml` and Viola config are allowed to grow freely without clear responsibility boundaries, the redesign will fail one of its main goals.

### R5 — diagnostics can still feel fragmented if rendering ownership is unclear

The split between issue generation and workflow-facing block rendering must be deliberate.

### R6 — future Rust-core discussion can become a distraction

The Rust-core idea is strategically valuable, but it should not derail the near-term redesign into an oversized runtime rewrite program.

---

## Open questions for external review

These are the questions a domain review should focus on.

### Q1 — Is bundled Viola the right primary architecture?
Not "is Viola interesting," but whether it should become the main lint runtime beneath `mockspace`.

### Q2 — Is the proposed responsibility split sound?
Does the division between `mockspace` orchestration and Viola runtime ownership look right?

### Q3 — Is out-of-process plugin hosting the right first bridge?
Should binary plugins be the preferred initial path, with dylibs later?

### Q4 — What should the stable unified plugin contract look like?
How small can it be while still supporting runner + lint roles and future-proof compatibility?

### Q5 — How much workflow context should be provided to lints?
What belongs in env vars versus structured sidecars?

### Q6 — What should remain in `mockspace.toml` versus move into Viola config?
What is the cleanest long-term config responsibility split?

### Q7 — What is the best migration strategy for existing Rust-native lint packs?
How should current `mock/lints/` and external crates evolve with minimal pain?

### Q8 — Should native plugin support land in Viola core directly, or first as a companion package?
What rollout path best balances ecosystem cleanliness and implementation risk?

### Q9 — Is the Rust-core-under-Viola direction strategically worthwhile?
If yes, what should be designed now so that future move remains possible without redoing the architecture?

---

## Decisions captured here

### D1 — `mockspace` should adopt bundled Viola as its primary lint engine
This is the core direction of the redesign.

### D2 — `mockspace` remains the orchestrator
Workflow, bootstrap, hooks, and context gathering remain `mockspace` responsibilities.

### D3 — integration should use a `mockspace`-owned runner, not `viola-cli`
The runner should call the Viola library directly.

### D4 — the preferred packaging model is a self-contained bundled Deno binary in `target/`
This is the first implementation target.

### D5 — grammar plugins must emit one normalized, versioned structure model
Language-specific internals are allowed; output contract is shared.

### D6 — plugins use one unified ABI across roles
Runner and lint roles share one contract/lifecycle with role-specific capabilities.

### D7 — native Rust lints remain first-class
The redesign must support existing and future Rust-native lints rather than forcing immediate TS rewrites.

### D8 — binary plugins are the preferred first mode
Dylib hosting is valuable later, but not the first requirement.

### D9 — the unified plugin contract should be stable, versioned, and data-oriented
A serializable, transport-agnostic model is preferred.

### D10 — `mockspace.toml` should stop absorbing every lint concern
Lint rule configuration should move toward Viola-owned configuration.

### D11 — diagnostics should be structurally improved through the new issue/context model
Not merely by rewriting strings in many places.

### D12 — a future Rust-backed Viola core is worth discussing but must not block the redesign
Near-term adoption should proceed without requiring that rewrite first.

---

## Summary

The redesign should no longer be about making `mockspace`'s bespoke Rust lint system incrementally less awkward.

It should instead be about:

- making **Viola** the primary lint runtime
- making **`mockspace`** the workflow and orchestration shell around it
- bundling a dedicated Viola runner into `target/`
- providing a clean workflow-context bridge from `mockspace` to Viola
- building a **Rust grammar package**
- building a **native Rust lint bridge** so current Rust lint investment remains viable
- keeping the option open for a future Rust-backed Viola core without making that a blocker now

This is the cleaner architecture, the better ecosystem story, and the more scalable direction for `mockspace` going forward.