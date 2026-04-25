# Namespace Tasks, Branches, Phases, and Epochs — design for mockspace

**Status:** proposed  
**Scope:** mockspace framework design  
**Audience:** mockspace maintainers and consumer repos adopting the workflow

## Purpose

This document proposes a tightened workflow model for mockspace centered on four first-class concepts:

- **tasks** — canonical, namespace-owned work records
- **phases** — the active planning/application cycle
- **manifests** — sealed phase contracts
- **epochs** — historical compaction boundaries

In this model, **git branch remains important but is no longer a first-class stored workflow object**. It is the ambient integration context in which the active phase workspace exists.

The goal is to preserve the strongest parts of the existing mockspace design-first workflow while removing machinery that duplicates git or turns mockspace into a workflow database.

This design is for the **framework**, not for any one consumer repo's concern taxonomy. The framework must support arbitrary namespace depth and arbitrary namespace naming without imposing domain structure.

---

## Problem statement

The current mockspace model is strong on design governance, but the old branch-heavy expansion drifted too far toward duplicating git and introducing additional durable workflow identity where it was not actually needed.

Mockspace already has real strengths:

- design-first planning
- explicit phase transitions
- lintable workflow artifacts
- strong supersession/deprecation semantics
- archival of completed work
- repo-native, versioned workflow history

Those strengths should remain.

However, the old expanded model was trying to make too many things first-class at once:

- tasks
- branches
- phases
- epochs
- manifests
- branch-local workspaces
- branch durability metadata
- mixed archive formats

That was more machinery than the workflow actually needs.

The corrected design should:

- keep the philosophical core strong
- remove artifacts that duplicate what git already does well
- keep active workflow centered on phase and manifest
- keep task storage simple and grep-friendly
- preserve strong shame/accounting semantics when going backwards
- preserve historical readability without overbuilding

---

## Design goals

### G1 — repo-native canonical task graph

Tasks must live in the repo under `mock/tasks/`, versioned with the rest of the design and workflow artifacts.

### G2 — no framework-imposed concern tree

Mockspace must not dictate how consumer repos categorize their work. It should operate on arbitrarily nested namespaces.

### G3 — phase-first workflow

The active workflow should be modeled primarily in terms of **phase transitions**, not as a heavyweight branch object model.

### G4 — manifest-centered execution contracts

Active work should be governed by manifests, not by a second redundant branch-owned tasklist or workspace identity layer.

### G5 — use git for branch identity instead of re-modeling it

Git branch remains the real integration context. Mockspace should not create a parallel durable branch identity system unless later implementation experience proves that absolutely necessary.

### G6 — Markdown all the way through for tasks

Open tasks and archived tasks should both be Markdown, not split across editable Markdown and JSONL archival formats.

### G7 — strong historical preservation without workflow-database complexity

History should remain clear, reviewable, grep-friendly, and versioned in git, without requiring complex hidden identity layers.

### G8 — preserve deprecating replan semantics

Once a phase has entered `APPLY(...)`, going backwards through mockspace should always preserve the historical fact that the earlier manifest proved insufficient.

### G9 — epochs remain real but lightweight

Epoch should remain a real framework concept, but not start life as a huge release-governance system.

### G10 — strict guards with better guidance

Simplifying artifact/storage complexity is not an argument for weaker enforcement. If anything, simpler storage should permit stronger, clearer guardrails.

---

## Non-goals

This design does **not** define:

- a required namespace taxonomy for any consumer repo
- a required number of namespace levels
- exact consumer-facing concern names
- exact final command spelling for every CLI operation
- exact migration sequencing for every existing repo
- release automation or CI/CD policy
- exact implementation of archive rendering/generation internals
- branch durability metadata beyond what is necessary for historical readability
- a workflow database or durable branch identity subsystem

---

## Executive summary

The tightened model is:

- **task** = canonical work record
- **phase** = active workflow lifecycle
- **manifest** = sealed phase contract
- **epoch** = historical compaction boundary
- **git branch** = ambient integration context

This means:

