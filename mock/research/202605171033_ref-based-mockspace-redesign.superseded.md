# Ref-based mockspace redesign

**Status:** proposal / research, self-contained
**Authored:** 2026-05-17
**Revisions:** seven review passes folded in (architectural, tooling-lifecycle, operational-scale, standalone-completeness, soundness-audit, security-audit, security-audit + fresh-eyes-package-management pass)

> This document is a self-contained specification for the mockspace tool.
> No external reading is required to understand or implement what is
> described here. Historical design documents may exist in the
> repository as audit-trail context but are not required and should not
> be consulted for implementation; this document supersedes them.

---

## TODO: framing realignment pending

This doc as it stands captures the **storage architecture** (ref-based
state, lockfile shape, signing model, transition atomicity, etc.) but
**misframes the product**. The user's articulation of the actual heart
of mockspace, captured 2026-05-17, is:

### The heart (load-bearing principle)

Mockspace is documentation-first development discipline. Source lies,
documentation lies; you have to pick which one is the truth and
rigidly enforce that the other follows. Mockspace picks documentation.
Lints, hooks, and agent integrations make any other state literally
fail to commit, build, or push.

**Source ALWAYS follows docs. Never more, never less, never different.**
If implementation reveals docs were wrong, the docs get rewritten.
Source never deviates first.

### The workflow heart

Design takes several rounds of discussions, recorded as topic files —
historically often running over 10 topic discussion transcripts before
the first doc manifests are attempted to be consolidated (the
establishing-the-framework usage); short rounds are also a valid and
the current main usage, but the principle is the same. Almost always
includes several **bench rounds** to confirm or potential design
decisions; **sketching** to quickly check certain API assumptions
hold true; **research** done on the side, cross-referenced as
appropriate across the topic transcripts and docs.

Once docs are consolidated based on these discussions, research,
benches, sketches and all, the docs are implemented into **design
templates at several distinct levels**:

- **README** — repo-level summary
- A short-form description (one paragraph) for the "crate" / "domain"
  / etc. — assembled into the overall design files
- **Design doc** — the regular top-level design
- **Arbitrary deep-dive docs** per domain or topic within the design

All naturally generated into sophisticated publish-ready documents,
templated for consistency and easy maintenance. Overall design files
are constructed from the individual crate/domain one-paragraph
descriptions. The pipeline also generates a **dependency graph and a
visual presentation** of it to go with the docs.

### The enforcement heart

Once docs are locked in, **lints, git hooks, custom hooks, and agent
integrations** (rules, skills, hooks per AI tool) ensure that there
literally can not be any drift between the docs and the source.

At the centre: **convention lints** — configurable conventions,
principles, and workflows, such as:

- maximum lines per individual source file
- disallow certain APIs
- ensure consistent naming conventions
- check for existing similarly-named things before allowing to commit
  to some new added function or class (in case there's already
  something that could be refactored to be reusable, a shared
  abstraction between the two, etc.)

Lints fire at **configurable severities** and at **configurable gate
levels** (build, commit, push). Convention lints, doc-source-drift
lints, primitive-vocabulary lints, no-bare-type lints, custom project
lints — all run on the same machinery; all gate-able.

### What is and isn't part of the redesign

Mockspace stays what it is today (a docs-first discipline tool with
the above workflow). We're rewriting underneath for maintainability,
restructuring for clarity, improving the storage layer (refs), and
formalising the different domains within the framework. **The way it
is used stays.** Short rounds, long rounds, the bench/sketch/research
interleave, the multi-tier doc generation, the convention-lint heart —
all preserved.

### What the current doc misframes or misses

Concrete revision targets, in priority order:

1. **No §0 "The heart" section.** Need this BEFORE §Motivation,
   capturing the docs-as-truth principle, the multi-tier workflow
   activity (research, benches, sketches, topic transcripts), the
   multi-tier doc generation pipeline, the lint-driven enforcement,
   convention lints as the centre.

2. **§Motivation leads with "ref-based redesign" (storage)** rather
   than with the docs-as-truth product principle. Storage is the
   means; discipline is the end.

3. **§Workflow model leads with the six-phase chain** without
   explaining that TOPIC is where most of the actual work happens
   (multi-round discussion, research, benches, sketches). The
   downstream phases are consolidation, not the activity.

