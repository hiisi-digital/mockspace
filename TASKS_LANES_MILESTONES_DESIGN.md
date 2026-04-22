# Namespace Tasks, Branches, Phases, and Epochs — design for mockspace

**Status:** proposed  
**Scope:** mockspace framework design  
**Audience:** mockspace maintainers and consumer repos adopting the workflow

## Purpose

This document proposes a new workflow layer for mockspace that introduces and connects five cooperating concepts:

- **tasks** — structured, versioned work items owned by a namespace
- **branches** — active branch-scoped integration bundles
- **phases** — the repeated planning/application cycle inside a branch
- **epochs** — larger culmination points that compact working history into archival packages
- **manifests** — active phase contracts that govern what a phase is allowed to do

The goal is to replace the current gap between:

1. unstructured or prose-heavy backlog notes inside `BACKLOG.md.tmpl`
2. active changelists that act as execution contracts
3. external, non-versioned task systems (for example assistant-local session task stores)

with a repo-native, structured, versioned workflow that integrates directly with mockspace's existing design-first, phase-driven model.

This design is for the **framework**, not for any one consumer repo's exact concern tree. The framework must support arbitrary namespace depth and arbitrary namespace naming without imposing a domain taxonomy.

---

## Problem statement

The current mockspace model is strong on **design governance** but weak on **structured work tracking**.

### Strengths of the current model

Mockspace already provides:

- a clear design-first phase machine
- explicit transition commands
- file-based, lintable design artifacts
- archival of closed work
- documentation generation
- git hooks and agent integration

These are valuable and must be preserved.

### Gaps in the current model

However, the current model does not provide a canonical, structured, versioned home for work items.

Current options are all insufficient:

- `BACKLOG.md.tmpl` is human-readable but not structured enough for robust tooling.
- changelists are excellent active-work contracts, but they are not the right place to be the long-lived canonical store of every work item.
- assistant-local task systems are structured, but are outside version control, repo-local workflow, and mockspace enforcement.

The result is fragmentation:

- future work lives in prose
- active work lives in changelists
- asynchronous task tracking lives outside the repo
- no unified repo-native reference system ties these together cleanly

This design addresses that gap without discarding the strongest parts of the current workflow.

---

## Design goals

### G1 — repo-native canonical work graph

Tasks must live in the repo, under `mock/`, versioned with the rest of the design and workflow artifacts.

### G2 — no framework-imposed concern tree

Mockspace must not dictate how consumer repos categorize their work. It should operate on arbitrarily nested **namespaces**.

### G3 — preserve the design-first contract model

The existing distinction between exploratory design work, sealed plans, and implementation/application work must remain intact.

### G4 — avoid duplicate active-work artifacts

The framework must not introduce a branch-local tasklist that duplicates manifest scope.

### G5 — clean active workspace, strong historical preservation

The active task surface should remain manageable. Closed work must remain referenceable later.

### G6 — machine-readable and human-readable without parallel canonical representations

There must be one canonical representation per concept. Derived views are acceptable; parallel editable sources of truth are not.

### G7 — support cross-cutting work

A branch may touch many namespaces and concerns. Tasks must remain namespace-owned, not branch-owned.

### G8 — support meaningful larger archival boundaries

Mockspace needs a notion of an **epoch** as a larger culmination boundary beyond individual branch closure.

### G9 — improve discoverability and guidance

The framework should guide the user toward the right command from the current situation, with explicit and actionable block messages.

### G10 — preserve strong shame / supersession semantics

Going backwards after real work has begun should remain intentionally expensive and explicit. The framework should preserve the current discipline that a bad plan must be deprecated and replaced, not silently rewritten.

---

## Non-goals

This design does **not** define:

- a required namespace taxonomy for any consumer repo
- a required number of namespace levels
- exact consumer-facing concern names such as `compiler/ir` vs `frontend/ir`
- detailed implementation of rendering/generation internals
- exact migration steps for each current consumer repo
- exact commit message wording for all transitions
- exact release automation or CI/CD integration behavior

Those are consumer concerns or follow-up implementation concerns.

---

## Vocabulary

### Namespace

A namespace is an arbitrarily nested path under the task tree.

Examples:

- `compiler/ir`
- `runtime/abi`
- `cross-cutting/lints`
- `engine/plan/analysis`