- tasks remain namespace-owned
- manifests reference task refs
- active workflow artifacts live in one active phase workspace
- git branch determines which version of that active workspace you are looking at
- completed workflow artifacts are archived into epoch history with branch name recorded as context
- open tasks are Markdown files
- closed tasks are archived as Markdown files
- there is no JSONL task archive model
- there is no cheap workflow-level reopen path after `APPLY(...)`
- lints remain strict and should become more instructional

That is not a temporary simplification. It is the corrected intended model.

---

## Vocabulary

### Namespace

A namespace is an arbitrarily nested path under the task tree.

Examples:

- `compiler/ir`
- `runtime/abi`
- `cross-cutting/lints`
- `engine/plan/analysis`

Mockspace treats namespaces mechanically, not semantically.

### Task

A task is a structured work item owned by a namespace.

Tasks are the canonical work graph. They are not branch-owned.

### Phase

A phase is the active step in the planning/application cycle.

The active workflow is fundamentally phase-driven.

### Manifest

A manifest is the active contract for a phase.

This is the clearer long-term name for what older terminology called a changelist. The old filenames or migration language may persist temporarily, but conceptually these are manifests.

### Epoch

An epoch is a larger archival boundary that compacts closed workflow history into a historical package while keeping the active workspace clean.

### Branch context

Branch remains important, but as **context**, not as a heavyweight mockspace-managed workflow object.

Branch context means:

- the current git branch
- the integration surface on which the active phase workspace exists
- the branch name recorded in status, guidance, closure metadata, and epoch archives

### Topic document

A topic document is exploratory design/rationale material.

It is not the canonical task store.
It is not the canonical phase contract.

### Deprecated manifest

A deprecated manifest is a sealed or meaningful prior manifest that has been superseded after application had already begun.

Deprecated manifests remain immutable historical artifacts.

---

## Core conceptual split

Mockspace should model workflow with four cooperating layers.

### 1. Standing project surfaces

These are long-lived concern/domain surfaces such as:

- crate `DESIGN.md.tmpl`
- `README.md.tmpl`
- `BACKLOG.md.tmpl` during migration
- `research/`
- `benches/`

They answer: what is this part of the project, what is it trying to become, what principles govern it?

### 2. Structured work graph

This is the `mock/tasks/` surface.

It answers:

- what work items exist?
- which namespace owns them?
- what depends on what?
- what is open, blocked, deferred, or in progress?
- what has closed historically?

### 3. Active phase workflow

This is the phase + manifest + topic system.

It answers:

- what phase is currently active?
- what design is being discussed?
- what tasks are in scope?
- what surfaces are legal to change right now?
- what sealed contract governs the current step?

### 4. Historical compaction

This is the epoch system.

It answers:

- what has completed since the previous epoch?
- which task archives and workflow artifacts belong together historically?
- what carry-forward design material exists for the next era of work?

---

## High-level model

### Canonical task ownership

Tasks are namespace-owned.

They do **not** belong to the current git branch as their primary identity.

A branch may work on tasks.
A manifest may reference tasks.
But the task remains owned by its namespace.

### Canonical active-work contract

Active work is governed by manifests.

A manifest references task refs from the task tree.
A manifest does not duplicate the full task body as the canonical source of meaning.

### Canonical active execution unit

The primary active unit is the **phase**, not the branch.

The branch determines ambient repo context.
The phase determines workflow meaning.

### Canonical branch role

Git branch is still important:

- it carries the repository state
- it determines which active phase workspace revision you are seeing
- it is the integration context shown in status and guidance
- it is recorded in historical archive metadata

But branch does **not** need to be modeled as a heavyweight stored mockspace object with its own durable identity layer in v1.

### Canonical history of a task

A task's long-lived history stays with its namespace:

- open task file while active
- archived Markdown task record when closed
- epoch package later preserves historical slices

### Canonical history of a workflow run

Historical workflow artifacts preserve:

- topic material
- manifests
- closure/accounting metadata
- branch name as context
- timestamps and relevant integration notes
- which task refs were in scope
- which completed, deferred, split, or produced follow-ups

### Canonical historical compaction boundary

An epoch package preserves:

- closed workflow artifacts since the previous epoch
- closed task archives since the previous epoch
- retrospective / carry-forward design material