4. **Bench framework as first-party isn't elevated.** Currently a
   passing reference in §Vocabulary + storage area. Should be a
   first-class workflow stage with its own section ("Benches:
   confirming or refuting design assumptions, integrated into the
   topic phase").

5. **Sketches similarly under-elevated.** Same shape: storage area
   only. Should be "Sketches: quick API-assumption verification
   between research, design, and benchmarking."

6. **Research workflow same.** `refs/mock/research/<slug>` exists in
   storage; the activity isn't named at the top-level.

7. **Multi-tier doc generation isn't framed as a unified concern.**
   The doc says "render pipeline three targets" (source-tree /
   local-only / forge API) but never describes the per-crate
   one-paragraph → README → design doc → deep-dive → assembled
   publish-ready set pipeline. Templates, consistency, dependency
   graph + visualisation generation are absent or scattered.

8. **Convention lints aren't named anywhere.** Lint surface focuses
   on workspace-supplied lints (no-bare-numeric etc.) but doesn't
   elevate convention lints (max-lines-per-file, disallowed APIs,
   naming-convention, existence-check-before-add) as a first-class
   concept. This is **the centre** per the user's framing.

9. **Configurable gate levels (build/commit/push) aren't called out.**
   The lint pack has gate severities but the framing is buried.
   Should be a top-level section: "Lint gates — same lints, multiple
   blocking thresholds."

10. **Agent integrations (rules/skills/hooks per AI tool) aren't named
    as part of the enforcement layer.** Hooks are spec'd; agent
    rules/skills sit somewhere in the harness but aren't elevated
    as part of the "no drift possible" property.

11. **§Threat model is too prominent (§2).** Supply-chain security is
    real but it's not the second thing a new reader should encounter.
    Demote to late in the doc (§40s).

12. **Replan and undo get full sections.** The user explicitly noted
    these are inconsequential — replan exists because the old design
    needed it (replacing unlock+deprecation); undo is a recoverability
    nicety. Trim both heavily; both become small recovery primitives,
    not headline features.

13. **The verifier catalog framing is supply-chain-security-shaped**
    (no shell escape, regex hardening) rather than
    "this-is-how-docs-and-source-stay-locked-together" shaped.

14. **The "source always lies, docs are truth" principle is never
    stated.** This is the load-bearing belief; should be in §0 and
    referenced throughout where drift-prevention is discussed.

### Process note

A deep-dive into actual mockspace usage (across hilavitkutin/mock/,
arvo/mock/, notko/mock/, this repo's mock/) is in progress to refine
this framing against real artefacts. Findings will inform the actual
revision pass. Until then, the doc's current shape captures the
storage architecture correctly but the framing above is the load-bearing
correction.

**Do not lock the doc or treat its structure as final until this TODO
section is addressed.**

### Deep-dive findings (2026-05-17, into existing FS-based mockspace use)

A walk through `arvo/mock/`, `hilavitkutin/mock/`, `vehje/mock/`, and
the workspace-level `.claude/rules/imports/` aggregation surfaces
a far richer picture than the ref-based redesign doc captured.
The redesign is a thin storage-layer change on top of a much larger
actual product. The full mockspace is:

A **state machine** (rounds → phases → locks → close) layered on top
of a **five-tier doc template system** with named templates and an
assembly pipeline, layered on top of a **lint engine** with built-in
lints, custom-Rust lints, external lint packs, per-gate severities,
and per-scope rule encoding, layered on top of an **AST extraction
pipeline** (tree-sitter) that makes lints semantic-aware, integrated
with a **bench framework** with deep statistical reporting, integrated
with a **sketch protocol** for design feasibility, integrated with
an **agent-rule generation pipeline** that surfaces all of the above
to AI tools, all assembled under a **mockspace.toml** declarative
config with rich domain metadata.

Concrete additions to the misframing list (continuing from item 14):

15. **The five-layer doc template system isn't in the doc at all.**
    Every crate carries `README.md.tmpl` (3-10 line summary, inserted
    into root DESIGN.md via `{{crate_summaries}}`),
    `DESIGN.md.tmpl` (shipping contract), `BACKLOG.md.tmpl`
    (designed-but-deferred, names deliberately unbackticked to evade
    the design-doc lint), `SHAME.md.tmpl` (structured lint-escape via
    `## <key>` + 50+ word explanation), and arbitrary
    `DEEPDIVE_*.md.tmpl` per topic. Plus mock-root `DESIGN.md.tmpl`,
    `PRINCIPLES.md.tmpl`, `WORKFLOW.md.tmpl`. This is the centre of
    the docs-as-truth machinery.

16. **The `design-doc-source-mismatch` lint** isn't named in the doc.
    This is THE specific mechanism enforcing docs-as-truth: every
    backticked type name in `DESIGN.md.tmpl` must exist in source. The
    lint blocks at the gate it's configured for. Without naming this
    explicitly the redesign reads as "verifiers check manifest claims"
    when the actual feature is "lints check docs claims against tree-
    sitter-extracted pub items." Adjacent: the `deprecation-comparison`
    lint requires every active CL superseding a deprecated CL to
    contain a `## Comparison to deprecated changelist` section.

17. **The `SHAME.md.tmpl` structured-escape-hatch protocol** is
    sophisticated and undocumented. A `## key` heading + 50+ word
    rationale silences a keyed lint violation. Not ad-hoc; a designed
    bypass channel with audit trail.

18. **Auto-generated agent rules from lint config.** Each
    `forbidden-imports` scope generates a per-scope agent rule file
    (`mock/agent/.../lint-forbidden-<scope>.md`) telling the agent
    what's forbidden in that crate context. Per-rule headers
    (`> **MOCKSPACE:** docs=design. source=untrusted...`) auto-injected
    so the discipline reinforces on every rule context-load. The agent
    receives the linting state as instructions, in addition to being
    blocked at commit/build/push time. The redesign doc has hooks +
    profiles but no concept of "lint config flows into agent rules."

19. **`mockspace.toml` doc-generation metadata** — `[crate_colors]`
    bg/fg pairs for the dependency graph; `[domain_kinds]` with
    emoji glyphs + label strings; `[known_macros]` description +
    usage; `layers = [...]` for depth-index labels; `primary_domain_macro`
    + `primary_domain_label` for the dominant macro axis. These feed
    the multi-tier doc generation, including the dependency graph
    visualisation. The redesign's "render pipeline three targets" is
    not the same thing.

20. **Bench framework statistical depth.** From `arvo/mock/benches/`:
    each bundle declared in `bench.toml` with workload, master_seed,
    multiple sizes (n=64/256/1024/4096), variants as separate compiled
    binaries. Per-(bundle, size): `.csv` raw data, `.meta.json` run
    metadata, `_findings.md` rendered analysis. Findings carry mean,
    median, best 20%, mid 60%, worst 20%, Δ mean vs baseline, 95%
    bootstrap CI, adjusted p, sign p, ties, per-cooldown breakdown,
    per-pass consistency. hilavitkutin has 395 bench files. This
    framework is used **during topic-discussion phases** to confirm or
    refute design assumptions; not a side feature.

21. **Sketch protocol.** From `arvo/mock/research/sketches/`:
    sketches are actual `.rs` files compile-probed with
    `rustc --edition 2024 -Z next-solver=globally`. README explicitly
    distinguishes live compile probes ("WORKS") from design probes
    ("DESIGN-PROBE-DURING-SRC", validated during SRC phase by
    replacing one existing body with the generic form and confirming
    `cargo check --workspace` green). Sketches are numbered (S1, S5)
    and cross-referenced from topic files. Cite `cl-claim-sketch-
    discipline.md` rule. Sketches commit BEFORE doc CL locks.

22. **Multi-topic rounds with sister-correction discipline.** From
    arvo round 202605111719: three topic files in one round
    (`graph-spectral-for-hilavitkutin`, `algorithm-genericity-
    correction`, `arvo-arbitrary-limitations-audit`). Sister topics
    correct each other's framing without rewriting; original stays as
    audit trail. "Scope absorption" sections explain how a sister
    topic merges into the round without opening a new one. The
    redesign treats TOPIC as one activity per round; reality is N
    topic files per round, often correcting each other.

23. **`[primitive-introductions]` meta-config.** Declarative
    "this crate brings this substrate category to the table"; anti-
    bypass via known-category-tag matching only (raw `u32` /
    `Option` / arbitrary strings have no effect). Crates that bring
    a category self-exempt; everyone else stays strict.

24. **External lint pack inclusion** via `[lint-crates]` with git-
    pinned dep. Shared stack-wide lint rules live as standalone repos
    (e.g., `mockspace-hilavitkutin-stack-lints`); projects pull them
    in. Cross-project lint discipline is a first-class concern.

25. **Tree-sitter-driven pub-item extraction.** The lint engine uses
    tree-sitter to parse Rust source and check claim alignment. The
    redesign's verifier catalog is filesystem-shape (`grep_present`,
    `path_exists`); reality is AST-shape against real Rust syntax
    trees. Language-specific extension (Rust today; future TS via a
    grammar plugin).

26. **Workspace-level agent rule aggregation via homma.** Per-repo
    `<repo>--<rule>` aggregated to workspace-level
    `.claude/rules/<repo>--*.md`. Plus mirrored
    `.claude/rules/imports/<repo>/*` for an alternative resolution
    path. Workspace agent has visibility into every repo's rule
    state; per-repo overlay scoping via `paths:` front-matter. This
    is homma's job per the workspace-tool boundary, but the
    aggregation property matters for the design.

27. **Per-crate doc directory structure.** `mock/crates/<crate>/`
    contains `README.md.tmpl`, `DESIGN.md.tmpl`, `BACKLOG.md.tmpl`,
    `SHAME.md.tmpl`, arbitrary `DEEPDIVE_*.md.tmpl`, plus `src/lib.rs`
    and `Cargo.toml`. The "mock workspace" itself is a real
    cargo workspace. This is not a sidecar; it's a parallel cargo
    workspace acting as design source-of-truth.

28. **Round artefacts beyond topic + CL.** Each round directory
    contains `.history` (state machine transition audit log),
    `.meta` (round metadata, creation time), and the various topic /
    CL files. Deprecated CLs renamed `.deprecated.<n>.md`. The
    `deprecation-comparison` lint requires the active CL to carry a
    `## Comparison to deprecated changelist` section.

29. **Doc CL structure is highly mechanical.** Per-file edit plans,
    section-by-section, with explicit line-range references ("lines
    22-44"). Per-axis mapping tables (topic axis → doc landing). Full
    Rust code blocks for new trait declarations. Concrete-impl tables.
    Cross-references to BACKLOG promotion/demotion semantics. This is
    far richer than the manifest grammar in the redesign.

30. **arvo has 61 design_rounds folders.** hilavitkutin has 97. Real
    usage. Most rounds are small ("short rounds is a valid workflow
    too"); some span months and many topic files. The redesign must
    accommodate both.

### What the redesign needs to become

The current 3170-line ref-based-mockspace-redesign doc is, at best,
**one chapter of the full mockspace v2 design** — the storage layer.
Other chapters need to exist before "what is mockspace" is a
complete answer:

- **Doc template system** — README/DESIGN/BACKLOG/SHAME/DEEPDIVE
  hierarchy, per-crate + mock-root, `{{crate_summaries}}` and other
  template substitution, assembly pipeline.
- **Lint engine** — built-in lints, custom `.rs` lints registered
  via `mockspace::Lint`, external lint packs, per-gate severities
  (commit/build/push × error/warn/info/off), per-scope rules with
  reasons, the `design-doc-source-mismatch` lint as docs-as-truth
  enforcer, the `deprecation-comparison` lint, the file-size lint,
  the `forbidden-imports` lint as layer enforcement, the
  `[primitive-introductions]` self-exemption machinery, the
  `SHAME.md.tmpl` structured-escape-hatch protocol.
- **AST extraction** — tree-sitter-driven pub-item parse, language
  plugin shape, per-language extension surface.
- **Doc generation pipeline** — templates with metadata
  (`crate_colors`, `domain_kinds`, `layers`, `known_macros`),
  dependency graph + visualisation generation, render to source-tree
  vs forge-API vs local-only, single-source-of-truth update
  propagation.
- **Bench framework** — `bench.toml` schema, workload + variants +
  sizes, statistical depth in findings, integration with topic
  phase, `mock bench run|report` commands.
- **Sketch framework** — sketch dir layout per round, compile-probe
  vs design-probe distinction, sketch ↔ topic file cross-reference,
  `cl-claim-sketch-discipline` rule integration.
- **Research workflow** — `mock/research/` cross-references,
  imports-from-prior-art (`imported-from-polka-dots/`,
  `imported-from-saalis/`), history files
  (`<crate>-history.md`), audit subdir for external review.
- **Agent integration** — `mock/agent/` per-repo: `MAIN.md.tmpl`,
  `PREAMBLE.md.tmpl`, `POSTAMBLE.md.tmpl`, `config.toml`,
  `rules/<rule>.md.tmpl`, `skills/<skill>/SKILL.md.tmpl`,
  `hooks/<hook>.sh.tmpl`. Auto-generation of per-scope rule files
  from lint config. Header injection on every rule context-load.
  Workspace-level aggregation surface (homma's responsibility but
  consumed by mockspace).
- **Multi-topic round discipline** — sister topics, scope absorption,
  `.history` audit log, `.meta` metadata, deprecated-CL renames.
- **Workflow state machine** (the ref-based piece) — refs/mock/* as
  storage substrate, phase transitions, manifest grammar, verifier
  catalog (which needs to include the AST-aware lints, not just
  filesystem-shape verifiers).

### Scope decision required

Two paths for the redesign doc:

- **Path A:** Treat the current doc as the storage-layer spec only.
  Add a sibling doc (or separate research note) capturing the full
  mockspace v2 product spec. The current doc gets renamed to
  `202605171033_ref-based-mockspace-storage-layer.md` or similar.
- **Path B:** Expand the current doc to cover the full product.
  Add the chapters listed above. Doc grows substantially (likely
  to 5000-8000 lines). Risk: the storage-layer detail dominates
  the doc structure.

Recommendation: **Path A.** The storage layer is one independently-
designable concern; the full mockspace is the user-facing product.
Keeping them in separate docs lets each evolve at its own pace and
prevents the storage detail from drowning the heart of the product
in any one read.

---

## Table of contents

1. [Motivation](#motivation)
2. [Threat model](#threat-model)
3. [Vocabulary](#vocabulary)
4. [The workflow model](#the-workflow-model)
5. [Reference architecture](#reference-architecture)
6. [Local materialisation](#local-materialisation)
7. [Source-side vs mock-side refs](#source-side-vs-mock-side-refs)
8. [The harness](#the-harness)
9. [`mock commit` and the commit boundary](#mock-commit-and-the-commit-boundary)
10. [Hosts, imports, and exports](#hosts-imports-and-exports)
11. [Signing and integrity](#signing-and-integrity)
12. [The `@` first-party source and trust model](#the--first-party-source-and-trust-model)
13. [The transparency log](#the-transparency-log)
14. [The lockfile](#the-lockfile)
15. [Hook protocol](#hook-protocol)
16. [Env and bins policy](#env-and-bins-policy)
17. [Profiles and reactive policy](#profiles-and-reactive-policy)
18. [Language-specific runners](#language-specific-runners)
19. [`mockspace.toml` schema](#mockspacetoml-schema)
20. [Manifest schema](#manifest-schema)
21. [Verifier catalog](#verifier-catalog)
22. [Version compatibility](#version-compatibility)
23. [Active phase storage on refs](#active-phase-storage-on-refs)
24. [Transition atomicity](#transition-atomicity)
25. [Content-stable anchors](#content-stable-anchors)
26. [Replan flow](#replan-flow)
27. [Tasks and the archive ref](#tasks-and-the-archive-ref)
28. [Rendering pipeline](#rendering-pipeline)
29. [PR lifecycle](#pr-lifecycle)
30. [`mock sync` and the staleness model](#mock-sync-and-the-staleness-model)
31. [`mock doctor`](#mock-doctor)
32. [CLI: commands](#cli-commands)
33. [CLI: porcelain](#cli-porcelain)
34. [Undo and redo](#undo-and-redo)
35. [CLI: cross-cutting concerns](#cli-cross-cutting-concerns)
36. [Audit trail durability](#audit-trail-durability)
37. [Day in the life of a round](#day-in-the-life-of-a-round)
38. [Crate organisation](#crate-organisation)
39. [Migration tooling](#migration-tooling)
40. [Round and task archival](#round-and-task-archival)
41. [Windows support and platform notes](#windows-support-and-platform-notes)
42. [Future direction: vehje as hook language](#future-direction-vehje-as-hook-language)
43. [Open questions](#open-questions)
44. [Relationship to workspace-level tools](#relationship-to-workspace-level-tools)

## Motivation

Mockspace is a design-discipline tool that wraps a git repository with
structured workflow ceremony. A project using mockspace authors design
decisions explicitly (topic documents, structured change manifests),
tracks them as durable refs in git, projects them to forge pull
requests for outside visibility, and preserves the authoring trail
forever for future archaeology.

This document specifies a **ref-based storage model** for mockspace
plus an **import/export extensibility model** that makes mockspace-core
a small kernel with everything else expressed as imports from named
hosts.

The redesign addresses several pains in the prior `mock/`-in-`refs/heads/*`
model:

- **Working-tree clutter.** Outsiders see `mock/` and ask what it is.
  Cargo publishes bundle too much. Casual contributors don't need it.
- **Tag-namespace pollution.** Round tags compete with release tags.
- **Branch coupling.** Switching branches forces switching mockspace
  state; mockspace state is often branch-orthogonal.
- **Implicit toolchain coupling.** mockspace is Rust-only by accident.
- **Lack of extensibility.** Today's mockspace bundles its lints and
  hooks in the binary. No clean way to ship third-party packages.

The redesign decouples mockspace state from `refs/heads/*`, makes
mockspace language-agnostic, and exposes a first-class import/export
system.

## Threat model

Mockspace is a tool. Its trust model is git's trust model, no more,
no less. The project's source-of-truth lives in a git repository;
whoever has push access to that repository can change project state,
and reviewers review diffs. Mockspace does not introduce a
project-internal trust layer above git's existing access-control
mechanisms.

This section names what mockspace mitigates against **external** attack
classes (compromised hosts, MITM, malicious imports) and what is
**out of scope** (handled by git, the forge, the developer's machine,
the install channel, or the project's review process).

### In scope (mockspace mitigates structurally)

These are attacks at the import / fetch / verification boundary —
where mockspace pulls external code and runs it.

- **MITM substitution of imported package content.** Lockfile pinning
  by SHA plus signature verification means a host that serves
  different bytes than the maintainer published is detected on every
  fetch (D026).
- **Stale signed content served by a compromised host (freeze attack).**
  Optional transparency-log witnessing (when a project opts in) lets
  consumers detect content that signature-validates but lacks a recent
  cross-witness. Per-developer freshness state in
  `.git/mockspace/observations.toml` surfaces stale imports via D030
  / D032; the freshness signal is advisory, not blocking.
- **Surreptitious maintainer key rotation.** TOFU per-developer: a
  fingerprint change between fetches by the same developer fires
  D026. The developer's explicit acknowledgement
  (`mock import rotate --accept-new-key`) is required to accept.
  Projects using transparency logs additionally verify a log-recorded
  rotation entry signed by the prior key.
- **Verifier-string code execution from PR content.** Manifest
  verifier kinds are a closed, structurally-typed catalog. No shell
  escape exists at the verifier layer.
- **URI path traversal / namespace shadowing.** The URI grammar
  rejects `..`, normalises lexically, refuses root-escape. Reserved
  prefixes (`local | ~ | @ | ext`) disambiguate scope; host names
  cannot collide with reserved prefixes.
- **Inline executable code in TOML.** Forbidden at the schema layer;
  hooks are file paths or `mock://` URIs only.
- **Concurrent multi-machine writes losing work.** Side-branch
  preservation is pushed to remote BEFORE local reset; D037 fires on
  any failure to preserve.
- **Anchor-storage path collisions.** Content-addressed storage
  eliminates the failure mode at the data-layout level.
- **Migration crash leaving inconsistent state.** Idempotent step
  recovery via content-addressed re-derivation; D029 surfaces drift.

### Out of scope (handled by git, the forge, the user)

These are attack classes mockspace deliberately does not address.
They're handled by layers above or below mockspace, or by the project's
own governance.

- **Who can change project state.** Project access control — who can
  push to the repo, who can merge PRs, what review gates apply — is
  the forge's and the project's concern. Mockspace operates on
  whatever state lands in the repo; it does not arbitrate who is
  allowed to put state there. Projects that want signed commits use
  `commit.gpgsign` or forge branch protection; projects that want
  required reviews configure their forge accordingly. Mockspace does
  not impose a special supply-chain ceremony for harness commits.
- **Compromise of the mockspace binary itself.** The user's install
  channel (package manager, `cargo install`, binary download) is the
  root of trust. Mockspace does not defend against a malicious binary
  served as "mockspace".
- **Compromise of the developer's machine.** Hooks run with the
  developer's shell environment by design (same trust model as Cargo
  `build.rs`, npm scripts, git hooks). A compromised developer
  machine cannot be defended at the mockspace layer.
- **Insider attacks within the team.** A team member with commit
  access can change any project state. This is the same as git: a
  team member with push access can rewrite history, change `.gitignore`,
  inject `.gitattributes` filters, modify CI workflows, alter `Cargo.toml`
  dependencies. The defence is review, governance, and forge access
  control — not a mockspace-specific layer.
- **Coordinated host + canonical-log compromise.** An attacker who
  controls both the package's host AND the project's transparency log
  can produce coherent malicious records. Cross-host federation
  (multiple mirrors, project-local log on a separate host) is the
  practical mitigation; cryptographic countermeasures (multi-party
  signing, hardware-backed keys, sigstore-shaped full transparency)
  are out of scope for v1.
- **Denial-of-service against forge or host.** Mirror federation
  defends against single-host outage; mockspace does not defend
  against coordinated DDoS or sustained availability attacks.
- **Supply-chain compromise of mockspace's own dependencies.** Cargo
  and the Rust ecosystem provide their own supply-chain layer
  (`cargo audit`, `cargo vet`). Mockspace's defences are at the
  mockspace layer; the layer below is the wider ecosystem's problem.

### Mitigation summary by attack class

| Attack class | Defence |
|---|---|
| MITM content substitution | SHA pin + signature verification (D026) |
| Freeze attack | Optional transparency log + per-developer freshness cache + D030/D032 |
| Maintainer key rotation (silent) | TOFU per-developer (D026); transparency-log rotation entry when enabled |
| Verifier RCE from PR | Closed verifier-kind catalog; no shell escape |
| Hook RCE from imports | Lockfile pin + signature + per-developer TOFU |
| URI path traversal | Grammar refuses `..`; lexical canonicalisation refuses escape |
| URI namespace shadowing | Reserved-prefix discipline + content stored in separate cache |
| Multi-machine race losing work | Side-branch preserved on remote before any reset (D037) |
| Anchor path collision | Content-addressed storage |
| Migration crash | Idempotent step recovery (D029) |
| TOFU first contact MITM | Documented; prompt shows actual signing key from `git verify-commit --raw` |
| Malicious team-member harness change | Out of scope (git access control + project review) |
| Compromised binary | Out of scope (install channel is root of trust) |
| Compromised developer machine | Out of scope (same as Cargo build.rs, npm scripts, git hooks) |

## Vocabulary

- **Round.** Discrete unit of mockspace workflow, scoped to one
  coherent design change.
- **Slug.** Identifier `<YYYYMMDDHHMM>-<short-name>` for rounds /
  research / bench / sketch. Lowercase alphanumeric plus hyphens.
- **Phase.** One of six round states.
- **Manifest.** Sealed contract per phase. Doc + src per round.
- **Task.** Work item with identity `<namespace>#<slug>` and Markdown
  body. Lifecycle independent of rounds.
- **Namespace.** Hierarchical path under which tasks are organised.
- **Sub-task / step.** Discrete unit of work within a task.
- **Topic document.** Free-form Markdown prose authored during a round.
- **Harness.** mockspace-state ref carrying configuration, rules,
  templates, hooks, and project-local extensions.
- **Anchor.** Per-file content snapshot at APPLY phase entry.
- **Replan.** Backward transition from APPLY(...) to PLAN(...). Always
  deprecating.
- **Source-side branch.** `refs/heads/round/<slug>` carrying source-side
  commits of a round.
- **Mock-side ref.** `refs/mock/round/<slug>` carrying mockspace artefacts.
- **PR projection.** Auto-generated forge pull request from mock-side
  artefacts.
- **Five-element diagnostic format.** Structured message shape (see
  [The workflow model](#the-workflow-model)).
- **mock:// URI.** Mockspace's URI scheme.
- **Host.** Named alias for a git URL serving mockspace-shaped content.
- **Import.** Consumed `mock://` URI declared in `[imports]`.
- **Export.** Package this project publishes for others. Commit ref
  `refs/mock/export/<package-name>`.
- **Package.** Content addressable via `mock://export/<name>`. Tree of
  files + optional `package.toml` manifest.
- **Hook event.** Reactive moment in mockspace's flow where
  user-configurable logic fires.
- **Profile.** TOML table `[profile.<name>]` declaring per-event
  reactive handlers.
- **Mirror.** Locally-stored 1:1 copy of an externally-referenced ref.
- **Lockfile.** `mockspace.lock` at harness root, pinning every import
  to a SHA + signing-key fingerprint.
- **Verifier.** Lock-time mechanism running structured checks per
  manifest claim.
- **Claim.** A `[[change]]` entry in a manifest.

## The workflow model

This section defines mockspace's workflow model end to end.

### Concepts overview

Projects organise design evolution into discrete **rounds**. A round
runs through six **phases**, ending in DONE.

A round carries: topic documents, doc manifest, src manifest, anchors,
PR projection.

Rounds reference **tasks** with their own lifecycle.

The **harness** is the project's mockspace configuration ref.

### The six phases

```
TOPIC ──new──▶ PLAN(DOC) ──apply──▶ APPLY(DOC) ──finish──▶ PLAN(SRC) ──apply──▶ APPLY(SRC) ──finish──▶ DONE
                  ▲                       │                    ▲                       │
                  └───────replan──────────┘                    └────────replan─────────┘
                  (always deprecates)                          (always deprecates)
```

**Phase semantics:**

- **TOPIC** — exploration; topic documents authored; no manifest.
- **PLAN(DOC)** — drafting doc manifest; mutable.
- **APPLY(DOC)** — doc manifest sealed; source-side commits land;
  PR projection open; `.anchor.doc.toml` captured.
- **PLAN(SRC)** — drafting src manifest.
- **APPLY(SRC)** — src manifest sealed; source-side commits land;
  `.anchor.src.toml` captured.
- **DONE** — ready for `mock close`.

**Forward transitions:**

| From | Command | Effect |
|---|---|---|
| TOPIC | `mock phase plan` | scaffold doc manifest |
| PLAN(DOC) | `mock phase apply` | validate, verify, capture anchor, transition |
| APPLY(DOC) | `mock phase finish` | scaffold src manifest |
| PLAN(SRC) | `mock phase apply` | validate, verify, capture anchor, transition |
| APPLY(SRC) | `mock phase finish` | transition to DONE |
| DONE | `mock close` | ingest comments, freeze, optional merge |

**Backward transition:** `mock phase replan` always deprecates the
failed manifest, restores phase-owned source files from the anchor,
scaffolds a fresh manifest. The new manifest must explicitly account
for every claim in the deprecated manifest.

**Phase invariants:**

- One active round per repository by default.
- One phase current per active round.
- Sealed manifests immutable forever.
- Deprecated manifests immutable forever (numbered by replan iteration).
- Topic documents mutable until DONE.
- Round refs never squashed or rebased before close.

### Tasks

Identity: `<namespace>#<slug>`. In refs: `refs/mock/task/<ns-path>/<slug>`
with `/`. In URIs: `<ns-path-segments>::<slug>` with `::`.

Slugs and namespace segments match `[a-z][a-z0-9-]{0,62}`.

**States** (in `.state` marker): `open`, `in-progress`, `blocked`,
`deferred`, `closed`.

**Resolutions** for closed tasks (in `meta.toml`'s `[closure]`):
`completed`, `cancelled`, `superseded`, `wontfix`.

**Sub-tasks (steps)** declared in `meta.toml` as `[steps.<key>]` with
state + phase tag (`doc` / `src` / `doc+src`).

### Manifests

TOML documents declaring scope, claims, acceptance.

**Lifecycle:**

1. Scaffolded at PLAN entry.
2. Drafted during PLAN.
3. Sealed at APPLY entry. Validation + verifier run; anchor captured.
4. Deprecated if replan invoked. Renamed `manifest.<phase>.deprecated.<n>.toml`.

**Branch role:** ambient integration context, NOT a workflow object.

### The five-element diagnostic format

Every error, warning, or guidance:

1. **What invariant failed.**
2. **What mockspace thinks the situation is.**
3. **Why that is blocked.**
4. **Which command the user probably meant instead.**
5. **Safest recovery path.**

JSON shape under `--json`:

```json
{
  "exit_code": 5,
  "finding_id": "D020",
  "invariant": "...",
  "situation": "...",
  "blocked_because": "...",
  "suggested_command": "...",
  "recovery": "..."
}
```

## Reference architecture

```
refs/heads/main                    public release line
refs/heads/dev                     public dev trunk (source only)
refs/heads/round/<slug>            per-round source-side branch

refs/mock/harness                  the harness ref
refs/mock/round/<slug>             per-round overlay ref (orphan, flat)
refs/mock/round/<slug>-conflict-<host>-<ts>
                                   side-branch preserving lost-race commit
refs/mock/task/<ns-path>/<slug>    per-active-task ref (orphan, flat)
refs/mock/task-archive             single archive for closed tasks
refs/mock/round-archive            single archive for closed rounds
refs/mock/research/<slug>          research clusters
refs/mock/bench/<slug>             bench clusters
refs/mock/sketch/<slug>            sketches
refs/mock/export/<package>         published package
refs/mock/export-archive           single archive for retired exports
refs/mock/mirror/<host>/<kind>/<slug>
                                   1:1 mirror of externally referenced refs
```

There is no `refs/mock/index`. Index is local cache only.

All `refs/mock/*` refs except `harness`, `*-archive`, `mirror/*` are
orphan.

## Local materialisation

`.mock/` is **not** a git worktree. mockspace CLI uses git plumbing to
render ref content into `.mock/` and commit edits back.

Parent worktree's `.gitignore` lists `.mock/`. Outsiders see no
mockspace surface.

Three distinct storage locations, three distinct concerns:

### `.mock/` — user-facing rendered surface (per-project, gitignored)

```
<repo>/                          parent worktree on refs/heads/<branch>
├── .gitignore                   includes /.mock/
├── crates/                      source code (visible to outsiders)
└── .mock/                       mock-CLI-managed rendered surface (gitignored)
    ├── mockspace.toml           rendered from harness ref
    ├── mockspace.lock           rendered from harness ref
    ├── .ref-sha.harness         SHA the harness was last rendered from
    ├── agent/                   rendered from harness ref
    ├── lints/                   rendered from harness ref
    ├── templates/               rendered from harness ref
    ├── hooks/                   rendered from harness ref
    ├── export/                  rendered from refs/mock/export/* (this project's)
    │   └── <package-name>/
    ├── round/                   rendered from active round ref
    ├── tasks/                   rendered from active task refs
    ├── research/, bench/, sketch/   authoring
    └── refs/                    consultation worktrees (read-only)
```

The `.mock/` tree contains ONLY rendered content the developer
interacts with directly. No cache files, no internal state, no
advisory locks. Cache and internals live in `.git/mockspace/` (per
the git-LFS convention); machine-global content-addressed import bytes
live in `~/.cache/mockspace/`.

### `.git/mockspace/` — per-project per-developer internals

Following git-LFS's convention (which stores objects under
`.git/lfs/`), all mockspace-internal per-project per-developer state
lives under `.git/mockspace/`. This directory is automatically
excluded from git tracking (everything under `.git/` is invisible
to git as repo content), survives clones cleanly, and disappears on
repo delete.

```
<repo>/.git/mockspace/           per-project per-developer state (not pushed)
├── .lock                        flock-based advisory lock
├── index.bin                    local index cache (ref state snapshot for fast reads)
├── observations.toml            per-import last_observed_at, last_witness_at
├── doctor.log                   structured journal of mock doctor operations
└── undo/                        short-span undo/redo log (see Undo and redo)
    ├── log.jsonl                append-only operation journal
    └── <ts>-<seq>.json          per-snapshot ref-state
```

### `~/.cache/mockspace/` — machine-global content cache

```
~/.cache/mockspace/              XDG_CACHE_HOME; shared across all projects
├── imports/                     content-addressed cache of imported package bytes
│   └── <host>/<ref-path>/<sha>/ same SHA = same bytes regardless of project
└── helpers/                     baseline helper scripts extracted from binary on first run
```

Imported package bytes are content-addressed by SHA. Two projects on
the same machine importing `mock://ext/runner-rs@<a1b2...>` share the
same on-disk bytes — no per-project duplication. Cache eviction via
`mock cache prune` (see CLI).

### `~/.config/mockspace/` — per-developer config

```
~/.config/mockspace/             XDG_CONFIG_HOME; per-developer, machine-global
└── trust.toml                   TOFU acceptances (per (host, fingerprint))
```

**No `git worktree add` anywhere.**

Discovery: `mock` walks up from cwd looking for the nearest `.git`
directory; `.mock/mockspace.toml` is rendered alongside (per the
harness ref). Stops at filesystem boundary. Configurable via
`MOCK_ROOT`.

## Source-side vs mock-side refs

Two parallel tracks per round:

- **Mock-side ref** `refs/mock/round/<slug>`: orphan ref with topic
  documents, manifests, anchors, phase marker, comment snapshots.
- **Source-side branch** `refs/heads/round/<slug>`: normal feature branch
  with source-side commits. PR projection targets this.

Both created at `mock round new`. Independent in history; paired by slug.

Phase transitions commit on the mock-side ref only. Source-side commits
happen via normal git workflow.

**Parent worktree HEAD must be on the round's source-side branch for:**

- `mock phase apply` from PLAN(SRC) (verifier execution).
- `mock phase replan` from APPLY(...) (file restoration).
- Source-side commits.

`mock commit` for `.mock/round/` works regardless of parent HEAD.

When operation requires HEAD on round's branch and it isn't, mockspace
emits structured diagnostic. No auto-switch.

## The harness

Project's mockspace configuration embodied as `refs/mock/harness`.

```
refs/mock/harness root tree
├── mockspace.toml               project-local config
├── mockspace.lock               lockfile (machine-managed; users don't edit)
├── agent/                       agent integration templates
├── lints/                       lint configurations
├── templates/                   render templates (.md.tmpl)
├── hooks/                       project-local hook scripts
└── tools/                       project-local CLI extensions (optional)
```

Harness lifecycle: created at first adoption; advances via commits.

Per-repository scope. Two developers sharing a clone share the harness.

## `mock commit` and the commit boundary

`mock commit` mirrors `git commit` semantics, routed per `.mock/<area>/`.

```
mock commit                        commit all pending changes
mock commit -m "<message>"
mock commit round
mock commit task <ns>::<slug>
mock commit harness
mock commit export <name>
mock commit --all
mock commit --dry-run
```

Default: inspect every `.mock/<area>/` for diff against rendered ref
content; per-area commits with auto-generated messages.

## Hosts, imports, and exports

Mockspace's extensibility flows through hosts, imports, exports.

### Hosts

Named alias in `[hosts.<name>]` for a git URL serving mockspace-shaped
content.

```toml
[hosts.mockspace-rs]
url = "https://codeberg.org/mockspace/mockspace-rs.git"
mirrors = [                                  # optional fall-through fetch URLs
  "https://github.com/mockspace/mockspace-rs.git",
  "git@private-mirror.example:mockspace/mockspace-rs.git",
]
token_env = "MOCK_HOST_TOKEN_MOCKSPACE_RS"   # optional, unified fallback
read_token_env = "MOCK_HOST_READ_TOKEN_MOCKSPACE_RS"   # optional, read-only fetches
write_token_env = "MOCK_HOST_WRITE_TOKEN_MOCKSPACE_RS" # optional, push operations
pinned_at = "<sha>"                          # optional

[hosts.arvo]
url = "https://github.com/orgrinrt/arvo.git"
forge_url_template = "https://github.com/orgrinrt/arvo/tree/{ref}"
```

**Mirror federation.** When `mirrors = [...]` is set, fetches try `url`
first, then each mirror in order. Substitution is cryptographically safe
because the lockfile pins by SHA + signing-key fingerprint; any mirror
serving different content fails verification. This makes the
"federates with git for redundancy" property real: if the primary host
goes down (forge outage, account suspension, migration in progress),
work continues against mirrors as long as one of them still serves the
locked content. Mirrors are user-managed; mockspace does not auto-discover
mirror URLs.

`mock host add-mirror <name> <url>` appends to the list.
`mock host remove-mirror <name> <url>` removes one. `mock host fetch
<name> --verify-mirrors` walks every configured mirror, fetches the
pinned ref, verifies signature, reports which mirrors are healthy.

**Token resolution.** `read_token_env` and `write_token_env` take
precedence when present; `token_env` is the unified fallback. The
distinction matters for shops where CI uses a read-only deploy token
and maintainers push with a separate write token. Tokens flow into git
operations via `GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_*` / `GIT_CONFIG_VALUE_*`
so they never enter argv.

**Host-name reserved-namespace check.** Configuration load refuses any
`[hosts.<name>]` whose `<name>` is `local`, `ext`, `@`, or matches any
reserved `local_kind` (see [the URI scheme](#the-mock-uri-scheme)).

### Exports as commit refs

A published mockspace package. Author content in
`.mock/export/<package-name>/`, commit to `refs/mock/export/<package-name>`,
push to remote.

Each export ref is a commit ref whose tree IS the package content.
`package.toml` at tree root declares metadata:

```toml
schema_version = "1.0"
name = "some-runner"
version = "1.2.3"
description = "Runs .rs files in mockspace hooks/ and lints/."
entrypoint = "runner.sh"
mockspace_version = "1.0"

[signing]
key_fingerprint = "SHA256:abc123..."
key_type = "ssh-ed25519"

[dependencies]
"mock://export/shared-helpers" = "1.0"
```

**Multi-file packages**: commit tree contains a directory.
**Single-file packages**: tree contains the file + `package.toml`.
**Releases**: new versions = new commits on the ref. Optional tags.

### The `mock://` URI scheme

```ebnf
mock_uri        = "mock://" target [ intra_path ] [ pin ] [ fragment ]

target          = local_target | self_target | first_party_target | external_target

local_target    = local_kind "/" identifier_path
                  # bare form ALWAYS means local (importer's scope)

self_target     = "~/" local_kind "/" identifier_path
                  # shorthand for "this package's own scope"
                  # only meaningful inside an exported package's content

first_party_target = "@/" local_kind "/" identifier_path
                                          # @ = first-party (hardcoded)

external_target = "ext/" host_name "/" local_kind "/" identifier_path

local_kind      = "round" | "task" | "research" | "bench" | "sketch"
                | "export" | "hook" | "lint" | "agent" | "template"
                  # reserved namespace; future kinds added by minor bumps

host_name       = segment      # MUST NOT match any local_kind, "@", "ext",
                               # "~", or any future reserved prefix

identifier_path = segment ("::" segment)*

segment         = [a-z][a-z0-9-]{0,62}

intra_path      = "/" path_segment ( "/" path_segment )*
                  # only legal for kinds with file structure

path_segment    = [a-zA-Z0-9._-]+
                  # explicitly excludes "..", lone ".", any "/" within,
                  # and leading "."

pin             = "@" sha                  # sha40 (SHA-1) or sha64 (SHA-256)

sha             = sha40 | sha64
sha40           = [0-9a-f]{40}
sha64           = [0-9a-f]{64}

fragment        = "#" step_key
step_key        = segment
```

**Reserved-namespace rule.** The `local_kind` keywords plus `@`, `ext`,
and the literal `local` are reserved. `host_name` MUST NOT collide with
any of them. Mockspace verifies this at config load and refuses the
configuration with a structured diagnostic if a `[hosts.<name>]` clashes.
Adding a new `local_kind` is a minor-version bump and the new kind is
added to the reserved list.

**Path-traversal defence.** The `intra_path` grammar excludes `..` and
lone `.` segments at the parse layer. The resolver additionally
lexically canonicalises the resolved path (resolving `.`, refusing
`..`, refusing symlink-escapes) and verifies the result remains within
the package's root directory. Any path that would escape the package's
root is refused with a structured diagnostic; never silently
re-anchored.

**Worked examples:**

| URI | Resolves to |
|---|---|
| `mock://round/202605171800-trunk-refactor` | Local round ref (importer's scope) |
| `mock://task/compiler::ir::structural-robust-ir` | Local task; active then archive |
| `mock://task/compiler::ir::structural-robust-ir#define-grammar` | Local task + step |
| `mock://export/some-package` | Local export (importer's scope) |
| `mock://hook/on_dirty_state.sh` | Importer's hook in harness `hooks/` |
| `mock://~/hook/helper.sh` | Self-scope hook (only meaningful inside an exported package, referring to that package's own bundled hook) |
| `mock://~/lint/internal.rs` | Self-scope lint |
| `mock://@/export/profile-dev` | First-party export (hardcoded binary trust) |
| `mock://@/handler/auto/on_dirty_state.sh` | First-party auto handler |
| `mock://ext/arvo/round/202605111719-graph-algos` | External arvo round |
| `mock://ext/mockspace-rs/export/runner-rs@<sha>` | External Rust runner, pinned |
| `mock://ext/runner-rs/hook/setup.sh@<sha>` | Specific hook bundled with an external package (explicit external form) |

**Resolution rules.**

1. **Parse** the URI per the grammar above. Refuse on grammar violation
   (path-traversal segments, reserved-namespace collision).
2. **Resolve target** to a host scope: local (current project), `@/`
   (first-party), or `ext/<host>` (named external).
3. **Apply SHA pin** if present; otherwise resolve to the host's
   current tip for the target.
4. **Verify signature** (see [Signing and integrity](#signing-and-integrity))
   against the lockfile-recorded fingerprint.
5. **Check lockfile** entry SHA against the resolved SHA.
6. **Resolve intra-path** lexically within the cached package directory;
   refuse on canonicalisation failure or root-escape.
7. **Cache** the resolved bytes.

**Resolution scope.** Imported packages are stored as separate
artefacts under the local cache (`~/.cache/mockspace/imports/...`).
They never overlay the consumer's filesystem; there is no shadowing
surface to attack. URI resolution is by explicit prefix:

- **Bare form** (`mock://hook/foo.sh`, `mock://export/foo`): ALWAYS
  resolves to the consumer's local scope, regardless of where the URI
  string physically lives (in the consumer's harness, in an imported
  package's content, in a rendered PR body). Local is the default.
- **`~/`** (`mock://~/hook/helper.sh`): self-scope shorthand for "this
  package's own bundled content". Only meaningful inside an exported
  package's content; the package author uses this to call into
  helpers they bundled with their export. Mockspace resolves `~/` to
  whichever host+package is the current resolution context (the
  package being executed).
- **`@/`**: first-party (mockspace canonical).
- **`ext/<host>/`**: explicit external host.

**This is the security boundary.** An exported package that wants to
invoke its own bundled hook MUST write `mock://~/hook/foo.sh` or the
explicit `mock://ext/<own-name>/hook/foo.sh`. It MUST NOT write
`mock://hook/foo.sh` and expect that to resolve to its own bundle;
the bare form is always the consumer's. An exported package CAN
deliberately reach into the consumer's scope by writing
`mock://hook/foo.sh` (e.g., to call a customisation hook the consumer
provides as an extension point), but this is opt-in by the package
author, not by accident.

The asymmetry is intentional: consumers expect imported packages to
behave as bounded artefacts, so the default has to be "calls go to
the local consumer". Package authors writing self-references know
they need the explicit `~/` or `ext/<self>/` form. Mockspace lints
package content at publish time for bare-form URIs that look like
typos for self-references (`mock://hook/<name>` where `<name>`
matches a file under the package's `hook/` directory) and warns the
package author to use `~/` if self-reference was intended.

### `~/` scope-pinning rule

The `~/` shorthand only has meaning inside a **package-execution
context**: when mockspace is actively executing the bytes of an
imported package, and the URI string being resolved comes from inside
that package's tree, then `~/` resolves to that package. Outside this
context, `~/` is meaningless.

Specifically, **mockspace refuses `~/` URIs in these contexts** with
a structured diagnostic:

- In the consumer's own `mockspace.toml` (the harness has no
  package-self-scope).
- In manifests committed to the source-side branch.
- In rendered output (PR body, source-tree files, local-only files).
- In doctor diagnostics that re-quote a URI back to the user.
- In hook environment variables or any other string the resolver
  reads after a URI has been extracted from text rather than parsed
  as part of a package's structured content.

The resolver records the resolution context explicitly: every URI
parse takes a "scope context" parameter (either `None` for top-level
contexts, or `Some(<package-name>)` when resolving within a package's
tree). `~/` requires `Some(...)`; absence is a parse error.

The doctor finding D041 fires when `~/` appears in any harness-ref
content (`mockspace.toml`, hook files, manifest files), in lint
diagnostics, or anywhere mockspace itself owns the text. Package
content is the only legitimate `~/` site.

Task resolution tries active task ref first, then archive ref.

PR-body resolution is **single-hop only** (no transitive expansion).

### Imports

```toml
[imports]
import = [
  # Local imports (no prefix needed; trusted by default)
  "mock://hook/on_custom_doctor.sh",

  # First-party imports (verified against hardcoded binary trust)
  "mock://@/export/profile-dev",

  # External imports (SHA-pinned for executable content)
  "mock://ext/mockspace-rs/export/runner-rs@<sha>",
]

[imports.ext.mockspace-rs]
include = ["hooks/**/*.rs", "lints/**/*.rs"]
runner = "mock://ext/mockspace-rs/export/runner-rs"
```

### Export commands

```
mock export new <name>             create new orphan ref with scaffolded package.toml
mock export list
mock export show <name>
mock export bump <name> <version>  commit a new version
mock export archive <name>         roll into refs/mock/export-archive
mock export publish <name>         push to remote
```

## Signing and integrity

Every executable import is verified against a cryptographic signature
before mockspace runs it. Mockspace leans on **git's native commit
signing** rather than inventing a new signature format.

### Signing model

Every export commit MUST be signed via git's commit signing (`git commit -S`).
Supported key types:

- **SSH keys** (via `gpg.format = ssh` in git 2.34+). Same key used to
  push commits.
- **GPG keys** (traditional git signing).

The package's `package.toml` includes the maintainer's public key
fingerprint at `[signing] key_fingerprint`. Verifiers check that the
commit signature was produced by the declared key.

### Trust on First Use (TOFU) is per-developer

Mockspace's TOFU model mirrors SSH's `known_hosts` exactly: each
developer's first encounter with a `(host, signing-key)` pair prompts
that developer for acceptance, and acceptance lands in their local
trust file. **The lockfile in the harness ref records pins (what
version + what fingerprint), not who-trusted-what.**

When a developer encounters an import that has not been seen on this
machine before:

1. Fetch the ref's tip and verify its commit signature via
   `git verify-commit --raw`. Parse only the machine-readable status
   lines (e.g., `[GNUPG:]VALIDSIG ...` for GPG, `[SSH]` lines for SSH
   signing). Force `LANG=C` for the invocation so localised output
   does not interfere. Refuse if `git --version` is older than 2.34;
   refuse if the signing backend is SSH and `gpg.ssh.allowedSignersFile`
   is unset, missing, world-writable, or unreadable.
2. Compare the actual signing-key fingerprint (from `--raw` output)
   against the lockfile's `signing_key_fingerprint`. Mismatch fires
   D026.
3. Read `package.toml`'s declared `key_fingerprint` (if present) for
   cross-check. If declared and actual differ, refuse with `D031`
   ("declared signing key does not match commit signature").
4. **Per-developer TOFU prompt** unless `MOCK_NON_INTERACTIVE=1` or
   the profile's `on_first_trust = "auto"`:

   ```
   New (host, signing-key) pair not in your local trust file:

     Host:        codeberg.org/mockspace/mockspace-rs
     Package:     runner-rs
     Version:     1.2.3
     Signing key: SHA256:abc123...    (from git verify-commit --raw)
     Key type:    ssh-ed25519
     Signed by:   Maintainer Name <maintainer@example.com>

     This project's lockfile pins this fingerprint.

   Trust this signing key on this machine? [y/N]:
   ```

5. On `y`: record `(host, fingerprint, key_type, accepted_at)` in
   `~/.config/mockspace/trust.toml`. Subsequent fetches by this
   developer of the same pair are silent. On `N` or non-interactive
   without auto: refuse with structured diagnostic.

The prompt fires per-developer, not per-project. Alice accepting a
fingerprint on her machine has no effect on Bob's. Bob's first
encounter with the same import prompts Bob locally. This is exactly
how SSH's `known_hosts` works and exactly how it should work.

A package that self-declares one fingerprint but is actually signed
by another is a red flag, not a config drift; mockspace refuses
(D031), never auto-reconciles.

### Hardcoded trust for `@/` first-party

The mockspace binary embeds the canonical mockspace project's public
key fingerprint at compile time. Every `@/` import verification checks
against this hardcoded fingerprint. No TOFU; no interactive prompt; no
override possible without rebuilding the binary or declaring an
explicit `[hosts.mockspace-core]` (see [The `@` first-party source](#the--first-party-source-and-trust-model)).

Key rotation of the canonical mockspace project requires binary rebuild
+ redistribution. Acceptable; happens rarely.

### Subsequent fetches

On every subsequent fetch of a previously-trusted package:

1. Fetch the new commit.
2. Verify signature against the **lockfile-recorded** fingerprint.
3. On match: proceed.
4. On mismatch (key rotation by maintainer):
   - Refuse with `D026` finding ("Import package signed by unrecorded key").
   - User explicitly runs `mock import rotate <ext>/<pkg> --accept-new-key`
     to acknowledge.
5. On unsigned commit: refuse with `D027` ("Import package signature
   invalid or unsigned").

### SHA pinning + signature verification together

Both are required for trust:

- **SHA pin** ensures content integrity (no MITM substitution).
- **Signature** ensures source authenticity (the SHA WAS published by
  the legitimate maintainer).

SHA-1 is technically supported (40-hex sha) but SHA-256 (64-hex)
preferred when the host repo supports it. With signature verification,
an SHA-1 collision attack is insufficient — the attacker would still
need the maintainer's signing key.

### Trust commands

```
mock import update                     refresh lockfile (fetch latest, verify, prompt)
mock import update <ext>/<pkg>         refresh one import
mock trust accept <ext>/<pkg>          explicit y-press for a pending trust prompt
mock trust verify [<ext>/<pkg>]        re-run signature verification
mock import rotate <ext>/<pkg> --accept-new-key
                                       acknowledge a key rotation
```

## The `@` first-party source and trust model

`mock://@/...` URIs resolve to mockspace's first-party content. The
`@` placeholder is the binary's **hardcoded canonical mockspace project
git URL** plus signing-key fingerprint.

### Trust source

The mockspace binary embeds at compile time:

```rust
const MOCKSPACE_SOURCE_URL: &str = "https://codeberg.org/mockspace/mockspace.git";
const MOCKSPACE_CONTENT_KEY: &str = "SHA256:abc123...";   // canonical content-signing key fingerprint
const MOCKSPACE_LOG_KEY: &str = "SHA256:def456...";       // canonical log-signing key fingerprint (distinct)
```

The URL + content-signing key + log-signing key are the root of trust.
The binary's install channel (the user's package manager,
`cargo install`, the user's chosen distribution) is the actual
root-of-trust delivery; mockspace inherits whatever trust the user
already extended to the binary.

### Verification

On every `@/` resolution:

1. Resolve `@/...` against the binary's compiled-in URL.
2. Fetch the ref; verify the commit signature via `git verify-commit --raw`.
3. Extract the actual signing-key fingerprint; compare against the
   binary's compiled-in `MOCKSPACE_CONTENT_KEY`.
4. **Match**: proceed.
5. **Mismatch**: refuse with `D026` ("import package signed by
   unrecorded key"). For `@/`, this is a hard refuse; the binary's
   compiled-in fingerprint is authoritative.

There is no per-project `trust.toml` for the `@/` source. There is no
"first-run recording" step. The user's per-developer trust file
(`~/.config/mockspace/trust.toml` — see
[Trust on First Use](#trust-on-first-use-tofu-is-per-developer))
records TOFU acceptances for OTHER hosts (third-party imports), not
for `@/`. The `@/` source is governed entirely by the binary's
compiled-in constants.

### Override via mockspace.toml

If a project wants a non-default mockspace-core source (fork, mirror,
internal redistribution):

```toml
[hosts.mockspace-core]
url = "https://internal.example.com/mockspace.git"
key_fingerprint = "SHA256:def456..."
pinned_at = "<sha>"

[imports]
import = [
  "mock://ext/mockspace-core/export/profile-dev",
]
```

When `[hosts.mockspace-core]` is declared, the `@` shortcut is disabled
for that project; all references must use the explicit `ext/` form.
The override is a project-level configuration choice (committed to the
harness ref like any other config); developers cloning the project see
the override applies. There's no per-developer escalation; this is
the project's choice about its own upstream.

### Why this design

The canonical URL + keys are baked into the binary. Trust is rooted in
the binary, which the user obtains through their normal install channel.
Forks of mockspace must rebuild with their own canonical URL + keys
hardcoded.

This does NOT defend against compromise of the binary itself at install
time. The user's package manager / install channel is the root of trust,
exactly as it is for git, cargo, or any other tool. Mockspace inherits
whatever trust the user already extended to the binary — no more,
no less.

## The transparency log

**Optional** witness mechanism for projects that want defence-in-depth
against host-level supply-chain attacks beyond what signature
verification provides. Projects that don't configure a log get
signature + lockfile pinning as their full defence; that's appropriate
for most consumers.

Signed commits plus lockfile pinning together defend against simple
attacks (MITM substitution, opportunistic forgery) but do not defend
against:

- **Freeze attacks.** A compromised host keeps serving an old, validly
  signed commit even after the maintainer has shipped fixes. Consumers
  see no signature failure; their lockfile pins an old SHA; they have
  no out-of-band signal that they are stuck on a stale tip.
- **Surreptitious key rotation against per-developer TOFU.** A
  compromised maintainer can rotate keys; consumers' next encounter
  fires D026, but the developer's `mock import rotate --accept-new-key`
  accepts based only on developer-side trust. A third-party witness
  log raises the bar.

These are real attacks worth defending against in some contexts.
They're also rare enough at small-time scale that the transparency-log
infrastructure is not mandatory. Projects opt in by configuring
`[transparency]` in their `mockspace.toml`.

### The canonical log: `refs/mock/transparency-log`

The mockspace first-party project hosts an optional transparency log
as an orphan ref `refs/mock/transparency-log` on the binary-hardcoded
`@/` host. When a maintainer publishes a new `@/`-namespace package
version, they may additionally append a signed log entry. Consumers
that opt in via `[transparency]` configuration cross-check fetched
content against the log; consumers that don't, don't.

Each log commit's trailers carry the structured entry:

```
Mockspace-Log-Entry: v1
Mockspace-Package-Host: codeberg.org/mockspace/mockspace-rs
Mockspace-Package-Ref: refs/mock/export/runner-rs
Mockspace-Package-Version: 1.2.3
Mockspace-Package-SHA: a1b2c3d4e5f6789012345678901234567890abcd
Mockspace-Package-SHA-Algo: sha1
Mockspace-Package-Signing-Key: SHA256:abc123...
Mockspace-Observed-At: 2026-05-17T10:00:00Z
Mockspace-Witness: <signature of (host, ref, version, sha, key, at)>
```

The canonical log is signed by the binary-hardcoded
`MOCKSPACE_LOG_KEY` — a key DISTINCT from the content-signing key
(`MOCKSPACE_CONTENT_KEY`). The two keys can be held by the same
maintainer but should be in different security boundaries (different
hardware tokens, different machines, etc.). This separation is what
makes the log a third-party witness rather than a content-signer
self-attestation. The commit history is append-only by convention;
force-pushes are refused by branch protection on the canonical host.

### What the log defends against

- **Freeze attacks.** Consumers' `mock import update` fetches the log
  alongside the package fetch. If the locked package's SHA does not
  appear in the log, or appears only at an `observed_at` older than
  `staleness_threshold_days` (default 90), `mock doctor` raises
  `D032` ("locked import has no recent transparency-log witness").
  The maintainer can append a fresh log entry without changing the
  package SHA (just re-attesting "this is still current"); a
  compromised host that can no longer reach the maintainer cannot.

- **Surreptitious key rotation.** When a maintainer legitimately
  rotates keys, they append a `Mockspace-Key-Rotation` entry to the
  log signed by their OLD key naming the NEW key fingerprint. The
  consumer's `mock import rotate --accept-new-key` cross-references
  the log: the new key must appear in a rotation entry signed by the
  old key, OR the consumer must explicitly bypass via
  `--no-transparency-check`. A compromised maintainer who has the new
  key but not the old cannot produce a valid rotation entry.

- **Compromised host serving different content to different clients.**
  Two clients fetching the same package version see the same log
  entry. Disagreement between served content and log entry is a
  detectable inconsistency.

### Federation properties

The log is an orphan git ref. Any forge can serve it; any client can
mirror it; cross-mirror verification is trivial because the commits
are signed. The log is small (one commit per published version; a
busy ecosystem might produce a few hundred commits per year — `git
clone --depth 1` keeps the working size small). No separate service
infrastructure, no separate registry, no separate transparency
backend. It is git all the way down.

### Maintainer workflow

When a maintainer publishes a new version of an exported package:

1. `mock export publish <name>` pushes the export ref normally.
2. `mock export witness <name>` constructs the log entry, signs it,
   pushes a new commit to `refs/mock/transparency-log` on the
   canonical host (or to a project-local log; see below).

The two-step shape is intentional. `publish` is local to the
maintainer's host; `witness` is the act of cross-attesting on the
canonical log. A maintainer who does not witness is implicitly opting
out of transparency for that version, which the consumer's doctor
will surface as `D032`.

### Project-local logs

Projects that do not use the canonical mockspace first-party host can
configure their own log:

```toml
[transparency]
log_uri = "mock://ext/our-org-log/transparency-log"
log_signing_key = "SHA256:def456..."
```

The log can itself be hosted on any forge mockspace supports. The
property is "an independent witness exists", not "the first-party
canonical witness exists". A consumer importing from multiple
ecosystems may consult multiple logs.

### What the log does NOT do

- It does not prevent a compromised host from serving content; it
  detects that the served content disagrees with the cross-witness.
- It does not prevent a compromised maintainer (who has both old and
  new keys) from rotating without leaving a malicious trail; key
  rotation is detectable but not preventable.
- It does not provide non-repudiation of contributorship; it is a log
  of what the maintainer attested to, not of who pushed the source.

For a small-time ecosystem (handful of first-party packages plus
opportunistic third-party contributions), the asymmetric cost is good:
maintainers spend seconds per release on `mock export witness`;
consumers gain structural defence against the two most common
supply-chain attacks at zero per-fetch cost.

## The lockfile

`mockspace.lock` lives at the harness ref root. Cargo.lock-shaped:
technical pins only, no team-trust bookkeeping, no observation state.
Machine-managed; users don't edit by hand.

```toml
schema_version = "1.0"

# Pinned imports with SHA + signing-key fingerprint
[[imports]]
uri = "mock://ext/mockspace-rs/export/runner-rs"
host = "mockspace-rs"
kind = "export"
path = "runner-rs"
sha = "a1b2c3d4e5f6789012345678901234567890abcd"
sha_algo = "sha1"                                # or "sha256"
signing_key_fingerprint = "SHA256:abc123..."     # from git verify-commit --raw
signing_key_type = "ssh-ed25519"

[[imports]]
uri = "mock://@/export/profile-dev"
sha = "..."
# (no signing_key_fingerprint here; @/ uses binary-hardcoded key)

[[imports]]
uri = "mock://ext/arvo/export/lint/no-bare-numeric"
host = "arvo"
kind = "export"
path = "lint/no-bare-numeric"
sha = "..."
signing_key_fingerprint = "..."
```

### What the lockfile is (and isn't)

The lockfile is **project configuration committed to version control**,
exactly like `Cargo.lock` or `package-lock.json`. It records what
versions of what packages this project resolves; it makes a fresh
clone reproducible.

The lockfile is NOT a team-trust ledger. It does NOT record "Alice
trusted this on Tuesday" or "Bob fetched this last week". Per-developer
trust acceptance and observation state live elsewhere (see
[Per-developer trust](#per-developer-trust)).

The trust model is git's. Whoever has push access to the repository
can update the lockfile; reviewers review the diff like any other
config change. Mockspace doesn't impose a special supply-chain
trust-root ceremony on top of git's existing access-control mechanisms.
Projects that want signed commits, branch protection, or merge
gating use forge facilities (`commit.gpgsign`, branch rulesets,
required reviews) — not a mockspace-specific layer.

### Lockfile semantics

- Updated by `mock import update` (and on `mock init` for first-time
  imports).
- Read on every mockspace invocation that resolves an import.
- Compared on every fetch: if remote ref's resolved SHA differs from
  locked SHA, refuse fetch (mismatch) unless `--update` or
  `mock import update`.
- Signature fingerprint checked on every fetch: if commit signature
  doesn't match locked fingerprint, refuse with D026.
- Committed to the harness ref alongside `mockspace.toml`. Shared
  across developers via the harness ref. Per-developer TOFU + freshness
  state stays local.

### Verification flow on fetch

1. Look up import URI in `mockspace.lock`.
2. If not present in lockfile: first-time-for-this-project flow
   (`mock import update` or initial setup). Lockfile entry is written
   with the observed SHA + fingerprint.
3. If present: fetch the ref's current tip from the host (falling
   through mirrors if configured).
4. Verify the fetched commit's SHA matches `[[imports]].sha`.
   - On match: proceed.
   - On mismatch: refuse with diagnostic; user runs `mock import update`
     to acknowledge.
5. Verify the fetched commit's signature via `git verify-commit --raw`.
   Extract the actual signing-key fingerprint from the machine-readable
   status lines (NOT from human-readable output; format is unstable
   across git versions, locales, and signing backends). Compare against
   `[[imports]].signing_key_fingerprint`.
   - On match: proceed.
   - On mismatch: D026 finding.
6. **Per-developer TOFU gate:** if this developer's local trust file
   (`~/.config/mockspace/trust.toml`) has not seen this `(host,
   fingerprint)` tuple before, prompt for acceptance (interactive) or
   refuse (non-interactive). Acceptance lands in the local trust file;
   it does NOT mutate the lockfile.
7. Use the cached content (under `~/.cache/mockspace/imports/...`).

The lockfile is never written by routine fetch operations. Only
`mock import update`, `mock import rotate --accept-new-key`, and the
initial `mock init`-time lockfile creation write to it. This means
the harness ref does not accumulate noise commits from read-shaped
operations.

### Per-developer trust

Per-developer state lives in two places, both outside version control:

- **`~/.config/mockspace/trust.toml`** — TOFU acceptances. The
  developer's "I have personally seen this (host, fingerprint)
  combination" record. Analogous to SSH's `~/.ssh/known_hosts`.
- **`.git/mockspace/observations.toml`** — per-project, per-developer
  freshness cache. Records `last_observed_at` and `last_witness_at`
  per import. Used to compute `D030` (staleness) and `D032`
  (witness-staleness). Gitignored. Not shared.

The split is the central architectural property: the lockfile in the
harness ref carries policy (what versions are resolved); per-developer
trust + observation state lives per-developer (who has personally
accepted what, when each developer last verified freshness). This is
exactly the SSH model and exactly the Cargo model. Adding a write-loop
between fetches and the trust root (as earlier drafts did) was an
overreach corrected before lock.

## Hook protocol

Mockspace's reactive events fire **hooks**. A hook is a script invoked
with structured environment variables.

### Trust posture: hooks run with your environment

Hooks default to inheriting the parent process environment, the
parent process's `PATH`, and the developer's shell-resolved
binaries. This is the same trust posture as a Cargo `build.rs`, an
npm package's `scripts`, or a git `hooks/` script. **A hook can see
`SSH_AUTH_SOCK`, `GH_TOKEN`, every `*_TOKEN`, every shell credential
the developer has in their session.** If you import a hook from an
external host, you are extending that trust to that host's
maintainer.

The mitigations are upstream: signed commits, SHA pinning,
transparency-log witnessing, lockfile drift detection, and the
opt-in env/bins restrictions in [Env and bins policy](#env-and-bins-policy).
There is no sandbox at the hook-execution layer; sandboxing would
require a separate runner (which is the long-term direction for
[vehje as hook language](#future-direction-vehje-as-hook-language)
when that matures).

### Event vars passed to every hook

Default-inherited unless overridden by env policy (see next section):

```
MOCK_EVENT            event name
MOCK_PROFILE          active profile name
MOCK_ROUND_SLUG       active round slug, if any
MOCK_PHASE            current phase, if applicable
MOCK_HOST             hostname
MOCK_USER             git user.name + user.email
MOCK_TS               ISO 8601 UTC timestamp
MOCK_HELPERS          path to sourceable helpers
MOCK_NON_INTERACTIVE  "1" if --non-interactive or CI profile; "0" otherwise
```

Plus per-event vars (e.g., `MOCK_DIRTY_AREA`, `MOCK_DIRTY_FILES`).

### Sourceable helpers

For each event, `${MOCK_HELPERS}/<event>.sh` validates and exposes
event-specific vars with documentation. User hooks source the helper:

```bash
#!/usr/bin/env bash
source "${MOCK_HELPERS}/on_dirty_state.sh"
# Now you have MOCK_DIRTY_AREA, MOCK_DIRTY_FILES (array), etc.

if [ "$MOCK_DIRTY_AREA" = "round" ]; then
    git add ".mock/round/"
    git commit -m "auto-save from custom hook"
    exit 0
else
    exit 2  # fall back to mockspace default
fi
```

### Hook exit codes

- `0`: handled, proceed
- `1`: handled but failed, abort with diagnostic
- `2`: not handled, fall back to mockspace's default behaviour (refuse)
- Any other: treated as `1`

### Hook event registry

| Event | When it fires | Action options |
|---|---|---|
| `on_dirty_state` | Phase transition with dirty `.mock/<area>/` | prompt commit / auto-commit / refuse |
| `on_phase_race` | Mock-side ref push race lost | prompt-resolve / auto-rebase / side-branch-refuse |
| `on_replan_nonclaimed_edits` | Replan refused due to non-claimed source edits | prompt / refuse |
| `on_doctor_finding` | mock doctor found inconsistency | prompt / auto-repair / refuse |
| `on_pr_body_conflict` | PR body managed-section regen race | prompt / auto-overwrite / backup |
| `on_external_unpinned` | Non-pinned external in rendered output | warn / refuse |
| `on_schema_version_skew` | Binary version mismatch | warn / refuse |
| `on_archive_contention` | Archive ref push race | retry / surrender |
| `on_verifier_failure` | Manifest claim verifier failed at seal | abort / continue with --force |
| `on_first_trust` | First-time import requiring trust acknowledgement | prompt y/n / auto-accept / refuse |
| `on_signing_mismatch` | Signature doesn't match locked fingerprint | prompt / refuse |

## Env and bins policy

Hooks run with the user's parent environment by default (same trust
model as git hooks, npm scripts, Cargo build.rs). Users opt into
restrictions via `[profile.<name>.env]` and `[profile.<name>.bins]`
sections using a unified glob + negation syntax.

### Glob + negation syntax

A list field with semantics:

- Field absent or empty: **inherit everything from parent** (no
  filtering).
- Field present: build the resulting set left-to-right:
  - `"*"` adds all parent items to the set
  - `"<exact>"` adds that item if present in parent
  - `"<glob>"` adds all parent items matching the glob
  - `"!<exact>"` removes from set
  - `"!<glob>"` removes all matching from set

Order matters; later entries override earlier. Negation is the only way
to remove from the set.

**Glob dialect.** Gitignore-style globs (familiar to every developer
working with git): `*` matches any run of characters within a single
segment, `**` is reserved for future hierarchical use, `?` matches one
character, `[A-Z]` matches one character from the bracketed class, `\`
escapes a literal special character. No brace expansion, no command
substitution, no extglob. Mockspace parses globs once at config load
and refuses unsupported syntax with a structured diagnostic.

**Case sensitivity.** Env-variable names: case-sensitive on Linux and
macOS (matches POSIX semantics); case-insensitive on Windows but
preserving case of first occurrence. Bin names: case-insensitive on
Windows (matches `PATH` resolution semantics); case-sensitive on
Linux and macOS. Cross-platform configs that need to work everywhere
should write env names in uppercase by convention.

**Worked edge cases:**

| Pattern | Parent set | Result | Notes |
|---|---|---|---|
| (absent / empty) | `{FOO, BAR, BAZ}` | `{FOO, BAR, BAZ}` | full inheritance |
| `["FOO", "!FOO"]` | `{FOO, BAR}` | `{}` | empty; later overrides earlier |
| `["*", "!*"]` | `{FOO, BAR}` | `{}` | last clears everything |
| `["*", "!*", "BAR"]` | `{FOO, BAR}` | `{BAR}` | add-back after clear |
| `["[A-Z]*"]` | `{FOO, bar}` | `{FOO}` | bracket class |
| `["FOO"]` | `{BAR, BAZ}` | `{}` | FOO not in parent; not synthesised |
| `["*_TOKEN"]` | `{GH_TOKEN, FOO}` | `{GH_TOKEN}` | suffix glob |
| `["*", "!*_TOKEN*"]` | `{GH_TOKEN, FOO}` | `{FOO}` | wide allow + targeted deny |
| `["!FOO"]` | `{FOO, BAR}` | `{}` | bare negation; nothing added first |

The "bare negation" case (`["!FOO"]` with no preceding add) results in
an empty set, NOT the parent minus FOO. The mental model is: the
output set starts empty and entries either add to it or remove from it.
A negation against an empty set is a no-op.

### Env policy

```toml
[profile.dev.env]
# Default (absent): inherit parent env unchanged
# Inherit all (explicit):
inherit = ["*"]

# Inherit all but token-shaped vars:
inherit = ["*", "!*_TOKEN*", "!*_SECRET*", "!*_KEY*", "!*_CREDENTIALS*", "!*_PASSWORD*"]

# Strict allowlist:
inherit = ["MOCK_*", "PATH", "HOME", "TERM", "LANG"]
```

Per-event override: `[profile.<name>.on_<event>.env].inherit = [...]`
overrides the profile default for that event.

### Bins policy

Controls which binaries are on PATH for the hook. Same glob + negation
syntax:

```toml
[profile.dev.bins]
# Default (absent): inherit full parent PATH
# Strict allowlist (no network tools):
inherit = ["git", "grep", "sed", "awk", "cat", "echo", "ls", "find", "test"]

# Inherit all but explicit denials:
inherit = ["*", "!curl", "!wget", "!nc", "!ncat", "!socat", "!ssh"]
```

Implementation: mockspace constructs a temp directory, symlinks the
allowed binaries from the resolved parent PATH, sets `PATH` to that
temp dir for the hook subprocess. On hook exit, temp dir is cleaned up.

### Combining env and bins

A hook with the strict allowlist for both gets a tightly scoped
environment. A hook with neither set runs with full parent env + PATH
(default).

Mockspace's auto profile defaults are deliberately permissive:

```toml
[profile.dev.env]
# no restriction by default; user opts in if they want

[profile.dev.bins]
# no restriction by default; user opts in if they want
```

Users who want defense-in-depth can opt into restrictive policies
without mockspace forcing them.

## Profiles and reactive policy

```toml
[mockspace]
default_profile = "dev"

[profile.dev]
on_dirty_state = "prompt"
on_phase_race = "prompt"
on_replan_nonclaimed_edits = "prompt"
on_doctor_finding = "prompt"
on_pr_body_conflict = "prompt"
on_external_unpinned = "refuse"
on_schema_version_skew = "refuse"
on_archive_contention = "auto"
on_verifier_failure = "refuse"
on_first_trust = "prompt"
on_signing_mismatch = "refuse"

[profile.ci]
on_dirty_state = "refuse"
on_phase_race = "refuse"
on_replan_nonclaimed_edits = "refuse"
on_doctor_finding = "refuse"
on_pr_body_conflict = "refuse"
on_external_unpinned = "refuse"
on_schema_version_skew = "refuse"
on_archive_contention = "auto"
on_verifier_failure = "refuse"
on_first_trust = "refuse"
on_signing_mismatch = "refuse"

[profile.auto]
on_dirty_state = "auto"
on_phase_race = "auto"
on_replan_nonclaimed_edits = "refuse"
on_doctor_finding = "auto"
on_pr_body_conflict = "auto"
on_external_unpinned = "refuse"
on_schema_version_skew = "refuse"
on_archive_contention = "auto"
on_verifier_failure = "refuse"
on_first_trust = "refuse"          # CI/auto contexts should pre-trust via lockfile
on_signing_mismatch = "refuse"
```

### Handler value types

Each `on_*` field accepts:

1. **Built-in directive**: `"prompt"`, `"refuse"`, `"auto"`. Resolves to
   the mockspace-shipped handler from the embedded baseline.
2. **`mock://` URI**: import; SHA-pinned required for external URIs that
   resolve to executable content.
3. **Script path**: relative to harness root (e.g., `"hooks/on_dirty_state.sh"`).

Hook scripts have a default size cap of 1 MB and a default execution
timeout of 60 seconds, both configurable per profile via
`[profile.<name>].hook_max_bytes` and `[profile.<name>].hook_timeout_seconds`.

**Inline bash in TOML is not a hook value type.** Earlier drafts of
this design permitted multi-line TOML strings as hook bodies; that
shape was removed before lock. Inline executable code in
`mockspace.toml` is low-visibility to reviewers (TOML diffs are
line-noisy and reviewers' eyes glaze over) while carrying the same
trust authority as a checked-in hook file. Forcing hook code into
files under `hooks/` puts the executable surface on the reviewable
file-diff surface where reviewers naturally look.

`mock doctor` raises `D036` ("hook value contains inline shell")
if any `on_*` field's value parses as inline bash (starts with `#!`
or contains newlines and non-path characters). The check is
heuristic; the structural defence is that file paths and `mock://`
URIs are the documented value types.

### Profile selection precedence

```
CLI flag (--profile=<name>)            highest priority
   ↓
Env var (MOCK_PROFILE=<name>)
   ↓
mockspace.toml [mockspace].default_profile
   ↓
Built-in "dev"                          lowest priority
```

One-shot CLI flags: `--interactive` / `--non-interactive`,
`--auto-repair` / `--no-auto-repair`.

### Embedded baseline + first-party imports

mockspace's binary embeds a baseline of helper scripts + default
handlers (extracted to `~/.cache/mockspace/helpers/` on first run).
Equivalent to what you'd get from `mock://@/handler/<directive>/<event>`.

Projects rely on the embedded baseline by default; can import explicitly
for version-pinning.

## Language-specific runners

Mockspace-core handles bash by default. Other languages via per-language
runner packages imported from language-specific mockspace extensions.

```toml
[hosts.mockspace-rs]
url = "https://codeberg.org/mockspace/mockspace-rs.git"

[imports]
import = [
  "mock://ext/mockspace-rs/export/runner-rs@<sha>",
]

[imports.ext.mockspace-rs]
include = ["hooks/**/*.rs", "lints/**/*.rs"]
runner = "mock://ext/mockspace-rs/export/runner-rs"
```

A runner package exports a binary that mockspace invokes when a file's
extension matches `include`. The runner receives standard hook env vars
+ event-specific vars and is responsible for compilation/execution.

## `mockspace.toml` schema

```toml
[mockspace]
version = "1.0"                    # intended mockspace tool version (major.minor)
default_profile = "dev"
default_one_active_round = true
verifier_timeout_seconds = 30      # per-verifier wall-clock budget

# Commit-signing of harness commits is not a mockspace setting.
# Projects that want signed commits use git's commit.gpgsign or forge
# branch protection rules. Mockspace is agnostic.

[refs]
mirror_ext_refs = true
push_mirrors = false
fetch_on_reference = true
task_archive_threshold_days = 90
round_archive_threshold_days = 365

[refs.security]
# domain_allowlist = ["github.com", "codeberg.org", "*.example.com"]
require_https = true               # http:// URLs refused at config load

[forge]
type = "github"                    # or "forgejo"
token_env = "MOCK_FORGE_TOKEN"
auto_open_pr = true
auto_push_body = true
auto_merge_on_done = false
merge_style = "squash"
default_base_branch = "dev"
pr_body_managed_section_delimiter_start = "<!-- mockspace-managed -->"
pr_body_managed_section_delimiter_end = "<!-- /mockspace-managed -->"
api_retry_attempts = 3
api_retry_backoff_seconds = [1, 4, 16]

# Hosts
[hosts.mockspace-rs]
url = "https://codeberg.org/mockspace/mockspace-rs.git"

[hosts.arvo]
url = "https://github.com/orgrinrt/arvo.git"
forge_url_template = "https://github.com/orgrinrt/arvo/tree/{ref}"

# Imports
[imports]
import = [
  "mock://hook/on_custom_doctor.sh",
  "mock://@/export/profile-dev",
  "mock://ext/mockspace-rs/export/runner-rs@<sha>",
]

[imports.ext.mockspace-rs]
include = ["hooks/**/*.rs", "lints/**/*.rs"]
runner = "mock://ext/mockspace-rs/export/runner-rs"

# Profiles
[profile.dev]
on_dirty_state = "prompt"
# ... (see Profiles section)

[profile.dev.env]
# inherit = ["*", "!*_TOKEN*"]    # optional restriction

[profile.dev.bins]
# inherit = ["*", "!curl", "!wget"]   # optional restriction

[profile.ci]
on_dirty_state = "refuse"
# ... (see Profiles section)

[profile.auto]
on_dirty_state = "auto"
# ... (see Profiles section)

# Per-lint severity (legacy lint-pack support during migration)
[lints.no-bare-numeric]
commit = "error"
build = "warn"
push = "error"
```

## Manifest schema

```toml
mockspace_version = "1.0"
round_slug = "202605171800-trunk-refactor"
phase = "doc"

[scope]
description = "..."
in_scope_tasks = [
  "mock://task/compiler::ir::structural-robust-ir",
  "mock://task/compiler::ir::lower-pass#define-grammar",
]
out_of_scope = [
  "Renaming the feat/ branch prefix convention (separate concern).",
]

[acceptance]
criteria = """
1. Every claimed file has a passing verifier at seal time.
2. mock doctor returns clean on the resulting state.
"""

# Per-file changes. Each is a claim.
[[change]]
task = "mock://task/compiler::ir::structural-robust-ir#define-grammar"
file = "crates/ir/src/grammar.rs"
description = "Rename Bar to Baz; update doc-comments on the trait."
# Structured verifier (see Verifier catalog section):
[change.verify]
all_of = [
  { kind = "grep_present", pattern = "pub struct Baz", file = "crates/ir/src/grammar.rs" },
  { kind = "grep_absent",  pattern = "pub struct Bar", file = "crates/ir/src/grammar.rs" },
]

[[change]]
task = "mock://task/compiler::ir::lower-pass#define-grammar"
file = "docs/DESIGN.md"
description = "Update IR section to describe the new Baz struct."
[change.verify]
kind = "grep_present"
pattern = "Baz"
file = "docs/DESIGN.md"

# Required when superseding a deprecated manifest (after replan):
[[deprecated_accounting]]
file = "crates/ir/src/old_helper.rs"
omitted_reason = "Concept removed entirely; the file no longer applies."
```

### Validation rules at seal time

1. **TOML well-formed.**
2. **Required fields present.**
3. **Schema version compatible.**
4. **All task refs resolve.**
5. **All step-key refs resolve.**
6. **Step phase tags match manifest phase.**
7. **All required steps closed.**
8. **Files exist or are created by the change.**
9. **Verifier checks pass** against source-side branch tip, executed in
   a temporary worktree (`git worktree add --detach <temp> <tip-sha>`).
   All-pass-or-no-transition.
10. **Deprecated accounting complete.** Every `file` from
    `manifest.<phase>.deprecated.<n>.toml` must appear either as
    `[[change]].file` in the new manifest OR in `[[deprecated_accounting]]`
    with `omitted_reason`. Paths canonicalised (resolve `..`, `.`,
    trailing slashes, symlinks resolved at capture time and re-verified)
    before comparison.

Validation is all-pass-or-no-transition. On failure, structured
five-element diagnostic.

## Verifier catalog

Mockspace ships a strict, structured set of verifier kinds. **No free
shell execution.** New kinds added upstream by contributing to mockspace
core or to a language-specific extension.

### Built-in verifier kinds

| Kind | Description | Fields |
|---|---|---|
| `grep_present` | Regex match present in file | `pattern`, `file` |
| `grep_absent` | Regex match absent from file | `pattern`, `file` |
| `path_exists` | Path exists in working tree | `file` |
| `path_absent` | Path does not exist | `file` |
| `file_size_below` | File byte count below threshold | `file`, `bytes` |
| `file_size_above` | File byte count above threshold | `file`, `bytes` |
| `line_count_below` | File line count below threshold | `file`, `lines` |
| `line_count_above` | File line count above threshold | `file`, `lines` |
| `json_field_equals` | JSON field at path equals value | `file`, `path`, `value` |
| `toml_field_equals` | TOML field at path equals value | `file`, `path`, `value` |
| `yaml_field_equals` | YAML field at path equals value | `file`, `path`, `value` |
| `all_of` | All sub-checks pass | `all_of = [...]` |
| `any_of` | Any sub-check passes | `any_of = [...]` |
| `not` | Negate a sub-check | `not = { ... }` |

### Language-specific verifier kinds

Contributed by language extensions (imported separately):

| Kind | Source | Description |
|---|---|---|
| `function_present` | mockspace-rs / mockspace-ts | Function with given name declared in file |
| `function_absent` | mockspace-rs / mockspace-ts | Function not declared |
| `type_implements_trait` | mockspace-rs | Type implements a specified trait |
| `import_present` | mockspace-rs / mockspace-ts | Specific import statement present |
| `module_exports` | mockspace-ts | Module exports a named binding |
| ... | (extensible) | |

### Adding new verifier kinds

When a project needs a check not in the catalog:

1. Propose the kind upstream (mockspace-core for generic kinds,
   language-specific extension for language-bound kinds).
2. The kind is reviewed and merged.
3. Bump the relevant mockspace-version requirement.
4. Consumers using the new kind set `mockspace_version` to the version
   that ships it.

This is healthier than per-project shell escape hatches:

- Verifier kinds become a shared vocabulary.
- Each kind is implemented once and vetted.
- No project-specific verifier-RCE surface.

### No shell escape hatch

Earlier drafts of this design permitted an opt-in `command_succeeds`
verifier behind `allow_shell_verifiers = true`. That escape hatch was
removed before lock. The structural defence is that **verifier kinds
are a closed, structurally-typed catalog**. A project that needs a
check the catalog does not yet support contributes the new kind
upstream rather than reaching for a per-project shell escape.

The rationale: a shell-form verifier reachable from a PR-author-
controlled manifest is a code-execution surface. Even with strict
env/bins policy, the manifest itself can construct arbitrary command
strings (`command = "$(curl ...)"`). Lockfile pinning and signing do
not help here because the manifest lives in the consumer's own
source-side branch and is authored fresh per PR. The only structural
answer is "no free-shell at the verifier layer at all".

If a verifier kind appears genuinely missing for a legitimate use
case, the path is `mock new verifier-kind <name>` (contribute
upstream) plus a temporary workaround using existing kinds (often
`grep_present` / `path_exists` / `file_size_*` can compose to cover
the case).

### Regex and parser hardening

`grep_present` / `grep_absent` use Rust's `regex` crate (linear-time,
no catastrophic backtracking). Mockspace mandates this engine; PCRE-
style backtracking engines are rejected at the verifier-implementation
layer.

`yaml_field_equals` uses safe-load mode only; YAML custom tags
(`!!python/object`, `!Ref`, etc.) and YAML anchors that reference
external content are refused with a structured diagnostic.

Each verifier execution has a wall-clock budget of `verifier_timeout_seconds`
(default 30s, configurable per-project) inherited from the hook
timeout machinery. Exceeding the budget is a verifier failure.

### `file` field constraints

Every verifier kind takes a `file` field naming a path within the
source tree. The field is **PR-author-controlled** (it comes from
manifests in the source-side branch). Mockspace applies the same
path-traversal defence to `file` as it does to `mock://` intra-package
paths:

- The path MUST be relative to the temporary worktree's root.
- Absolute paths are refused.
- `..` segments are refused after lexical canonicalisation.
- Symbolic links that resolve outside the worktree root are refused.
- Special paths (`/dev/*`, `/proc/*`, named pipes, sockets) are
  refused; only regular files and directories are accepted.
- Maximum file-size budget (default 16 MB; configurable via
  `[verifier].max_file_bytes`) — verifier short-circuits with a
  diagnostic if the file exceeds.

Mockspace reads file contents via `std::fs::read_to_string` (or
`read` for binary kinds), NOT via shell or external commands. There
is no command-substitution evaluation on `file` values.

## Version compatibility

Cargo-style versioning.

- **`[mockspace].version`** declares intended major.minor.
- **`Mockspace-Version:`** commit trailer records exact tool version
  that wrote each commit.

### Compatibility matrix

| Binary major | Binary minor | Status |
|---|---|---|
| same major | same or higher minor | proceed |
| same major | lower minor than declared | refuse (upgrade binary) |
| higher major | any | refuse (run `mock upgrade` after upgrade) |
| lower major | any | refuse (upgrade binary) |

### Forward-incompatible read fallback

Older binary on newer-minor refs: read operations proceed (read schema
from trailer); writes refuse.

### Reading historical refs

Closed/archived refs record their writing version. Mockspace ships
parsers for all historical schemas indefinitely.

### `mock upgrade`

Migrates active artefacts to a new version. Single commit on harness +
round refs.

## Active phase storage on refs

| State | Command | Storage actions |
|---|---|---|
| (no round) | `mock round new <slug>` | create `refs/heads/round/<slug>` + orphan `refs/mock/round/<slug>` with initial tree |
| TOPIC | `mock phase plan` | scaffold doc manifest; rewrite `.phase`; commit |
| PLAN(DOC) | `mock phase apply` | validate; run verifier; capture anchor; transition; pull-rebase-push; forge API |
| APPLY(DOC) | `mock phase finish` | scaffold src manifest; transition |
| PLAN(SRC) | `mock phase apply` | validate; verifier; capture anchor; transition |
| APPLY(SRC) | `mock phase finish` | transition to DONE |
| DONE | `mock close` | fetch comments; freeze; optional merge |

## Transition atomicity

```
mock phase apply (from PLAN(DOC)):

  1. Acquire .git/mockspace/.lock via flock(2).
  2. Fetch refs/mock/round/<slug> from origin (fast-forward only).
  3. Verify local tip == remote tip; on divergence: invoke on_phase_race.
  4. Verify clean state in .mock/round/ (or auto-commit per profile).
  5. Read source-side branch tip SHA.
  6. Validate manifest.
  7. Run claim verifier in temporary worktree at branch tip.
     All-pass-or-no-transition.
  8. Capture per-file blob SHAs into .anchor.<phase>.toml.
     Store blob contents content-addressed under
     .anchor.<phase>.blobs/<sha-prefix>/<sha-rest> (see
     [Content-stable anchors](#content-stable-anchors)).
  9. Build new tree for round ref.
  10. Render source-tree and local-only targets locally first
      (see [Rendering pipeline](#rendering-pipeline)). On failure here:
      abort BEFORE the ref update; nothing pushed.
  11. git update-ref refs/mock/round/<slug> <new-commit>.
  12. git push origin refs/mock/round/<slug>.
      On non-fast-forward: invoke on_phase_race (see below).
  13. Release .git/mockspace/.lock.
  14. If [forge].auto_open_pr: attempt forge API target render with retry.
      On failure: log warning; round state is valid; recommend
      mock pr regen.
```

`--no-forge` skips step 14. `--resume` re-runs steps 12 and 14.

### Render-failure ordering

Source-tree and local-only renders run BEFORE the ref update (step 10),
so a render failure aborts cleanly without leaving the round ref
ahead of local state. Forge-API render runs AFTER the ref push (step
14), so a forge failure leaves a valid round-state on the remote;
the user runs `mock pr regen` later to retry. This ordering is the
"local-first commit, public-last announce" pattern: the durable state
lands first, the announcement happens after.

### `on_phase_race` handler

When step 12 detects a non-fast-forward (remote tip moved between
fetch and push):

```
  12a. Refuse the push.
  12b. Rename local round-ref tip to
       refs/mock/round/<slug>-conflict-<host>-<ts>.
  12c. PUSH the conflict side-branch to origin BEFORE local reset
       (so the lost work is durable even if this machine dies next).
       On push failure here: hard-stop. Do not reset local. Emit D037
       ("race conflict could not be preserved on remote; local state
       retained"). User intervention required.
  12d. Reset local round ref to the remote tip we just observed.
  12e. Invoke on_phase_race per profile.
       Default: refuse (user runs `mock phase resolve <slug>`).
```

**Never silently drops work.** Step 12c is the structural guarantee:
the conflict side-branch lives on the remote before any local reset
happens. If 12c fails, mockspace stops; partial state on the local
machine is recoverable, but no automatic reset can race with a
machine crash to lose the work.

### flock semantics and filesystem caveats

`flock(2)` BSD-style. Lock file content: hostname + PID + start time
for debugging only. Kernel-managed; auto-released on process exit.

The flock-based design assumes the filesystem honours POSIX advisory
locks. Known unsupported / unreliable substrates:

- **NFS (any version):** not supported. Use a local filesystem.
- **sshfs / CIFS / SMB:** flock often returns success without actually
  locking. Concurrent writers can both "win". Not supported.
- **Cloud-sync directories** (`iCloud Drive`, `Dropbox`, `OneDrive`,
  `Google Drive`): the local FS honours flock but the userspace sync
  daemon interposes and may sync partial states between machines.
  Two developers using cloud-synced clones can produce corruption.
  Strongly discouraged; mockspace doctor raises `D038`
  ("`.mock/` parent appears to be a cloud-sync directory") on
  detection.
- **FUSE filesystems generally:** behaviour varies per implementation;
  test before relying.
- **Docker bind-mounts on macOS:** xnu flock semantics through the
  Linux VM are inconsistent. Use a Linux container or a Linux host
  for production CI.

`mock doctor` detects substrate via `df -T` (Linux) or `mount` (macOS)
plus heuristic path-marker checks (e.g., is the parent path under
`~/Library/Mobile Documents/com~apple~CloudDocs/`?). D038 is a
soft warning, not a hard refuse; users on unsupported substrates
proceed at their own risk.

Windows behaves differently; see
[Windows support and platform notes](#windows-support-and-platform-notes).

## Content-stable anchors

```toml
# .mock/round/.anchor.doc.toml
mockspace_version = "1.0"
captured_at = "2026-05-17T11:30:00Z"
captured_from_source_branch_tip = "abc123def456..."

[[file]]
path = "crates/foo/src/lib.rs"
blob_sha = "a1b2c3d4e5f67890..."

[[file]]
path = "crates/foo__src/lib.rs"     # different path, possibly same content
blob_sha = "a1b2c3d4e5f67890..."

[[file]]
path = "docs/DESIGN.md"
blob_sha = "deadbeef..."
```

**Content-addressed storage.** Blob contents live under
`.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>` in the round ref's
tree, keyed by the blob's SHA, not by the source path. Example:

```
.anchor.doc.blobs/
  a1/b2c3d4e5f67890...      <- one blob, referenced by both files
  de/adbeef...
```

Two-character SHA prefix as the first directory level matches git's
own object-store layout and keeps any single directory bounded in
size.

**Why content-addressed.**

- **No path-flattening collision.** Path-flattening schemes
  (`crates/foo/src/lib.rs` → `crates__foo__src__lib.rs`) collide when
  the source path naturally contains the chosen separator. Two
  distinct paths can flatten to the same name and either overwrite or
  silently lose anchor data. Content-addressing eliminates this
  failure mode entirely.
- **Dedupe for free.** Multiple files with identical content share
  one blob in the anchor storage. Common for projects with many
  similar boilerplate files or with a refactor that propagates
  identical lines across files.
- **Integrity verification is a tautology.** The on-disk name IS the
  expected hash; restoration recomputes the hash of the read bytes
  and compares to the path-name. Tampering at the storage layer is
  detected automatically.
- **Non-UTF-8 source paths handled.** Source paths can be arbitrary
  bytes (git tolerates non-UTF-8); the blob storage layout uses only
  hex digits and is safe on every filesystem.

**Restoration flow.** During replan, for each `[[file]]` entry:

1. Read `path` and `blob_sha`.
2. Read bytes from `.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>`.
3. Verify SHA of read bytes matches `blob_sha`.
4. Write bytes to `path` on the source-side worktree.

Findings:

- `D004`: anchor blob SHA mismatch (read bytes hash to a different SHA).
- `D040`: anchor blob missing from storage (entry references a SHA
  that doesn't exist under `.anchor.<phase>.blobs/`).

Reachable via being part of the round ref's tree directly; survives
force-push, GC, rebase of source-side branch.

## Replan flow

### Replan from APPLY(...)

1. Verify parent worktree on round's source-side branch.
2. **Check for non-claimed source-side edits** since APPLY entry
   (uncommitted worktree changes outside the claimed-files set).
   Invoke `on_replan_nonclaimed_edits`. Default refuse.
3. **Check for post-APPLY commits to claimed files.** Walk the
   source-side branch from the APPLY-entry commit to current tip; for
   each commit, intersect changed-files with claimed-files. If any
   claimed file was modified by a post-APPLY commit, the destructive
   restore would overwrite that work. Refuse with structured diagnostic
   naming each affected file and the commits that touched it. The user
   must either:
   - Revert the post-APPLY commits to claimed files (returning to
     anchor-equivalent state), then re-run replan.
   - Pass `--accept-restoration-loss <file>...` for each file whose
     post-APPLY work the user explicitly chooses to discard.
   - Run `mock phase replan --restore-by-commit` instead (see below)
     which preserves the post-APPLY state by applying anchor
     restoration as a new commit on top.
4. Verify source-side worktree clean for claimed files.
5. Read `.anchor.<phase>.toml`.
6. For each `[[file]]`: restore blob to file path; verify SHA.
7. Auto-commit restorations on source-side branch:
   `Workflow-Replan: restore <phase>-side surfaces from anchor`.
8. Build new round-ref tree: rename manifest to `.deprecated.<n>.toml`,
   scaffold fresh manifest, rewrite `.phase`, delete anchors.
9. Commit + push round ref.
10. Forge API: regen PR body.

### Replan modes

- **`mock phase replan`** (default): the destructive flow above.
  Refuses on post-APPLY claim-file commits without explicit per-file
  acceptance.
- **`mock phase replan --restore-by-commit`**: additive flow. Same
  steps but step 3 is skipped and step 6 commits restoration on top
  of the current source-side state rather than overwriting. The
  source-side branch retains post-APPLY history; anchor restoration
  appears as a new commit
  (`Workflow-Replan: restore via additive commit`). This preserves
  work at the cost of a more cluttered history.

### Replan from PLAN(...)

Deprecates draft manifest. No source-side restoration (no anchor exists).

### Deprecation accounting

Validation at seal time of new manifest enforces every `file` in the
deprecated manifest is either claimed or in `[[deprecated_accounting]]`.
Symlink resolution + path canonicalisation applied.

## Tasks and the archive ref

```
refs/mock/task/<ns-path>/<slug>/
├── .state
├── meta.toml
└── <slug>.md
```

`meta.toml` schema:

```toml
mockspace_version = "1.0"
slug = "structural-robust-ir"
namespace = "compiler/ir"
title = "..."
created = "2026-05-17T10:00:00Z"
priority = "P1"
group = "ref-based-redesign"

[steps.define_grammar]
description = "..."
phase = "doc"
state = "closed"

[refs]
blocks = []
blocked_by = []
relates_to = []

[closure]
resolution = "completed"
closed_at = "2026-06-18T14:00:00Z"
closed_branch = "feat/round/202605171800-trunk-refactor"
closing_phase = "apply_src"
closing_round_slug = "202605171800-trunk-refactor"
```

### Task commands

```
mock task new <ns>::<slug> [--title=...]
mock task start <ns>::<slug>
mock task block <ns>::<slug>
mock task defer <ns>::<slug>
mock task close <ns>::<slug> --resolution=<kind>
mock task archive <ns>::<slug>
mock task move <old> <new>
mock task list [--namespace=<ns>] [--include-archive]
mock task show <ns>::<slug>
mock task step start <ns>::<slug>#<key>
mock task step close <ns>::<slug>#<key>
```

`mock task new` fetches origin first; refuses if ref exists remotely or
in archive.

### Archive ref

`refs/mock/task-archive` carries closed tasks as unified tree.
Auto-archival per `task_archive_threshold_days` on `mock close` (full
sweep) and `mock sync --full`.

### Concurrent archive ref writes

`on_archive_contention`: exponential backoff with jitter, unbounded
retries until success or 60s timeout (configurable).

## Rendering pipeline

Three targets:

1. **Source-tree**: committed to `refs/heads/*`. README, docs, etc.
2. **Local-only**: filesystem only, gitignored. `.claude/rules/`,
   `.github/instructions/`.
3. **Forge API**: PR title + body, pushed via API.

Multi-target rendering:

```toml
[render]
template = "some-doc.md.tmpl"
[[render.targets]]
target = "source_tree"
path = "docs/some-doc.md"
[[render.targets]]
target = "local_only"
path = ".claude/rules/some-doc.md"
```

Round artefacts, task artefacts, research/bench/sketch never render
anywhere.

## PR lifecycle

### Branch creation

Source-side branch off `[forge].default_base_branch` with scaffolding
commit at `mock round new`.

### PR creation

Auto-opened at PLAN(DOC) → APPLY(DOC) per `auto_open_pr`.

### PR body autogeneration

Body has managed section with HTML-comment delimiters and a clear
visible warning:

```markdown
<!-- mockspace-managed -->
> ⚠️ Auto-generated. Edits inside this block are overwritten on each
> phase transition. Put notes above this block to preserve them.

## Description
...
<!-- /mockspace-managed -->
```

### Conditional updates

Etag-conditional when forge supports; on 412 conflict: refetch, retry
once. On retry conflict: write backup to `.mock/pr.backup.<ts>.md`,
emit warning, never silently overwrite.

If delimiters missing: refuse + backup; suggest `mock pr regen --force`
to re-establish.

### Auto-merge

`auto_merge_on_done = false` default. When true, mockspace issues merge
API call; respects branch protection; round state advances regardless.

### Token scopes

- **GitHub**: classic PAT with `repo`, or fine-grained PAT with
  `contents:read-write` + `pull-requests:read-write`.
- **Forgejo/Codeberg**: PAT with `repository:write` + `issue:write`.

### Retry policy

3 attempts, [1, 4, 16] second backoff. 429 honors `Retry-After`
header; no in-invocation retry on rate limit.

### Comment ingestion

Mandatory unless `--no-comments`. Paged with checkpointing.
`.comments.status` records `complete` / `partial` / `skipped:<reason>`.

## `mock sync` and the staleness model

`.mock/<area>/.ref-sha` tracks rendered-from-SHA per area. Every
mockspace invocation does staleness check.

`mock sync` commands:

```
mock sync                          sync all areas
mock sync round                    sync only round
mock sync harness                  sync only harness
mock sync tasks                    sync all active tasks
mock sync --full                   sync + run age-based auto-archival
mock sync --force                  discard local edits and force re-render
mock sync --check                  print staleness state; change nothing
```

Auto-archival on `mock close` (full sweep of closed tasks past threshold)
and `mock sync --full`. Never on read commands.

## `mock doctor`

Read-only by default; `--repair` applies fixes.

### Findings

| ID | Description |
|---|---|
| D001 | Round ref exists but `.mock/round/` doesn't |
| D002 | `.mock/round/` exists but no round ref |
| D003 | `.mock/round/` content drift from ref tree |
| D004 | Anchor blob SHA mismatch |
| D005 | PR phase ≠ round-state phase |
| D006 | Source-side branch missing or empty |
| D007 | Pre-existing task ref + archive entry for same identity |
| D008 | Stale `.git/mockspace/.lock` (flock owner not running) |
| D009 | Local mirror ref tip ≠ remote |
| D010 | Manifest references unknown task ref |
| D011 | Sealed manifest has been edited |
| D012 | Round ref local tip ahead of remote |
| D013 | Round ref local tip behind remote |
| D014 | Schema-version mismatch |
| D015 | Non-pinned external in rendered output |
| D016 | Archived task ref's tree drifts from immutability rule |
| D017 | PR body cache differs from forge managed section |
| D018 | `.gitignore` does not include `.mock/` |
| D019 | Round DONE but PR open past threshold |
| D020 | Round-ref race: side-branch at `<conflict-ref>` |
| D021 | (retired; project trust.toml removed; per-developer trust file replaces) |
| D022 | Non-pinned external import resolving to executable |
| D023 | Verifier ran against working tree with parent dirty (fallback used) |
| D024 | (retired; shell verifier no longer reachable) |
| D025 | External mirror force-update detected; not yet accepted |
| D026 | Import package signed by unrecorded key |
| D027 | Import package signature invalid or unsigned |
| D028 | (retired; `allow_shell_verifiers` removed from schema) |
| D029 | Migration journal partial or drifted; resume / rollback / force-redo |
| D030 | Import has not been observed recently on this machine (per-developer freshness) |
| D031 | Declared signing key does not match commit signature |
| D032 | Import has no recent transparency-log witness (when log configured) |
| D033 | (retired; harness commit signing is project policy, not mockspace's) |
| D034 | (retired; harness as supply-chain trust root removed) |
| D035 | (retired; harness as supply-chain trust root removed) |
| D036 | Hook value parses as inline shell (use a `hooks/` file instead) |
| D037 | Race conflict could not be preserved on remote; local state retained |
| D038 | `.mock/` parent appears to be a cloud-sync directory |
| D039 | Lockfile drift: declared imports don't match lockfile entries |
| D040 | Anchor blob missing from `.anchor.<phase>.blobs/` storage |
| D041 | `mock://~/...` URI found outside a package-execution context |

### Doctor commands

```
mock doctor
mock doctor --json
mock doctor --json --report-file=path
mock doctor --repair
mock doctor --repair --interactive
mock doctor --repair --finding D001
mock doctor --ci
```

Per-finding repair is atomic. Repair operations log to `.git/mockspace/doctor.log`
in structured JSON with pre/post SHAs, finding ID, success/failure.

Repairs sorted topologically. Refuse `--repair` on a finding whose trust
source is itself flagged.

## CLI: commands

### Root commands

```
mock init                                bootstrap
mock status [--fast|--json]              primary status view
mock sync [<area>]                       re-render from refs
mock commit [-m "<msg>"] [<area>]        commit pending changes
mock prune                               clean consultation worktrees
mock upgrade                             schema migration
mock doctor [...]                        diagnostics + repair
mock migrate [...]                       one-time migration
mock completion install [...]            shell completion
mock help [<topic>]                      help system
```

### Domain: rounds

```
mock round new <slug>
mock round close [<slug>]
mock round list [--include-archive]
mock round show [<slug>]
mock round archive <slug>
mock round pr status
mock round pr regen [--force]
mock round pr merge                      manual trigger
```

### Domain: phases

```
mock phase                               show current
mock phase plan
mock phase apply [--auto-commit] [--no-forge] [--resume]
mock phase finish
mock phase replan [--force]
mock phase resolve                       interactive race recovery (D020)
```

### Domain: tasks

```
mock task new <ns>::<slug> [--title=...]
mock task start <ns>::<slug>
mock task block <ns>::<slug>
mock task defer <ns>::<slug>
mock task close <ns>::<slug> --resolution=<kind>
mock task archive <ns>::<slug>
mock task move <old> <new>
mock task list [--namespace=<ns>] [--include-archive]
mock task show <ns>::<slug>
mock task step start <ns>::<slug>#<key>
mock task step close <ns>::<slug>#<key>
```

### Domain: topics

```
mock topic
mock topic new <name>
mock topic show <name>
```

### Domain: manifests

```
mock manifest                            show current phase's manifest
mock manifest show
mock manifest verify [--json]            run verifier without sealing
```

### Domain: forge

```
mock forge sync [--resume-rate-limited]
```

### Domain: external refs

```
mock ext fetch <ref-name>
mock ext refresh [<host>/<kind>/<slug>]
mock ext refresh --all
mock ext refresh --accept-rewrite        explicit acknowledge of force-push
```

### Domain: imports + trust

```
mock import update                       refresh lockfile (fetch latest, verify)
mock import update <ext>/<pkg>           refresh one import
mock import rotate <ext>/<pkg> --accept-new-key
                                         acknowledge key rotation
mock trust list                          show this developer's local TOFU acceptances
mock trust accept <host> <fingerprint>   explicitly accept a (host, fingerprint) pair
mock trust forget <host>                 forget this developer's acceptances for a host
mock trust verify [<ext>/<pkg>]          re-run signature verification
```

All `mock trust ...` commands operate on the developer's local trust
file (`~/.config/mockspace/trust.toml`). None of them touch the
project's harness ref or lockfile.

### Domain: exports

```
mock export new <name>
mock export list
mock export show <name>
mock export bump <name> <version>
mock export archive <name>
mock export publish <name>
mock export witness <name>                       append signed entry to transparency log
```

### Domain: hosts

```
mock host list
mock host add <name> <url>                       add to [hosts]
mock host remove <name>
mock host add-mirror <name> <url>                append to mirrors list
mock host remove-mirror <name> <url>
mock host fetch <name> [--verify-mirrors]        fetch + optionally verify each mirror
mock host show <name>
```

### Domain: harness

```
mock harness show                                show resolved harness config + status
```

Note: there is no `mock harness verify` or `mock harness accept-rotation`.
Commit signing of harness commits is project policy handled by git
(`commit.gpgsign`) or forge branch protection — not a mockspace
concern. `mock harness show` is informational only.

### Domain: cache

```
mock cache show                                  show cache size, location, age
mock cache prune [--older-than=<duration>]       evict unused imports + helpers
mock cache verify                                re-hash cached imports against lockfile
```

`mock cache prune` evicts content not referenced by any project's
current `mockspace.lock` on this machine, plus any entry older than
`--older-than` (default 90 days unreferenced, 365 days total).

## CLI: porcelain

The commands above are **plumbing**: precise, per-domain, do
exactly what they say. Power users and automation target them.
Stable, scriptable, `--json` everywhere.

The commands below are **porcelain**: high-level workflow-aware verbs
that read context and Do The Right Thing. Newcomers and daily use
target them. Each porcelain verb decomposes to a sequence of plumbing
calls; `--explain` prints the plumbing equivalent without running.

The git plumbing-vs-porcelain split is the model. Most users only
ever touch porcelain; plumbing stays available when fine-grained
control matters.

### Porcelain verbs

```
mock status [--fast] [--json]            primary "where am I" view
mock work [<topic>]                      start working on the current/next thing
mock advance                             advance to the next sensible phase
mock commit [-m "<msg>"]                 auto-detect changed areas; commit per area
mock done                                finish active round (phase finish + close + forge sync)
mock sync                                fetch all imports + ext mirrors + forge state
mock add <ext>/<pkg>                     add an import (host config + import + first fetch + TOFU)
mock open [<area>]                       open the relevant rendered surface in $EDITOR
mock undo                                undo the last destructive operation
mock redo                                re-apply the last undone operation
```

### Properties shared across porcelain

- **`--explain`** prints the plumbing decomposition without running.
  Decomposability is verifiable: the porcelain implementation literally
  constructs the plumbing call sequence, then either executes or
  prints. This is the property that makes porcelain a thin layer
  rather than a parallel implementation.
- **`--dry-run`** prints what would happen, asks for confirmation,
  then runs.
- **Reads context.** Each porcelain verb inspects the current phase,
  active round, dirty areas, lockfile state, recent operations.
  Doesn't ask the developer to spell out what mockspace can infer.
- **Asks on ambiguity.** Composite operations like `mock done` describe
  each step in the structured-diagnostic format and offer per-step
  `y`/`N` or `Y` (accept all). No silent multi-step execution that
  the developer might not have intended.
- **Snapshotted destructive ops.** Every porcelain verb that mutates
  state lands one composite undo entry (see
  [Undo and redo](#undo-and-redo)).
- **Doesn't replace plumbing.** All plumbing verbs stay. Porcelain
  is convenience; plumbing remains the contract surface for tools,
  agents, CI, and editors.

### Per-verb decomposition

- **`mock status`** — already in plumbing root commands; the porcelain
  status command is the same call. Cold under 300ms, warm under 50ms,
  `--fast` under 20ms. Shows: current round, current phase, dirty
  areas, recent undo entries, top doctor findings if any.

- **`mock work [<topic>]`** — if no active round, runs `mock round new`
  with an auto-generated slug, then `mock topic new <topic>`. If
  active round exists and `<topic>` provided, runs `mock topic new
  <topic>` on it. If active round and no `<topic>`, opens the active
  round's current phase surface (equivalent to `mock open`). Sets
  the active-round context for subsequent invocations.

- **`mock advance`** — reads current phase. If dirty areas exist,
  prompts to `mock commit` first (or auto-commits per profile). Then
  runs the next plumbing transition: `mock phase plan` from
  TOPIC/PLAN(...), `mock phase apply` from PLAN(...), `mock phase
  finish` from APPLY(...).

- **`mock commit [-m "..."]`** — inspects every `.mock/<area>/` for
  changes against the rendered ref state. Routes commit per area:
  - Changes in `.mock/round/<active>/` → mock-side commit on
    `refs/mock/round/<active>`.
  - Changes in `.mock/tasks/<task>/` → mock-side commit on the task's
    ref.
  - Changes in `.mock/research/<slug>/`, `.mock/bench/<slug>/`,
    `.mock/sketch/<slug>/` → mock-side commit on the corresponding ref.
  - Changes in `.mock/mockspace.toml`, `.mock/mockspace.lock`,
    `.mock/agent/`, `.mock/lints/`, `.mock/templates/`, `.mock/hooks/`
    → mock-side commit on `refs/mock/harness`.
  - Changes in `.mock/export/<package>/` → mock-side commit on
    `refs/mock/export/<package>`.
  `-m "<msg>"` applies to all per-area commits. Without `-m`,
  mockspace generates a per-area commit message. `--per-area` lets
  the developer specify different messages per area interactively.

- **`mock done`** — runs `mock phase finish`, then `mock round close`,
  then `mock forge sync`. Each step described before execution; the
  developer accepts the whole composite or per step.

- **`mock sync`** — already in plumbing root commands; the porcelain
  shape adds `--all` which subsumes: fetch all imports
  (`mock import update`), fetch all ext mirrors
  (`mock ext refresh --all`), refresh forge state (`mock forge sync`).

- **`mock add <ext>/<pkg>`** — porcelain for "add this import":
  1. If `<ext>` (host alias) is not configured, prompt for URL +
     optional mirrors; write to `[hosts.<ext>]` in harness's
     `mockspace.toml`.
  2. Add `mock://ext/<ext>/<pkg>` to `[imports]`.
  3. Fetch the ref; verify signature.
  4. Per-developer TOFU prompt for the `(host, fingerprint)` pair.
  5. Write the lockfile entry.
  6. Commit harness changes.
  Each step shown before execution.

- **`mock open [<area>]`** — opens the relevant rendered surface in
  `$EDITOR`:
  - No arg: opens the active round's current-phase manifest file.
  - `mock open round`: opens `.mock/round/<active>/` (directory).
  - `mock open task [<ns>::<slug>]`: opens an active task's directory.
  - `mock open harness`: opens `.mock/mockspace.toml`.
  - `mock open lock`: opens `.mock/mockspace.lock`.
  - `mock open <slug>`: opens the matching round/task/research/sketch.

- **`mock undo` / `mock redo`** — see
  [Undo and redo](#undo-and-redo).

### When porcelain refuses

Porcelain is opinionated about The Right Thing but never silently
guesses on destructive ambiguity. Specific refusals:

- `mock advance` refuses when the current phase's manifest has
  unsealed unresolved questions (e.g., TOPIC has open subtopics
  without decisions). Suggests `mock open round` to resolve first.
- `mock done` refuses when the active round has open tasks. Suggests
  `mock task close` per task or `--force-close-tasks`.
- `mock commit` (bare form) refuses when no area has changed; asks
  whether the developer meant `mock commit --empty` (no-op signal
  commit) or a specific area.
- `mock add` refuses when the target `<ext>/<pkg>` already has a
  conflicting `[hosts.<ext>]` (different URL); suggests
  `mock host show <ext>` to inspect and `mock host remove` if intentional.

### What porcelain is NOT

- Not a replacement for plumbing. Every porcelain decomposes; the
  plumbing is the contract.
- Not a hidden layer. `--explain` makes every porcelain operation
  fully transparent.
- Not magic. Porcelain reads observable state and runs known
  sequences. No ML, no heuristics-that-might-be-wrong, no inference
  from history beyond the immediate phase context.

## Undo and redo

Destructive mockspace operations are recoverable via a short-span
undo log. The mechanism is invisible to daily use; developers only
discover it when they need it.

### Mechanism

Every destructive mockspace operation appends an entry to
`.git/mockspace/undo/log.jsonl` BEFORE applying the operation:

```json
{
  "ts": "2026-05-17T14:30:00Z",
  "op": "phase apply",
  "description": "advance round 202605171800-trunk-refactor from PLAN(DOC) to APPLY(DOC)",
  "before": {
    "refs/mock/round/202605171800-trunk-refactor": "abc123...",
    "refs/mock/harness": "def456..."
  },
  "after": {
    "refs/mock/round/202605171800-trunk-refactor": "789abc...",
    "refs/mock/harness": "def456..."
  },
  "metadata": { /* op-specific */ }
}
```

The log is append-only. `mock undo` does NOT delete entries; it
appends a counter-entry marking the prior entry as undone.

Because refs are SHAs, snapshots record SHA + description, not
content. Git's normal reflog retention (default 90 days) keeps the
underlying objects reachable for the undo window — no extra storage.

### Operations snapshotted

| Operation | Snapshot? |
|---|---|
| Phase transitions (`plan` / `apply` / `finish` / `replan`) | Yes |
| Round / task lifecycle (`new` / `close` / `archive`) | Yes |
| Lockfile mutations (`import update` / `rotate`) | Yes |
| Export lifecycle (`bump` / `archive`) | Yes |
| Harness commits | Yes |
| Migration steps | Yes |
| `mock prune` (deletions) | Yes |
| `mock commit` (any area) | Yes |
| Porcelain composites (`advance`, `done`, `add`) | Yes, as one composite entry |
| Reads (`status`, `doctor`, `manifest verify`, `trust list`) | No |
| Cache eviction (`cache prune`) | No (cache is regenerable) |
| Fetch-only operations (`sync`, `ext fetch`) | No (idempotent reads) |
| Forge API calls (`forge sync`) | No (network reads) |

### `mock undo`

1. Reads the most-recent non-undone entry from the log.
2. For each ref in the entry's `before` map, runs `git update-ref
   <ref> <before-sha>`.
3. Appends a counter-entry to the log marking the entry as undone.

If any of the affected refs has been advanced beyond the entry's
`after` state by a subsequent NON-mockspace operation (e.g., manual
`git update-ref` or a tool other than mockspace), `mock undo`
presents the diff and refuses without explicit `--force`. The
default is "don't silently rewind a divergence I didn't cause".

### `mock redo`

After `mock undo`, the entry stays in the log but is marked undone.
A new destructive operation clears all undone entries (linear
history; vim semantics). Until that happens, `mock redo` finds the
most-recent undone entry, re-applies its `after` state, and
appends a counter-entry marking it re-done.

### Inspection

```
mock undo --list                         show the recent undo log
mock undo --show <n>                     show the Nth most recent entry's diff
mock undo --explain                      show what mock undo would do without running
mock redo --explain                      show what mock redo would do without running
```

### Retention

Default retention: keep the last **50 entries** OR all entries
within the last **30 days**, whichever bound retains more.
Configurable via `[undo].keep_entries` and `[undo].keep_days` in
`mockspace.toml`.

Beyond that, entries roll out of the log. The referenced ref state
remains in git's object store for git's reflog retention window
(default 90 days); after that, GC may reclaim. Mockspace's undo log
falling off does not guarantee object-store unreachability — `mock
undo` past the log boundary just isn't a thing mockspace will do
for you. Manual recovery via `git reflog` remains possible during
git's own retention window.

### Pushed operations

`mock undo` on an operation that was already pushed to a remote
does NOT silently rewind the remote ref. Instead:

1. The local rewind succeeds (local refs are the developer's).
2. Mockspace prints a structured diagnostic: "the remote ref is
   currently at `<after-sha>`; your local is now at `<before-sha>`."
3. Suggests two follow-ups:
   - `mock undo --apply-remote` to push a new commit on top that
     restores the prior state (preserves remote history; everyone
     else sees an "undo commit" rather than a rewrite).
   - Explicit `git push --force-with-lease` if the developer is sure
     no one else has pulled.

Default behaviour: never force-push. Undo against pushed history
becomes an additive commit unless explicitly overridden.

### What undo is NOT

- Not a substitute for review. `mock undo` reverses a recent
  mistake; it doesn't redo a multi-step design decision.
- Not a time machine. The window is short (50 / 30d); long-term
  history is git's job.
- Not transactional across machines. Each developer's undo log is
  local; one developer's `mock undo` doesn't affect another's.

## CLI: cross-cutting concerns

### Exit codes

```
0   success
1   user error
2   system error
3   state inconsistency (mock doctor would report)
4   unauthorised (forge auth, push permission denied)
5   conflict (push non-FF, PR body race, race lost)
6   rate-limited (honor Retry-After)
7   trust gate (signature mismatch, first-trust not accepted)
```

### Output modes

`--json` on every read command. Structured stderr in `--json` mode.
`--report-file=<path>` writes structured report.

### Dry-run

`--dry-run` on every state-changing command.

### Signal handling

`SIGINT`/`SIGTERM`: before update-ref boundary → clean abort. After
local commit before push → un-pushed, `--resume` completes. After push
before forge → reconcile via `mock pr regen`.

### Latency targets

- `mock status` cold: under 300ms.
- Warm: under 50ms.
- `--fast`: under 20ms.

### Shell completion

```
mock completion install --shell bash --path <p>
mock completion install --shell fish --path <p>
mock completion install --shell zsh --path <p>
```

## Audit trail durability

Every workflow-transition commit carries trailers:

```
Workflow-Transition: plan_doc -> apply_doc
Workflow-Round-Slug: 202605171800-trunk-refactor
Workflow-Branch: feat/round/202605171800-trunk-refactor
Workflow-Machine: alice-laptop.example.com
Workflow-Tool-Version: mockspace 1.2.3
Workflow-User: Alice Example <alice@example.com>
Workflow-Timestamp: 2026-05-17T10:00:00Z
Mockspace-Version: 1.2.3
```

Commit history IS the durable audit trail. **Do not rely on reflog** for
anything that must survive a fresh clone.

In `--strict` mode, mockspace verifies every workflow-transition commit
is signed; refuses to read trailers from unsigned commits.

## Day in the life of a round

```
$ mock init                       # one-time per fresh clone
                                  # interactive trust prompts on first imports

$ mock round new ref-redesign     # creates feat/round/<slug> + refs/mock/round/<slug>
                                  # auto-opens draft PR

$ mock topic new motivation       # opens editor on .mock/round/topic.motivation.md
$ mock commit -m "draft motivation topic"

$ mock topic new architecture
$ mock commit

$ mock phase plan                 # refuses if .mock/round/ dirty
                                  # scaffolds manifest.doc.toml; transitions

# edit .mock/round/manifest.doc.toml (structured verify blocks)
$ mock commit

$ mock phase apply                # validates manifest; runs verifier in temp worktree
                                  # captures anchor; transitions; pushes round ref
                                  # forge API: opens PR

# edit doc-side files; commit on feat/round/<slug> via git
$ mock phase finish               # transitions to plan_src

# edit manifest.src.toml
$ mock commit
$ mock phase apply                # validates + verifier; captures src anchor

# edit source code as claimed; commit on feat/round/<slug>

$ mock phase finish               # transitions to done
$ mock close                      # fetches PR comments (mandatory, resumable)
                                  # freezes round ref
```

## Crate organisation

### `mockspace` (core, language-agnostic)

- State machine, ref management, hook protocol, profile dispatcher.
- Import resolver + cache + lockfile manager.
- Signing verification (git verify-commit wrappers).
- Template renderer.
- Config parser.
- Standalone `mock` binary.
- Embedded baseline handlers and helpers.
- `mock doctor`, `mock migrate`, `mock sync`, `mock commit`, etc.

### `mockspace-rs` (Rust toolchain extension)

Distributed as a separate git repo, published as a host. Exports:

- `runner-rs` package for `.rs` hooks/lints.
- Rust-specific lints + agent rules.
- Rust-specific verifier kinds (`function_present`, `type_implements_trait`, etc.).
- `cargo mock` cargo subcommand alias.
- `build.rs` bootstrap convenience.

`build.rs` sketch:

```rust
fn main() {
    if std::env::var("USE_MOCKSPACE").as_deref() != Ok("1") {
        return;
    }
    mockspace_rs::bootstrap::ensure_mock_dir()
        .expect("mockspace bootstrap failed");
}
```

### Lint-pack ABI versioning (legacy, gradually replaced by imports)

```toml
[package.metadata.mockspace]
abi_version = "1.0"
```

Loader refuses mismatch. mockspace ships ABI bridges for one-minor-back
during the imports-system transition.

### `mockspace-ts` and viola integration

Future. Same shape: separate host, published exports, language-specific
runner + verifier kinds.

## Migration tooling

`mock migrate` with subcommands:

```
mock migrate                       interactive plan + prompt
mock migrate harness               build refs/mock/harness from current mock/
mock migrate rounds                walk mock/design_rounds/
mock migrate tasks                 walk mock/tasks/
mock migrate research              walk mock/research/
mock migrate all                   full sequence
mock migrate --dry-run             print plan
mock migrate --rollback            revert via journal
mock migrate --strict              fail on any edge case
mock migrate --accept-imperfect    tolerate edge cases
mock migrate finalize              remove mock/ from refs/heads/
```

`.git/mockspace/migrate-<timestamp>.log` (JSON Lines) records every ref created,
every file preserved, success/failure per step. Each line is a discrete
journal entry; line-buffered writes mean a crashed migration leaves a
truncated final line, which the recovery code detects (incomplete JSON).

Migration is additive until `mock migrate finalize`.

### Idempotency under crash and resume

Migration is a sequence of discrete steps (`harness`, `rounds`,
`tasks`, `research`, `finalize`). Each step is a transactional unit
that produces or extends one or more `refs/mock/*` orphan refs.
**Each step is idempotent on resume** because every output is content-
addressed by tree SHA.

The recovery algorithm:

1. On `mock migrate` (no subcommand) or `mock migrate --resume`, walk
   the journal log lines in order.
2. The last complete journal entry names the step that succeeded and
   its post-state (the ref SHAs it produced).
3. If the file ends with an incomplete entry: the prior step's
   post-state IS observed; the in-flight step had not completed.
4. For each subsequent step the user requests:
   - Compute the would-be-written ref SHA from current source `mock/`
     content.
   - If the ref already exists at that SHA: log "skip (already
     migrated)", record a no-op journal entry.
   - If the ref exists at a different SHA: refuse with `D029`
     (migration drift); the source `mock/` content has changed since
     the prior migration step recorded a different SHA. User runs
     `mock migrate --rollback` to start over, or `mock migrate
     --force-redo <step>` to overwrite (loud).
   - If the ref doesn't exist: write it, append journal entry on
     success.

The result: re-running migration after a crash is safe; the only
failure mode is "the source filesystem changed between attempts" and
that surfaces structurally, not as silent corruption.

`mock migrate verify` is the dry-run shape: walks the journal,
re-computes expected SHAs from current source, compares to recorded
state, reports any drift without mutating refs.

### Edge cases

Partial historical rounds, naming collisions, missing files,
ambiguous phase: each surfaces structured findings.

### `main` migration timing

Rides next release PR per project's release cadence.

### Old release tags

Retain `mock/` in tarballs. Documented; not remediated.

## Round and task archival

Single archive refs (`refs/mock/task-archive`, `refs/mock/round-archive`)
carrying archived entries as unified trees.

Auto-archival per threshold on `mock close` (full sweep) +
`mock sync --full`.

Concurrent writes use `on_archive_contention` (exponential backoff +
jitter + unbounded retries with 60s timeout).

## Windows support and platform notes

Mockspace is Unix-primary. Linux + macOS first-class.

Windows usage supported with caveats:

- Default helpers are bash; Windows users need git bash (ships with Git
  for Windows).
- `flock(2)` works on Linux + macOS; Windows uses git's `.git/refs/<...>.lock`
  as canonical mutex fallback.
- Nested git worktrees not used.
- CRLF: scripts in harness ref must use LF; `.gitattributes` enforces
  `* text eol=lf`.
- Path separators: mockspace uses `/` internally; Windows-style `\\`
  not accepted.

Windows polish is a known gap; contributions welcome.

## Future direction: vehje as hook language

The hook protocol currently defaults to bash, with language-specific
runners (mockspace-rs for Rust, mockspace-ts future for TypeScript)
extending the shape.

A future evolution is to support **vehje** (a programming language being
developed in the workspace) as a hook language. Vehje is designed for
boundedness and safety; if its capabilities mature to cover hook use
cases (file ops, structured I/O, git plumbing wrappers), it could
become a sandboxed/safer alternative to bash for projects that opt
into it.

Specifically:

- Vehje-shaped hooks could declare capabilities they require
  (file-read, file-write, network, command-exec). The runtime grants
  only declared capabilities.
- Hook scripts in `.vehje` would route through a `runner-vehje` package
  similar to `runner-rs`.
- Existing bash hooks continue to work indefinitely; vehje is additive.

This is out of scope for v1. Note here so future agents know the
direction exists.

## Open questions

### Cross-machine round-ref conflict resolution: detailed UX

`mock phase resolve <slug>` specified at high level. Detailed prompt
flow including rebase-conflict surfacing is worth a future round.

### Verifier extensibility process

The verifier catalog grows over time. Formalising the contribution
process (RFC template, review gate, version bump) is worth a future
round. For now: PRs to mockspace-core or to a language extension.

### `package.toml` `[dependencies]` semantics

Recursive resolution, version-range constraints, cycle detection,
diamond conflict resolution: specify before this design ships.

**Working direction (Nix-flake-style flat lockfile).** Imports
specified by the consumer flatten into a single `mockspace.lock`; the
consumer sees the entire dependency closure explicitly. Packages may
declare `[dependencies]` for documentation / discoverability, but
mockspace does not recursively resolve them at the consumer side.
The consumer adds each transitive dependency to their own `[imports]`
list explicitly. The trade is "more explicit, less convenient"
versus Cargo-style recursive resolution; for the small-time scope this
design targets, explicitness wins (no diamond conflicts to resolve,
no transitive surprises). If consumer pain warrants it, a
`mock import resolve <uri>` command can pre-compute the closure and
suggest imports to add.

### Multi-active-round mode

`default_one_active_round = true` is the default. Multi-active mode
named in config only; not implemented. Deferred.

### Self-hosting timeline

Mockspace IS itself a mockspace user. Under the redesign, mockspace's
own rounds live on `refs/mock/round/*`. Deferred until externally
validated.

### Tool extension contract

`tools/` in harness is named for project-local CLI extensions. Contract
unspecified. Out of scope for this round.

### Archive ref unbounded growth at scale

For 5000+ archived tasks, single archive ref's enumeration cost may
require sharding (e.g., `refs/mock/task-archive/<year>`). Defer until
measurement warrants.

### vehje hook language details

When vehje matures enough to be a hook language, specify the runner
contract, capability declarations, sandboxing properties. Note as
future direction; not blocked on v1.

## Relationship to workspace-level tools

Mockspace is **project-scoped**. Operates on one repository.

Mockspace's only cross-repo surface: read-only consumption of external
refs via `mock://ext/<host>/...`. **Awareness and utilisation**, not
**management**.

A workspace-level tool is **workspace-scoped**. Operates on N
repositories simultaneously, coordinating their state.

### The boundary

- **Mockspace knows:** one project's refs, harness, index cache,
  imports, exports; read-only references to other projects' refs.
- **Mockspace doesn't know:** other projects' workflow state, "the
  workspace", multi-repo aggregation.
- **A workspace tool knows:** workspace membership, per-repo forge
  bindings, cross-repo orchestration, multi-repo migration.
- **A workspace tool doesn't know:** specific projects' mockspace
  internals.

### Composition

Solo project: mockspace; no workspace tool. Workspace: workspace tool;
each project may or may not use mockspace. Combined: both per repo.

A workspace tool's per-repo aggregation reads target-2 renders from
each repo's mockspace-initialised local clone.