Mockspace treats namespaces as opaque nested names. It does not interpret their semantics.

### Task

A task is a structured work item owned by a namespace.

Tasks are canonical work records. They may be:

- open
- deferred
- blocked
- in-progress
- or otherwise active in the current epoch

Closed tasks are archived.

### Branch

A branch is the branch-scoped active integration bundle.

Here, "branch" means the mockspace workflow object that maps to the current git branch. This is intentionally aligned 1:1 with git branch identity.

A branch is the unit of:

- branch identity
- active topic/design work
- active manifests
- phase transitions
- closure into branch history

A branch may address tasks from many namespaces.

### Phase

A phase is one step in the repeated planning/application cycle inside a branch.

The branch moves through these active phases:

- `TOPIC`
- `PLAN(DOC)`
- `APPLY(DOC)`
- `PLAN(SRC)`
- `APPLY(SRC)`
- `DONE`

Then, separately, the branch may be closed and archived.

### Epoch

An epoch is a larger culmination boundary.

An epoch is **not** every release. It is a landmark-level closure event that compacts:

- rolling closed task archives
- closed branch/design artifacts
- retrospective and carry-forward design material

into an epoch package, leaving a cleaner active workspace.

### Manifest

This document uses **manifest** as the clearer long-term name for the active phase contracts currently represented by `changelist.doc` and `changelist.src`.

The current naming may remain during migration, but conceptually these are manifests.

- `doc manifest` — active documentation/design-side phase contract
- `src manifest` — active source/application phase contract

### Topic document

A topic document remains an exploratory design document.

It is not the canonical task store.
It is not the canonical active implementation contract.

It is design exploration and rationale.

### Deprecated manifest

A deprecated manifest is a previously sealed or meaningful plan that has been superseded because the earlier plan was not good enough and real work had already begun against it.

Deprecation is not the same thing as a cheap accidental undo.

Deprecated manifests are historical records and remain immutable.

---

## Core conceptual split

Mockspace should model the workflow with three orthogonal axes.

### 1. Standing project surfaces

These are long-lived concern/domain surfaces:

- crate `DESIGN.md.tmpl`
- `README.md.tmpl`
- `BACKLOG.md.tmpl` during migration
- `research/`
- `benches/`

They answer: what is this part of the project, what should it become, what principles govern it?

### 2. Structured work graph

This is the new `mock/tasks/` surface.

It answers:

- what work items exist?
- which namespace owns them?
- what depends on what?
- what is open or deferred?
- what has closed during the current epoch?

### 3. Active integration workflow

This is the branch + phase + manifest + topic system.

It answers:

- what branch-scoped bundle is active?
- what design is being discussed?
- what tasks is this phase allowed to act on?
- what was bundled together and closed together?

This split is the key to avoiding redundancy.

---

## High-level workflow model

### Canonical task ownership

Tasks are namespace-owned.

They do **not** belong to a branch as their primary identity.

A branch may reference and act on tasks, but it does not own them.

### Canonical active-work contract

Active work inside a branch is governed by manifests.

A manifest references task refs from the namespace task tree.

A manifest does **not** duplicate the full task description body.

### Canonical history of a task

A task's long-lived history stays with its namespace.

A branch archive preserves the grouping and execution history of the branch, but not the canonical task record body.

### Canonical history of a branch

A branch archive preserves:

- topics
- manifests
- closure metadata
- which task refs were addressed
- which were completed, partial, deferred, or spawned as follow-ups

### Canonical historical compaction boundary

An epoch package preserves:

- all closed branches since the previous epoch
- all rolling namespace task archives since the previous epoch
- epoch retrospective / carry-forward design docs

---

## Namespace task tree

### Framework-level rule

Mockspace only knows that `mock/tasks/` contains nested namespaces.

The namespace tree may be any depth, with any names.

The framework enforces mechanics, not taxonomy.

### Open tasks

Open tasks are one file per task.

This is deliberately optimized for active work:

- visible in the tree
- easy to open directly
- easy to edit
- easy to move between namespaces if still open
- naturally grep-friendly

### Closed tasks

Closed tasks do not remain as files.

Each namespace directory contains one rolling archive file:

- `archive.jsonl`

This archive contains full self-contained task snapshots for tasks closed during the current epoch.