---

## Branch: demote it, do not delete it

This design does **not** say that branch stops mattering.

It says branch matters in a **different way**.

### What branch still is

Branch is still:

- the current git branch
- the integration context
- the thing shown in status and guidance
- the thing recorded in closure metadata
- the thing used to group archived workflow artifacts inside epoch history

### What branch no longer needs to be

Branch does **not** need to be:

- a full mockspace-managed workflow object with a separate durable identity model
- a separate active workspace namespace inside `mock/`
- a top-level planning artifact
- a second abstraction layer parallel to git branch identity

### Why this simplification is correct

Git already multiplexes repository state by branch.

If active workflow artifacts are tracked files in the repo, then different git branches naturally already carry different versions of those active artifacts.

A branch-nested mockspace workspace would merely restate information git already provides.

So the correct model is:

- active workflow artifacts live in one active workflow area
- the current git branch determines which version you are seeing
- when work completes, those artifacts are archived into historical storage with branch name recorded there

That is simpler and more coherent.

---

## Namespace task tree

### Framework-level rule

Mockspace only knows that `mock/tasks/` contains nested namespaces.

The namespace tree may be any depth, with any names.

The framework enforces mechanics, not taxonomy.

### Open tasks

Open tasks are one Markdown file per task.

This is optimized for active work:

- visible in the tree
- easy to open directly
- easy to review in git
- grep-friendly
- straightforward for humans and tooling alike

### Archived tasks

Closed tasks move into a simple archive directory in the same namespace, still as Markdown files.

Example conceptual shape:

- `mock/tasks/compiler/ir/structural-robust-ir.md`
- `mock/tasks/compiler/ir/archive/2026-04-22--structural-robust-ir.md`

The exact final filename convention can be refined later, but the governing principle is:

> open tasks are Markdown, archived tasks are Markdown, and namespace-local history remains readable without schema-juggling.

### No JSONL task archive model

This design intentionally drops the JSONL archive idea.

Reasons:

- Markdown is already the human-facing editing medium
- git is the real long-term history engine
- grep and direct inspection matter more than append-optimized machine packing
- schema management for task archives is not worth the complexity here
- mockspace is stronger when canonical workflow artifacts stay easy to inspect

### No global task graveyard

There should not be one giant repo-wide closed-task dump.

Closed task archives should remain namespace-local until epoch compaction, preserving concern ownership and readability.

---

## Task identity and references

### Canonical task identity

Task identity is:

- namespace path
- plus stable task slug

Conceptual examples:

- `compiler/ir#structural-robust-ir`
- `runtime/abi#stabilize-zig-c-boundary`
- `cross-cutting/lints#no-bare-option`

Numeric ids are not required.

### Stable slug principle

The slug is identity, not presentation.

Therefore:

- the filename slug should stay stable once created
- the human-facing title may evolve
- if the conceptual work changes so much that the slug wants to change materially, that usually means a new or superseding task, not a rename

### Uniqueness rule

Within the same namespace directory, open task slugs must be unique.

Across different namespaces, duplicate slugs are acceptable because namespace path disambiguates them.

### Namespace moves are identity-sensitive

Because canonical task identity is `namespace#slug`, moving a task across namespaces is not a trivial cosmetic file move.

Therefore the framework should require one of the following strategies:

- dedicated task-move tooling that rewrites references safely
- explicit supersession instead of movement
- or a later hidden immutable-id strategy if implementation experience demands it

What this document does **not** allow is treating raw filesystem moves as harmless.

### Reference resolution

Mockspace must resolve task refs across:

- open task files
- namespace-local archived Markdown task files
- epoch task archives

This is why refs must remain logical (`namespace#slug`) rather than path-bound.

---

## Task representation

### Open task representation

Open tasks are Markdown files that are both human-readable and machine-parseable.

Framework-level requirements:

- stable slug identity derived from filename
- parseable structured metadata
- human-readable body content
- dependency refs
- tags/labels if the consumer repo wants them
- activation/ownership metadata if needed by the workflow
- enough structure to support linting and manifest references

### Archived task representation

Archived tasks remain Markdown and must be self-contained historical records.

Each archived task should preserve at least:

- canonical task ref
- namespace
- slug
- title
- structured metadata
- full body content
- closure metadata such as:
  - resolution kind
  - closed at
  - closed in branch context
  - closed by phase/manifest if relevant
  - epoch at closure time if known

Archived task records should not merely be stubs pointing to vanished open files.

### Archived tasks are immutable historical records

Once archived, task records should be treated as immutable workflow history except through dedicated repair tooling if absolutely necessary.

That keeps the archive trustworthy.

---

## Task state model

### Open-side states

The exact state vocabulary can evolve, but framework semantics should support distinctions such as:

- open
- blocked
- deferred
- in-progress

All of these remain open-side task files.

### Closed-side resolutions

Archived tasks should close with a meaningful resolution such as:

- completed
- cancelled
- superseded
- wontfix

### Reopen policy

Historical archive entries should remain historical.

If previously closed work returns, the default should be:

- create a new open task
- reference the archived predecessor
- preserve the old closure record

That is better than mutating old archive history.

---

## Manifests

### Rationale

The active contract artifact should be called a **manifest**.

This is clearer than `changelist` for what the artifact actually does:

- defines scope
- names task refs
- names allowed surfaces
- records acceptance criteria
- serves as the sealed contract for the current phase

### Why manifests remain separate from tasks

Tasks and manifests answer different questions.

Tasks answer:

- what work item is this?
- what does it mean?
- what depends on it?
- which namespace owns it?

Manifests answer:

- which subset of tasks is in scope for this phase?
- what managed surfaces are allowed to change?
- what is explicitly out of scope?
- what acceptance constraints govern this phase?

Combining them would either weaken manifests or over-rigidify tasks.

So they remain separate.

### Manifest references

Manifests should reference task refs instead of duplicating task bodies.

However, at seal/apply time the manifest should preserve enough task snapshot information that the sealed contract does not drift retroactively if the underlying tasks later evolve.

At minimum, a sealed manifest should snapshot for each referenced task:

- task ref
- task title
- task state
- dependency snapshot if relevant

A stronger implementation may later add a content digest or similar stable content identity.

### Doc/src separation remains

Tasks are not structurally split by phase.

Instead:

- tasks remain namespace-owned
- manifests remain phase-specific

So the active contracts are still:

- doc manifest
- src manifest

This keeps one of the strongest properties of the existing mockspace model.

### Deprecated manifests remain immutable

Once deprecated, a manifest is historical record, not live editable planning material.

---

## Phase-first workflow

### Why phase is the real first-class active concept

Once branch is demoted to contextual primitive, the real active workflow concept becomes very clear:

- tasks = canonical work graph
- phases = active execution cycle
- manifests = phase contracts
- epochs = historical compaction boundaries
- git branch = ambient integration context

That is more coherent than forcing branch into the same level as task and epoch.

### Active phases

The active workflow phases are:

- `TOPIC`
- `PLAN(DOC)`
- `APPLY(DOC)`
- `PLAN(SRC)`
- `APPLY(SRC)`
- `DONE`

### Phase semantics

#### `TOPIC`

Exploration only.
No active manifest yet.

#### `PLAN(DOC)`

Draft the doc manifest:

- task refs
- scope
- acceptance criteria
- expected design/doc/task-surface changes

#### `APPLY(DOC)`

The doc manifest is sealed and is now being applied to doc-side managed surfaces.

#### `PLAN(SRC)`

Draft the src manifest based on settled doc-side state.

#### `APPLY(SRC)`

The src manifest is sealed and is now being applied to src-side managed surfaces.

#### `DONE`

The current phase workflow is complete and ready for archival/closure in historical storage.

### Forward transition commands

The phase workflow should center around explicit commands such as:

- `mock phase plan`
- `mock phase apply`
- `mock phase finish`

These are clearer than a generic `advance`.

### Authoritative forward transition table