This is deliberately asymmetric with open tasks:

- open work is optimized for editing and visibility
- closed work is optimized for compact, append-friendly preservation

This asymmetry is intentional and desirable.

### No global archive

There is no repo-wide task archive.

That would recreate a giant undifferentiated task graveyard.

All task closure remains namespace-local until epoch compaction.

---

## Task identity and references

### Canonical task identity

Task identity is:

- namespace path
- plus stable task slug

Example conceptual refs:

- `compiler/ir#structural-robust-ir`
- `runtime/abi#stabilize-zig-c-boundary`
- `cross-cutting/lints#no-bare-option`

Numeric ids are not required.

### Stable slug principle

The slug is identity, not presentation.

Therefore:

- the filename slug must be stable once created
- the human-facing title may change independently in the task metadata/body
- if a change is so large that the slug would want to change materially, that should usually be a new task or a superseding task, not a rename

### Uniqueness rule

Within the same namespace directory, open task slugs must be unique.

Mockspace should lint this and block invalid duplicates.

Across different namespaces, duplicate slugs are allowed by framework rules because the namespace path disambiguates them, though consumers may adopt stronger conventions if desired.

### Reference resolution

Mockspace must support task refs across:

- open task files in `mock/tasks/`
- rolling `archive.jsonl` files in active namespaces
- archived epoch task archives

The ref syntax must remain stable even when storage location changes due to archival.

This is why refs must be logical (`namespace#slug`) rather than path-based.

---

## Task representation

### Open task representation

Open tasks are Markdown files.

They should be both human-readable and machine-readable.

The exact file format may evolve in implementation design, but the framework-level requirements are:

- stable slug identity derived from filename
- structured metadata parseable by mockspace tooling
- human-readable body content
- support for tags/labels
- support for dependency refs
- support for branch association when active
- support for epoch provenance metadata when helpful

### Closed task representation

Closed tasks are appended as full self-contained records into `archive.jsonl` in the same namespace directory.

Each archived record must include:

- canonical task ref
- namespace
- slug
- title
- all frontmatter-equivalent structured metadata
- full body content
- closure metadata:
  - resolution kind
  - closed at
  - closed by branch
  - closed by manifest/phase if relevant
  - epoch at time of closure if available

Archived records must not merely point to vanished open files. They must be self-contained.

---

## Task state model

At the framework level, task states should be minimal and meaningful.

### Open-side states

The exact set can be refined later, but framework semantics should support distinctions such as:

- open
- deferred
- blocked
- in-progress

All of these remain open-side task files.

### Closed-side resolutions

Closed tasks should archive with a resolution kind such as:

- completed
- cancelled
- superseded
- wontfix

These are archived records, not open files.

### Reopening policy

Archived tasks should remain append-only historical records.

If previously closed work needs to re-enter active work, that should generally be a new open task that references the archived one, not mutation of the archive entry.

This keeps archives reliable and append-only.

---

## Branches

### Role of branches

A branch is the active branch-scoped integration bundle.

The branch is the unit of:

- git branch identity
- active topic/design documents
- active manifests
- phase transitions
- eventual closure into branch history

A branch may touch many namespaces and many tasks.

### Branches do not own tasks

This is a critical rule.

If branch-local tasklists were introduced as canonical active work stores, they would duplicate manifest scope and overlap with the current design-first contract model.

This design avoids that.

Instead:

- branches own the active workflow bundle
- manifests name the task refs in scope
- tasks remain canonical under namespaces

### Branch identity must derive from git branch identity

This is a key robustness rule.

Mockspace must not depend on a separately mutable hidden "current active branch" pointer inside `mock/`.

Instead:

- current git branch determines current mockspace branch identity
- branch state and status must resolve from that identity
- manual raw git checkout must be survivable, even if it is not the preferred workflow
- branch mismatches must produce clear diagnostics and recovery guidance

This rule exists because raw checkout/switch cannot be as reliably pre-blocked as commit/push.

### Branch closure

When a branch closes:

- branch history is archived in the current epoch working set
- tasks referenced by that branch may resolve in one of several ways:
  - completed -> move into namespace `archive.jsonl`
  - cancelled/superseded -> move into namespace `archive.jsonl`
  - partially addressed -> remain open and updated
  - deferred -> remain open
  - split into follow-ups -> old task may close, new tasks may open