| Current state | Command | Next state | Notes |
|---|---|---|---|
| `TOPIC` | `mock phase plan` | `PLAN(DOC)` | scaffold initial doc manifest |
| `PLAN(DOC)` | `mock phase apply` | `APPLY(DOC)` | validate and seal doc manifest |
| `APPLY(DOC)` | `mock phase finish` | `PLAN(SRC)` | complete doc application and scaffold src planning surface |
| `PLAN(SRC)` | `mock phase apply` | `APPLY(SRC)` | validate and seal src manifest |
| `APPLY(SRC)` | `mock phase finish` | `DONE` | complete src application |

This table exists to make planning/apply/finish semantics explicit.

### Transition commits

Semantic workflow transitions should be tool-owned and auto-committed.

This gives each transition:

- a canonical entrypoint
- a canonical recovery boundary
- a stable restore anchor
- obvious history markers

---

## Going backwards in phases

### Principle

Once you have entered `APPLY(...)`, going backwards through mockspace should remain intentionally expensive and explicit.

This is not accidental bureaucracy.

It preserves a meaningful rule:

- if the earlier plan could not survive application, that fact should remain visible
- going back should require explicit accounting
- the workflow should encourage better planning before entering apply

### No cheap workflow-level reopen path

The corrected model removes cheap workflow-level reopen semantics.

If someone truly made a zero-work accidental phase transition and wants to erase it cleanly, they can do so manually with raw git, knowingly, outside the workflow abstraction.

Inside mockspace, the meaning should stay consistent:

> once `APPLY(...)` has been entered, going back is always deprecating.

That boundary is cleaner.

### Backward command semantics

The user-facing backward command should be framed as re-entering planning:

- `mock phase replan`

But its semantics are always the meaningful path:

1. restore phase-owned managed surfaces to the apply-start snapshot
2. mark the old manifest deprecated
3. scaffold a fresh manifest
4. require explicit coverage/accounting of the old manifest in the new one
5. auto-commit the semantic transition

### Why deprecation remains essential

Deprecation is not ceremony for its own sake.

It preserves reviewable history:

- what the earlier plan said
- where it proved insufficient
- how the replacement plan differs
- whether the replacement truly accounts for what the previous one attempted

This is useful process history, not mere shame language.

### Restore phase-owned surfaces, not the whole branch

Replan should restore only the **phase-owned managed surfaces** to the apply-start snapshot.

Mockspace should **not** hard-reset the whole branch because:

- ordinary raw git commits remain allowed
- unrelated changes may exist outside the phase-owned surfaces
- whole-branch reset is too destructive

So the restore scope should be precise.

#### For DOC

Restore relevant doc-side managed surfaces such as:

- doc manifest
- topic document
- phase-owned design/doc/task surfaces covered by the phase contract

#### For SRC

Restore relevant src-side managed surfaces such as:

- src manifest
- phase-owned source/application surfaces covered by the phase contract

### The `APPLY(...)` transition commit is the restore anchor

This is one reason semantic transitions should auto-commit.

The transition into `APPLY(...)` creates the stable restore anchor for replan behavior.

### Messaging requirements

When mockspace blocks or performs replan, the messaging must be explicit.

It should explain:

- you entered `APPLY(...)`
- mockspace does not support cheap workflow-level reopen after apply
- the prior manifest is being deprecated
- phase-owned managed surfaces will be restored to the apply-start snapshot
- a new manifest is being scaffolded
- all prior manifest sections must be explicitly accounted for

This should be instructional, not terse.

---

## Active workflow storage model

### One active workflow area, not branch-nested storage

Because git branch already multiplexes repo state, active workflow artifacts do **not** need to live under branch-specific directories.

Instead:

- there is one active workflow area
- the current git branch determines which revision of that area you are seeing
- branch checkout naturally switches active workflow state by switching tracked repo content

This is the key architectural consequence of demoting branch.

### What belongs in the active workflow area

The active workflow area should hold the current live phase artifacts, such as:

- current topic document
- current doc manifest
- current src manifest
- any phase-local metadata needed for tooling
- current status/guidance surfaces

The exact physical layout can be refined later.

### What should not exist

The framework should avoid:

- branch-specific nested active workflow trees
- a durable branch identity directory hierarchy
- additional active workspace structures whose only purpose is to repeat git branch context

---

## Historical workflow archives

### Archived workflow artifacts

When a completed workflow is archived, the historical bundle should preserve:

- topic material
- manifests
- closure notes/accounting
- branch name as context
- timestamp and ordering information
- task refs addressed by the workflow
- outcomes such as completed, deferred, partial, or follow-up work

### Group archived workflow artifacts by epoch

Archived workflow artifacts should move into the current epoch package.

Inside that package, branch name may still be used as an organizational/readability key, but as historical context rather than as evidence of a first-class branch identity system.

### Historical readability matters more than theoretical normalization

The archive layout should be optimized for later humans asking questions like:

- what was this workflow trying to do?
- what branch context did it happen on?
- which tasks did it address?
- what replaced this deprecated manifest?
- what carried forward into the next epoch?

That is more important than building a perfectly normalized workflow store.

---

## Epochs

### Role of epochs

Epoch is a real concept and should remain one.

An epoch is a larger archival/compaction boundary above individual completed workflows.

### Why epochs are still useful

Without epochs, active-space historical material grows indefinitely.

Epoch gives mockspace:

- a periodic historical package
- a cleaner active workspace
- a place for retrospective/carry-forward material
- a meaningful boundary for grouped workflow history

### Epochs should stay lightweight at first

Epoch should not begin life as a large ceremonial release-management subsystem.

The framework should keep epoch real but lightweight:

- enough structure to compact and preserve history
- enough guardrails to keep boundaries meaningful
- not so much machinery that normal work becomes hostage to epoch process

### Active epoch working set

During an active epoch, the workspace contains:

- open task files
- namespace-local archived task Markdown since the previous epoch
- active workflow artifacts
- completed workflow archives accumulated since the previous epoch
- current design/topic material

### Epoch close

When an epoch closes, the framework should:

1. collect namespace-local archived task Markdown into the epoch package
2. collect completed workflow artifacts into the epoch package
3. require retrospective / carry-forward documentation coverage
4. reset the active rolling historical surfaces appropriately
5. begin the next epoch with a cleaner active workspace

### Preserve namespace structure inside epoch packages

Epoch packages should preserve namespace ownership structure for task archives rather than flattening everything into one undifferentiated dump.

That keeps historical inspection tractable.

---

## Relationship between epochs and branch context

### Completed workflows are routine

Archiving a completed workflow should remain relatively routine.

### Epoch close is broader

Epoch close is rarer and broader than routine workflow closure.

### Branch context remains visible in epoch history

Even though branch is demoted from first-class active artifact, epoch history should still preserve branch context clearly.

This preserves historical readability without requiring a branch durability subsystem.

### Work spanning epochs

Epoch boundaries should not leave historical ownership ambiguous.

If work must continue beyond an epoch boundary, the framework should ensure that carry-forward state is explicit and that the next epoch begins with clean active semantics.

The exact ergonomics can be refined later.

---

## Git discipline and workflow commits

### Ordinary git usage remains allowed

Raw git usage should remain allowed for ordinary work, including:

- normal local commits
- normal pushes
- read-only inspection commands
- manual recovery by expert users when they knowingly step outside the workflow abstraction

Mockspace should not try to replace git.

### Semantic workflow transitions are tool-owned

What mockspace should own are semantic transitions such as:

- `mock phase plan`
- `mock phase apply`
- `mock phase finish`
- `mock phase replan`
- task close/archive operations
- workflow archive/close operations
- epoch close operations

These should be:

- explicit
- validated
- linted
- auto-committed

### Not every commit maps to a task

The framework must not require every ordinary commit to correspond to a task.

Placeholder tasks created only to justify commits are undesirable.

### No broad git policing

Mockspace should not try to block all raw git behavior.

The right balance is:

- raw git remains available
- semantic workflow transitions are explicit and tool-owned
- strict guards exist around managed workflow semantics
- diagnostics clearly explain intended next commands and safe recovery paths

---

## CLI model

The tightened model suggests a CLI centered primarily on:

- `mock status`
- `mock phase`
- `mock task`
- `mock epoch`

### `mock status`

This should likely be the primary status entrypoint.

Without extra arguments, it should show things like:

- current git branch context
- current phase
- active manifest/topic state
- current epoch
- relevant task/workflow summary
- most likely next command
- recovery guidance if state is inconsistent

### `mock phase`

This command family owns phase lifecycle and phase-level status.

Without a subcommand, it should show:

- current phase
- active manifest/topic
- what surfaces are legal to edit
- next valid phase action
- replan guidance if relevant

Primary subcommands:

- `plan`
- `apply`
- `finish`
- `replan`

### `mock task`

This command family owns task lifecycle and task-level status.

Without a subcommand, it should show:

- open task summary
- current task context if derivable
- useful task refs and guidance
- archive/state information

Likely subcommands:

- `create`
- `close`
- `move`
- later query/inspect helpers as needed

### `mock epoch`

This command family owns epoch lifecycle and epoch-level status.

Without a subcommand, it should show:

- current epoch
- accumulated completed workflow history since epoch start
- archived-task rolling status
- epoch-close readiness/gaps
- likely next commands

Likely subcommands:

- `start`
- `close`

### `mock branch`

A `mock branch` command family may still exist as a thin helper/status alias around current git branch context.

But it no longer needs to be a primary heavyweight command family in the conceptual model.

That is an important shift.

---

## Lints and guards

This tightened model should keep lints strong.

Simpler storage is not a reason for weaker enforcement.

If the framework uses fewer heavy artifacts to encode convention, it should use **stronger guards and clearer diagnostics** to preserve workflow meaning.

### Task lints

- duplicate open task slug in the same namespace is forbidden
- invalid task ref syntax is forbidden
- unresolved task refs are forbidden where resolution is required
- malformed archived task records are forbidden
- illegal raw rename/move behavior should be blocked or diagnosed when identity-sensitive

### Manifest/phase lints

- manifests referencing unknown tasks are forbidden
- illegal edits to sealed manifests are forbidden
- illegal edits to deprecated manifests are forbidden
- illegal edits outside the current phase's managed surfaces are forbidden
- replacement manifests on replan must explicitly account for deprecated-manifest sections

### Workflow archive lints

- completed workflow archival with inconsistent task outcomes is forbidden
- archival with invalid manifest/task relationships is forbidden
- historical bundle layout should remain valid and inspectable

### Epoch lints

- epoch close without required retrospective/carry-forward coverage is forbidden
- epoch close with unhandled historical material is forbidden
- epoch close with inconsistent active state is forbidden

### Guidance-oriented diagnostics

Every block should aim to explain:

- what invariant failed
- what mockspace thinks the current situation is
- why that is blocked
- which command the user probably meant instead
- what the safest recovery path is

This is not cosmetic. It is part of the framework contract.

### Hook/agent implications

Hooks and agent-facing workflow guidance should be updated so that:

- task refs are first-class
- archived historical material is protected appropriately
- phase/task/epoch relationships are visible in guidance
- diagnostic output points toward intended commands and recovery paths
- strictness increases without increasing ambiguity

---

## Framework vs consumer split

This split must remain explicit.

### Framework concerns

Mockspace defines:

- namespace mechanics
- task mechanics
- open vs archived task storage model
- task ref syntax
- manifest/phase/task/epoch relationships
- phase model and transitions
- deprecating replan semantics
- workflow archival semantics
- epoch compaction semantics
- CLI capabilities
- lints and hooks
- transition and close preconditions

### Consumer concerns

Consumer repos define:

- which namespaces exist
- how namespaces are named
- how deep they are nested
- what epochs mean semantically in that project
- exact task metadata conventions beyond framework minimums
- editorial style of retrospective/carry-forward docs
- which managed surfaces belong to doc-side vs src-side application for that consumer

Mockspace should be strict about framework invariants and deliberately hands-off about namespace taxonomy.

---

## Migration direction

A likely migration direction is:

1. establish `mock/tasks/` as the namespace task tree
2. represent open tasks as Markdown files
3. represent closed tasks as namespace-local archived Markdown files
4. teach manifests to reference task refs
5. retain older backlog summary surfaces during migration if needed
6. rename `changelist` semantics conceptually to `manifest`
7. simplify active workflow storage to one active phase workspace rather than branch-nested active trees
8. retain epoch as a lightweight but real archival concept
9. update lints and hooks to reflect stricter guidance around the simpler model