This means not every task touched by a branch necessarily closes with it.

---

## Phases

### Why phases are first-class

Once branch lifetime and phase lifetime are separated, command semantics become much clearer.

`branch close` should mean branch closure.
It should not also mean "finish the current doc or src work."

Therefore phases deserve their own first-class command family.

### Active branch phases

The active branch lifetime is:

- `TOPIC`
- `PLAN(DOC)`
- `APPLY(DOC)`
- `PLAN(SRC)`
- `APPLY(SRC)`
- `DONE`

Then, separately:

- branch closure/archive

### Why `APPLY(...)` is preferred over `IMPL(...)`

`IMPL(SRC)` sounds natural, but `IMPL(DOC)` is awkward and too code-biased.

What happens in the active post-plan phase is better described as:

- applying a sealed plan to the phase-owned managed surfaces

This is true for both doc-side and src-side work.

So `APPLY(DOC)` and `APPLY(SRC)` are the preferred conceptual names.

### Phase semantics

#### `TOPIC`
Exploration only.
No active manifest yet.

#### `PLAN(DOC)`
Draft the doc manifest:
- task refs
- scope
- acceptance criteria
- what documentation/design/task surfaces are expected to change

#### `APPLY(DOC)`
The doc manifest is sealed.
The plan is now being applied to doc-side managed surfaces.

#### `PLAN(SRC)`
Draft the src manifest based on the now-settled doc state.

#### `APPLY(SRC)`
The src manifest is sealed.
The plan is now being applied to src-side managed surfaces.

#### `DONE`
The active branch lifecycle is complete.
The branch is ready to close/archive.

### Phase transitions

The forward phase transition commands are:

- `mock phase plan`
- `mock phase apply`
- `mock phase finish`

These should be the primary transition verbs, not `advance`.

`advance` may exist later as convenience, but should not be the main documented command because it hides too much intent.

### `mock phase plan`

This opens/scaffolds the next planning manifest.

Valid transitions:
- `TOPIC -> PLAN(DOC)`
- `APPLY(DOC) -> PLAN(SRC)`

This is a semantic workflow transition and should auto-commit by default.

### `mock phase apply`

This validates and seals the current manifest, then begins the matching apply phase.

Valid transitions:
- `PLAN(DOC) -> APPLY(DOC)`
- `PLAN(SRC) -> APPLY(SRC)`

This replaces the confusing old "freeze/lock" framing.

This is a semantic workflow transition and should auto-commit by default.

### `mock phase finish`

This completes the current apply phase and moves forward.

Valid transitions:
- `APPLY(DOC) -> PLAN(SRC)`
- `APPLY(SRC) -> DONE`

From `APPLY(DOC)`, the transition should also scaffold the next src planning surface so that `PLAN(SRC)` begins concretely.

This is a semantic workflow transition and should auto-commit by default.

---

## Going backwards in phases

### The principle

Going backwards after real work has begun should remain intentionally expensive.

This is not accidental bureaucracy. It encodes an important discipline:

- if a plan was too poor to survive application, that fact should be recorded
- revisiting the plan should require explicit accounting
- the cost of going back should encourage better planning before entering `APPLY(...)`

This existing mockspace value must be preserved.

### Two backward paths

There should be two backward behaviors, selected automatically based on what happened after `APPLY(...)` began.

#### A. Cheap reopen
If nothing meaningful has been done since entering `APPLY(...)`, then going back should be cheap.

This is the "legitimate accident" path.

If mockspace can prove that no real work occurred, it may simply reopen the manifest and return to planning without deprecating it.

#### B. Deprecating replan
If anything meaningful has been changed or written during `APPLY(...)`, then going back should require deprecation.

This is the "you did not plan well enough" path.

The old manifest is deprecated, a new one is created, and the new one must explicitly account for all prior manifest sections and explain how the new one differs and why.

### Why deprecation remains essential

Deprecation is the only meaningful way to preserve the history of a bad plan that was already being applied.

This is not vague shame. It is useful reviewable process history.

The current lints that require each deprecated section to be accounted for in the replacement manifest are valuable and should remain.

### Backward command semantics

The user-facing backward command should not be framed primarily as `deprecate`.

It should be framed as re-entering planning.

A strong conceptual command is:

- `mock phase replan`

Its semantics are:

- if no meaningful work happened after `APPLY(...)` began, reopen the existing manifest cheaply
- if meaningful work did happen, automatically deprecate the old manifest and scaffold a new one

So `deprecate` survives as artifact semantics, even if the user-facing command becomes `replan`.

### Cheap reopen conditions must be conservative

Cheap reopen should only be allowed if mockspace can prove that no real apply work happened.

That should be conservative. If in doubt, the heavier deprecating path should be required.

### What happens on deprecating replan

When real work has happened and the phase must go back to planning, the framework should:

1. restore the phase-owned managed surfaces to the exact snapshot from when `APPLY(...)` began
2. mark the old manifest deprecated
3. create a fresh new manifest scaffold
4. require explicit coverage/comparison of all old manifest sections in the new one
5. auto-commit this semantic transition

### Restore phase-owned surfaces, not the whole branch

The old behavior of nuking or broadly resetting is too blunt.

The framework should **not** hard-reset the whole branch, because:

- raw git commits remain allowed for ordinary work
- there may be unrelated changes outside the phase-owned surfaces
- full-branch reset is too destructive

Instead, deprecating replan should restore only the **phase-owned managed surfaces** to the `APPLY(...)`-start snapshot.

This is the right behavior for both doc and src phases.

#### For DOC
Restore:
- doc-side manifests
- topic/task/doc managed surfaces relevant to the phase
- not "wipe docs entirely"

#### For SRC
Restore:
- src-side manifests
- source-side managed surfaces relevant to the phase
- not more than necessary

### The `APPLY(...)` auto-commit is the restore anchor

This is another reason semantic transitions must auto-commit.

The auto-commit created by `mock phase apply` becomes the reliable restore anchor for:

- cheap reopen checks
- deprecating replan restore behavior
- predictable user recovery

### Messaging requirements for replan blocks

When mockspace refuses a cheap reopen and requires deprecating replan, it must explain this very clearly.

The message should convey:

- you entered `APPLY(...)`
- meaningful changes were made
- therefore this was not an accidental advance
- going back now deprecates the earlier manifest
- a new manifest will be scaffolded
- all prior manifest sections must be explicitly accounted for in the new one
- this exists because the earlier plan was insufficient

This should be instructional, not terse.

---

## Manifests (current changelists)

### Rationale

The current changelist system should evolve conceptually into a manifest system.

This is largely a naming and framing clarification:

- topic docs = exploration
- manifests = active contract

### Why manifests should remain separate from tasks

Tasks and manifests are related but not the same.

Tasks answer:
- what work item is this?
- what does it mean?
- what depends on it?
- which namespace owns it?

Manifests answer:
- what subset of tasks is in scope for this phase?
- what files/surfaces are allowed to change?
- what is explicitly out of scope?
- what are the phase-bound acceptance constraints?

Combining them would either:
- make tasks too rigid
- or make manifests too weak

Therefore they remain separate.

### Manifest references

Manifests should reference task refs instead of duplicating full task bodies.

A manifest becomes a phase-authorized view over the task graph.

This is the key non-redundant integration between tasks and the existing mockspace phase system.

### Doc/src separation

The existing separation between doc and src work remains intact.

Tasks are not split structurally by phase.

Instead, tasks are namespace-owned and may be addressed in one or both phases.

Manifests remain phase-specific:
- doc manifest
- src manifest

This preserves one of the best properties of the current mockspace model.

### Deprecated manifests remain immutable

Once a manifest is deprecated, it remains an immutable historical artifact.

It must not be silently edited or rewritten.

---

## Epochs

### Role of epochs

An epoch is the larger culmination boundary above branches.

Epochs are not routine releases. They are landmarks.

### Why epochs are needed

Without epochs, rolling archives and archived branch/design artifacts would grow indefinitely in the active workspace.

Epochs give mockspace a clean compaction boundary.

### Active epoch working set

During an active epoch, the workspace contains:

- open namespace task files
- rolling namespace `archive.jsonl` files
- active branches
- closed branch/design artifacts accumulated since the previous epoch
- current design/topic work

### Epoch close

When an epoch closes:

1. all rolling namespace `archive.jsonl` files are moved into the epoch package
2. all closed branch/design artifacts accumulated since the previous epoch are moved into the epoch package
3. epoch retrospective / carry-forward design documentation is written and validated
4. rolling archives in the active workspace are reset
5. the next epoch begins with a cleaner active workspace

### Epoch package contents

An epoch package should preserve:

- archived branch artifacts
- archived namespace task archives, preserving namespace structure
- epoch summary / retrospective / carry-forward design docs
- any `.meta` / `.history` sidecars or equivalent branch closure metadata

The epoch package must remain referenceable later as a meaningful historical slice.

### Preserve namespace structure inside epoch packages

Epoch packages must not flatten task archives into one giant file.

They should preserve namespace ownership structure so that one can still inspect the task history of a concern within an epoch package.

---

## Epoch close preconditions

Epoch close should be a guarded, lint-enforced workflow action.

An epoch should not close unless:

- all branches intended for that epoch are closed or explicitly handled by policy
- epoch retrospective / carry-forward design docs exist
- every archived branch/round since the previous epoch is accounted for in the epoch-close documentation set
- rolling task archives are ready for compaction
- any configured sanity checks pass

### Coverage, not necessarily monolith

The framework should require **coverage** of archived branch/design history in epoch-close docs.

It should not require one enormous monolithic summary file if the documentation set can be structured more cleanly.

This lets consumer repos choose whether they want:
- one epoch retrospective
- one overview plus per-domain carry-forward docs
- or another equivalent covered structure

Framework concern is coverage and linkage, not editorial layout.

---

## Relationship between epochs and branches

### Branch close is routine
A branch close should remain a relatively routine operation.

### Epoch close is ceremonial
An epoch close is rarer, broader, and more constrained.

This distinction is important. Epochs should not be so common or heavy that they impede ordinary work.

### Work spanning epochs

An epoch should not close with ambiguous active branch ownership.

The framework should define a policy such as:

- an epoch cannot close with unresolved branches belonging to it
- if work must continue across epochs, a successor branch is opened in the next epoch and the old branch is closed appropriately

The exact consumer-facing ergonomics can be refined later, but the framework must preserve clean epoch boundaries.

### Optional tags and release automation

Epoch close is a natural future integration point for:

- auto-tags
- release automation
- CI/CD workflows

These are worthwhile possibilities, but they are follow-up implementation concerns, not core framework semantics in this document.

---

## Git discipline and workflow commits

### Ordinary git usage remains allowed

Raw git usage should remain allowed for ordinary developer work, including:

- normal local commits
- normal pushes
- read-only git inspection commands

Mockspace should not try to replace normal git entirely.

### Semantic workflow transitions are tool-owned

What mockspace **should** own are semantic workflow transitions that change the meaning of the workflow state.

Examples:

- `mock phase plan`
- `mock phase apply`
- `mock phase finish`
- `mock phase replan`
- `mock task close`
- `mock branch close`
- `mock epoch close`

These should be:

- explicit commands
- validated
- linted
- auto-committed

This gives each semantic transition:
- one canonical entrypoint
- one canonical recovery boundary
- one obvious history marker

### Not every commit maps to a task

This is important.

The framework must not require that every ordinary git commit corresponds to a task.

Placeholder tasks created only to justify commits are undesirable.

This is one of the reasons raw git commits remain allowed.

### Transition commits vs ordinary developer commits

Mockspace should distinguish conceptually between:

#### Ordinary developer commits
Regular git commits during day-to-day work.

#### Workflow transition commits
Auto-generated by mockspace for semantic transitions.

This distinction should be visible in documentation, messaging, and perhaps later in implementation metadata.

### No broad git policing

Mockspace should not try to block all raw git behavior.

The right balance is:

- keep raw git available for ordinary work
- keep strict hooks and lints around managed-path semantics
- make semantic transitions explicit and tool-owned
- provide much better diagnostics and guidance when blocked

---

## CLI model

This design implies a refined mockspace CLI model built around four first-class nouns:

- `mock branch`
- `mock phase`
- `mock task`
- `mock epoch`

### `mock branch`

This command family owns branch lifecycle and branch-level status.

Without a subcommand, `mock branch` should show:

- current git branch
- whether a corresponding mockspace branch workspace exists
- current phase
- active manifests/topics
- current epoch
- most relevant next command
- recovery guidance if branch/workspace state is inconsistent

Likely subcommands:

- `start`
- `switch`
- `close`

The exact command set can be refined later, but branch should be the branch-level noun.

### `mock phase`

This command family owns repeated phase-cycle transitions and phase-level status.

Without a subcommand, `mock phase` should show:

- current phase
- what surfaces are legal to edit right now
- which manifest/topic is active
- the next valid phase action
- recovery guidance if the state is invalid or inconsistent

Primary subcommands:

- `plan`
- `apply`
- `finish`
- `replan`

The exact command set can be refined later, but phase should be the phase-level noun.

### `mock task`

This command family owns task lifecycle and task-level status.

Without a subcommand, `mock task` should show:

- current task context if derivable
- summary of open task state
- useful task commands
- guidance on refs, archives, and namespace ownership

Likely subcommands:

- `create`
- `close`
- `move`
- later query or inspect commands as needed

The exact command set can be refined later.

### `mock epoch`

This command family owns epoch lifecycle and epoch-level status.

Without a subcommand, `mock epoch` should show:

- current epoch
- closed branches accumulated since epoch start
- rolling task archive status
- epoch-close preconditions
- most relevant next commands

Likely subcommands:

- `start`
- `close`

The exact command set can be refined later.

### Important CLI design rule

The CLI should operate on framework primitives:

- namespaces
- tasks
- branches
- phases
- manifests
- epochs

It must not assume a consumer repo's concern taxonomy.

---

## Lints and guards required at framework level

This feature requires first-class lints and guards.

The old lint ideas are still valuable, but they should be retargeted to the new vocabulary and artifact model.

### Namespace/task lints

- duplicate open task slug in same namespace is forbidden
- invalid task ref syntax is forbidden
- unresolved task refs are forbidden where resolution is required
- archived/open task shape validity
- malformed archive records are forbidden
- illegal task rename rules if slug-as-identity is used

### Manifest/phase lints

- manifests referencing unknown tasks are forbidden
- illegal edits to sealed manifests are forbidden
- illegal edits to deprecated manifests are forbidden
- branch/phase surface gating must remain strict
- deprecating replan must preserve supersession accounting
- replacement manifests must cover all required deprecated-manifest sections

### Branch lints

- current git branch must resolve cleanly to a mockspace branch workspace
- branch close with inconsistent task outcomes is forbidden
- branch close with invalid manifest/task relationship is forbidden

### Epoch lints

- epoch close without retrospective/carry-forward docs is forbidden
- epoch close without full branch coverage is forbidden
- epoch close while rolling archives remain unhandled is forbidden
- epoch close with active branches violating policy is forbidden

### Guidance-oriented diagnostics

This redesign is a chance to improve messages substantially.

Every block should aim to explain:

- what invariant failed
- what mockspace believes the current situation is
- why this is blocked
- which command the user probably meant to run instead
- what the safest recovery path is

This is a framework requirement, not a cosmetic nice-to-have.

### Hook/agent workflow implications

Built-in agent rules and hooks should be updated so that:

- task refs are recognized as first-class workflow entities
- illegal edits to archived epoch material can be blocked or warned
- epoch-close preconditions are surfaced before destructive transitions
- branch/phase/task relationships are visible in generated guidance
- diagnostic output always points toward the intended command and workflow path when derivable

This belongs at the mockspace framework level.

---

## Framework vs consumer split

This split must remain explicit.

### Framework concerns

Mockspace defines:

- what a namespace is, mechanically
- what a task is, mechanically
- open vs closed storage model
- reference syntax
- branch/task/manifest/epoch relationships
- phase model and transitions
- deprecating replan semantics
- archival and compaction semantics
- CLI operations
- lints and hooks
- transition and close preconditions

### Consumer concerns

Consumer repos define:

- what namespaces exist
- how they are named
- how deep they are nested
- which namespaces correspond to crates, domains, concerns, sub-concerns
- what epochs mean semantically in that project
- exact task metadata conventions beyond framework minimums
- editorial style of retrospective/carry-forward docs
- which managed surfaces belong to doc-side vs src-side application in that consumer

Mockspace must be strict about framework invariants and deliberately hands-off about namespace taxonomy.