This document does not prescribe exact migration order for every consumer repo.

---

## Decisions captured here

### D1 — the conceptual core is task, phase, manifest, epoch

These are the actual first-class workflow concepts.

### D2 — branch is demoted to contextual primitive

Git branch remains important as integration context, but is not modeled as a heavyweight first-class stored workflow object.

### D3 — active workflow artifacts live in one active phase workspace

The current git branch determines which revision of that workspace you are seeing.

### D4 — tasks are namespace-owned, not branch-owned

This prevents duplication and keeps long-lived work records stable.

### D5 — tasks are Markdown all the way through

Open tasks are Markdown files.
Archived tasks are Markdown files.
There is no JSONL archive model.

### D6 — manifests are the active phase contracts

They reference task refs and remain distinct from task records.

### D7 — phase transitions use `plan -> apply -> finish`

This is the primary active workflow lifecycle.

### D8 — once `APPLY(...)` is entered, going backwards through mockspace is always deprecating

There is no cheap workflow-level reopen path.

### D9 — replan restores phase-owned managed surfaces to the apply-start snapshot

Mockspace should restore the precise phase-owned surface set rather than hard-resetting the whole branch.

### D10 — deprecated manifests remain immutable

They are historical artifacts, not editable live plans.

### D11 — epochs are real but lightweight archival boundaries

They compact historical material without becoming an overbuilt governance subsystem.

### D12 — strict lints remain essential

If the framework uses lighter artifact/storage machinery, it must rely on stronger, clearer guards and diagnostics.

### D13 — the CLI center of gravity shifts toward status, phase, task, and epoch

Branch helpers may remain, but branch is no longer the primary conceptual command family.

### D14 — git remains the underlying history engine

Mockspace should use git, not duplicate it.

---

## Open questions for later implementation design

This document intentionally leaves some implementation details open.

### Q1 — exact open task file format

Frontmatter plus body? Another parseable Markdown shape? The framework requirements are defined here; exact syntax is deferred.

### Q2 — exact archived task filename convention

Timestamp plus slug? Another namespace-local scheme? The governing principle is Markdown archival readability, not a particular spelling.

### Q3 — exact active workflow directory layout

This document fixes the one-active-workspace model, not the final path schema.

### Q4 — exact historical workflow archive layout inside epochs

This document requires branch-context readability and manifest/topic preservation, not one exact directory tree.

### Q5 — exact command spelling for workflow archival/closure

The design requires the capability, but the final command naming can be refined later.

### Q6 — exact phase-owned managed-surface restoration mechanics

The design requires precise restore-to-anchor semantics; implementation details are deferred.

### Q7 — exact task move policy

The design requires identity-aware treatment; exact rewrite/supersession/tombstone mechanics are deferred.

### Q8 — exact task concurrency policy across parallel branch contexts

The framework should eventually define ownership/closure behavior more explicitly if needed.

### Q9 — exact epoch-close preconditions by default

The design requires meaningful guards and documentation coverage; specific defaults can evolve.

### Q10 — exact guidance and hook severity policy

The design requires strong enforcement and better messaging; exact severities remain an implementation concern.

---

## Summary

The corrected mockspace workflow model is:

- **tasks** are canonical, namespace-owned Markdown work records
- **phases** are the first-class active workflow lifecycle
- **manifests** are the sealed contracts that govern each phase
- **epochs** are lightweight but real historical compaction boundaries
- **git branch** is the ambient integration context, not a heavyweight stored workflow object

This means:

- active workflow should be phase-first rather than branch-first
- branch identity should come from git rather than a parallel workflow identity layer
- active workflow artifacts should live in one active workspace rather than branch-nested storage
- tasks should remain Markdown in both open and archived form
- going backwards after `APPLY(...)` should always be deprecating and history-preserving
- strict lints should remain and become more explicit, contextual, and instructive

That preserves the strongest ideas in mockspace while removing the parts that were drifting into workflow-database territory.