---

## Migration direction

A likely migration sequence for consumers is:

1. add `mock/tasks/` with namespace task registers
2. introduce task ref syntax and linting
3. teach active manifests to reference task refs
4. retain existing `BACKLOG.md.tmpl` during migration
5. gradually move structured work items out of prose backlog into namespace task files
6. keep `BACKLOG.md.tmpl` as a summary or derived/documentary surface rather than canonical structured source
7. rename design-round concepts into branch/phase concepts in the updated mockspace workflow
8. introduce epochs in the updated workflow
9. compact current archived round history into the first epoch package when appropriate

This document does not prescribe the exact migration order for each consumer repo.

---

## Decisions captured here

### D1 — tasks are namespace-owned, not branch-owned
This prevents duplication with manifests and makes task history stable across branches.

### D2 — open tasks are files, closed tasks are namespace-local rolling archive records
This optimizes each state for its actual use.

### D3 — task identity is namespace path + stable slug
No global numeric id is required at framework level.

### D4 — manifests reference tasks instead of duplicating them
This integrates tasks into the current design-first workflow without creating parallel active-work artifacts.

### D5 — branches are the branch-scoped integration units
This aligns the workflow object with the real git primitive rather than a more abstract lane/workstream term.

### D6 — phases are first-class and use `plan -> apply -> finish` semantics
This clarifies the repeated branch-internal cycle and removes the awkward `freeze/lock` framing.

### D7 — going backwards after real work triggers deprecating replan
This preserves the workflow's intentional shame/accounting semantics.

### D8 — deprecating replan restores phase-owned managed surfaces to the apply-start snapshot
This replaces broad nuking or whole-branch hard reset with a more precise and safer restore model.

### D9 — epochs are archival compaction boundaries
They collect rolling closed-task archives and closed branch/design artifacts into meaningful historical packages.

### D10 — framework only knows namespaces mechanically
Namespace taxonomy is a consumer concern.

### D11 — semantic workflow transitions are tool-owned and auto-committed
Ordinary git commits remain normal git commits; semantic state transitions get explicit CLI commands and recovery boundaries.

### D12 — guidance quality is a framework requirement
Strictness is only useful if the tool clearly explains the expected workflow and the likely intended next step.

---

## Open questions for follow-up implementation design

This document intentionally leaves some questions open for later implementation design passes.

### Q1 — exact open task file format
Markdown heading/body plus frontmatter? Another parseable shape? The framework requirements are defined here, but exact syntax is deferred.

### Q2 — exact archive record schema
JSONL is selected as direction, but the exact field schema can be defined in implementation design.

### Q3 — exact branch directory/artifact shape
This document fixes semantics, not final physical branch layout.

### Q4 — exact epoch package layout
This document requires preservation of namespace structure and branch/design history, but not the final path schema.

### Q5 — exact CLI names and subcommand tree
This document defines required capabilities, not final command spelling.

### Q6 — exact lints and severities by default
This is an implementation/policy pass.

### Q7 — exact cheap-reopen detection heuristics
The design requires conservative cheap-reopen behavior, but exact implementation criteria are deferred.

### Q8 — exact phase-owned managed-surface restoration mechanics
The design requires path-scoped restore-to-anchor semantics, but exact implementation details are deferred.

---

## Summary

The proposed mockspace workflow model is:

- **tasks** are canonical, structured work items owned by arbitrary nested namespaces
- **branches** are active branch-scoped integration bundles that reference tasks through manifests
- **phases** are first-class and follow a repeated `plan -> apply -> finish` cycle inside each branch
- **epochs** are larger culmination boundaries that compact rolling task and branch archives into meaningful historical packages
- **manifests** remain the active phase contracts and do not collapse into tasklists
- **deprecated manifests** remain the correct historical consequence of going backwards after real work has begun
- **namespaces** are arbitrary and consumer-defined; mockspace enforces mechanics, not taxonomy
- **semantic workflow transitions** are explicit CLI actions with auto-commits and strong recovery boundaries
- **ordinary git usage** remains normal, while hooks/lints/messages become more instructive and workflow-aware

This preserves the strongest parts of the existing mockspace design-first workflow while adding the missing structured, versioned, repo-native task graph and larger epoch compaction model needed for long-lived project management.