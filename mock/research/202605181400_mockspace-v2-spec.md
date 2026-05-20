# mockspace v2 design specification

**Status:** proposal / self-contained
**Authored:** 2026-05-18
**Supersedes:** the earlier ref-based-redesign draft, which is preserved alongside as audit trail.

> This document is the full specification for mockspace as a product. It is self-contained: no external reading is required. Where this spec disagrees with anything else in the repository, this spec wins.

> Read top to bottom on first contact. The structure goes from product (what mockspace IS and how it is used) to mechanism (how it works underneath) to reference (schemas, catalogs, edge cases). Implementers can jump straight to Parts III through VIII; first-time readers should not skip Part I.

---

## Table of contents

**Part I. The product**

1. The heart: documentation as truth
2. How mockspace is used
3. The workflow shape
4. The five-tier doc template system
5. The lint engine
6. Convention lints, claim verification, escape hatches
7. AST extraction
8. Agent integration
9. The bench framework
10. The sketch protocol
11. The research workflow
12. Multi-topic rounds and sister-correction
13. Vocabulary

**Part II. The state machine**

14. The six phases
15. Phase transitions
16. Tasks
17. Manifests
18. Topic documents

**Part III. The storage foundation**

19. Reference architecture
20. Local materialisation
21. Source-side versus mock-side refs
22. The harness ref
23. Content-addressed anchors
24. Transition atomicity
25. Active phase storage
26. Tasks, archives, retention

**Part IV. Imports, exports, trust**

27. The `mock://` URI scheme
28. Hosts
29. Exports
30. Imports
31. Signing and per-developer TOFU
32. The lockfile
33. The first-party `@/` source
34. Optional transparency log

**Part V. Hooks, profiles, policy**

35. Hook protocol
36. Profiles and reactive policy
37. Env and bins policy
38. Language-specific runners

**Part VI. The interface**

39. CLI plumbing
40. CLI porcelain
41. `mock status` as the primary entry
42. Undo and redo
43. `mock doctor` and the findings catalog
44. `mock sync` and staleness
45. Cross-cutting concerns

**Part VII. Operational**

46. The `mockspace.toml` schema
47. Render pipeline
48. PR lifecycle
49. Audit-trail commit trailers
50. Version compatibility
51. Migration from filesystem-only mockspace
52. Day in the life of a round

**Part VIII. Reference**

53. Manifest schema
54. Verifier catalog
55. Findings catalog
56. Threat model
57. Crate organisation
58. Platform notes
59. Future directions
60. Open questions
61. Boundary: workspace-level tools

---

# Part I. The product

## 1. The heart: documentation as truth

Source code lies. Documentation lies. Pick one to enforce, then make the other follow.

Mockspace picks documentation. Every shape that ships from a project using mockspace is described first in docs, then implemented in source. The docs are the contract; the source is the mechanical realisation. When implementation reveals the docs were wrong, the docs get rewritten first, then the source is updated to match. The source never gets to deviate first.

This is not aspirational. It is enforced. Lints, git hooks, and agent integrations make any other state literally fail to commit, fail to build, or fail to push. A repository configured with mockspace cannot accept a state where source and docs disagree, because the gates refuse it.

The mechanism is a network of three reinforcing systems:

**The doc template system.** A small set of named templates (README, DESIGN, BACKLOG, SHAME, deep dives) capture the truth at multiple granularities. Per-crate templates assemble into root-level docs. The rendered output ships to readers; the templates are what authors edit. Source-of-truth is one file per concern.

**The lint engine.** Lints check that source matches docs and that source follows the project's conventions. Built-in lints (claim verification, deprecation accounting, file-size, naming) run alongside project-defined custom lints (registered as Rust source, loaded as dynamic libraries) and external lint packs (shared across a project family). Every lint has per-gate severity, so projects choose whether a violation merely warns at build or hard-blocks at push.

**Agent integration.** The same lint configuration generates per-scope agent rule files. When an AI assistant (Claude Code, Copilot, future tools) loads the project context, it receives the docs-as-truth principle and the project's specific conventions as instructions, in the agent's own rule format. The agent is blocked at commit/build/push time by the same lints, but it also knows the rules upfront and writes code that already complies.

The three systems share configuration. A single declaration in `mockspace.toml` produces lint enforcement at the gate, agent guidance at write time, and rendered documentation at publish time. There is no separate "tell the lint", "tell the agent", "update the docs" step. The truth is one place.

### Why this framing matters

A reader new to mockspace often expects "design discipline tool" to mean "structured process around commits" (like Conventional Commits or pull-request templates). Mockspace is something larger. The commit-side ceremony is one slice; the load-bearing claim is about whose word counts as truth.

The phrase used inside the project is: **source always follows docs. Never more, never less, never in a different manner.** If a feature appears in source but not in docs, the lint blocks. If a doc claims a function exists but the source has renamed it, the lint blocks. If a project's convention says "no module exceeds 500 lines" and a module grows to 501, the lint blocks. The discipline is uniform: the documented shape is what shipped; everything else is a regression.

This document specifies the full mockspace product that delivers this discipline. The remaining parts describe how. Read Part I to understand what mockspace IS. Read Parts II-VIII to implement it.

## 2. How mockspace is used

A project using mockspace has a working tree like any other repository. The user's daily work happens in source files (Rust crates, TypeScript packages, whatever the project uses) and the rendered docs that ship with the project. Mockspace's own surface (`mock/` directory templates, design-round artefacts, lint configuration) lives alongside but does not pollute the published artefact.

The day-to-day shape is:

1. The author opens a topic of design discussion. This becomes a topic file in the current round. Multiple topics can coexist in one round; sister topics often correct each other's framing as the design matures.
2. The discussion reaches a point where source-level claims need verification (does this approach actually compile? does this layout actually pay off?). The author writes a **sketch**: a small, isolated piece of code that demonstrates feasibility. The sketch lives in research and stays forever as audit trail.
3. The discussion reaches a point where performance claims need verification (is approach A faster than approach B on this workload?). The author writes a **bench**: a structured experiment with workload, variants, multiple input sizes, statistical analysis. The bench produces a findings file that the topic references.
4. The discussion reaches a point where design-by-design comparison against prior art is needed (how did similar projects solve this?). The author writes **research** notes: long-form prose, often imported from sibling projects, cross-referenced from the topic.
5. The topic discussions converge. The author writes a **doc manifest**: a structured list of every documentation surface that will change, with the specific edits per file. The doc manifest is sealed, the documentation surfaces are edited, and source-side files claimed by the docs are noted (but not yet changed).
6. After doc edits land, the author writes a **src manifest**: a structured list of every source surface that will change, with verifier rules per claim. The src manifest is sealed, the source-side files are edited to match. Lints check that the actual source state matches the docs.
7. The round closes. The artefacts persist as durable record. The published documentation reflects the new state. The lint configuration reflects the new conventions. The agent rules reflect the new constraints.

A round can take minutes (a small fix), hours (a single-crate refinement), days (a multi-crate refactor), or months (a foundational language-design round with dozens of topic files). The shape is uniform; the duration is whatever the work requires.

The author never directly changes the lint configuration to make a problem go away. The author never directly disables an agent rule to silence an objection. The author never directly edits the rendered docs (the rendered output is generated; templates are what the author edits). The discipline is structural: the only paths through the system are the documented ones.

## 3. The workflow shape

A **round** is a discrete unit of mockspace workflow scoped to one coherent design change. A round runs through six phases:

```
TOPIC → PLAN(DOC) → APPLY(DOC) → PLAN(SRC) → APPLY(SRC) → DONE
```

The phases serve different purposes:

- **TOPIC** is exploration. The author writes topic files: prose framing the problem, the options, the tradeoffs. Sketches, benches, and research happen during TOPIC. There is no formal manifest yet. The author iterates with the team (or with themselves) until the design has converged.

- **PLAN(DOC)** is consolidation. The author writes the doc manifest: a TOML document declaring exactly which documentation templates will change and how. The manifest is mutable during PLAN.

- **APPLY(DOC)** is execution. The doc manifest is sealed (no further edits allowed). The documentation templates are edited to match the manifest. A verifier runs against the edited state to confirm every claim landed.

- **PLAN(SRC)** is the second consolidation. With docs now describing the new shape, the author writes the src manifest declaring exactly which source files will change. Same TOML shape as the doc manifest, focused on `*.rs` (or whatever language).

- **APPLY(SRC)** is the second execution. The src manifest is sealed; source files are edited; the verifier runs again. At this point source and docs must agree exactly. The lints enforce this on every commit.

- **DONE** is finalisation. PR comments are ingested into the round's record. The round is frozen and archived. The round's slug becomes a permanent reference.

The phase transitions are explicit commands:

- `mock phase plan` opens the next planning phase (scaffolds the manifest).
- `mock phase apply` seals the planning phase, runs the verifier, transitions to APPLY.
- `mock phase finish` completes the APPLY phase, transitioning to the next PLAN or to DONE.
- `mock phase replan` is the backward verb: deprecate the current manifest, restore phase-owned source files from the captured anchor, scaffold a fresh manifest.

The forward verbs are deliberately three: `plan` opens, `apply` seals, `finish` closes. Earlier mockspace versions used `lock` / `unlock` / `deprecate` / `close`, and the semantics were a frequent source of confusion (was "lock" the verb that opens the next phase or the verb that closes the current one?). The plan/apply/finish vocabulary is unambiguous because each verb names what the user does, not what mockspace does internally.

### Where the activity actually lives

The phases above describe the structure, not the time distribution. In practice, most of the wall-clock effort of a round happens during TOPIC. That is where the design problems get solved. PLAN and APPLY are consolidation and execution; they are short because the hard thinking already happened.

This bears stating because the older mockspace docs sometimes read as if TOPIC were a brief opening ceremony and the bulk of the work were the manifest authoring. The opposite is true. A round with three topic files and four sketches that spans two weeks of discussion produces a manifest in an hour and applies it in another hour. The product is the design decision; the manifest is the receipt.

### Multi-topic rounds

A round can carry one topic file or twenty. Common patterns:

- **Single-topic short rounds.** One topic, one set of doc + src changes, no sketches or benches. Common for routine refactors, doc cleanups, single-bug fixes. Often one hour to one day total.

- **Multi-topic single-round.** Several related topics covered together, especially when they correct or refine each other. Sister topics within one round can deprecate each other's framing without rewriting the original (the original stays as audit trail; the corrective topic explains the reframing). This pattern is common when a topic surfaces an issue that another topic should address in the same scope.

- **Multi-round single-area.** A large area of the codebase that takes many rounds to mature. Each round has one or a few topics; the rounds stack chronologically. Common during foundational design phases.

The discipline applies the same way to every shape. Short rounds, long rounds, single-topic, multi-topic: same six phases, same templates, same lints.

## 4. The five-tier doc template system

The doc template system is the centre of docs-as-truth. There are five named template kinds, all living under `mock/` in the project tree (or under the harness ref in the storage layer described in Part III), all rendered into either the project's `docs/` tree or alongside the source for publication.

### README.md.tmpl (per crate, 3 to 10 lines)

The shortest tier. One paragraph describing the crate. Used by the assembly pipeline to compose root-level docs. Crate authors keep this tight: the reader needs the gist in twenty seconds.

```markdown
# arvo-bits

Bit-level types and operations for arvo: `Bits<N, S>` storage primitive,
`Bit`/`Nibble`/`Byte`/`Word`/`DWord`/`QWord` aliases, bit-access traits
(`BitAccess`, `BitLogic`, `Mask`, `Narrow`). Const-generic widths with
strategy markers; lowers transparently to packed integer representations.
```

### DESIGN.md.tmpl (per crate, the shipping contract)

The crate's load-bearing design document. Names every public type, trait, function. Every backticked identifier in this template is a claim that the identifier exists in source; the `design-doc-source-mismatch` lint enforces alignment on every commit.

DESIGN.md.tmpl is what readers consult to understand "what does this crate do, what does it ship, what are the guarantees." It is also what the lint engine treats as canonical. If the doc says `pub struct Width(pub Uint7)`, the source must declare that struct with that exact name and shape.

### BACKLOG.md.tmpl (per crate, designed but not yet shipped)

Records work designed and intended but not yet in source. The contents look similar to DESIGN.md.tmpl entries: type names, trait names, function names. The difference is that BACKLOG.md.tmpl deliberately writes them **without backticks** so the `design-doc-source-mismatch` lint does not flag them. They are designed; they are not yet shipped; the doc records both facts.

The BACKLOG also lists deferred concerns (research-driven future work), discovered limitations to address in later rounds, and follow-up tasks. Each entry has a short rationale: why it is deferred, what needs to land first, what would unblock it.

### SHAME.md.tmpl (per crate, structured escape hatch)

Sometimes a project genuinely needs to disable a lint for a specific case. SHAME.md.tmpl is the structured channel. Each escape has a `## <lint-key>` heading and at least 50 words of explanation: what the lint is, why this case is genuinely exceptional, what would have to change for the escape to be retired.

The `## <lint-key>` heading is parsed; the lint at that key is suppressed for the file or scope named in the body. The 50-word minimum is enforced. Drive-by lint disablings without explanation do not pass.

This is not a backdoor: SHAME entries are reviewable in PRs, accumulating SHAMEs is visible at a glance, and the rationale text is searchable when someone later asks "why is this lint off here?"

### DEEPDIVE_*.md.tmpl (per crate, arbitrary topics)

Deep dives are unstructured. Per-crate, the author writes as many `DEEPDIVE_<topic>.md.tmpl` files as needed for the design surface. Topics that span multiple sections of DESIGN.md or that need extended rationale live here. Examples: `DEEPDIVE_strategy-bound-trilemma.md`, `DEEPDIVE_const-trait-bridge-home.md`.

Deep dives are linked from DESIGN.md by name. The assembly pipeline composes the rendered docs/ tree to include deep dives alongside the main design surface.

### Mock-root templates

In addition to per-crate templates, the project root carries three templates that describe the project as a whole:

- `mock/DESIGN.md.tmpl`: the project-level design document. Composes per-crate `README.md.tmpl` summaries via `{{crate_summaries}}` interpolation. Includes a dependency-graph visualisation generated from the project's actual Cargo (or equivalent) dependency declarations.

- `mock/PRINCIPLES.md.tmpl`: the load-bearing rules. Why this project exists, what it commits to, what it refuses. Often shorter than DESIGN.md.tmpl but more carefully phrased: every sentence is a declared invariant.

- `mock/WORKFLOW.md.tmpl`: how the project is developed. The mockspace workflow itself shows up here in project-specific form, plus any project-specific conventions (branch naming, PR shape, release cadence).

### Assembly and rendering

The render pipeline composes these templates into a finished `docs/` tree at the project root. The assembly is:

1. Per-crate `README.md.tmpl` summaries are collected.
2. The mock-root `DESIGN.md.tmpl` interpolates `{{crate_summaries}}` to embed the per-crate summaries.
3. The dependency graph is computed from the actual project structure and rendered (default Graphviz output; alternative renderers configurable).
4. The mock-root `DESIGN.md` is written to `docs/DESIGN.md`.
5. Per-crate `DESIGN.md.tmpl` files are written to `docs/<crate>/DESIGN.md` (or to the crate's own `README.md` for crates published to a registry).
6. `BACKLOG.md.tmpl` files are written to `docs/<crate>/BACKLOG.md`.
7. `SHAME.md.tmpl` files are NOT rendered to public output; they live only in `mock/` for the lint engine to read.
8. Deep dives render under `docs/<crate>/deepdives/<topic>.md`.

The output is structured, consistent, and reflects the source-of-truth templates one-to-one. A diff to a template produces a diff to the rendered output; a reader on docs.rs (or the GitHub web UI) sees current truth.

### Per-crate metadata

`mockspace.toml` declares per-crate display metadata that drives the assembly pipeline:

```toml
[crate_colors.arvo-bits]
fg = "#ffffff"
bg = "#3f51b5"

[crate_colors.arvo-graph]
fg = "#ffffff"
bg = "#009688"

[domain_kinds.numeric]
glyph = "n"
label = "Numeric"

[domain_kinds.bit]
glyph = "b"
label = "Bit-level"

[known_macros.strategy_marker_required]
description = "Every public numeric type carries a Strategy marker."
usage = "S: Strategy = Hot"

layers = ["L0", "L1", "L2", "L3"]
primary_domain_macro = "strategy_marker_required"
primary_domain_label = "Strategy axis"
```

The metadata feeds the dependency-graph render (color per crate, glyph per domain kind) and the cross-reference renderer that produces "this crate is at L2 in the dependency stack, depends on L1 crates X and Y, exists at the Strategy-axis decision."

### Template-engine specifics

The template engine is minijinja-shaped: `{{variable}}` interpolation, `{% for %}` loops over collections, `{% if %}` conditionals, `{% include %}` for fragment inclusion. The data model exposed to templates is the parsed `mockspace.toml`, the discovered crate set, the dependency graph, the per-crate metadata, and any explicit data the project supplies via `mock/data/*.toml`.

Fragment inclusion supports cross-crate shared content. A template fragment authored in `mock/fragments/license-block.md.tmpl` can be `{% include %}`-d by every per-crate `README.md.tmpl`, keeping cross-crate text in one place.

The engine refuses arbitrary code execution: no Python `eval`, no shell-out, no filesystem access beyond the template-resolution path. The template language is data-substitution plus the listed control structures, nothing more.

## 5. The lint engine

The lint engine is the centre of enforcement. Where doc templates establish what is true, lints establish that source matches truth and that source follows the project's conventions.

### Lint architecture

A lint is a Rust trait implementation:

```rust
pub trait Lint {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    fn default_severity() -> Severity;
    fn check(&self, ctx: &LintContext, file: &ParsedFile) -> Outcome<Vec<Finding>, LintError>;
}
```

A `CrossCrateLint` is the same shape but receives the full project's parsed-file collection and emits findings that span crates:

```rust
pub trait CrossCrateLint {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    fn default_severity() -> Severity;
    fn check(&self, ctx: &CrossCrateContext, files: &CrossCrateFiles)
        -> Outcome<Vec<Finding>, LintError>;
}
```

The lint engine walks the project's source tree, parses each file using a tree-sitter grammar (Rust grammar built in; future grammars contribute through language-specific extensions), and runs every registered lint against each file. CrossCrateLint runs once per project invocation with the full parsed set.

### Three categories of lints

**Built-in lints.** Ship with the mockspace binary. Cover universal concerns: claim verification (`design-doc-source-mismatch`), deprecation accounting (`deprecation-comparison`), structural soundness (`changelist-immutability`, `gate-enforcement`), file-size discipline (`file-size`), and the workflow lints that enforce the state machine.

**Custom project lints.** Authored as `.rs` files under `lints/` in the harness ref (see §22 for the trust class framing). Each file exports a `pub fn lint() -> Box<dyn Lint>` (or `pub fn cross_lint() -> Box<dyn CrossCrateLint>`). Projects use this for project-specific rules: a custom naming convention, a project-specific forbidden API, a domain-specific consistency check.

Example: `lints/strategy_marker_required.rs` in the arvo project's harness ref encodes the rule "every public numeric type carries `S: Strategy`" as a Rust source file. The lint is registered, configured per-gate, and runs alongside the built-ins.

Loading mechanism:

- **Compilation.** Mockspace maintains an internal cargo workspace at `.git/mockspace/lint-build/`. On first invocation, custom lint sources are copied into the workspace, compiled via `cargo build --release`, and the resulting cdylibs are cached.
- **Cache key.** Each lint's cached artefact is keyed by `(source-content-sha, rustc-version, MOCKSPACE_LINT_ABI_VERSION)`. Mismatch triggers rebuild. The mockspace binary embeds `MOCKSPACE_LINT_ABI_VERSION` as a compile-time constant; binary-version bumps that change the lint ABI invalidate all cached cdylibs.
- **Cache location.** `.git/mockspace/lint-cache/<key>/`. Cleaned by `mock cache prune`. Survives across mockspace invocations within the same binary version.
- **Toolchain selection.** Mockspace honours the project's `rust-toolchain.toml` for the lint build, so lints use the same toolchain as the project's source code. Toolchain mismatch between the mockspace binary's own build and the project's toolchain is tolerated for ABI-compatible nightly versions; mismatches that break the ABI surface as a structured load-time error.
- **Future direction.** Viola (per the long-term integration plan) subsumes the lint runtime. The dylib-loading mechanism is the v1 path; viola-as-mockspace-runtime is the v2 path. Lints authored for the v1 mechanism migrate to viola plugins; the source-file shape stays largely the same.

**External lint packs.** Shared rule sets distributed as standalone repositories. A project's `mockspace.toml` declares them under `[lint-crates]`:

```toml
[lint-crates]
"mockspace-hilavitkutin-stack-lints" = {
  git = "https://codeberg.org/orgrinrt/mockspace-hilavitkutin-stack-lints.git",
  rev = "abc123def456",
}
```

The pack exposes `pub fn lints()` and `pub fn cross_lints()` functions that return the registered lints. The mockspace binary fetches the pack at the pinned revision (signature-verified, lockfile-pinned), compiles it, loads the registered lints. Stack-wide discipline is one decision per stack, not one decision per project.

### Per-gate severities

Every lint runs at three configurable gates: commit (pre-commit hook), build (during `cargo build` or `cargo check`), push (pre-push hook). Each gate independently choses the severity:

```toml
[lints.no-bare-numeric]
commit = "error"     # blocks the commit
build = "warn"       # warns but allows the build
push = "error"       # blocks the push

[lints.file-size]
commit = "warn"
build = "off"
push = "error"

[lints.naming-convention]
commit = "warn"
build = "off"
push = "warn"
```

Severities: `error` (blocks the gate), `warn` (prints a warning, allows the gate), `info` (prints an informational message), `off` (silent, no check at all).

The split lets projects calibrate. A project might allow naming-convention warnings at commit (to not interrupt local flow) but require resolution at push (so anything reaching CI is clean). A project might want file-size errors at push (to keep main clean) but warnings at commit (so the author sees the trend during the work).

### Severity-downgrade discipline

Severity-downgrades on existing lints are an overseer-level decision, not an agent-level decision. When the lint engine flags a violation, the right response is to fix the violation, not to lower the severity. Severity-downgrades imply the lint itself is wrong (genuinely buggy, or the rule no longer applies), in which case the right fix is to fix the lint or to declare the rule no longer applies.

The mockspace tool emits a structured diagnostic on any `mockspace.toml` change that lowers a gate's severity, and pre-commit / pre-push hooks check for severity-downgrades in the diff. The diagnostic asks the author to confirm: "is the lint wrong? if yes, fix the lint. if no, fix the violation." Severity-downgrades land only with explicit acknowledgement.

This discipline is not enforced by the binary refusing the edit; it is surfaced as a structured diagnostic, and the project's review process decides whether the downgrade is appropriate. The signal-shape is the protection.

## 6. Convention lints, claim verification, escape hatches

Three load-bearing lint categories deserve named treatment.

### Convention lints

Convention lints encode the project's structural conventions. They are the centre of the lint engine for everyday work: they fire often, they catch real drift, they accumulate as the project's living conventions.

Examples of convention lints (each configurable; each ships built-in or as a pack):

- **`file-size`** (built-in). A file exceeding `max_lines` (project-configurable) triggers a finding. Default is 500 lines but per-project tuning is common. The rationale: source files that grow unbounded become hard to navigate; the lint nudges authors to split when growth indicates the file is doing too many concerns.

- **`naming-convention`** (per-project custom). Encodes the project's naming rules. arvo's rule: type names use exact-width form (`UFixed<I, F, S>`, not `Uint16`). Custom lints encode this with a regex-driven check against pub-item declarations.

- **`existence-check-before-add`** (per-project custom). Before allowing a new function or type, the lint scans for similarly-named pre-existing things and warns "did you mean to extend `Foo::bar` instead of adding `do_bar`?" Reduces accidental duplication.

- **`disallowed-apis`** (built-in, project-configured). Forbids specific imports, function calls, or syntactic patterns. arvo's instance forbids `std::*`, `alloc::*`, and bare numeric types in public APIs. The configuration is in `mockspace.toml`:

```toml
[lints.disallowed-apis]
commit = "error"
build = "error"
push = "error"
forbidden_imports = ["std", "alloc"]
forbidden_types = ["u8", "u16", "u32", "u64", "u128", "usize",
                   "i8", "i16", "i32", "i64", "i128", "isize",
                   "f32", "f64", "bool"]
exceptions_in = ["arvo-bits/src/container.rs"]  # the one place where bare primitives live
```

- **`forbidden-imports`** (built-in). The stronger form of disallowed-imports. Encodes layer enforcement: a crate at L1 cannot import from an L2 crate. Configured per-scope with reasons:

```toml
[lints.forbidden-imports.scope.arvo-strategy]
commit = "error"
forbidden = ["arvo-storage", "arvo-graph", "arvo-spectral"]
reason = "arvo-strategy is L0; importing L1+ would create a layer inversion."

[lints.forbidden-imports.scope.arvo-storage]
commit = "error"
forbidden = ["arvo-graph", "arvo-spectral"]
reason = "arvo-storage is L1; cannot depend on L2."
```

Each scope's forbidden list, plus the reason, gets surfaced to agents as a per-scope rule file (see Part I §8). Convention lints become agent-visible without separate authoring.

### Claim verification: `design-doc-source-mismatch`

The lint that makes docs-as-truth structural. Algorithm:

1. Parse every `mock/crates/<crate>/DESIGN.md.tmpl`.
2. Extract every backticked identifier. Filter to identifiers that look like Rust pub-items (matches `[A-Z][a-zA-Z0-9_]*` for types or `[a-z][a-z0-9_]*` for functions).
3. Parse every `crates/<crate>/src/**.rs` file via tree-sitter.
4. Walk the AST to collect every declared pub-item.
5. For each backticked identifier in DESIGN.md.tmpl: if the identifier exists as a pub-item, pass. If not, emit a finding ("DESIGN.md.tmpl claims `Foo` exists but source has no such pub-item").

The check is one-directional: the doc names what shipped; the source must have it. Pub-items that exist in source but are not mentioned in DESIGN.md.tmpl are not flagged here (that is what the BACKLOG and the doc generation pipeline handle).

The lint runs per gate. At commit, it blocks if any source declaration referenced by DESIGN.md.tmpl is missing. At push, the same check runs. The project chooses severity.

The lint understands rename: when a manifest's `[[change]]` block declares a rename from `Foo` to `Bar`, the verifier resolves to the post-rename source state. Renames in flight are not transient violations; the manifest's claim about the rename is what the lint reads.

### Deprecation accounting: `deprecation-comparison`

When a manifest deprecates a prior manifest (via the replan path described in §15), the active manifest must contain a `## Comparison to deprecated changelist` section listing what changed since the deprecation: which files are still in scope, which files dropped out and why, which new files entered. The `deprecation-comparison` lint parses this section and verifies it covers every file from the deprecated manifest.

The rationale: replans are normal (designs evolve), but they should not lose information. The comparison section forces the author to account for every claim the previous manifest made.

### Structured escape: `SHAME.md.tmpl`

Some lint findings cannot be fixed by changing source. Common cases:

- An FFI boundary requires bare types (the C ABI demands `u32`, not `UFixed<32, 0>`).
- A test fixture deliberately exercises a forbidden pattern to verify the lint catches it.
- A pre-1.0 crate genuinely needs a one-time exception during migration.

For these, the project writes a SHAME entry. Format:

```markdown
# crates/arvo-bits/SHAME.md

## no-bare-numeric

The `Bits<N, S>` container at `arvo-bits/src/container.rs:42` declares
`MultiContainer { lo: u128, hi: u128 }` because the heterogeneous N-ary
container layout requires repr-transparent bare-primitive backing for
the inner storage limbs. The single concrete bare-primitive site is
documented; the public surface around this container exposes only
strategy-tagged Bits values; the bare-primitive escape is invisible at
the boundary. Retiring this escape would require a different storage
layout (e.g., array-backed limbs with separate metadata) at a runtime
cost of one extra indirection per access.
```

The `## no-bare-numeric` heading matches the lint key. The body must be at least 50 words. The body must describe the specific case (which file, which line, which type), the rationale (why this exception is genuinely needed), and the retirement condition (what would have to change for the escape to go away). Drive-by SHAME entries without these elements do not pass the SHAME lint itself.

The SHAME entries are not silently disabled lints. They are reviewable in PRs, they accumulate visibly per crate, and the project's review process treats accumulating SHAMEs as a signal that the lint may need refinement or that the design has drifted.

### Meta-config: `[primitive-introductions]`

Some lints have category-level anti-bypass. The bare-primitive lints (`no-bare-numeric`, `no-bare-string`, etc.) check that bare types are not used at API boundaries. But the type-introducing crate itself (`arvo-bits` introduces `Bits<N, S>` for `u8`-shaped data; `arvo` introduces `UFixed`, `IFixed`) genuinely needs bare types in its internal implementation.

`[primitive-introductions]` declares which crates introduce which primitive categories:

```toml
[primitive-introductions]
arvo-bits = ["bit-storage"]
arvo = ["numeric-fixed-point", "boolean", "platform-pointer"]
hilavitkutin-str = ["string"]
```

Crates that introduce a category self-exempt from the bare-primitive lints in that category. Crates that do not introduce a category remain strict.

The configuration accepts only declared category tags, not free strings. A typo or a freelance entry (`arvo-bits = ["raw u32"]`) is rejected by the loader; the category set is closed. This anti-bypass is what stops authors from adding `"raw"` as a category to skirt the lint.

### Category registry

The category set is owned by the lint that defines it. A lint that ships with categories declares them as part of its registration:

```rust
impl Lint for NoBareNumeric {
    const CATEGORIES: &[&str] = &[
        "bit-storage",
        "numeric-fixed-point",
        "boolean",
        "platform-pointer",
    ];
    // ...
}
```

Built-in mockspace lints ship the canonical categories above plus `"string"` (owned by `no-bare-string`).

External lint packs (declared under `[lint-crates]`, §5) can introduce new categories, namespaced under the pack's name to prevent collision:

```toml
[primitive-introductions]
arvo-bits = ["bit-storage"]
my-pack-crate = ["stack-lints::custom-domain"]
```

The loader cross-references every category in `[primitive-introductions]` against the active set of registered lints. Categories not declared by any loaded lint are refused with a structured diagnostic. This is the closed-set guarantee: a `[primitive-introductions]` entry cannot exempt a crate from a lint that does not exist.

When two lints declare categories with the same simple name, the namespaced form is mandatory (`built-in::bit-storage` vs `my-pack-crate::bit-storage`). Built-in categories have no namespace prefix by default but accept `built-in::<name>` as an alias.

## 7. AST extraction

The lint engine's semantic awareness comes from tree-sitter. Each source language has a grammar; mockspace ships the Rust grammar built in. Future languages plug in via separate grammar packages.

### Tree-sitter integration

For each source file, mockspace parses it once per project invocation. The parsed AST is exposed to lints through a `ParsedFile` interface:

```rust
pub struct ParsedFile<'tree> {
    pub path: &'tree Path,
    pub source: &'tree str,
    pub tree: &'tree Tree,
}

impl<'tree> ParsedFile<'tree> {
    pub fn walk_pub_items(&self) -> impl Iterator<Item = PubItem<'tree>>;
    pub fn walk_imports(&self) -> impl Iterator<Item = Import<'tree>>;
    pub fn walk_function_calls(&self) -> impl Iterator<Item = FunctionCall<'tree>>;
    pub fn walk_type_references(&self) -> impl Iterator<Item = TypeReference<'tree>>;
}
```

A lint asks for the AST shape it needs. The single parse is amortised across every lint that runs against the file. The lint engine never re-parses for re-checks within one invocation.

### Pub-item extraction

The most-used AST query is "what pub items does this file declare." Tree-sitter walks the AST; the extractor produces a structured list:

```rust
pub struct PubItem<'tree> {
    pub name: &'tree str,
    pub kind: PubItemKind,         // Struct, Enum, Trait, Fn, Const, Mod, ...
    pub generics: Vec<&'tree str>,
    pub span: Span,
}
```

The `design-doc-source-mismatch` lint uses this directly: collect pub-items per crate, match against DESIGN.md.tmpl claims.

### Language plugin shape

A non-Rust language is supported by a grammar plugin: a separate crate that exports the tree-sitter grammar plus a `LanguageAdapter` impl. The adapter declares which file extensions it claims (`*.ts`, `*.tsx`), which AST node kinds correspond to which pub-item kinds, and which lints it ships.

```rust
pub trait LanguageAdapter {
    fn file_extensions(&self) -> &[&str];
    fn grammar(&self) -> &Grammar;
    fn pub_item_kinds(&self) -> &HashMap<&'static str, PubItemKind>;
    fn ships_lints(&self) -> Vec<Box<dyn Lint>>;
}
```

A project's `mockspace.toml` declares which language adapters apply:

```toml
[languages]
rust = "built-in"
typescript = {
  git = "https://codeberg.org/mockspace/mockspace-ts.git",
  rev = "abc123",
}
```

Mockspace's core stays language-agnostic. Rust is built in as a convenience (it is the most-common language in the workspace this spec targets); TypeScript and others land via plugin.

### What the AST does not do

The AST is for structural queries: "does this pub-item exist?", "what does this file import?", "what trait does this type implement?" It is not for full semantic analysis (type inference, lifetime checking, borrow checking). For deeper analysis, projects integrate with the language's own toolchain (cargo, tsc) and parse the toolchain's structured output.

This bounds the lint engine's responsibility: structural matching against documented claims is its job; the language's own compiler does the rest.

## 8. Agent integration

The same lint configuration generates per-tool agent rule files. The mockspace binary owns the rendering; the AI tools (Claude Code, Copilot, future) just consume the rendered files in their native format.

### Per-tool surfaces

The current supported tools are Claude Code (rules at `.claude/rules/*.md`, skills at `.claude/skills/*/SKILL.md`, hooks at `.claude/hooks/*.sh`) and GitHub Copilot (instructions at `.github/instructions/*.instructions.md`, skills at `.github/skills/*/SKILL.md`, hooks at `.github/hooks/*.sh`). Future tools plug in by adding new render targets to the agent render pipeline.

### Auto-generated rules from lint config

Every `[lints.<name>]` configuration is potentially a rule. For convention lints with project-specific configuration, the lint generates a per-scope rule file describing what is forbidden, why, and the cookbook for the right pattern instead.

Example: the arvo project's `forbidden-imports` lint with per-scope configuration generates `mock/agent/rules/lint-forbidden-imports-arvo-strategy.md.tmpl`:

```markdown
# Forbidden imports in arvo-strategy

This scope is `crates/arvo-strategy/src/**/*.rs`.

## Forbidden

- `arvo-storage`
- `arvo-graph`
- `arvo-spectral`

## Reason

arvo-strategy is L0 (the lowest layer). Importing from L1+ would create
a layer inversion. The strategy axis is consumed by L1+ crates; it
cannot depend on them.

## What to do instead

If you need to express something L1-shaped from L0, the pattern is
to declare a trait at L0 and let L1 implement it. For example, a
predicate-bridge trait that returns `Bool` lives at L1 (where `Bool`
is defined); the L0 type names the trait, the L1 type implements it,
and L0 never names `Bool` directly.

See `mock/crates/arvo-strategy/DEEPDIVE_const-trait-bridge-home.md`
for the worked-example version of this principle.
```

The render target is `.claude/rules/lint-forbidden-imports-arvo-strategy.md` for Claude Code, `.github/instructions/lint-forbidden-imports-arvo-strategy.instructions.md` for Copilot. The same source-of-truth template; the render adapters know each tool's format.

### Per-rule header injection

Every rendered agent rule carries an auto-injected header:

```markdown
> **MOCKSPACE:** docs=design. source=untrusted. Lints never exempt;
> stop if blocked. Flow: topic → doc CL → lock → src CL → lock →
> close. No shortcuts.

> **<PROJECT>:** <project-specific principle injection here>
```

The first line is universal: the docs-as-truth framing reinforced on every rule context-load. The second line is project-specific (per-project configurable in `mock/agent/PREAMBLE.md.tmpl`).

This is not a thumbnail; it is the load-bearing context the agent needs every time it reads any rule. Agents that load rules but skip the preamble miss the foundational principle.

### Skills

Beyond rules (which the agent loads on demand), mockspace renders skills: invokable workflow templates the agent runs when a matching task starts. Skills cover repeated operations: "you are starting a design round; here is the conversation flow," "you are writing a new lint; here is the trait shape and the test harness," "you are reviewing a doc CL; here is the audit checklist."

Per-skill source-of-truth is `mock/agent/skills/<skill-name>/SKILL.md.tmpl`. Each skill renders to per-tool format. The skill content typically includes the rule references it builds on, the step-by-step flow, and any cross-references to project docs.

### Hooks

Hooks are the third agent surface: pre-tool guards that run before the agent invokes a tool. Mockspace renders hook scripts from `mock/agent/hooks/<hook-name>.sh.tmpl` to per-tool format.

Common hook usage: before the agent edits `mockspace.toml`, check whether the edit lowers a gate severity; if yes, surface a structured prompt requiring explicit acknowledgement. Before the agent commits, run the design-doc-source-mismatch check; if any claim is unaligned, refuse the commit until the agent fixes the docs first.

The hook surface gives the agent a "stop sign" channel that does not require the agent to internalise every rule: even if the agent forgets the discipline, the hook catches the violation at tool-call time.

### Agent-rendering pipeline

The pipeline runs as part of every `mock` invocation that touches relevant inputs:

1. Read `mock/agent/MAIN.md.tmpl`, `mock/agent/PREAMBLE.md.tmpl`, `mock/agent/POSTAMBLE.md.tmpl`, `mock/agent/config.toml`.
2. Read `mock/agent/rules/*.md.tmpl`, `mock/agent/skills/*/SKILL.md.tmpl`, `mock/agent/hooks/*.sh.tmpl`.
3. Generate per-scope rule files from lint config (one per scope per scoped lint).
4. Interpolate template variables (project name, lint config, crate set, dep graph).
5. Render to each configured tool's output location (`.claude/`, `.github/`).
6. Merge per-repo overrides if any.

The rendered output is gitignored when the mockspace storage layer is on refs (described in Part III). The local rendered output is what the agent reads; the templates are what authors edit.

### What mockspace knows about AI tools

Mockspace knows file paths, file formats, and platform conventions. It does not know the agents' internal models, behaviours, or capabilities. The render adapters are mechanical translators: per-platform format, per-platform path, per-platform header conventions.

This boundary lets mockspace stay agnostic to which AI tool the developer uses. Adding a new tool is adding a new render adapter, not reworking the source-of-truth templates.

## 9. The bench framework

### Harness contract

The bench harness implementation is the polka-dots-derived port that lands as the workspace bench infrastructure (the multi-round port completed under task #270, shipped as `bench-harness` v2). Mockspace v2 consumes that crate as a dependency.

The bench-harness crate's own contract covers:

- Bench bundle declaration (per-variant function registration, workload generator, cooldown shape, paired-seed contract).
- Statistical analysis (paired bootstrap CIs, BH-adjusted p, sign test, ties handling, per-pass variance).
- Per-iteration sensor recording (wall-clock-ns, perf counters, cache-miss profiles when supported).
- Findings file generation (the structured `.md` shape described below).
- History tracking across runs.

This spec describes mockspace's integration surface (bundles in the round mock-side tree, archival policy, status reporting in topic discussions); the underlying harness API is the bench-harness crate's own contract, version-pinned by mockspace's binary.

The seal contract: when a topic discussion cites a bench bundle, mockspace verifies (at PLAN seal time) that the bundle exists in the round's mock-side ref, the findings.md is structurally valid (parses under the harness's grammar), and the conclusion section is non-empty. The harness owns the statistical correctness; mockspace owns the discipline of citation.



Benches are a first-party design-decision-validation tool. Mockspace ships a bench framework with statistical depth; projects use it during TOPIC phase to confirm or refute design assumptions before locking decisions in.

### Why first-party

A common pattern in design rounds is "we think approach A is faster than approach B, but we should measure before committing." If measurement is offloaded to ad-hoc scripts, the data is inconsistent across rounds, hard to compare, hard to revisit. By shipping benches as a first-party concern of mockspace, the project gets:

- Consistent declaration: bench bundles live under `mock/benches/`.
- Consistent invocation: `mock bench run <bundle>`.
- Consistent output: structured CSV + meta + findings per (bundle, size).
- Consistent analysis: bootstrap CI, paired statistics, ties, per-cooldown breakdown.
- Consistent referencing: topic files cross-reference bench findings; the audit trail is durable.

### Bench bundle declaration

A bundle is declared in `mock/benches/<bundle-name>/bench.toml`:

```toml
schema_version = "1.0"
name = "structural-decomposition"
description = "Compares dense-matrix vs CSR layout for arvo-graph's
  topological-sort kernel across realistic input shapes."
master_seed = 0x1234_5678_DEAD_BEEF

[workload]
generator = "random-acyclic-digraph"
density = [0.05, 0.15, 0.30]

[sizes]
n = [64, 256, 1024, 4096]

[variants]
dense_matrix = { build = "cargo run -p arvo-graph --bin bench-dense", baseline = true }
csr = { build = "cargo run -p arvo-graph --bin bench-csr" }

[analysis]
metric = "wall-clock-ns"
aggregations = ["mean", "median", "best_20pct", "mid_60pct", "worst_20pct"]
ci_level = 0.95
ci_method = "bootstrap"
pairing = "paired-by-seed"
ties_method = "fisher-yates"
```

The bundle declares variants (the implementations being compared), sizes (the input cardinalities to test), workload generation rules (so the bench is reproducible), and analysis configuration.

### Bench execution

`mock bench run <bundle>` runs every variant against every size:

1. For each (variant, size, seed) tuple, build the variant binary if not cached.
2. Per (variant, size), run N iterations (configurable, default 200).
3. Discard the first M iterations (warmup, default 20).
4. Record per-iteration wall-clock-ns to `mock/benches/<bundle>/runs/<variant>-<size>.csv`.
5. Write `mock/benches/<bundle>/runs/<variant>-<size>.meta.json` with: rustc version, host info, build flags, timestamp, master_seed, this run's seed, sample size N, warmup M, raw counts.
6. After all variants per size: run analysis, write `mock/benches/<bundle>/<size>_findings.md`.

### Bench findings

The findings file is structured Markdown:

```markdown
# structural-decomposition n=1024

Generated: 2026-05-18T14:32:11Z. Master seed: 0x1234_5678_DEAD_BEEF.

## Summary

| Variant | Mean | Median | Best 20% | Mid 60% | Worst 20% | Δ Mean vs baseline | 95% CI |
|---|---|---|---|---|---|---|---|
| dense_matrix (baseline) | 12.4 µs | 11.8 µs | 9.2 µs | 12.0 µs | 18.5 µs | (baseline) | (baseline) |
| csr | 8.6 µs | 8.3 µs | 6.5 µs | 8.4 µs | 11.7 µs | -3.8 µs (-30.6%) | [-4.2, -3.4] µs |

## Statistical comparison (csr vs dense_matrix)

Paired test (each seed run on both variants):
- Mean Δ: -3.8 µs (csr faster)
- 95% bootstrap CI on Δ: [-4.2, -3.4] µs (excludes 0 → significant)
- Adjusted p (Benjamini-Hochberg, FDR controlled): 1.2e-4
- Sign test p: 4.7e-3
- Ties: 0/200 iterations

## Per-cooldown breakdown

Cooldown is the inter-iteration sleep (10ms default). Per-cooldown groups:
- 0-10ms post-cooldown: csr 8.4 µs ± 0.3, dense_matrix 12.6 µs ± 0.4
- 10-20ms post-cooldown: csr 8.5 µs ± 0.3, dense_matrix 12.4 µs ± 0.4
- 20+ms post-cooldown: csr 8.7 µs ± 0.4, dense_matrix 12.2 µs ± 0.5

No cold-cache regression for csr at any cooldown.

## Per-pass consistency

Re-running the bench produces consistent ordering: csr faster in 100% of paired
seeds across 5 invocations. Variance: ±0.2 µs in mean Δ.

## Conclusion

csr is approximately 30% faster than dense_matrix for n=1024 across the workload
shape tested. The advantage is consistent across cooldowns and re-runs. Topic
202605181400-arvo-graph-csr-vs-dense recommends csr as the default backend for
arvo-graph's topological-sort kernel at this size range.
```

### Bench findings in topic discussions

Topic files cross-reference findings:

```markdown
# Topic: arvo-graph storage layout

[...prose...]

## Decision: CSR over dense matrix

The dense-matrix layout was the initial sketch; CSR was proposed mid-round
after the L1-cache-occupancy analysis. The structural-decomposition bench
confirms a 30% speedup at n=1024 (see `mock/benches/structural-decomposition/1024_findings.md`),
holding consistently across cooldowns and re-runs. CSR is the default
backend going forward; the dense-matrix variant retires.
```

The cross-reference is plain Markdown; no special syntax. The discipline is that decisions cite findings: an unsourced "I think CSR is faster" claim does not survive review.

### When benches happen

Benches are integrated into the TOPIC phase. A round that touches performance-sensitive code typically runs:

1. Early TOPIC discussion of design alternatives.
2. Sketch (Part I §10) to confirm each alternative is feasible.
3. Bench bundle declaration covering the alternatives.
4. `mock bench run <bundle>` to gather data.
5. Findings analysis informs the design decision.
6. Topic file documents the decision with cross-reference to findings.

The order is not strict: sometimes benches surface during APPLY when a verifier reveals a regression. The point is that benches are a workflow tool available throughout the round, not a separate ceremony.

### Bench bundle archival

Benches that informed locked design decisions stay forever as audit trail. Benches that produced inconclusive findings or that were superseded by later benches remain too; the discipline is the same as for sketches and research. The full bench history is durable record of "what we tested, what we found, what we decided."

For projects with high bench volume, archival follows the round lifecycle: when a round closes, its benches are part of the round's record (see Part III). Cross-bundle reuse is by reference, not by copy.

## 10. The sketch protocol

A sketch is a small, isolated piece of code authored during TOPIC phase to confirm that a proposed design is feasible: does the toolchain actually accept this? does the type system actually compose the way we expect? does the runtime actually behave the way the design assumes?

Sketches are not benches (benches measure performance) and not research (research is prose). Sketches are minimal code that compiles and runs.

### Sketch directory layout

```
mock/research/sketches/<round-slug>/
├── README.md
├── 01-sketch-name.rs
├── 02-other-sketch.rs
└── ...
```

The `README.md` per directory describes the sketches as a group:

```markdown
# Sketches for round 202605181400

Sketches for the heterogeneous N-ary container redesign (topic 02).

## Sketches

- **01-cons-list-projection.rs**: live compile probe. Builds a cons-list
  of concrete `MultiContainer` types and projects to a homogeneous trait
  via the projection bridge. Verifies the projection compiles under
  `next-solver=globally`.
- **02-binpack-maybe-sentinel.rs**: design probe. Demonstrates the
  Maybe-sentinel pattern proposed in the topic; validated during SRC
  by replacing one existing body with the sketch's pattern and
  confirming `cargo check --workspace` green.

## Status

| Sketch | Kind | Status |
|---|---|---|
| 01-cons-list-projection.rs | live | WORKS |
| 02-binpack-maybe-sentinel.rs | design | DESIGN-PROBE-DURING-SRC |
```

### Live compile probes versus design probes

The README's status column captures a load-bearing distinction:

- **Live compile probe.** The sketch is a complete `.rs` file that mockspace compiles via the project-configured sketch runner. The result is one of `WORKS` (compiles), `FAILS WITH <error>` (compiles errors named), or `INCONCLUSIVE: needs deeper investigation`. Live probes ground the topic discussion in actual toolchain behaviour: a design that claims "this trait bound is satisfiable" stops being a claim if the sketch fails to compile. The runner invocation is project-configured under `[sketch.runner]` in `mockspace.toml` (default: invoke `rustc` with whatever flags the project's `rust-toolchain.toml` implies; Rust projects with experimental needs add toolchain-specific flags like `-Z next-solver=globally` there). Non-Rust projects override the runner entirely. See §46 for the schema.

- **Design probe.** The sketch is a `.rs` file that demonstrates the desired shape but cannot be compiled in isolation (it depends on workspace types). The probe's hypothesis is stated at the top; during SRC phase, the implementer applies the probe's pattern to one real call site and confirms `cargo check --workspace` accepts the change. The design probe's validation happens during SRC, not during TOPIC.

  Design probes are enforced. The src manifest declares `validated_design_probes = ["mock://sketch/<round-slug>/02-binpack-maybe-sentinel.rs", ...]`. The verifier at APPLY(SRC) seal time confirms each named sketch corresponds to at least one `[[change]]` block in the manifest whose `file` matches a path the sketch's hypothesis names as the validation target. Sketches listed but not validated by any change block emit a finding that blocks seal; the implementer either adds the validating change or removes the sketch from the list with a SHAME entry explaining why the design probe was abandoned.

The distinction matters because the false confidence shape is real. A design that "looks right in isolation" but fails when integrated produces a wasted SRC phase. A design that compiles in isolation but does not actually solve the problem the workspace has produces wasted commit volume. Splitting probes by kind makes the validation explicit.

### Sketch invocation

`mock sketch new <round-slug> <sketch-name>` scaffolds the sketch directory + a starter `.rs` file with the hypothesis stub. `mock sketch run <sketch-path>` compiles a live probe. `mock sketch report <round-slug>` writes the README's status table from the per-sketch result files.

For design probes, there is no automated runner; the SRC phase's validation is part of the SRC manifest's verifier (see Part III §17).

### Sketches commit before doc-CL lock

The discipline (from `cl-claim-sketch-discipline`) is that sketches land before the round's doc manifest locks. A round whose doc manifest claims a design is feasible must point to a sketch demonstrating that feasibility, recorded in the round's record at lock time. Doc manifests that name design decisions without sketch backing (when the design has trait-solver-cycle risk, generic-const-expr risk, repr(transparent) layout risk, or similar toolchain-acceptance concerns) are flagged by the doc-CL audit.

### Sketches live forever

A sketch that worked stays as audit trail. A sketch that failed stays as audit trail. A sketch superseded by a later sketch is not deleted; the superseding sketch references the predecessor. The accumulated sketch directory tells the design story: "we tried X (FAILED), pivoted to Y (WORKS), and shipped Z."

This is the same discipline as the deprecated-CL chain: the failure record is the load-bearing part of the audit trail.

### When sketches happen

Sketches happen during TOPIC phase, often interleaved with research and benches. A typical round flow:

1. Topic file opens the discussion.
2. Research notes summarise prior art.
3. Sketches confirm toolchain acceptance of proposed designs.
4. Benches confirm performance claims.
5. Topic file converges on a decision.
6. Doc manifest locks the decision in.
7. SRC manifest specifies the implementation.
8. SRC phase applies the implementation; design probes get validated during APPLY.

The order is fluid; the elements are durable.

## 11. The research workflow

Research notes are long-form prose authored during TOPIC. They cover the things that don't fit a topic file's structure: prior-art surveys, sibling-project analyses, theoretical discussions, retrospective reviews of past rounds.

### Directory layout

```
mock/research/
├── <round-slug>_topic.<name>.md          (round-scoped topic-adjacent prose)
├── <round-slug>/                         (round-scoped research bundle)
│   ├── README.md
│   ├── <topic>.md
│   └── ...
├── imported-from-<source>/                (prior-art imports)
│   ├── README.md
│   └── <copied-or-adapted-content>.md
├── <crate>-history.md                    (per-crate accumulated history)
└── sketches/                             (see Part I §10)
```

Research is not rendered to public output. It lives in `mock/research/` as authoring substrate; published docs assemble from templates, not from research.

### Imports from prior art

When a round consults prior-art from a sibling project (often a research repo by the same author), the relevant content is imported under `mock/research/imported-from-<source>/`. The import preserves the original content verbatim where possible; an `IMPORT.md` file in the directory states what was imported, from where, and on what date. Imports are read-only; later corrections come as separate research notes that reference the import.

This pattern is common when a project draws on prior research not yet published as a stable artefact (sketches in a sibling repo, design docs from a precursor project). The import is the durable record; the original may evolve, but the imported copy stays as the snapshot the round consulted.

### Per-crate history files

Long-running projects accumulate per-crate history: which rounds touched this crate, what shipped, what was deferred, what was reworked. A per-crate `<crate>-history.md` file maintains the chronological record. Entries are usually short (one paragraph per round) with cross-references to the round artefacts.

The history file is part of `mock/research/` (not `mock/crates/<crate>/`) because it is per-crate prose without backticked claims; the design-doc-source-mismatch lint does not apply to it.

### When research happens

Research is asynchronous. A round in TOPIC phase might pause to import prior-art (writing research notes), might run a sketch (recording the result alongside research), might consult bench findings (cross-referencing from research). Research notes feed the topic discussion; the topic file is the structured summary; research is the long-form backing.

Research outside an active round (a researcher writing a survey, a design-explorer documenting a prior project's shape) lives at `mock/research/` root without a round slug. These are durable artefacts available to future rounds.

## 12. Multi-topic rounds and sister-correction

Most rounds are single-topic (one topic file, focused scope). Some rounds carry multiple topics that correct each other's framing as the design matures. Both shapes use the same six-phase structure.

### When a single topic suffices

Single-topic rounds cover bounded scope: one bug fix, one refactor of one module, one new feature in one crate, one doc cleanup. The author opens the topic, discusses the design, locks the doc manifest, applies it, locks the src manifest, applies it, closes. A single-topic round is the common shape.

### When multiple topics fit one round

Several topics belong in one round when they share scope and timing:

- **Sister-correction.** Topic 1 proposes an approach; mid-round, deeper investigation reveals the approach has a flaw; Topic 2 names the flaw and proposes a corrected approach. Topic 2 deprecates Topic 1's framing (without rewriting Topic 1, which stays as audit trail).

- **Scope absorption.** Topic 1 covers the primary design decision; Topic 2 surfaces an adjacent concern that the primary work necessarily touches. Topic 2 absorbs the adjacent concern into the same round rather than opening a separate one.

- **Coordinated change.** Several distinct decisions land together because they only make sense together. Topic A, Topic B, Topic C each cover one decision; the round's doc + src manifests cover all three.

### Sister-correction mechanics

A sister topic is a new topic file in the same round directory, typically with a slightly later timestamp. The new file states what it corrects, why, and what the new framing is. The deprecated topic stays unchanged; the new topic names it by filename.

Example: an arvo round started with `01_topic.fnv1a-hash-only.md` proposing FNV-1a as the only hash algorithm. Mid-round, an audit surfaced that several use cases require content-addressing (where xxhash3 wins). A sister topic `02_topic.hash-family-expansion.md` opened: "Topic 01 is too narrow; the substrate ships a hash family with FNV-1a, xxhash3, and SipHash. Topic 01 stays as audit trail; this topic is the new direction." The round's doc manifest reflects the corrected scope.

### Scope absorption mechanics

A scope-absorption topic states: "topic X surfaces concern Y, which we'd otherwise split off. Absorbing Y into this round because <reason>." The doc manifest covers both. The author does not open a new round for Y because the work is naturally entangled with X.

The discipline against runaway absorption: each absorption topic states the reason explicitly. "Y is trivial, less than 10 lines" is a fine reason. "Y is the consequence of X" is a fine reason. "Y is somewhat related" is not; that gets a separate round.

### Multi-topic round closure

At round close, the round's record contains all topic files (original and corrective), all sketches, all bench bundles, all research notes, the locked doc manifest, the locked src manifest, and the closure record. Future rounds reading the closed round see the full history including the corrections.

## 13. Vocabulary

This spec uses the following terms uniformly.

| Term | Meaning |
|---|---|
| Round | Discrete unit of workflow scoped to one coherent design change. |
| Slug | Identifier `<YYYYMMDDHHMM>-<short-name>` for rounds, research, benches, sketches. |
| Phase | One of TOPIC, PLAN(DOC), APPLY(DOC), PLAN(SRC), APPLY(SRC), DONE. |
| Manifest | Sealed contract per APPLY phase. Doc + src per round. |
| Task | Work item with namespace-qualified slug. Lifecycle independent of rounds. |
| Topic | Free-form Markdown prose authored during a round. |
| Sketch | Exploratory code in `mock/research/sketches/`. Lives forever. |
| Research | Long-form prose in `mock/research/`. Lives forever. |
| Bench | Statistical experiment under `mock/benches/`. Findings live forever. |
| Lint | Rule check at commit / build / push gate. |
| Lint pack | External shared lint set, imported from a host. |
| Convention lint | Lint encoding a project-specific structural rule. |
| SHAME entry | Structured lint-escape with rationale, in `SHAME.md.tmpl`. |
| Harness | The project's mockspace configuration ref. |
| Anchor | Per-file content snapshot captured at APPLY phase entry. |
| Replan | Backward transition from APPLY to PLAN. Always deprecating. |
| Mock-side ref | `refs/mock/<...>` carrying mockspace artefacts. |
| Source-side branch | `refs/heads/round/<slug>` carrying source-side commits. |
| Host | Named alias for a git URL serving mockspace-shaped content. |
| Import | Consumed `mock://` URI declared in `[imports]`. |
| Export | Package this project publishes for others. |
| Package | Content addressable via `mock://export/<name>`. |
| Hook | Reactive script invoked on mockspace event with structured env. |
| Profile | TOML section declaring per-event reactive handlers. |
| Mirror | Locally-stored 1:1 copy of an externally-referenced ref. |
| Lockfile | `mockspace.lock` at harness root pinning imports by SHA + signing key. |
| Verifier | Lock-time mechanism running structured checks per manifest claim. |
| Claim | A `[[change]]` entry in a manifest. |
| TOFU | Trust on First Use; per-developer signing-key acceptance. |
| Cons-list | Recursive type-level list `Empty` or `Cons<H, T>`. (Not "HList".) |
| Record | One data point in a collection (column or set). Not "entry". |
| Foundations | The base layers of an abstraction. (Not "substrate".) |

Banned in this spec: em-dashes (use period, comma, parens, colon, or semicolon), the words "essentially / basically / fundamentally" as filler, marketing words (powerful / robust / seamless / blazing / streamline / leverage / utilize / unlock / paradigm / holistic), exclamation marks in prose, ASCII box-and-arrow diagrams, leading flat bullet lists at section openings.

# Part II. The state machine

## 14. The six phases

A round always carries exactly one current phase. Phase identity drives every command's semantics: `mock phase apply` does different things in PLAN(DOC) versus PLAN(SRC). The six phases are:

**TOPIC.** Free-form exploration. Topic files, sketches, benches, research notes are authored here. No manifest exists. The round can stay in TOPIC for arbitrary duration. Exit is by calling `mock phase plan`, which scaffolds the first doc manifest and transitions to PLAN(DOC).

**PLAN(DOC).** Doc-manifest authoring. The manifest is mutable. The author writes per-template change blocks: which template files change, what edits land in each, which task IDs the changes resolve. The manifest is just a TOML document on the round's mock-side ref; it edits like any text file. Exit is by `mock phase apply`, which seals the manifest, runs the doc-side verifier, captures the doc-side anchor, and transitions to APPLY(DOC).

**APPLY(DOC).** Doc execution. The manifest is sealed (no further edits). The author edits doc templates per the sealed manifest. The verifier runs on every commit to confirm the doc state matches the manifest's claims; mismatches block. PR projection opens at APPLY(DOC) entry. Exit is by `mock phase finish`, which transitions to PLAN(SRC). The doc manifest is now permanent; the source files it claimed are recorded; the doc-side anchor (per-file content snapshots captured at APPLY entry) is preserved for replan.

**PLAN(SRC).** Src-manifest authoring. Same shape as PLAN(DOC): mutable TOML, per-file change blocks, verifier rules. The author writes the src manifest with the docs (now locked) as the contract. The src manifest's claims must align with what the doc manifest already claimed: types named in DESIGN.md.tmpl appear in the src manifest's change list with verifier rules confirming their presence at the new shape. Exit is by `mock phase apply`.

**APPLY(SRC).** Src execution. The src manifest is sealed; source files are edited; the verifier runs on every commit. At each commit, the lint engine runs design-doc-source-mismatch across the whole project, confirming source and docs are aligned. Exit is by `mock phase finish`, which transitions to DONE.

**DONE.** Final phase. The round is no longer in active development. PR comments are ingested. The round's record is frozen. Exit is by `mock close`, which archives the round and optionally merges the PR.

### Active round resolution

With `default_one_active_round = true` (the default), mockspace computes "the active round" as the single `refs/mock/round/<slug>` whose `.phase` marker is not `DONE`. The state machine guarantees uniqueness: `mock round new` refuses to create a second round while an existing one is non-DONE; closure (`mock close`) advances `.phase` to `DONE` before archive, which frees the slot.

Branch checkout state is observational, not authoritative. `mock status` from any branch resolves to the same active round; the round identity comes from the ref set, not from parent HEAD. This matters because phase transitions commit on the mock-side ref independently of parent worktree state.

For multi-active mode (deferred; see Part VIII §59), an explicit `refs/mock/.active` pointer would carry the per-developer choice. The v1 default has no such pointer because the constraint reduces it to derived state.

### Phase invariants

The state machine enforces:

- One active round per repository by default. (Multi-active mode named in config but not implemented in v1; see Part VIII §59.)
- One phase current per active round.
- Sealed manifests are immutable forever.
- Deprecated manifests are immutable forever; numbered by replan iteration.
- Topic documents are mutable until the round reaches DONE.
- Round mock-side refs are never squashed or rebased before close.

### What each phase enforces

The phase machine is not just a sequencer; it is also a gate. Each phase has rules about what the developer can and cannot do:

| Phase | Mock-side ref edits | Source-side commits | Lints run |
|---|---|---|---|
| TOPIC | Topics, sketches, benches, research | Allowed (off the round's branch) | Standard + sketch validity |
| PLAN(DOC) | Doc manifest (mutable) | Allowed | + manifest grammar |
| APPLY(DOC) | None to manifest; doc templates | Required to match manifest | + design-doc-source-mismatch on templates |
| PLAN(SRC) | Src manifest (mutable) | Allowed | + src-manifest references doc-manifest claims |
| APPLY(SRC) | None to manifest | Required to match manifest | + design-doc-source-mismatch on full project |
| DONE | Comment ingestion | None | (Read-only mode) |

The lint engine knows the phase. Lints check appropriate things at appropriate times: file-size lints run at every commit; design-doc-source-mismatch runs at APPLY(SRC) commit; manifest-grammar lints run at PLAN(SRC) commit. The phase context comes from `mock status`.

## 15. Phase transitions

Three forward verbs and one backward verb.

### `mock phase plan`

Valid from TOPIC, APPLY(DOC), APPLY(SRC) (when a replan didn't already produce a deprecated manifest at this position). Scaffolds the next manifest:

```toml
# Example scaffolded manifest.doc.toml
mockspace_version = "1.0"
round_slug = "202605181400-arvo-graph-csr"
phase = "doc"

[scope]
description = ""

[acceptance]
criteria = """
"""

# [[change]] blocks: one per file
```

The author fills in scope, acceptance, and per-file `[[change]]` blocks (see Part VIII §53 for the full schema). The manifest is a normal text file; the author edits it like any other.

The verb is named `plan` because that is what the user does: opens the planning surface. From mockspace's perspective, it scaffolds a manifest and updates the phase marker.

### `mock phase apply`

Valid from PLAN(DOC) and PLAN(SRC). Seals the current manifest and transitions to APPLY:

1. Acquire flock on `.git/mockspace/.lock`.
2. Fetch the round's mock-side ref from origin; verify clean state.
3. Read the current source-side branch tip SHA.
4. Validate the manifest's grammar (TOML well-formed, required fields, schema-version compatible).
5. Validate the manifest's references (every task ref resolves; every step-key reference is valid; every claimed file exists or is created by the change).
6. Run the manifest's verifier against the source-side branch tip in a temporary worktree. All claims must pass.
7. Capture per-file anchor: for every claimed file, record `(path, blob_sha)`. Store blob bytes content-addressed under `.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>`. (See Part III §23 for anchor mechanics.)
8. Build the new tree for the round mock-side ref: rename `manifest.<phase>.toml` to `manifest.<phase>.locked.toml`; record the anchor; rewrite the phase marker.
9. Render the source-tree and local-only targets locally (templates, agent files, etc.).
10. `git update-ref refs/mock/round/<slug> <new-commit>`.
11. `git push origin refs/mock/round/<slug>`; on non-fast-forward, invoke `on_phase_race` per profile (see Part V §35).
12. Release the flock.
13. If forge integration is configured: open or update the PR projection.

The verb is named `apply` because that is what the user does: applies the planned changes. The manifest goes from draft to sealed; the verifier confirms the source-side state aligns; the anchor freezes the per-file state for possible later replan.

### `mock phase finish`

Valid from APPLY(DOC) and APPLY(SRC). Transitions to the next PLAN or to DONE:

- APPLY(DOC) → PLAN(SRC): scaffold the src manifest, transition.
- APPLY(SRC) → DONE: finalise the round state, transition.

`finish` is the verb that says "this phase's work is done; advance." No new validation runs at finish (the validation happened at the prior `apply`); finish is bookkeeping.

### `mock phase replan`

The backward verb. Valid from APPLY(DOC) and APPLY(SRC). Deprecates the current manifest and returns to PLAN:

1. Verify parent worktree is on the round's source-side branch.
2. Check for non-claimed source-side edits since APPLY entry (uncommitted changes outside the claimed-files set). Invoke `on_replan_nonclaimed_edits` per profile.
3. Check for post-APPLY commits to claimed files. If any, refuse the destructive restore; the user runs `--restore-by-commit` for additive restoration or `--accept-restoration-loss <file>...` per file to discard the post-APPLY work.
4. Read `.anchor.<phase>.toml`.
5. For each `[[file]]`: restore the blob bytes to the file path; verify the SHA matches.
6. Commit the restoration on the source-side branch with trailer `Workflow-Replan: restore <phase>-side surfaces from anchor`.
7. Build the new round mock-side tree: rename `manifest.<phase>.locked.toml` to `manifest.<phase>.deprecated.<n>.toml`; scaffold a fresh `manifest.<phase>.toml`; rewrite the phase marker; clear the anchor.
8. Commit + push the round mock-side ref.
9. Regenerate the PR body.

Replan is always deprecating. There is no "unlock the current manifest, edit it, lock it again" flow; the manifest IS the sealed contract, and deprecation is the explicit "this contract is no longer in effect; a new one supersedes it."

**Replan is phase-scoped.** From APPLY(SRC), only the SRC anchor and SRC manifest are touched. The DOC manifest (in `manifest.doc.locked.toml`) and DOC anchor (in `.anchor.doc.toml` + `.anchor.doc.blobs/`) persist as part of the round mock-side ref tree throughout SRC work, deprecated SRC iterations, and archive. See §23 Reachability and §25 tree layout. There is no path through `mock phase replan` alone that returns to PLAN(DOC); reverting doc-side work requires `mock undo` against the APPLY(DOC) transition (§42), which is a destructive operation requiring explicit acknowledgement.

The deprecated manifest stays in the round's record. The new manifest must include a `## Comparison to deprecated changelist` section accounting for every claim from the deprecated version (see Part I §6 and Part VIII §53 for the comparison-section schema). The `deprecation-comparison` lint enforces this at PLAN seal time.

### Additive replan: `--restore-by-commit`

Default replan overwrites the source-side files at restoration time. If post-APPLY work has built on those files, the overwrite discards work. `mock phase replan --restore-by-commit` uses an additive flow:

1. Steps 1-2 as above.
2. Step 3 is skipped (post-APPLY commits to claimed files are tolerated).
3. Step 5 commits the restoration on top of the current source-side state rather than overwriting. The result: the source-side branch has the post-APPLY work, followed by an "additive replan" commit that brings the anchor state back.
4. Steps 6-9 as above.

The history is more cluttered (the round shows the original work, the post-APPLY work, and the additive replan commit), but no work is lost. Useful when a replan happens after non-trivial post-APPLY work that should not be discarded.

### Anchor restoration verification

The anchor blobs are content-addressed under their SHA. Restoration reads the blob bytes from `.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>`, computes the SHA of the read bytes, and confirms it matches the path-encoded SHA. Mismatch fires `D004` (anchor blob SHA mismatch); missing blob fires `D040` (anchor blob missing from storage). Either is a hard error that aborts replan.

## 16. Tasks

Tasks are work items with identity, lifecycle, and content. A task is named by a `mock://` URI (`mock://task/<path>`), lives on a dedicated ref (`refs/mock/task/<path>`), and may be claimed by one or more rounds before closure.

Tasks are not rounds. A round is a workflow unit (TOPIC → PLAN → APPLY → DONE); a task is a discrete work item whose lifecycle is "open → in-progress → closed". A round may resolve zero, one, or many tasks. A task may span multiple rounds (one round opens it; a later round closes it). Tasks may also be born and closed within a single round.

### Task identity

A task's identity is a single path of slug-shaped segments. The final segment is the leaf (called the slug); any preceding segments form the namespace. Examples:

- `compiler::ir::structural-robust-ir` (two namespace segments, slug `structural-robust-ir`)
- `compiler::ir::lower-pass::implement-parser` (three namespace segments, slug `implement-parser`)
- `workspace::migrate-to-codeberg` (one namespace segment, slug `migrate-to-codeberg`)
- `migrate-to-codeberg` (single-segment, no namespace; permitted but see convention note below)

Slug characters: `[a-z][a-z0-9-]{0,62}`. Every path segment uses the same charset, including step keys.

In refs: `/` separator. `refs/mock/task/compiler/ir/structural-robust-ir`.
In URIs: `::` separator. `mock://task/compiler::ir::structural-robust-ir`.

**Single-segment task identifiers are permitted.** A top-level task `mock://task/migrate-to-codeberg` is valid: namespace empty, slug `migrate-to-codeberg`. Mockspace does not police away the no-namespace case.

**Convention: prefer namespaces.** Even one namespace segment unlocks task hierarchy for tooling. List filtering, namespace-scoped views, search, completion, and aggregate operations all become first-class once tasks are organised under namespaces. A task that genuinely belongs at the project root may stay top-level; tasks that fit any larger grouping should pick a namespace.

The `#` character is reserved for **step references** within a task. It never separates namespace from slug. A step reference is `<task-path>#<step-key>`, where `<step-key>` matches a key in the task's `meta.toml` `[steps.<key>]` table (see Sub-tasks below). Step keys themselves follow the slug charset. Manifests claim steps via this form:

```toml
[[change]]
task = "mock://task/compiler::ir::structural-robust-ir#define-grammar"
```

In that example: task at `mock://task/compiler::ir::structural-robust-ir` (3-segment path, leaf slug `structural-robust-ir`), step `define-grammar` within that task. A bare task claim (no step) omits the `#<step-key>` suffix.

### Task state machine

A task has a `.state` marker file at its ref's root:

| State | Marker | Meaning |
|---|---|---|
| open | `.state.open` | The task exists; no one is actively working on it. |
| in-progress | `.state.in-progress` | The task is being worked on. |
| blocked | `.state.blocked` | The task cannot proceed; waiting on something. |
| deferred | `.state.deferred` | The task is intentionally postponed. |
| closed | `.state.closed` | The task is no longer active. |

Closed tasks carry a `[closure]` block in `meta.toml`:

```toml
[closure]
resolution = "completed"   # or "cancelled", "superseded", "wontfix"
closed_at = "2026-05-18T14:30:00Z"
closed_branch = "round/202605181400-arvo-graph-csr"
closing_phase = "apply_src"
closing_round_slug = "202605181400-arvo-graph-csr"
```

### Task structure

A task's ref tree:

```
refs/mock/task/<ns>/<slug>/
├── .state.<current-state>
├── meta.toml
└── <slug>.md
```

`meta.toml`:

```toml
mockspace_version = "1.0"
id = "mock://task/compiler::ir::structural-robust-ir"
title = "Define structural robust IR shape"
created = "2026-05-18T10:00:00Z"
priority = "P1"
group = "ref-based-redesign"        # optional grouping label

# Sub-tasks (steps). Each step has state + phase tag. Step keys follow
# slug charset (`[a-z][a-z0-9-]{0,62}`).
[steps.define-grammar]
description = "Specify the IR grammar in DESIGN.md."
phase = "doc"                       # or "src" or "doc+src"
state = "closed"

[steps.implement-parser]
description = "Implement the IR parser in compiler-ir/src/parser.rs."
phase = "src"
state = "open"

# Cross-references to other tasks (bare mock://task/<path> URIs) or
# steps within them (URI plus `#<step-key>`). Any valid mock:// task
# or step URI is accepted in any of the three lists.
[refs]
blocks = ["mock://task/compiler::ir::lower-pass"]
blocked_by = []
relates_to = ["mock://task/compiler::ir::lower-pass#define-grammar"]
```

The `id` field is the canonical full identifier as a `mock://task/<path>` URI. Mockspace derives namespace and slug from this single field; the older `namespace` + `slug` split is retired in favour of one canonical URI form that matches the manifest claim shape and the cross-reference list shape.

`<slug>.md` is the task's content: prose describing the work, the constraints, the acceptance criteria. Free-form Markdown.

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

`mock task new` fetches origin first; refuses if the ref exists remotely or in the archive (preventing accidental overwrite of a task someone else created).

`mock task move` renames a task. The old ref redirects (a marker pointing at the new ref) so existing references continue to resolve.

### Task claims in manifests

A manifest's `[[change]]` block can claim a task or a step:

```toml
[[change]]
task = "mock://task/compiler::ir::structural-robust-ir#define-grammar"
file = "crates/compiler-ir/DESIGN.md"
description = "Define the grammar section."
# verifier block ...
```

At manifest seal time:

1. The verifier resolves the task ref. Failure: refuse seal.
2. The verifier checks the step's `phase` tag aligns with the manifest's phase (doc claims target doc-phase steps; src claims target src-phase steps; combined `doc+src` steps may be claimed by either).
3. The verifier checks the step's `state` is not already `closed` (closing an already-closed step is suspicious).

At manifest seal, the relevant step's state moves to `in-progress` (or stays if already there). At APPLY-phase commit that satisfies the claim, the step's state moves to `closed`. The state transitions are mockspace-managed; the author does not edit `meta.toml`'s state directly.

### Task archive

A closed task lives on its ref until archival. `mock task archive` (or auto-archival via `task_archive_threshold_days`) moves the task into the unified archive ref `refs/mock/task-archive`. Archive entries carry the same tree shape under `<ns-path>/<slug>/`.

The archive ref is one ref, not N (one per archived task). Auto-archival is triggered by `mock close` (full sweep) and `mock sync --full`.

## 17. Manifests

A manifest is the structured contract a round seals at APPLY entry. Two manifests per round: `manifest.doc.toml` for the doc phase and `manifest.src.toml` for the src phase. Same shape; different scope.

### Manifest lifecycle

1. **Scaffolded** at `mock phase plan` entry. The mock-side ref tree grows a fresh `manifest.<phase>.toml` with empty scope, empty acceptance, no `[[change]]` blocks. The author edits.
2. **Drafted** during PLAN. The author writes per-file `[[change]]` blocks. The manifest is mutable; iterative editing is fine.
3. **Sealed** at `mock phase apply`. The manifest is renamed `manifest.<phase>.locked.toml`. The verifier runs. An anchor is captured.
4. **Deprecated** if replan invoked. Renamed `manifest.<phase>.deprecated.<n>.toml`, where `<n>` is the next iteration. A fresh `manifest.<phase>.toml` is scaffolded.

Sealed manifests are immutable. Deprecated manifests are also immutable. The round's record accumulates: every manifest the round produced, in order, with the deprecated ones preserved as audit trail.

### Manifest content

The schema is described in detail in Part VIII §53. Summary:

- `mockspace_version`, `round_slug`, `phase`: metadata.
- `[scope]`: what the manifest covers; what's explicitly out of scope.
- `[acceptance]`: the criteria for considering the manifest's work complete.
- `[[change]]` array: per-file change blocks. Each block names: `task` (optional task ref), `file` (the file changed), `description` (one-paragraph summary), `[change.verify]` (structured verifier rules).
- `[[deprecated_accounting]]` array (only when superseding a deprecated manifest): per-file accounting: every file from the deprecated manifest must either appear as a `[[change]].file` or here with an `omitted_reason`.

### Manifest verifier

The verifier is a closed catalog of check kinds (Part VIII §54). Each `[change.verify]` block names a kind and its arguments:

```toml
[change.verify]
kind = "grep_present"
pattern = "pub struct Baz"
file = "crates/ir/src/grammar.rs"
```

Composition via `all_of`, `any_of`, `not`:

```toml
[change.verify]
all_of = [
  { kind = "grep_present", pattern = "pub struct Baz", file = "crates/ir/src/grammar.rs" },
  { kind = "grep_absent", pattern = "pub struct Bar", file = "crates/ir/src/grammar.rs" },
  { kind = "function_present", name = "Baz::new", file = "crates/ir/src/grammar.rs" },
]
```

The verifier runs at APPLY entry against a temporary worktree at the source-side branch tip. All claims must pass; one failure aborts seal. No partial seals.

### No free shell verifiers

The verifier kinds are a closed catalog. There is no `command_succeeds` kind, no `run_shell_script` kind, no escape hatch that executes arbitrary user-supplied commands. A manifest is PR-author-controlled (it lands on the source-side branch); allowing arbitrary command execution from it would be a code-execution surface.

The contrasting trust class is custom lints. Custom project lints live on the harness ref (`refs/mock/harness`, see §22), not on any source-side feature branch. PR authors cannot introduce custom lints by pushing a feature branch; harness commits go through the project's normal review process. The two surfaces are intentionally walled off: manifests are author-controlled and use only the closed verifier-kind catalog; custom lints are review-gated and may use full native code. See also §54 on the verifier catalog growth path for cases the catalog does not yet cover.

When a project needs a check the catalog does not yet cover, the path is to propose the new kind upstream (contribute to mockspace core or a language extension). The kind is reviewed and merged. The project bumps `mockspace_version` to the version that ships the kind. The catalog grows; the closed-catalog discipline holds.

In the interim, existing kinds compose to cover most cases: `grep_present` + `grep_absent` + `path_exists` + `file_size_below` + `line_count_above` chain into surprisingly rich checks.

### Manifest as audit trail

A manifest names what changed. A deprecated manifest names what was tried and abandoned. The sequence of manifests across a round is the structured record of the round's work. Future agents (human or AI) reading the round's record see the full progression: scope discussions in topics, decisions in topic-corrective sister topics, claims in the doc manifest, attempted changes in the src manifest, abandoned approaches in the deprecated manifests.

The manifest is durable: once committed to the round's mock-side ref, it stays there forever. Round closure freezes the round's tree; nothing in the closed round changes. Even the archive ref (after `mock round archive`) carries the unchanged tree.

## 18. Topic documents

Topic documents are the round's exploratory prose. Each topic is a single Markdown file at the round's mock-side ref root, named `<NN>_topic.<name>.md` where `<NN>` is a two-digit sequence and `<name>` is a short slug.

### Topic format

There is no strict schema. Topic files are prose, and prose is what makes them load-bearing: a topic file captures the actual thinking that produced a design decision. A typical topic file shape:

```markdown
# Topic: arvo-graph storage layout

**Round:** 202605181400-arvo-graph-csr
**Status:** active

## Background

[Prose framing the problem. Why does this topic exist? What is the context?]

## Options considered

### Option A: dense matrix

[Prose describing the option, the tradeoffs, the case for and against.]

### Option B: CSR

[Same.]

### Option C: hybrid

[Same.]

## Sketches and benches

- Sketch `01-dense-vs-csr-shape.rs` (LIVE, WORKS): both layouts compile.
- Bench `structural-decomposition`: CSR is 30% faster at n=1024
  (see `mock/benches/structural-decomposition/1024_findings.md`).

## Decision

[The decision, the rationale, the cross-references.]
```

The structure is suggestive, not mandatory. Some topics are short (one option, one decision, half a page). Some topics span dozens of paragraphs with extended discussion.

### Topic mutability

Topic files are mutable until the round reaches DONE. During DOC, SRC, even the apply phases, the author can edit topic files to refine framing, add notes, fix typos. The discipline is that topic files document the design; they evolve as the design evolves.

The exception is sister-correction. When a sister topic deprecates an earlier topic's framing, the earlier topic is **not** edited; the sister names it explicitly. This preserves the audit trail of "we thought X, then learned Y, and pivoted to Z."

### Topic numbering

The `<NN>` prefix is a two-digit sequence number. The first topic in a round is `01_topic.<name>.md`, the second is `02_topic.<name>.md`, etc. A sister-correction topic gets the next sequence number; it does not replace the original's number.

The numbering helps readers see the order in which topics emerged. A round with three topics numbered `01`, `02`, `03` reads chronologically. A round with topics numbered `01`, `02`, `03` plus a sister `02b` or `04` (depending on convention) signals that the round's framing evolved.

### Topic commands

```
mock topic                            list topics in the active round
mock topic new <name>                 create a new topic file; open in $EDITOR
mock topic show <name>                show a topic file
```

`mock topic new` chooses the next `<NN>` automatically. The author provides the `<name>`.

# Part III. The storage foundation

The storage layer is the mechanical substrate. Everything in Parts I-II works regardless of whether mockspace stores its state as files-in-tracked-directories or as orphan git refs. This part specifies the ref-based model, which v2 adopts for the reasons described below.

## 19. Reference architecture

Mockspace partitions state across distinct git ref namespaces. Source code lives on `refs/heads/*` (the project's normal branches). Mockspace state lives on `refs/mock/*` (a parallel namespace that is invisible in routine git operations).

```
refs/heads/main                    public release line
refs/heads/dev                     public dev trunk (source only)
refs/heads/round/<slug>            per-round source-side branch

refs/mock/harness                  the harness ref (project configuration)
refs/mock/round/<slug>             per-round overlay ref (orphan, flat)
refs/mock/round/<slug>-conflict-<host>-<ts>
                                   side-branch preserving lost-race commit
refs/mock/task/<ns-path>/<slug>    per-active-task ref (orphan, flat)
refs/mock/task-archive             single archive for closed tasks
refs/mock/round-archive            single archive for closed rounds
refs/mock/research/<slug>          per-research cluster
refs/mock/bench/<slug>             per-bench cluster
refs/mock/sketch/<slug>            per-sketch
refs/mock/export/<package>         published packages
refs/mock/export-archive           single archive for retired exports
refs/mock/mirror/<host>/<kind>/<slug>
                                   1:1 mirror of externally-referenced refs
```

The harness, the archives, and the mirrors are commit refs with real history. All other `refs/mock/*` refs are **orphan** (no parent linkage) and **flat** (no nested commits). An orphan flat ref has exactly the tree structure mockspace authored; switching to it does not switch the working tree (mockspace renders content into `.mock/` rather than checking out).

### Why orphan refs

Three properties motivate the orphan-ref choice:

- **Working-tree clutter is eliminated.** The project's outsiders cloning the repository see source code and the rendered `docs/` tree. They do not see `mock/`, design rounds, manifests, or any mockspace surface. Cargo's `cargo publish` bundles do not include mockspace artefacts.
- **Tag-namespace pollution is eliminated.** Releases live on `refs/tags/*`. Mockspace lives on `refs/mock/*`. They do not compete; round identifiers do not look like release tags.
- **Branch coupling is broken.** A developer can switch branches (between feature work, release branches, hotfix branches) without disturbing mockspace state. The active round is independent of the current source-side branch.

The cost: mockspace cannot rely on git's normal working-tree mechanisms. Materialisation into `.mock/` is mockspace's own concern (described in §20). The cost is mechanical and self-contained; the benefits are user-facing.

### No `refs/mock/index`

Earlier ref-based mockspace designs proposed an index ref (a registry of all rounds, tasks, etc.). v2 does not have one. The reasons:

- **Cost of staleness.** An index ref must be updated on every state change, doubling the commit volume.
- **Cost of contention.** A central index ref becomes a multi-writer hotspot; multiple developers' rounds compete to update it.
- **Cost of correctness.** Index-vs-truth divergence (the index claims a round exists, but the round ref is absent, or vice versa) becomes a `mock doctor` finding category.

Instead, mockspace queries refs directly. `mock round list` enumerates `refs/mock/round/*`. `mock task list` enumerates `refs/mock/task/*`. The enumeration is local (no network round-trip beyond the initial fetch); the cost is bounded (workspaces typically have hundreds of refs, not millions).

A local cache file under `.git/mockspace/index.bin` serves as a fast-read cache for status commands. It is mockspace-managed and can be rebuilt at any time from the actual ref set; correctness lives in the refs, the index is just a cache.

## 20. Local materialisation

`.mock/` is the developer's interaction surface. It is **not** a git worktree; it is a directory mockspace renders content into using git plumbing.

The parent worktree's `.gitignore` lists `.mock/`. Outsiders cloning the repository see no mockspace surface. The developer using mockspace sees a populated `.mock/` tree the moment they run `mock init`.

Three storage areas, three distinct concerns:

### `.mock/`: rendered surface (per-project, gitignored)

```
<repo>/                              parent worktree on refs/heads/<branch>
├── .gitignore                       includes /.mock/
├── crates/                          source code (visible to outsiders)
└── .mock/                           mock-CLI-managed rendered surface (gitignored)
    ├── mockspace.toml               rendered from harness ref
    ├── mockspace.lock               rendered from harness ref
    ├── agent/                       rendered from harness ref
    ├── lints/                       rendered from harness ref
    ├── templates/                   rendered from harness ref
    ├── hooks/                       rendered from harness ref
    ├── export/                      rendered from refs/mock/export/* (this project's)
    │   └── <package-name>/
    ├── round/                       rendered from active round ref
    ├── tasks/                       rendered from active task refs
    ├── research/                    authoring (research notes are committed back to refs)
    ├── bench/                       authoring + bench-run-output
    ├── sketch/                      authoring + sketch-result files
    └── refs/                        read-only consultation worktrees for external refs
```

The `.mock/` tree contains only the rendered content the developer interacts with directly: editable surfaces (manifests, topic files, comments) and configuration the developer consults (`mockspace.toml`, `lints/`, `templates/`). No cache files, no internal state, no advisory locks, and no bookkeeping markers live here. The user edits files under `.mock/<area>/`; the user runs `mock commit` to commit edits back to the underlying ref.

### Render filter: bookkeeping stays out of `.mock/`

Round refs and harness refs carry both editable content (topic files, manifests, configuration) and bookkeeping (phase markers, content-addressed anchors, per-file SHA indexes, render-pointer markers). The renderer filters bookkeeping out of `.mock/` materialisation. The filtered shapes:

| File shape | Role | How the developer interacts with it |
|---|---|---|
| `.phase` | Phase marker; one of TOPIC, PLAN.DOC, APPLY.DOC, PLAN.SRC, APPLY.SRC, DONE | `mock status` reports it; `mock phase plan/apply/finish/replan` rewrites it |
| `.anchor.<phase>.toml` | Per-file SHA index captured at APPLY entry | `mock phase apply` writes; `mock phase replan` reads |
| `.anchor.<phase>.blobs/<sha-prefix>/<rest>` | Content-addressed blob bytes for anchor restoration | Same |
| `.meta` | Round-internal audit bookkeeping (creation timestamp, branch pairing) | Mockspace-managed; not consumer-facing |
| Other dot-prefixed bookkeeping files at the ref tree root | Mockspace extensions reserved here | Mockspace-managed |

These remain canonical in the orphan ref tree (durable, replayable, source of truth). They are read by mockspace plumbing via `git cat-file` and cached for fast read in `.git/mockspace/index.bin`. The developer never sees them as files in their working surface.

Editable dot-files (e.g. `.gitignore` inside a research bundle, or `.cargo/config.toml` inside a sketch) are not filtered: the renderer recognises content the developer has authored and round-trips it. The filter targets bookkeeping prefixes mockspace itself owns, not user-authored hidden files.

### `.git/mockspace/`: per-project per-developer internals

Following git-LFS's convention (which stores objects under `.git/lfs/`), all mockspace-internal per-project per-developer state lives under `.git/mockspace/`. This directory is automatically excluded from git's normal content tracking (everything under `.git/` is invisible as repo content), survives clones cleanly (clones start fresh, no inherited state), and disappears on repo delete.

```
<repo>/.git/mockspace/               per-project per-developer state (not pushed)
├── .lock                            flock-based advisory lock
├── index.bin                        local index cache (ref state snapshot for fast reads)
├── observations.toml                per-import last_observed_at, last_witness_at
├── doctor.log                       structured journal of mock doctor operations
├── migrate-<timestamp>.log          per-migration JSON Lines journal (when migrating)
└── undo/                            short-span undo/redo log
    ├── log.jsonl                    append-only operation journal
    └── <ts>-<seq>.json              per-snapshot ref-state
```

The `index.bin` cache holds the marker state the developer never sees as files: per-round current phase (read from the orphan ref's `.phase` blob), per-render harness SHA (the commit the harness was last rendered from, equivalent to the retired `.ref-sha.harness` file shape), per-round anchor presence flags. Status commands read this cache instead of round-tripping through `git cat-file` for every call.

The cache is regenerable. `mock doctor --rebuild-index` walks the ref set and reconstructs `index.bin`. The refs remain the source of truth; the cache is a fast-read projection.

### `~/.cache/mockspace/`: machine-global content cache

```
~/.cache/mockspace/                  XDG_CACHE_HOME; shared across all projects
├── imports/                         content-addressed cache of imported package bytes
│   └── <host>/<ref-path>/<sha>/     same SHA = same bytes regardless of project
└── helpers/                         baseline helper scripts extracted from binary on first run
```

Imported package bytes are content-addressed by SHA. Two projects on the same machine importing `mock://ext/runner-rs@<a1b2...>` share the same on-disk bytes; no per-project duplication.

Cache eviction is on-demand via `mock cache prune`. Unreferenced content older than 90 days is evicted by default; total entries older than 365 days are evicted regardless of reference status.

### `~/.config/mockspace/`: per-developer config

```
~/.config/mockspace/                 XDG_CONFIG_HOME; per-developer, machine-global
└── trust.toml                       per-developer TOFU acceptances (per (host, fingerprint))
```

Trust acceptances are per-developer (analogous to SSH's `~/.ssh/known_hosts`). The project's lockfile records the pinned fingerprint; the developer's local trust file records "I have personally seen this fingerprint on this machine." See Part IV §31.

### Discovery and root resolution

`mock` walks up from cwd looking for the nearest `.git` directory. `.mock/mockspace.toml` is rendered alongside (per the harness ref). Discovery stops at filesystem boundary or at the `.git` directory, whichever comes first.

Configurable via the `MOCK_ROOT` environment variable, which overrides discovery. Useful in CI scenarios where the parent process knows the project root and wants mockspace to skip discovery.

### No `git worktree add`

The earlier filesystem-based mockspace used a `mock/` subdirectory tracked in `refs/heads/*`. The ref-based variant could in principle use `git worktree add` to materialise mock-side refs as worktrees. v2 does not. The reasons:

- Worktrees add bookkeeping (worktree registration, garbage collection, pruning).
- Worktrees fragment the filesystem (multiple worktrees for the same repo).
- The rendered content is generated; if the rendered output is lost, mockspace re-renders. There is nothing precious in `.mock/` beyond what is in the refs.

Instead, mockspace renders trees from refs using `git cat-file` and `git ls-tree` plumbing. The trees are written to `.mock/` as ordinary files. The developer edits them; `mock commit` reads the diffs and produces new commits on the underlying refs.

## 21. Source-side versus mock-side refs

Two parallel tracks per round:

**Mock-side ref** `refs/mock/round/<slug>`. Orphan, flat. Carries the round's topic documents, manifests, anchors, phase marker, comment snapshots, sketches-bundle (if any), bench-bundle (if any), research notes (if any). Mockspace authors all commits to this ref.

**Source-side branch** `refs/heads/round/<slug>`. A normal feature branch off `refs/heads/dev` (or whatever the project's working trunk is). Carries the actual source-code commits implementing the round's work. The PR projection targets this. The author commits to this via normal git workflow.

Both are created at `mock round new`. They are independent in history (no parent linkage between them); they are paired by slug.

### Coordination

Phase transitions commit on the mock-side ref only. Source-side commits happen via normal git workflow (the developer's `git add` + `git commit`). Mockspace observes the source-side state without commit-time intervention.

Some operations require parent-worktree alignment:

- **`mock phase apply` from PLAN(SRC)**: verifier executes against the source-side branch tip. Requires parent worktree HEAD on the round's source-side branch.
- **`mock phase replan` from APPLY(...)**: anchor restoration writes to source-side files. Requires parent worktree HEAD on the round's source-side branch.
- **Source-side commits**: trivially require parent worktree HEAD on the source-side branch.

`mock commit` for changes under `.mock/round/` works regardless of parent HEAD; it commits to the mock-side ref, not to the source-side branch.

When an operation requires HEAD on the round's branch and the parent is on a different branch, mockspace emits a structured diagnostic. There is no auto-switch; the developer chooses (`git switch round/<slug>` or abandon the operation).

### Independent lifecycles

The two refs serve different purposes. Source-side carries the code; mock-side carries the design record. They evolve together but their histories are not collapsed. A round closes by archiving both: the source-side branch merges into the trunk (via the PR), the mock-side ref archives into `refs/mock/round-archive`.

## 22. The harness ref

The harness is the project's mockspace configuration ref. It carries:

```
refs/mock/harness root tree
├── mockspace.toml                  project-local config
├── mockspace.lock                  lockfile (machine-managed; users don't edit)
├── agent/                          agent integration templates
├── lints/                          custom lint .rs files
├── templates/                      render templates (.md.tmpl)
└── hooks/                          project-local hook scripts
```

A `tools/` directory at the harness ref root is **not** part of the v1 contract. Future work may introduce project-local CLI extensions (a `mock <name>` sub-command shape, comparable to `yarn <script>` or `cargo <alias>` extensions) hosted under such a directory, but the contract (discovery, runtime, sandbox, ABI) is unspecified for v1 and the directory is reserved for that future design only. v1 implementations should not write to `tools/` and should not assume any behaviour from it.

The harness ref is a regular commit ref (not orphan): commits accumulate over time as the project's configuration evolves. Each commit on the harness ref carries the same audit-trail trailers as other workflow commits (see Part VII §49).

### Harness scope

The harness is per-repository. Two developers cloning the same repository share the harness; their local clones render the same `.mock/mockspace.toml`. A developer's local trust-acceptance state (`~/.config/mockspace/trust.toml`) is per-developer; the harness lockfile pin is per-project.

### Harness changes are commits

Adding a new lint to `mockspace.toml`, changing a gate severity, adding a new hook, importing a new external package: all of these are commits on the harness ref. The commits go through the project's review process like any other config change.

`mock commit harness` is the convenience: edit `.mock/mockspace.toml` (or other harness-side files), then `mock commit harness` commits the diff to the harness ref. Behind the scenes: read the diff, build a new tree, commit, push.

### Commit signing of harness

Commit signing of harness commits is project policy, not mockspace policy. Projects that want signed commits use `commit.gpgsign` or forge branch protection. Mockspace verifies the signatures of imported packages (Part IV §31), but it does not impose a signing requirement on the project's own harness commits.

### Harness vs source-side trust classes

The harness ref and the source-side branches sit at different trust classes:

- **Harness ref (`refs/mock/harness`).** Carries `mockspace.toml`, custom lints under `lints/`, hook scripts, render templates. Changes go through the project's normal review process (typically a PR merged into the harness ref by a reviewer with push access). Authors of harness commits are project-trust-class participants; custom lints run as full native code with the trust authority of project-trust-class.
- **Source-side branch (`refs/heads/<branch>`).** Carries the actual code. PR authors push feature branches freely; review happens at merge time. Manifests live here and use only the closed verifier-kind catalog (§17, §54) because the manifest is author-controlled at PR-author trust class.

The wall between the two is structural: a PR-author cannot push a custom lint by editing a feature branch, because custom lints are not on any source-side branch in the first place. They live on the harness ref, and the harness ref is updated by harness commits, which the project's review process controls. This is the trust split that lets §17's closed verifier-kind catalog coexist with §5's native-code custom lints without contradiction.

## 23. Content-addressed anchors

When a manifest seals at APPLY entry, mockspace captures an **anchor**: a per-file content snapshot of every file the manifest claims. The anchor enables replan to restore the pre-APPLY state cleanly.

### Anchor structure

```toml
# .mock/round/.anchor.doc.toml (and .anchor.src.toml)
mockspace_version = "1.0"
captured_at = "2026-05-18T11:30:00Z"
captured_from_source_branch_tip = "abc123def456..."

[[file]]
path = "crates/foo/src/lib.rs"
blob_sha = "a1b2c3d4e5f67890..."

[[file]]
path = "crates/foo/src/parser.rs"
blob_sha = "deadbeef..."

[[file]]
path = "docs/DESIGN.md"
blob_sha = "1234567890abcdef..."
```

Each `[[file]]` entry names a path and the SHA of that path's content at capture time.

### Content-addressed storage

The actual blob bytes live under the round mock-side ref's tree:

```
.anchor.doc.blobs/
  a1/b2c3d4e5f67890...                       <- one blob, referenced by [[file]] entries
  de/adbeef...
  12/34567890abcdef...
```

The first directory level is a 2-character SHA prefix (matching git's own object-store layout). The remaining 38 (or 62 for SHA-256) characters form the filename. This layout keeps any single directory bounded in size, which matters on filesystems with directory-listing performance penalties.

### Hash algorithm: inherit from git

Mockspace does not impose a parallel hash algorithm choice. The repo's `extensions.objectFormat` (git's repo-level setting; default `sha1`, opt-in `sha256`) determines the hex length of every `blob_sha` value in an anchor. A single anchor's entries are uniform: all SHA-1 or all SHA-256, matching the repo's choice at capture time. Mockspace reads the repo setting via `git config` and validates anchor entries against it; mixed-length anchors fire `D004` at restoration. Repos that later migrate object format produce a new anchor on the next `mock phase apply`; replan against an anchor captured under the old format continues to work because the anchor stores the bytes directly.

### Why content-addressed

- **No path-flattening collision.** Path-flattening schemes (e.g., `crates/foo/src/lib.rs` → `crates__foo__src__lib.rs`) collide when source paths naturally contain the chosen separator. Content-addressing eliminates this failure mode entirely.
- **Dedupe for free.** Multiple files with identical content (common for boilerplate, license headers, generated stubs) share one blob.
- **Integrity verification is a tautology.** The on-disk name IS the expected hash; restoration recomputes the hash and compares to the path-name. Tampering is detected automatically.
- **Non-UTF-8 source paths are handled.** Source paths can be arbitrary bytes; the blob storage layout uses only hex digits and is safe on every filesystem.

### Restoration flow

During replan, for each `[[file]]` entry in the anchor:

1. Read `path` and `blob_sha`.
2. Read bytes from `.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>`.
3. Verify the SHA of the read bytes matches `blob_sha`. Mismatch fires `D004`.
4. Write bytes to `path` on the source-side worktree.

Missing blob (entry references a SHA absent from `.anchor.<phase>.blobs/`) fires `D040`.

### Reachability

Blobs live in the round mock-side ref's tree directly. They are part of the ref's content; they survive force-push (of unrelated refs), they survive `git gc`, they survive rebase of the source-side branch. The only way to lose anchor blobs is to lose the round mock-side ref itself, which is what archival explicitly preserves.

## 24. Transition atomicity

A phase transition is a multi-step operation. `mock phase apply` from PLAN(DOC) does at minimum:

1. Acquire `.git/mockspace/.lock` via `flock(2)` for the duration of the transition.
2. Fetch `refs/mock/round/<slug>` from origin (fast-forward only).
3. **Early-detection check.** Compare local tip to the just-fetched remote tip. On divergence, invoke `on_phase_race` (see below). This check is an optimisation: it aborts before the expensive work in steps 6 to 10 (verifier, anchor capture, render). It is NOT the authoritative gate. Race conflicts that arise between this check and step 12's push are caught by step 12's push CAS. Both code paths flow through the same `on_phase_race` handler; the difference is at what cost the race is detected.
4. Verify clean state in `.mock/round/` (or auto-commit per profile).
5. Read source-side branch tip SHA.
6. Validate the manifest (TOML grammar, references resolve).
7. Run claim verifier in a temporary worktree at the source-side branch tip (`git worktree add --detach <temp> <tip-sha>`). All-pass-or-no-transition.
8. Capture per-file blob SHAs into `.anchor.<phase>.toml`. Store blob bytes content-addressed under `.anchor.<phase>.blobs/<sha-prefix>/<sha-rest>`.
9. Build the new tree for the round mock-side ref: rename manifest to locked form, write the anchor file + blob storage, rewrite the phase marker.
10. Render the source-tree and local-only targets locally first. On render failure here: abort before the ref update; nothing pushed.
11. `git update-ref refs/mock/round/<slug> <new-commit>`.
12. `git push origin refs/mock/round/<slug>`. On non-fast-forward, invoke `on_phase_race` (see below).
13. Release `.git/mockspace/.lock`.
14. If the primary host's `auto_open_pr` is true: attempt forge API target render with retry. On failure: log warning; round state is valid; recommend `mock pr regen`.

`--no-forge` skips step 14. `--resume` re-runs steps 12 and 14.

### Render-failure ordering

Source-tree and local-only renders run before the ref update (step 10). A render failure aborts cleanly without leaving the round ref ahead of local state. Forge-API render runs after the ref push (step 14). A forge failure leaves a valid round state on the remote; the user runs `mock pr regen` later to retry.

This is the "local-first commit, public-last announce" pattern: the durable state lands first; the announcement happens after.

### `on_phase_race` handler

When step 12 detects a non-fast-forward (remote tip moved between fetch and push):

```
12a. Refuse the push.
12b. Rename local round-ref tip to refs/mock/round/<slug>-conflict-<host>-<ts>.
12c. PUSH the conflict side-branch to origin BEFORE local reset.
     On push failure here: hard-stop. Do not reset local. Emit D037
     ("race conflict could not be preserved on remote; local state retained").
     User intervention required.
12d. Reset local round ref to the remote tip we just observed.
12e. Invoke on_phase_race per profile.
     Default: refuse (user runs `mock phase resolve <slug>`).
```

Step 12c is the load-bearing guarantee: the conflict side-branch lives on the remote before any local reset happens. If 12c fails, mockspace stops; the developer's local state is recoverable, but no automatic reset can race with a machine crash to lose the work.

The conflict side-branch ref name encodes the hostname and timestamp, so multiple race conflicts on the same round produce distinct refs. `mock phase resolve <slug>` is the recovery path: list the conflict side-branches, present the diff against the current ref, prompt for resolution (keep local, keep remote, rebase manually).

### flock semantics and filesystem caveats

`flock(2)` BSD-style. Lock file content is hostname + PID + start time for debugging; the kernel manages the lock; auto-released on process exit.

The flock-based design assumes the filesystem honours POSIX advisory locks. Known unsupported substrates:

- **NFS (any version).** Not supported.
- **sshfs / CIFS / SMB.** `flock` often returns success without actually locking. Concurrent writers can both "win." Not supported.
- **Cloud-sync directories** (iCloud Drive, Dropbox, OneDrive, Google Drive). The local FS honours flock but the userspace sync daemon interposes and may sync partial states between machines. Two developers using cloud-synced clones can produce corruption. `mock doctor` raises `D038` on detection.
- **FUSE filesystems generally.** Behaviour varies per implementation; test before relying.
- **Docker bind-mounts on macOS.** xnu flock semantics through the Linux VM are inconsistent.

Detection via `df -T` (Linux), `mount` (macOS), plus heuristic path-marker checks (is the path under `~/Library/Mobile Documents/com~apple~CloudDocs/`?). D038 is a soft warning, not a hard refuse; users on unsupported substrates proceed at their own risk.

Windows behaves differently; see Part VIII §58.

## 25. Active phase storage

Each round's mock-side ref tree carries the phase marker, the current manifest (or its locked form), the anchor (when in APPLY), and the topic documents.

```
refs/mock/round/<slug>/                       (orphan ref root tree)
├── .phase                                    phase marker file (one of TOPIC/PLAN.DOC/...)
├── round.toml                                round metadata
├── 01_topic.<name>.md                        topic file
├── 02_topic.<name>.md                        topic file
├── 03_topic.<corrective>.md                  topic file (sister-correction)
├── manifest.doc.toml                         current doc manifest (during PLAN(DOC))
├── manifest.doc.locked.toml                  sealed doc manifest (after APPLY(DOC))
├── manifest.doc.deprecated.1.toml            deprecated by replan (if any)
├── manifest.src.toml                         (during PLAN(SRC))
├── manifest.src.locked.toml                  (after APPLY(SRC))
├── .anchor.doc.toml                          per-file SHA index (during APPLY(DOC))
├── .anchor.doc.blobs/<sha-prefix>/<rest>     content-addressed blob bytes
├── .anchor.src.toml                          (during APPLY(SRC))
├── .anchor.src.blobs/...
└── comments/                                 ingested PR comments (after DONE)
    ├── 001-author-timestamp.md
    └── ...
```

There is no `.meta` file. The earlier (v1) design used `.meta` to hold round-bookkeeping fields (e.g. start commit SHA, end commit SHA) because rounds were ordinary files under `mock/design_rounds/` and needed an out-of-band place for git-level provenance. Under v2 each round IS a commit on its own orphan ref, so commit SHAs, parent linkage, author, timestamp, and tree state are all native git properties readable via `git log` / `git cat-file` on the round mock-ref. Any genuinely user-facing metadata lives in `round.toml`; any per-developer fast-read state lives in `.git/mockspace/index.bin`. The `.meta` file's prior purposes are fully subsumed.

Phase transitions rewrite the tree:

| State | Command | Storage actions |
|---|---|---|
| (no round) | `mock round new <slug>` | create `refs/heads/round/<slug>` + orphan `refs/mock/round/<slug>` with initial tree |
| TOPIC | `mock phase plan` | scaffold doc manifest; rewrite `.phase`; commit |
| PLAN(DOC) | `mock phase apply` | validate; verifier; capture anchor; transition; pull-rebase-push; forge API |
| APPLY(DOC) | `mock phase finish` | scaffold src manifest; transition |
| PLAN(SRC) | `mock phase apply` | validate; verifier; capture anchor; transition |
| APPLY(SRC) | `mock phase finish` | transition to DONE |
| DONE | `mock close` | fetch comments; freeze; optional merge |

The `.phase` marker is the single source of truth for the round's current phase. The file content is one of `TOPIC`, `PLAN.DOC`, `APPLY.DOC`, `PLAN.SRC`, `APPLY.SRC`, `DONE` (one line, no trailing whitespace). Phase queries read this file via `git cat-file` (or the cached projection in `.git/mockspace/index.bin`, see §20); phase transitions rewrite it as a tree update on the orphan ref. The marker never materialises into `.mock/round/<slug>/`; the bookkeeping filter (§20) keeps it out of the developer's edit surface. Same treatment for `.anchor.*.toml` and `.anchor.*.blobs/`: durable in the ref tree, cached for fast read, hidden from `.mock/`.

### Round metadata

`round.toml` carries the round's metadata, written at creation and amended at close:

```toml
mockspace_version = "1.0"
slug = "202605181400-arvo-graph-csr"
title = "arvo-graph storage layout (CSR vs dense matrix)"
created = "2026-05-18T14:00:00Z"
source_branch = "round/202605181400-arvo-graph-csr"

[pr]
number = 437                          # filled after PR creation
url = "https://github.com/orgrinrt/arvo/pull/437"

# Populated by `mock close` when the round transitions to DONE; absent
# until then. These fields preserve audit-trail facts that the orphan
# ref's own commit metadata records but that are convenient to surface
# at archive time without re-walking commit history.
[closed]
closed_at = "2026-05-19T18:30:00Z"
final_source_sha = "deadbeefcafebabe0000000000000000deadbeef"
original_mock_ref = "refs/mock/round/202605181400-arvo-graph-csr"
original_source_ref = "refs/heads/round/202605181400-arvo-graph-csr"
```

`round.toml` is the single user-facing metadata document for the round. There is no `.meta` companion file (see preceding `.meta` discussion). All audit-trail facts that a v1 `.meta` would have carried are either native git properties of the orphan ref's commits OR fields under `[pr]` / `[closed]` in `round.toml`.

## 26. Tasks, archives, retention

### Task archive

Closed tasks move to a single archive ref `refs/mock/task-archive`. The archive's tree:

```
refs/mock/task-archive root tree
├── compiler/
│   ├── ir/
│   │   ├── structural-robust-ir/
│   │   │   ├── .state.closed
│   │   │   ├── meta.toml
│   │   │   └── <slug>.md
│   │   └── lower-pass/
│   │       └── ...
│   └── ...
└── workspace/
    └── ...
```

Auto-archival is triggered by `mock close` (full sweep across all closed-but-not-archived tasks) and by `mock sync --full`. Manual archival via `mock task archive <ns>::<slug>`.

The `task_archive_threshold_days` config (default 90) controls auto-archival: closed tasks older than the threshold archive automatically.

### Round archive

Closed rounds move to `refs/mock/round-archive`. The archive's tree:

```
refs/mock/round-archive root tree
├── 2026/
│   ├── 05/
│   │   ├── 202605181400-arvo-graph-csr/
│   │   │   ├── round.toml                       (carries [closed] block per §25)
│   │   │   ├── 01_topic.<name>.md
│   │   │   ├── manifest.doc.locked.toml
│   │   │   ├── manifest.src.locked.toml
│   │   │   ├── .anchor.doc.toml
│   │   │   ├── .anchor.doc.blobs/...
│   │   │   ├── .anchor.src.toml
│   │   │   ├── .anchor.src.blobs/...
│   │   │   └── comments/...
│   │   └── ...
│   └── ...
└── ...
```

Year/month nesting keeps any single directory bounded. The year and month are taken from the **closure timestamp** (the time the round transitioned to DONE), not from the slug's date prefix; this aligns the directory tree with when the work concluded rather than when the round was opened. The closure timestamp lives in `round.toml`'s `[closed]` block alongside the other audit-trail facts preserved at archive time (see §25): `closed_at`, `final_source_sha`, `original_mock_ref`, `original_source_ref`, plus any `[pr]` data captured during the round.

Auto-archival via `round_archive_threshold_days` (default 365); manual via `mock round archive <slug>`. The archive entry preserves the round's full tree as it existed at close, minus the bookkeeping files (`.phase` etc.) filtered per §20.

### Concurrent archive writes

`refs/mock/task-archive` and `refs/mock/round-archive` are single refs; multiple developers archiving concurrently produce contention. The `on_archive_contention` handler (Part V §35) drives the retry strategy: exponential backoff with jitter, unbounded retries with a 60-second wall-clock timeout, configurable.

### Archive ref unbounded growth

At very large scale (5000+ archived rounds), the single archive ref's tree becomes unwieldy. Future refinement: shard by year (`refs/mock/round-archive/<year>`). Deferred until measurement warrants. v1 retains the single-ref shape.

### Retention policy

Archived content lives forever by default. There is no automatic deletion of archived rounds or tasks; the audit trail is the load-bearing property. Projects that want to prune very old archives do it manually (and rarely).

# Part IV. Imports, exports, trust

Mockspace's extensibility flows through three concepts:

- **Hosts** are named aliases for git URLs serving mockspace-shaped content.
- **Exports** are packages a project publishes for other projects to consume.
- **Imports** are external packages a project consumes.

All three converge through the `mock://` URI scheme. Trust is rooted in commit signatures plus per-developer TOFU.

## 27. The `mock://` URI scheme

Mockspace's URI scheme identifies any resource by a structured path. The grammar:

```ebnf
mock_uri        = "mock://" target [ intra_path ] [ pin ] [ fragment ]

target          = local_target | self_target | first_party_target | external_target

local_target    = local_kind "/" identifier_path
                  # bare form ALWAYS means local (importer's scope)

self_target     = "~/" local_kind "/" identifier_path
                  # shorthand for "this package's own scope"
                  # only meaningful inside an exported package's content

first_party_target = "@/" local_kind "/" identifier_path
                                          # @ = first-party (hardcoded into binary)

external_target = "ext/" host_name "/" local_kind "/" identifier_path

local_kind      = "round" | "task" | "research" | "bench" | "sketch"
                | "export" | "hook" | "lint" | "agent" | "template"
                  # reserved namespace; future kinds added by minor bumps

host_name       = segment      # MUST NOT match any local_kind, "@", "ext", "~"

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

### Worked examples

| URI | Resolves to |
|---|---|
| `mock://round/202605181400-arvo-graph-csr` | Local round ref (importer's scope) |
| `mock://task/compiler::ir::structural-robust-ir` | Local task; active then archive |
| `mock://task/compiler::ir::structural-robust-ir#define-grammar` | Local task + step |
| `mock://export/some-package` | Local export (importer's scope) |
| `mock://hook/on_dirty_state.sh` | Importer's hook in harness `hooks/` |
| `mock://~/hook/helper.sh` | Self-scope hook (only meaningful inside an exported package's content) |
| `mock://@/export/profile-dev` | First-party export (hardcoded binary trust) |
| `mock://ext/arvo/round/202605111719-graph-algos` | External arvo round |
| `mock://ext/runner-rs/hook/setup.sh@<sha>` | External package's bundled hook, pinned |

### Resolution scope

The bare form always resolves to the consumer's local scope, regardless of where the URI string physically lives (in the consumer's harness, in an imported package's content, in a rendered PR body). Local is the default.

`~/` is self-scope shorthand for "this package's own bundled content." Only meaningful inside an exported package's tree. An exported package that wants to invoke its own bundled hook writes `mock://~/hook/foo.sh`; mockspace resolves `~/` to whichever host+package is the current resolution context.

`@/` is first-party (mockspace canonical). Resolves against the mockspace binary's hardcoded canonical URL + signing key. See §33.

`ext/<host>/` is explicit external host. Resolves against the named `[hosts.<host>]` configuration.

### Security boundary

The bare-form-is-local rule is the load-bearing security property for imported packages. An exported package CAN deliberately reach into the consumer's scope by writing `mock://hook/foo.sh` (e.g., to call a customisation hook the consumer provides as an extension point), but this is opt-in by the package author, not by accident. An exported package MUST NOT write `mock://hook/foo.sh` and expect that to resolve to its own bundle; the bare form is always the consumer's.

The asymmetry is intentional: consumers expect imported packages to behave as bounded artefacts, so the default has to be "calls go to the local consumer."

Mockspace lints package content at publish time for bare-form URIs that look like typos for self-references (`mock://hook/<name>` where `<name>` matches a file under the package's `hook/` directory) and warns the package author to use `~/` if self-reference was intended.

### Path-traversal defence

The `intra_path` grammar excludes `..` and lone `.` segments at the parse layer. The resolver additionally lexically canonicalises the resolved path (resolving `.`, refusing `..`, refusing symlink-escapes) and verifies the result remains within the package's root directory. Any path that would escape the package's root is refused; never silently re-anchored.

### `~/` scope-pinning rule

The `~/` shorthand only has meaning inside a **package-execution context**: when mockspace is actively executing the bytes of an imported package, and the URI string being resolved comes from inside that package's tree, then `~/` resolves to that package. Outside this context, `~/` is meaningless.

Mockspace refuses `~/` URIs in these contexts with a structured diagnostic:

- In the consumer's own `mockspace.toml` (the harness has no package-self-scope).
- In manifests committed to the source-side branch.
- In rendered output (PR body, source-tree files, local-only files).
- In doctor diagnostics that re-quote a URI back to the user.
- In hook environment variables or any other string the resolver reads after a URI has been extracted from text rather than parsed as part of a package's structured content.

The resolver records the resolution context explicitly: every URI parse takes a "scope context" parameter (either `None` for top-level contexts, or `Some(<package-name>)` when resolving within a package's tree). `~/` requires `Some(...)`; absence is a parse error.

The doctor finding `D041` fires when `~/` appears in any harness-ref content (`mockspace.toml`, hook files, manifest files), in lint diagnostics, or anywhere mockspace itself owns the text. Package content is the only legitimate `~/` site.

### Archive resolution and forward-compat

URIs of the form `mock://round/<slug>` and `mock://task/<identifier-path>` resolve through a configured set of archive refs. v1 ships with a single archive ref per kind (`refs/mock/round-archive`, `refs/mock/task-archive`), but the resolver always walks an ordered list. The list is in `mockspace.toml`:

```toml
[archive.lookup]
round = ["refs/mock/round-archive"]
task = ["refs/mock/task-archive"]
```

Future sharding (year-partitioned, content-addressed, namespace-partitioned) extends the lookup chain without breaking existing URIs. A project that needs to shard its archive at scale adds the new refs to the lookup list; the resolver walks them in order; old URIs continue resolving as long as the slug is uniquely identifying within the union of archive refs.

The resolution order is configured, not hardcoded. Projects choosing to migrate archive layout document the change in their `mockspace.toml` and run `mock archive rebalance` (deferred to v1.x; see §59) to perform the partitioning. URI consumers see no change; the resolver does the walking.

This forecloses the breaking-change-on-archive-sharding concern. The URI grammar does not need a sharding-version suffix because the resolver, not the URI, owns the lookup strategy.

## 28. Hosts

A host is a named alias in `[hosts.<name>]` for a git URL serving mockspace-shaped content. Hosts are declared in the harness `mockspace.toml`:

```toml
[hosts.mockspace-rs]
url = "https://codeberg.org/mockspace/mockspace-rs.git"
mirrors = [
  "https://github.com/mockspace/mockspace-rs.git",
  "git@private-mirror.example:mockspace/mockspace-rs.git",
]
token_env = "MOCK_HOST_TOKEN_MOCKSPACE_RS"               # optional, unified fallback
read_token_env = "MOCK_HOST_READ_TOKEN_MOCKSPACE_RS"     # optional, read-only fetches
write_token_env = "MOCK_HOST_WRITE_TOKEN_MOCKSPACE_RS"   # optional, push operations
pinned_at = "<sha>"                                      # optional

[hosts.arvo]
url = "https://github.com/orgrinrt/arvo.git"
forge_url_template = "https://github.com/orgrinrt/arvo/tree/{ref}"
```

### Mirror federation

When `mirrors = [...]` is set, fetches try `url` first, then each mirror in order. Substitution is cryptographically safe because the lockfile pins by SHA + signing-key fingerprint; any mirror serving different content fails verification.

This makes the "federates with git for redundancy" property real: if the primary host goes down (forge outage, account suspension, migration in progress), work continues against mirrors as long as one of them still serves the locked content. Mirrors are user-managed; mockspace does not auto-discover mirror URLs.

```
mock host add-mirror <name> <url>           appends to the list
mock host remove-mirror <name> <url>        removes one
mock host fetch <name> --verify-mirrors     walks every mirror, fetches the pinned ref,
                                            verifies signature, reports which mirrors are healthy
```

### Token resolution

`read_token_env` and `write_token_env` take precedence when present; `token_env` is the unified fallback. The distinction matters for shops where CI uses a read-only deploy token and maintainers push with a separate write token.

Tokens flow into git operations via `GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_*` / `GIT_CONFIG_VALUE_*` environment variables so they never enter argv (where they would be visible in `ps`).

### Host-name reserved-namespace check

Configuration load refuses any `[hosts.<name>]` whose `<name>` matches `local`, `ext`, `@`, `~`, or any reserved `local_kind`. Mockspace verifies this at config load and refuses with a structured diagnostic if a host name clashes. Adding a new `local_kind` is a minor-version bump and the new kind enters the reserved list.

## 29. Exports

A project publishes packages by authoring content under `.mock/export/<package-name>/` and committing to `refs/mock/export/<package-name>`. The ref is a commit ref whose tree IS the package content.

### Package structure

```
refs/mock/export/<package-name> root tree
├── package.toml                    metadata
├── <content-files>                 package payload (single file or directory)
└── ~/                              package-bundled URIs use mock://~/...
```

`package.toml`:

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

### Single-file and multi-file packages

- **Single-file packages**: the export ref's tree contains the file plus `package.toml`.
- **Multi-file packages**: the export ref's tree contains a directory of files plus `package.toml`.

### Releases

New versions are new commits on the ref. The version is declared in `package.toml`; consumers pin by SHA in their lockfile, so the version field is metadata for human readers, not the resolution mechanism.

Optional tags (`refs/tags/export/<package-name>/<version>`) provide named release labels for consumers who want them.

### Export commands

```
mock export new <name>             create new orphan ref with scaffolded package.toml
mock export list                   show all exports this project ships
mock export show <name>            show metadata for one export
mock export bump <name> <version>  commit a new version (writes new package.toml + tree)
mock export archive <name>         roll into refs/mock/export-archive
mock export publish <name>         push to remote
mock export witness <name>         append signed entry to transparency log (when configured)
```

## 30. Imports

A project consumes external packages by declaring imports in `[imports]`:

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

### Import categories

- **Local imports.** Reference content in the consumer's own harness. No fetch, no signature verification beyond the project's own commit signing policy. Trusted by definition (the consumer authored the content).
- **First-party imports (`@/`).** Reference content in the mockspace canonical host. Signature verified against the binary's hardcoded fingerprint. No per-developer TOFU; trust is rooted in the binary.
- **External imports (`ext/<host>/`).** Reference content in a named third-party host. SHA-pinned in lockfile. Signature verified per fetch. Per-developer TOFU on first contact.

### Trust scaling

The three categories represent increasing trust ceremony. Local imports are free (you wrote the code). First-party imports add binary-rooted signature verification (you trust the install channel that delivered mockspace). External imports add lockfile pinning, signature verification, and per-developer TOFU acceptance (you trust whoever's key you accepted on first contact).

This scaling matches the actual trust surfaces: your own code is the easiest case; the canonical mockspace project's content is the canonical case; an arbitrary third-party host requires explicit acceptance.

## 31. Signing and per-developer TOFU

Every executable import is verified against a cryptographic signature before mockspace runs it. Mockspace leans on git's native commit signing rather than inventing a new signature format.

### Signing model

Every export commit MUST be signed via git's commit signing (`git commit -S`). Supported key types:

- **SSH keys** (via `gpg.format = ssh` in git 2.34+). Same key used to push commits.
- **GPG keys** (traditional git signing).

The package's `package.toml` includes the maintainer's public key fingerprint at `[signing] key_fingerprint`. Verifiers check that the commit signature was produced by the declared key.

### Trust on First Use (TOFU) is per-developer

Mockspace's TOFU model mirrors SSH's `known_hosts` exactly: each developer's first encounter with a `(host, signing-key)` pair prompts that developer for acceptance, and acceptance lands in their local trust file (`~/.config/mockspace/trust.toml`).

The lockfile in the harness ref records pins (what version + what fingerprint), not who-trusted-what.

When a developer encounters an import that has not been seen on this machine before:

1. Fetch the ref's tip and verify its commit signature.

   The verification contract: confirm the commit was signed by a key whose fingerprint matches the lockfile's `signing_key_fingerprint`. Implementation may either parse `git verify-commit --raw` output (delegated verification) or use a vendored verifier (sequoia-openpgp for GPG, ssh-key for ed25519/rsa SSH signatures) directly against the commit's signature payload. Both paths produce the same fingerprint-equivalence check; the spec does not pin the implementation.

   When the parse-`git verify-commit` path is used:
   - Minimum git version: 2.42. Older git's SSH-signing output shape is not stable.
   - Force `LANG=C` for the invocation.
   - For GPG signing: parse `[GNUPG:]VALIDSIG <primary-fingerprint> <sig-time> <expire-time> <sig-version> <reserved> <pubkey-algo> <hash-algo> <sig-class> <primary-fingerprint-again>` to extract the signing-key fingerprint. Recognise and treat as failure: `[GNUPG:]BADSIG`, `[GNUPG:]EXPSIG`, `[GNUPG:]EXPKEYSIG`, `[GNUPG:]REVKEYSIG`, `[GNUPG:]ERRSIG`.
   - For SSH signing: extract the fingerprint via `git verify-commit --raw 2>&1 | grep "Good \"git\" signature"` parser pattern (git 2.42+ stable); refuse if pattern does not match.
   - Refuse if the signing backend is SSH and `gpg.ssh.allowedSignersFile` is unset, missing, world-writable, or unreadable.

   When the vendored-verifier path is used:
   - Mockspace fetches the commit object via `git cat-file commit <sha>`, parses the signature trailer, and verifies against the public key material recorded in the lockfile (the fingerprint plus, on first-trust acceptance, the encoded public key bytes).
   - This path avoids the git-version-stability concern entirely.

   Either path's output is the same: pass/fail plus the verified fingerprint.

2. Compare the actual signing-key fingerprint (from `--raw` output) against the lockfile's `signing_key_fingerprint`. Mismatch fires `D026`.

3. Read `package.toml`'s declared `key_fingerprint` (if present) for cross-check. If declared and actual differ, refuse with `D031` ("declared signing key does not match commit signature").

4. **Per-developer TOFU prompt** unless `MOCK_NON_INTERACTIVE=1` or the profile's `on_first_trust = "auto"`:

```
New (host, signing-key) pair not in your local trust file:

  Host:        codeberg.org/mockspace/mockspace-rs
  Package:     runner-rs
  Version:     1.2.3
  Signing key: SHA256:abc123...    (verified against the commit signature)
  Key type:    ssh-ed25519
  Signed by:   Maintainer Name <maintainer@example.com>

  This fingerprint matches your project's lockfile pin (the pin was
  written by whoever first added or updated this import). The lockfile
  is project configuration, not a third-party witness; if you have not
  personally encountered this maintainer's key before, verify the
  fingerprint out-of-band before accepting.

Trust this signing key on this machine? [y/N]:
```

5. On `y`: record `(host, fingerprint, key_type, accepted_at)` in `~/.config/mockspace/trust.toml`. Subsequent fetches by this developer of the same pair are silent. On `N` or non-interactive without auto: refuse with structured diagnostic.

The prompt fires per-developer, not per-project. Alice accepting a fingerprint on her machine has no effect on Bob's. Bob's first encounter with the same import prompts Bob locally. This is exactly how SSH's `known_hosts` works.

### Subsequent fetches

On every subsequent fetch of a previously-trusted package:

1. Fetch the new commit.
2. Verify signature against the **lockfile-recorded** fingerprint.
3. On match: proceed.
4. On mismatch (key rotation by maintainer): refuse with `D026`. User explicitly runs `mock import rotate <ext>/<pkg> --accept-new-key` to acknowledge.
5. On unsigned commit: refuse with `D027`.

### SHA pinning + signature verification together

Both are required for trust:

- **SHA pin** ensures content integrity (no MITM substitution).
- **Signature** ensures source authenticity (the SHA WAS published by the legitimate maintainer).

SHA-1 is technically supported (40-hex sha) but SHA-256 (64-hex) preferred when the host repo supports it. With signature verification, an SHA-1 collision attack is insufficient because the attacker would still need the maintainer's signing key.

### Trust commands

```
mock import update                     refresh lockfile (fetch latest, verify, prompt)
mock import update <ext>/<pkg>         refresh one import
mock trust accept <ext>/<pkg>          explicit y-press for a pending trust prompt
mock trust verify [<ext>/<pkg>]        re-run signature verification
mock import rotate <ext>/<pkg> --accept-new-key
                                       acknowledge a key rotation
mock trust list                        show this developer's local TOFU acceptances
mock trust forget <host>               forget this developer's acceptances for a host
```

All `mock trust ...` commands operate on the developer's local trust file. None of them touch the project's harness ref or lockfile.

## 32. The lockfile

`mockspace.lock` lives at the harness ref root. Cargo.lock-shaped: technical pins only, no team-trust bookkeeping, no observation state. Machine-managed; users don't edit by hand.

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

The lockfile is **project configuration committed to version control**, exactly like `Cargo.lock` or `package-lock.json`. It records what versions of what packages this project resolves; it makes a fresh clone reproducible.

The lockfile is NOT a team-trust ledger. It does NOT record "Alice trusted this on Tuesday" or "Bob fetched this last week." Per-developer trust acceptance and observation state live in the developer's local files.

The trust model is git's. Whoever has push access to the repository can update the lockfile; reviewers review the diff like any other config change. Mockspace doesn't impose a special supply-chain trust-root ceremony on top of git's existing access-control mechanisms.

### Verification flow on fetch

1. Look up import URI in `mockspace.lock`.
2. If not present in lockfile: first-time-for-this-project flow (initial setup or `mock import update`). Lockfile entry is written with the observed SHA + fingerprint.
3. If present: fetch the ref's current tip from the host (falling through mirrors if configured).
4. Verify the fetched commit's SHA matches `[[imports]].sha`. Mismatch: refuse; user runs `mock import update` to acknowledge.
5. Verify the fetched commit's signature via `git verify-commit --raw`. Extract the actual signing-key fingerprint. Compare against `[[imports]].signing_key_fingerprint`. Mismatch: `D026`.
6. **Per-developer TOFU gate.** If this developer's local trust file has not seen this `(host, fingerprint)` pair before, prompt for acceptance (interactive) or refuse (non-interactive). Acceptance lands in the local trust file; it does NOT mutate the lockfile.
7. Use the cached content (under `~/.cache/mockspace/imports/...`).

The lockfile is never written by routine fetch operations. Only `mock import update`, `mock import rotate --accept-new-key`, and the initial `mock init`-time lockfile creation write to it. This means the harness ref does not accumulate noise commits from read-shaped operations.

### Per-developer trust files

Two files, both outside version control:

- **`~/.config/mockspace/trust.toml`**: TOFU acceptances. The developer's "I have personally seen this (host, fingerprint) combination" record. Analogous to SSH's `~/.ssh/known_hosts`.
- **`.git/mockspace/observations.toml`**: per-project, per-developer freshness cache. Records `last_observed_at` and `last_witness_at` per import. Used to compute `D030` (staleness) and `D032` (witness-staleness). Gitignored. Not shared.

The split is the central architectural property: the lockfile in the harness ref carries policy (what versions are resolved); per-developer trust + observation state lives per-developer (who has personally accepted what, when each developer last verified freshness).

## 33. The first-party `@/` source

`mock://@/...` URIs resolve to mockspace's first-party content. The `@` placeholder is the binary's hardcoded canonical mockspace project git URL plus signing-key fingerprint.

### Trust source

The mockspace binary embeds at compile time:

```rust
const MOCKSPACE_SOURCE_URL: &str = "https://codeberg.org/mockspace/mockspace.git";
const MOCKSPACE_CONTENT_KEY: &str = "SHA256:abc123...";   // canonical content-signing key
const MOCKSPACE_LOG_KEY: &str = "SHA256:def456...";       // canonical log-signing key (distinct)
```

The URL + content-signing key + log-signing key are the root of trust. The binary's install channel (the user's package manager, `cargo install`, the user's chosen distribution) is the actual root-of-trust delivery; mockspace inherits whatever trust the user already extended to the binary.

### Verification

On every `@/` resolution:

1. Resolve `@/...` against the binary's compiled-in URL.
2. Fetch the ref; verify the commit signature via `git verify-commit --raw`.
3. Extract the actual signing-key fingerprint; compare against the binary's compiled-in `MOCKSPACE_CONTENT_KEY`.
4. Match: proceed.
5. Mismatch: refuse with `D026`. For `@/`, this is a hard refuse; the binary's compiled-in fingerprint is authoritative.

There is no per-project `trust.toml` for the `@/` source. There is no first-run recording step. The user's per-developer trust file records TOFU acceptances for other hosts (third-party imports), not for `@/`. The `@/` source is governed entirely by the binary's compiled-in constants.

### Override via mockspace.toml

If a project wants a non-default mockspace-core source (fork, mirror, internal redistribution):

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

When `[hosts.mockspace-core]` is declared, the `@` shortcut is disabled for that project; all references must use the explicit `ext/` form. The override is a project-level configuration choice committed to the harness ref; developers cloning the project see the override applies.

### Key rotation

Canonical key rotation is a real operational concern. The binary supports it through a compiled-in key history:

```rust
const MOCKSPACE_CONTENT_KEY_HISTORY: &[(&str, &str)] = &[
    ("2026-01-01", "SHA256:abc123..."),   // first key, active from this date
    ("2027-06-15", "SHA256:def789..."),   // rotated key
];

const MOCKSPACE_LOG_KEY_HISTORY: &[(&str, &str)] = &[
    ("2026-01-01", "SHA256:log000..."),
];
```

Verification at `@/` resolution: extract the commit's timestamp, look up the active key from the history (the entry whose `active_from` is the latest predating the commit timestamp), verify the signature matches that key.

This allows:

- Old binaries continue verifying old commits with the original key (they never knew about the rotation but the old key stays in their history).
- New binaries verify old commits with the original key (looked up from history) and new commits with the new key (also in history).
- A user on a binary released between two rotation events sees commits signed before their binary's release verify normally; commits signed after their binary's release fail with `D026` until they upgrade.

The log-signing key (used by the optional transparency log, §34) follows the same pattern with its own history.

Rotation is a binary release: the new history table lands in source, the binary is rebuilt, distribution channels carry the new build. The old key never leaves the history; the canonical project's commitment is that the history is append-only across the binary's lifetime.

For projects with `[hosts.mockspace-core]` override active, key rotation follows the override's rules (per-project lockfile pin, TOFU on the override host), not the binary's `@/` history.

## 34. Optional transparency log

For projects that want defence-in-depth against host-level supply-chain attacks beyond what signature verification provides, mockspace supports a transparency log. Projects that don't configure a log get signature + lockfile pinning as their full defence; that's appropriate for most consumers.

Signed commits plus lockfile pinning together defend against simple attacks (MITM substitution, opportunistic forgery) but do not defend against:

- **Freeze attacks.** A compromised host keeps serving an old, validly signed commit even after the maintainer has shipped fixes. Consumers see no signature failure; their lockfile pins an old SHA; they have no out-of-band signal that they are stuck on a stale tip.
- **Surreptitious key rotation against per-developer TOFU.** A compromised maintainer can rotate keys; consumers' next encounter fires `D026`, but the developer's `mock import rotate --accept-new-key` accepts based only on developer-side trust. A third-party witness log raises the bar.

These are real attacks worth defending against in some contexts. They're also rare enough at small-time scale that the transparency-log infrastructure is not mandatory. Projects opt in by configuring `[transparency]` in their `mockspace.toml`.

### The canonical log: `refs/mock/transparency-log`

The mockspace first-party project hosts an optional transparency log as an orphan ref `refs/mock/transparency-log` on the binary-hardcoded `@/` host. When a maintainer publishes a new `@/`-namespace package version, they may additionally append a signed log entry. Consumers that opt in via `[transparency]` configuration cross-check fetched content against the log; consumers that don't, don't.

Each log commit's trailers carry the structured entry:

```
Mockspace-Log-Entry: v1
Mockspace-Package-Host: codeberg.org/mockspace/mockspace-rs
Mockspace-Package-Ref: refs/mock/export/runner-rs
Mockspace-Package-Version: 1.2.3
Mockspace-Package-SHA: a1b2c3d4e5f6789012345678901234567890abcd
Mockspace-Package-SHA-Algo: sha1
Mockspace-Package-Signing-Key: SHA256:abc123...
Mockspace-Observed-At: 2026-05-18T10:00:00Z
Mockspace-Witness: <signature of (host, ref, version, sha, key, at)>
```

The canonical log is signed by the binary-hardcoded `MOCKSPACE_LOG_KEY`, distinct from the content-signing key `MOCKSPACE_CONTENT_KEY`. The two keys can be held by the same maintainer but should be in different security boundaries (different hardware tokens, different machines). This separation is what makes the log a third-party witness rather than a content-signer self-attestation.

The commit history is append-only by convention; force-pushes are refused by branch protection on the canonical host.

### What the log defends against

- **Freeze attacks.** Consumers' `mock import update` fetches the log alongside the package fetch. If the locked package's SHA does not appear in the log, or appears only at an `observed_at` older than `staleness_threshold_days` (default 90), `mock doctor` raises `D032`. The maintainer can append a fresh log entry without changing the package SHA (re-attesting "this is still current"); a compromised host that can no longer reach the maintainer cannot.

- **Surreptitious key rotation.** When a maintainer legitimately rotates keys, they append a `Mockspace-Key-Rotation` entry to the log signed by their OLD key naming the NEW key fingerprint. The consumer's `mock import rotate --accept-new-key` cross-references the log: the new key must appear in a rotation entry signed by the old key, OR the consumer must explicitly bypass via `--no-transparency-check`. A compromised maintainer who has the new key but not the old cannot produce a valid rotation entry.

- **Compromised host serving different content to different clients.** Two clients fetching the same package version see the same log entry. Disagreement between served content and log entry is a detectable inconsistency.

### Federation

The log is an orphan git ref. Any forge can serve it; any client can mirror it; cross-mirror verification is trivial because the commits are signed. The log is small (one commit per published version; a busy ecosystem might produce a few hundred commits per year). No separate service infrastructure, no separate registry, no separate transparency backend. It is git all the way down.

### Maintainer workflow

When a maintainer publishes a new version of an exported package:

1. `mock export publish <name>` pushes the export ref normally.
2. `mock export witness <name>` constructs the log entry, signs it, pushes a new commit to `refs/mock/transparency-log` on the canonical host (or to a project-local log).

The two-step shape is intentional. `publish` is local to the maintainer's host; `witness` is the act of cross-attesting on the canonical log. A maintainer who does not witness is implicitly opting out of transparency for that version, which the consumer's doctor will surface as `D032`.

### Project-local logs

Projects that do not use the canonical mockspace first-party host can configure their own log:

```toml
[transparency]
log_uri = "mock://ext/our-org-log/transparency-log"
log_signing_key = "SHA256:def456..."
```

The log can itself be hosted on any forge mockspace supports. The property is "an independent witness exists," not "the first-party canonical witness exists." A consumer importing from multiple ecosystems may consult multiple logs.

### What the log does NOT do

- It does not prevent a compromised host from serving content; it detects that the served content disagrees with the cross-witness.
- It does not prevent a compromised maintainer (who has both old and new keys) from rotating without leaving a malicious trail; key rotation is detectable but not preventable.
- It does not provide non-repudiation of contributorship; it is a log of what the maintainer attested to, not of who pushed the source.

For a small-time ecosystem (handful of first-party packages plus opportunistic third-party contributions), the asymmetric cost is good: maintainers spend seconds per release on `mock export witness`; consumers gain structural defence against the two most common supply-chain attacks at zero per-fetch cost.

# Part V. Hooks, profiles, policy

Mockspace's reactive events fire **hooks**: scripts invoked with structured environment variables when specific events occur. **Profiles** group default-behaviour choices into named sets. **Env and bins policy** controls what executables and environment variables hooks see.

## 35. Hook protocol

A hook is a script invoked at a specific mockspace event. Hooks are project-controlled (declared per-profile in `mockspace.toml`); mockspace runs them and observes their exit codes.

### Trust posture: hooks run with the developer's environment

Hooks default to inheriting the parent process environment, the parent process's `PATH`, and the developer's shell-resolved binaries. This is the same trust posture as a Cargo `build.rs`, an npm package's `scripts`, or a git `hooks/` script.

A hook can see `SSH_AUTH_SOCK`, `GH_TOKEN`, every `*_TOKEN`, every shell credential the developer has in their session. If you import a hook from an external host, you are extending that trust to that host's maintainer.

The mitigations are upstream: signed commits, SHA pinning, transparency-log witnessing, lockfile drift detection, and the opt-in env/bins restrictions in §37. There is no sandbox at the hook-execution layer; sandboxing would require a separate runner (the long-term direction for vehje as hook language; see Part VIII §59).

### Event vars passed to every hook

Default-inherited unless overridden by env policy:

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

For each event, `${MOCK_HELPERS}/<event>.sh` validates and exposes event-specific vars with documentation. User hooks source the helper:

```bash
#!/usr/bin/env bash
source "${MOCK_HELPERS}/on_dirty_state.sh"
# now you have MOCK_DIRTY_AREA, MOCK_DIRTY_FILES (array), etc.

if [ "$MOCK_DIRTY_AREA" = "round" ]; then
    git add ".mock/round/"
    git commit -m "auto-save from custom hook"
    exit 0
else
    exit 2  # fall back to mockspace default
fi
```

### Hook exit codes

- `0`: handled, proceed.
- `1`: handled but failed, abort with diagnostic.
- `2`: not handled, fall back to mockspace's default behaviour (typically refuse).
- Any other: treated as `1`.

### Hook event registry

| Event | When it fires | Action options |
|---|---|---|
| `on_dirty_state` | Phase transition with dirty `.mock/<area>/` | prompt / auto-commit / refuse |
| `on_phase_race` | Mock-side ref push race lost | prompt-resolve / auto-rebase / side-branch-refuse |
| `on_replan_nonclaimed_edits` | Replan refused due to non-claimed source edits | prompt / refuse |
| `on_doctor_finding` | mock doctor found inconsistency | prompt / auto-repair / refuse |
| `on_pr_body_conflict` | PR body managed-section regen race | prompt / auto-overwrite-managed-section / backup |
| `on_external_unpinned` | Non-pinned external in rendered output | warn / refuse |
| `on_schema_version_skew` | Binary version mismatch | warn / refuse |
| `on_archive_contention` | Archive ref push race | retry / surrender |
| `on_verifier_failure` | Manifest claim verifier failed at seal | abort / continue with --force |
| `on_first_trust` | First-time import requiring trust acknowledgement | prompt y/n / auto-accept / refuse |
| `on_signing_mismatch` | Signature doesn't match locked fingerprint | prompt / refuse |

## 36. Profiles and reactive policy

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
on_first_trust = "refuse"           # CI/auto contexts should pre-trust via lockfile
on_signing_mismatch = "refuse"
```

### Handler value types

Each `on_*` field accepts:

1. **Built-in directive**: `"prompt"`, `"refuse"`, `"auto"`. Resolves to the mockspace-shipped handler from the embedded baseline.
2. **`mock://` URI**: import; SHA-pinned required for external URIs that resolve to executable content.
3. **Script path**: relative to harness root (e.g., `"hooks/on_dirty_state.sh"`).

Hook scripts have a default size cap of 1 MB and a default execution timeout of 60 seconds, both configurable per profile via `[profile.<name>].hook_max_bytes` and `[profile.<name>].hook_timeout_seconds`.

### Inline bash in TOML is not a hook value type

Earlier drafts permitted multi-line TOML strings as hook bodies; that shape was removed. Inline executable code in `mockspace.toml` is low-visibility to reviewers (TOML diffs are line-noisy) while carrying the same trust authority as a checked-in hook file. Forcing hook code into files under `hooks/` puts the executable surface on the reviewable file-diff surface where reviewers naturally look.

`mock doctor` raises `D036` if any `on_*` field's value parses as inline bash (starts with `#!` or contains newlines and non-path characters). The check is heuristic; the structural defence is that file paths and `mock://` URIs are the documented value types.

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

One-shot CLI flags: `--interactive` / `--non-interactive`, `--auto-repair` / `--no-auto-repair`.

### Embedded baseline + first-party imports

Mockspace's binary embeds a baseline of helper scripts + default handlers (extracted to `~/.cache/mockspace/helpers/` on first run). Equivalent to what you'd get from `mock://@/handler/<directive>/<event>`.

Projects rely on the embedded baseline by default; they can import explicitly for version-pinning when reproducibility matters.

## 37. Env and bins policy

Hooks run with the user's parent environment by default. Users opt into restrictions via `[profile.<name>.env]` and `[profile.<name>.bins]` sections using a unified glob + negation syntax.

### Glob + negation syntax

A list field with semantics:

- Field absent or empty: inherit everything from parent (no filtering).
- Field present: build the resulting set left-to-right:
  - `"*"` adds all parent items to the set.
  - `"<exact>"` adds that item if present in parent.
  - `"<glob>"` adds all parent items matching the glob.
  - `"!<exact>"` removes from set.
  - `"!<glob>"` removes all matching from set.

Order matters; later entries override earlier. Negation is the only way to remove from the set.

**Glob dialect.** Gitignore-style globs: `*` matches any run of characters within a single segment, `**` is reserved for future hierarchical use, `?` matches one character, `[A-Z]` matches one character from the bracketed class, `\` escapes a literal special character. No brace expansion, no command substitution, no extglob. Mockspace parses globs once at config load and refuses unsupported syntax with a structured diagnostic.

**Case sensitivity.** Env-variable names: case-sensitive on Linux and macOS (matches POSIX semantics); case-insensitive on Windows but preserving case of first occurrence. Bin names: case-insensitive on Windows (matches `PATH` resolution semantics); case-sensitive on Linux and macOS. Cross-platform configs that need to work everywhere should write env names in uppercase by convention.

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

The bare-negation case results in an empty set, not the parent minus FOO. The mental model: the output set starts empty; entries either add to or remove from it.

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

Per-event override: `[profile.<name>.on_<event>.env].inherit = [...]` overrides the profile default for that event.

### Bins policy

Controls which binaries are on `PATH` for the hook. Same glob + negation syntax:

```toml
[profile.dev.bins]
# Default (absent): inherit full parent PATH
# Strict allowlist (no network tools):
inherit = ["git", "grep", "sed", "awk", "cat", "echo", "ls", "find", "test"]

# Inherit all but explicit denials:
inherit = ["*", "!curl", "!wget", "!nc", "!ncat", "!socat", "!ssh"]
```

Implementation: mockspace constructs a temp directory, symlinks the allowed binaries from the resolved parent PATH, sets `PATH` to that temp dir for the hook subprocess. On hook exit, the temp dir is cleaned up.

### Combining env and bins

A hook with the strict allowlist for both gets a tightly scoped environment. A hook with neither set runs with full parent env + PATH (default).

Mockspace's auto profile defaults are deliberately permissive:

```toml
[profile.dev.env]
# no restriction by default; user opts in if they want

[profile.dev.bins]
# no restriction by default; user opts in if they want
```

Users who want defence-in-depth can opt into restrictive policies without mockspace forcing them.

## 38. Language-specific runners

Mockspace-core handles bash by default. Other languages run through per-language runner packages imported from language-specific mockspace extensions.

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

A runner package exports a binary that mockspace invokes when a file's extension matches `include`. The runner receives standard hook env vars + event-specific vars and is responsible for compilation/execution.

For Rust: `mockspace-rs/export/runner-rs` is a binary that compiles a `.rs` file and runs it. It receives the hook env vars; it returns the hook's exit code. From mockspace's perspective, the indirection is transparent: a `.rs` file is treated like a `.sh` file with a different runner.

For TypeScript (future `mockspace-ts`): same pattern. The runner is a Deno or Bun executable that runs `.ts` files; the runner package declares which file extensions it claims.

This is how mockspace stays language-agnostic at the core while supporting any language through extension packages. The runner contract is small: take env vars, run code, exit with a meaningful code.

# Part VI. The interface

Mockspace's CLI follows git's plumbing-versus-porcelain split. Plumbing commands are precise and do exactly what they say; they are stable, scriptable, `--json`-friendly. Porcelain commands are high-level workflow-aware verbs that read context and Do The Right Thing; they decompose into plumbing internally.

Most daily use targets porcelain. Power users, automation, agents, and editors target plumbing.

## 39. CLI plumbing

The full plumbing command set, grouped by domain.

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
mock phase replan [--force] [--restore-by-commit] [--accept-restoration-loss <file>...]
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
mock topic                               list topics in active round
mock topic new <name>                    create a new topic file; open in $EDITOR
mock topic show <name>                   show a topic file
```

### Domain: manifests

```
mock manifest                            show current phase's manifest
mock manifest show
mock manifest verify [--json]            run verifier without sealing
```

### Domain: research, benches, sketches

```
mock research new <slug>                 scaffold a research cluster ref
mock research list
mock research show <slug>

mock bench new <bundle-name>             scaffold a bench bundle under mock/benches/
mock bench list
mock bench run <bundle>                  build variants; run; analyse; write findings
mock bench report <bundle>               re-render findings from existing run data

mock sketch new <round-slug> <name>      scaffold a sketch in mock/research/sketches/<round-slug>/
mock sketch run <sketch-path>            compile-probe via rustc (or language-specific runner)
mock sketch report <round-slug>          regenerate the sketch README status table
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
mock trust list                          show developer's local TOFU acceptances
mock trust accept <host> <fingerprint>   explicitly accept a (host, fingerprint) pair
mock trust forget <host>                 forget acceptances for a host
mock trust verify [<ext>/<pkg>]          re-run signature verification
```

All `mock trust ...` commands operate on the developer's local trust file. None touch the project's harness ref or lockfile.

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
mock host fetch <name> [--verify-mirrors]
mock host show <name>
```

### Domain: harness

```
mock harness show                                show resolved harness config + status
```

Commit signing of harness commits is project policy handled by git (`commit.gpgsign`) or forge branch protection; not a mockspace concern. `mock harness show` is informational only.

### Domain: cache

```
mock cache show                                  show cache size, location, age
mock cache prune [--older-than=<duration>]       evict unused imports + helpers
mock cache verify                                re-hash cached imports against lockfile
```

`mock cache prune` evicts content not referenced by any project's current `mockspace.lock` on this machine, plus any entry older than `--older-than` (default 90 days unreferenced, 365 days total).

## 40. CLI porcelain

Porcelain verbs read context and decompose to plumbing. Each verb supports `--explain` (prints decomposition without running) and `--dry-run` (prints what would happen and asks for confirmation).

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

### Shared properties

- **`--explain`** prints the plumbing decomposition without running. Decomposability is verifiable: the porcelain implementation literally constructs the plumbing call sequence, then either executes or prints. This is the property that makes porcelain a thin layer rather than a parallel implementation.
- **`--dry-run`** prints what would happen, asks for confirmation, then runs.
- **Reads context.** Each porcelain verb inspects the current phase, active round, dirty areas, lockfile state, recent operations. Doesn't ask the developer to spell out what mockspace can infer.
- **Asks on ambiguity.** Composite operations like `mock done` describe each step in the structured-diagnostic format and offer per-step `y`/`N` or `Y` (accept all). No silent multi-step execution.
- **Snapshotted destructive ops.** Every porcelain verb that mutates state lands one composite undo entry (§42).
- **Doesn't replace plumbing.** All plumbing verbs stay. Porcelain is convenience; plumbing is the contract surface for tools, agents, CI, and editors.

### Per-verb decomposition

- **`mock work [<topic>]`**: if no active round, runs `mock round new` with an auto-generated slug of the form `<UTC-YYYYMMDDHHMM>-<sanitised-topic>` (the topic is sanitised by lowercasing, replacing whitespace with hyphens, and stripping non-`[a-z0-9-]` characters; when `<topic>` is empty, the suffix defaults to `quickstart`). Then runs `mock topic new <topic>` on the new round. If active round and `<topic>` provided, runs `mock topic new <topic>` on it. If active round and no `<topic>`, opens the active round's current-phase surface (equivalent to `mock open`).

- **`mock advance`**: reads current phase. If dirty areas exist, prompts to `mock commit` first (or auto-commits per profile). Then runs the next plumbing transition: `mock phase plan` from TOPIC/PLAN, `mock phase apply` from PLAN, `mock phase finish` from APPLY.

- **`mock commit [-m "..."]`**: inspects every `.mock/<area>/` for changes against rendered ref state. Routes commit per area:
  - `.mock/round/<active>/` → mock-side commit on `refs/mock/round/<active>`.
  - `.mock/tasks/<task>/` → mock-side commit on the task's ref.
  - `.mock/research/<slug>/`, `.mock/bench/<slug>/`, `.mock/sketch/<slug>/` → corresponding ref.
  - `.mock/mockspace.toml`, `.mock/mockspace.lock`, `.mock/agent/`, `.mock/lints/`, `.mock/templates/`, `.mock/hooks/` → harness.
  - `.mock/export/<package>/` → `refs/mock/export/<package>`.
  
  `-m "<msg>"` applies to all per-area commits. Without `-m`, mockspace generates a per-area commit message. `--per-area` lets the developer specify different messages per area interactively.

- **`mock done`**: runs `mock phase finish`, then `mock round close`, then `mock forge sync`. Each step described before execution; the developer accepts the whole composite or per step.

- **`mock sync`**: fetches all imports (`mock import update`), all ext mirrors (`mock ext refresh --all`), forge state (`mock forge sync`).

- **`mock add <ext>/<pkg>`**: porcelain for adding an import:
  1. If `<ext>` (host alias) is not configured, prompt for URL + optional mirrors; write to `[hosts.<ext>]`.
  2. Add `mock://ext/<ext>/<pkg>` to `[imports]`.
  3. Fetch the ref; verify signature.
  4. Per-developer TOFU prompt for the `(host, fingerprint)` pair.
  5. Write the lockfile entry.
  6. Commit harness changes.

- **`mock open [<area>]`**: opens the relevant rendered surface in `$EDITOR`:
  - No arg: opens the active round's current-phase manifest file.
  - `mock open round`: opens `.mock/round/<active>/`.
  - `mock open task [<ns>::<slug>]`: opens an active task's directory.
  - `mock open harness`: opens `.mock/mockspace.toml`.
  - `mock open lock`: opens `.mock/mockspace.lock`.
  - `mock open <slug>`: opens the matching round/task/research/sketch.

### When porcelain refuses

Porcelain is opinionated about The Right Thing but never silently guesses on destructive ambiguity:

- `mock advance` refuses when the current phase has unresolved questions (open subtopics without decisions). Suggests `mock open round` to resolve first.
- `mock done` refuses when the active round has open tasks. Suggests `mock task close` per task or `--force-close-tasks`.
- `mock commit` (bare form) refuses when no area has changed; asks whether the developer meant `mock commit --empty` or a specific area.
- `mock add` refuses when the target `<ext>/<pkg>` already has a conflicting `[hosts.<ext>]` (different URL); suggests `mock host show <ext>` to inspect.

### What porcelain is NOT

- Not a replacement for plumbing. Every porcelain decomposes; the plumbing is the contract.
- Not a hidden layer. `--explain` makes every porcelain operation fully transparent.
- Not magic. Porcelain reads observable state and runs known sequences. No ML, no heuristics-that-might-be-wrong, no inference from history beyond the immediate phase context.

## 41. `mock status` as the primary entry

`mock status` is the canonical "what's going on" command. New developers and daily users target it first; it answers the standing question without needing to know mockspace's internals.

### Output

```
mockspace 1.0.0 | project: arvo | branch: round/202605181400-arvo-graph-csr

Active round: 202605181400-arvo-graph-csr (PLAN(SRC))
  Title: arvo-graph storage layout
  Source-side branch: round/202605181400-arvo-graph-csr (at 7a3b...)
  PR: https://github.com/orgrinrt/arvo/pull/437

Dirty areas:
  .mock/round/202605181400-arvo-graph-csr/    (manifest.src.toml edited, 2 [[change]] blocks added)
  .mock/mockspace.toml                         (severity ramp on no-bare-numeric: warn → error at push)

Recent undo entries:
  1.  2026-05-18 14:32  phase apply (PLAN(DOC) → APPLY(DOC))
  2.  2026-05-18 14:15  commit round (manifest.doc.toml)

Doctor: clean

Next suggested action:
  mock commit round   (commit the manifest.src.toml edits)
  mock phase apply    (seal the manifest; transition to APPLY(SRC))
```

The output is opinionated: it describes the current state, dirty areas, recent undo, and the suggested next action. The author can take the action, ignore it, or dig deeper with subcommands.

### `--fast` and `--json`

- `--fast` skips the doctor scan and outputs in under 20ms. Useful in editor integrations that update on every save.
- `--json` outputs structured data for programmatic consumers.

### Latency targets

- Cold (first invocation in a session): under 300ms.
- Warm (subsequent invocations): under 50ms.
- `--fast`: under 20ms.

Achieved by caching the parsed harness + index under `.git/mockspace/index.bin` and only re-parsing on staleness detection.

## 42. Undo and redo

Destructive mockspace operations are recoverable via a short-span undo log. The mechanism is invisible to daily use; developers discover it when they need it.

### Mechanism

Every destructive mockspace operation appends an entry to `.git/mockspace/undo/log.jsonl` before applying:

```json
{
  "ts": "2026-05-18T14:30:00Z",
  "op": "phase apply",
  "description": "advance round 202605181400-arvo-graph-csr from PLAN(DOC) to APPLY(DOC)",
  "before": {
    "refs/mock/round/202605181400-arvo-graph-csr": "abc123...",
    "refs/mock/harness": "def456..."
  },
  "after": {
    "refs/mock/round/202605181400-arvo-graph-csr": "789abc...",
    "refs/mock/harness": "def456..."
  },
  "metadata": { /* op-specific */ }
}
```

The log is append-only. `mock undo` does NOT delete entries; it appends a counter-entry marking the prior entry as undone.

Because refs are SHAs, snapshots record SHA + description, not content. Git's normal reflog retention (default 90 days) keeps the underlying objects reachable for the undo window; no extra storage.

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
2. For each ref in the entry's `before` map, runs `git update-ref <ref> <before-sha>`.
3. Appends a counter-entry to the log marking the entry as undone.

If any of the affected refs has been advanced beyond the entry's `after` state by a subsequent NON-mockspace operation (e.g., manual `git update-ref` or another tool), `mock undo` presents the diff and refuses without `--force`. The default is "don't silently rewind a divergence I didn't cause."

### `mock redo`

After `mock undo`, the entry stays in the log but is marked undone. A new destructive operation clears all undone entries (linear history; vim semantics). Until that happens, `mock redo` finds the most-recent undone entry, re-applies its `after` state, and appends a counter-entry marking it re-done.

### Inspection

```
mock undo --list                         show the recent undo log
mock undo --show <n>                     show the Nth most recent entry's diff
mock undo --explain                      show what mock undo would do without running
mock redo --explain                      show what mock redo would do without running
```

### Retention

Default: keep the last 50 entries OR all entries within the last 30 days, whichever bound retains more. Configurable via `[undo].keep_entries` and `[undo].keep_days`.

Beyond that, entries roll out of the log.

### Coupling to git's reflog retention

`mock undo` rewinds refs by SHA. The SHAs are durable only as long as git's object store retains the referenced commits. Git's retention is controlled by `gc.reflogExpire` (default 90 days, controls reachable-from-reflog objects) and `gc.reflogExpireUnreachable` (default 30 days, controls objects only reachable through reflog and otherwise orphaned).

Mockspace's bootstrap writes these git config values to match `[undo].keep_days` (default 30), keeping the two retention windows aligned. The values land in `.git/config` (per-repo, not global):

```ini
[gc]
    reflogExpire = "30 days ago"
    reflogExpireUnreachable = "30 days ago"
```

`mock doctor` checks for divergence (the user ran `git gc --prune=now` or set conflicting values) and emits `D044` ("undo log retention longer than git reflog retention; undo entries may reference unreachable objects"). The bootstrap also adds a comment to `.git/config` explaining why mockspace touches these values.

A `mock undo` against an entry whose referenced objects have been GC'd surfaces a structured diagnostic ("the SHA `<before-sha>` is no longer in the object store; this entry's referenced state has been garbage-collected"). The entry stays in the log as audit trail; mockspace just cannot re-apply it.

### Pushed operations

`mock undo` on an operation that was already pushed to a remote does NOT silently rewind the remote ref. Instead:

1. The local rewind succeeds (local refs are the developer's).
2. Mockspace prints a structured diagnostic: "the remote ref is currently at `<after-sha>`; your local is now at `<before-sha>`."
3. Suggests two follow-ups:
   - `mock undo --apply-remote` to push a new commit on top that restores the prior state (preserves remote history; everyone else sees an "undo commit" rather than a rewrite).
   - Explicit `git push --force-with-lease` if the developer is sure no one else has pulled.

Default behaviour: never force-push. Undo against pushed history becomes an additive commit unless explicitly overridden.

## 43. `mock doctor` and the findings catalog

`mock doctor` is the diagnostic command. Read-only by default; `--repair` applies fixes.

### Findings

The full findings catalog is in Part VIII §55. Examples:

| ID | Description |
|---|---|
| D001 | Round ref exists but `.mock/round/` doesn't |
| D002 | `.mock/round/` exists but no round ref |
| D003 | `.mock/round/` content drift from ref tree |
| D004 | Anchor blob SHA mismatch |
| D026 | Import package signed by unrecorded key |
| D027 | Import package signature invalid or unsigned |
| D029 | Migration journal partial or drifted |
| D036 | Hook value parses as inline shell |
| D037 | Race conflict could not be preserved on remote |
| D038 | `.mock/` parent appears to be a cloud-sync directory |
| D040 | Anchor blob missing from storage |
| D041 | `mock://~/...` URI found outside a package-execution context |

Each finding has: an ID, a description, a five-element diagnostic message (invariant, situation, blocked_because, suggested_command, recovery), and an optional auto-repair routine.

### Doctor commands

```
mock doctor                              read-only scan
mock doctor --json                       structured output
mock doctor --json --report-file=path    write structured report
mock doctor --repair                     apply fixes for fixable findings
mock doctor --repair --interactive       prompt per finding
mock doctor --repair --finding D001      target one finding kind
mock doctor --ci                         CI-friendly output mode
```

Per-finding repair is atomic. Repair operations log to `.git/mockspace/doctor.log` in structured JSON with pre/post SHAs, finding ID, success/failure.

Repairs sorted topologically. Refuse `--repair` on a finding whose trust source is itself flagged.

### Five-element diagnostic format

Every error, warning, or guidance follows the same shape:

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

## 44. `mock sync` and staleness

`.mock/<area>/.ref-sha` tracks rendered-from-SHA per area. Every mockspace invocation does a staleness check: compares the SHA stored in `.ref-sha` against the current ref tip; if they differ, the area is stale.

`mock sync` commands:

```
mock sync                          sync all areas (re-render from current ref tips)
mock sync round                    sync only the active round
mock sync harness                  sync only the harness
mock sync tasks                    sync all active tasks
mock sync --full                   sync + run age-based auto-archival
mock sync --force                  discard local edits and force re-render
mock sync --check                  print staleness state; change nothing
```

Auto-archival runs on `mock close` (full sweep of closed tasks past threshold) and on `mock sync --full`. Never on read commands.

When `.mock/<area>/` has local edits AND the ref has advanced (someone else pushed), the sync surfaces the conflict: the local edits are uncommitted, the ref tip has new content, mockspace cannot silently merge. The developer chooses (`mock commit` then `mock sync`, or `mock sync --force` to discard local edits).

## 45. Cross-cutting concerns

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

`--json` on every read command. Structured stderr in `--json` mode. `--report-file=<path>` writes structured report to the path.

### Dry-run

`--dry-run` on every state-changing command. Prints what would happen; takes no action.

### Signal handling

`SIGINT`/`SIGTERM`:

- Before update-ref boundary: clean abort.
- After local commit before push: un-pushed, `--resume` completes the operation.
- After push before forge: reconcile via `mock pr regen`.

### Shell completion

```
mock completion install --shell bash --path <p>
mock completion install --shell fish --path <p>
mock completion install --shell zsh --path <p>
```

# Part VII. Operational

## 46. The `mockspace.toml` schema

```toml
[mockspace]
version = "1.0"                    # intended mockspace tool version (major.minor)
default_profile = "dev"
default_one_active_round = true
verifier_timeout_seconds = 30      # per-verifier wall-clock budget
# mock_bin_path = "target/release/mock"
#   Optional. Step 2 in the invocation resolution chain (§57).
#   Full order: MOCKSPACE_BIN_PATH env, this field, `which mock`,
#   `cargo mock` probe, error. Bootstrap-generated proxies always
#   set MOCKSPACE_BIN_PATH on exec so the running binary's own
#   resolution sees the same value the bootstrap baked.
#   Relative to this mockspace.toml's directory. Absolute paths
#   warn at load time but still work; see "Portable paths" in §57.

[refs]
mirror_ext_refs = true
push_mirrors = false
fetch_on_reference = true
task_archive_threshold_days = 90
round_archive_threshold_days = 365

[refs.security]
# domain_allowlist = ["github.com", "codeberg.org", "*.example.com"]
require_https = true               # http:// URLs refused at config load

# Hosts. The primary host is the active forge integration target
# (PRs, issues, signing). Other hosts are import / mirror sources.
# All hosts share one schema; the primary fills more fields.
primary_host = "self"

[hosts.self]
url = "https://codeberg.org/orgrinrt/mockspace.git"
type = "forgejo"                   # or "github"
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

# Lint packs (external)
[lint-crates]
"mockspace-hilavitkutin-stack-lints" = {
  git = "https://codeberg.org/orgrinrt/mockspace-hilavitkutin-stack-lints.git",
  rev = "abc123",
}

# Per-lint severity
[lints.no-bare-numeric]
commit = "error"
build = "warn"
push = "error"

[lints.file-size]
commit = "warn"
build = "off"
push = "error"
max_lines = 500

# Scoped lint configuration
[lints.forbidden-imports.scope.arvo-strategy]
commit = "error"
forbidden = ["arvo-storage", "arvo-graph", "arvo-spectral"]
reason = "L0 cannot depend on L1+; would create a layer inversion."

# Primitive-introductions meta-config
[primitive-introductions]
arvo-bits = ["bit-storage"]
arvo = ["numeric-fixed-point", "boolean", "platform-pointer"]
hilavitkutin-str = ["string"]

# Languages
[languages]
rust = "built-in"
typescript = { git = "https://codeberg.org/mockspace/mockspace-ts.git", rev = "abc123" }

# Profiles
[profile.dev]
on_dirty_state = "prompt"
# ... (see Part V §36)

[profile.ci]
on_dirty_state = "refuse"
# ...

[profile.auto]
on_dirty_state = "auto"
# ...

# Doc-generation metadata
[crate_colors.arvo-bits]
fg = "#ffffff"
bg = "#3f51b5"

[domain_kinds.numeric]
glyph = "n"
label = "Numeric"

[known_macros.strategy_marker_required]
description = "Every public numeric type carries a Strategy marker."
usage = "S: Strategy = Hot"

layers = ["L0", "L1", "L2", "L3"]
primary_domain_macro = "strategy_marker_required"
primary_domain_label = "Strategy axis"

# Transparency log (optional)
[transparency]
# log_uri = "mock://@/transparency-log"
# staleness_threshold_days = 90

# Undo retention
[undo]
keep_entries = 50
keep_days = 30
```

The schema is open in some places (per-project customisations under `[crate_colors]`, `[domain_kinds]`, `[known_macros]`) and closed in others (lint configuration uses a registered lint name; unknown lint names refuse at load). The closed positions are the supply-chain-sensitive ones: lint names, language names, host names. The open positions are display metadata.

### Loader rejection

The loader refuses configurations with:

- Host names colliding with reserved kinds (`local`, `ext`, `@`, `~`, or any `local_kind`).
- Lint names not registered (unknown lint).
- Severity values not in `{error, warn, info, off}`.
- HTTP URLs when `refs.security.require_https = true`.
- Inline shell in `on_*` hook values.
- TOML grammar errors.

Each refusal is a structured diagnostic in the five-element format.

## 47. Render pipeline

Mockspace renders content from refs to three targets:

1. **Source-tree**: committed to `refs/heads/*`. README, docs, etc. Visible to outsiders.
2. **Local-only**: filesystem only, gitignored. `.claude/rules/`, `.github/instructions/`, agent integration surfaces.
3. **Forge API**: PR title + body, pushed via API.

### Multi-target rendering

A template can render to multiple targets:

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

Some templates render to one target only (per-crate `README.md.tmpl` → source-tree only). Some render to multiple (workflow descriptions → both source-tree and agent surfaces).

### Render-time substitution

Templates support the substitution shapes described in Part I §4:

- `{{variable}}`: value interpolation.
- `{% for x in xs %}...{% endfor %}`: iteration.
- `{% if cond %}...{% else %}...{% endif %}`: conditional.
- `{% include "fragment.md.tmpl" %}`: fragment inclusion.
- `{{crate_summaries}}`: special: composes per-crate README.md.tmpl content.

The data model exposed to templates:

```
{
  "project": <parsed mockspace.toml>,
  "crates": <discovered crate set>,
  "deps": <dependency graph data>,
  "round": <active round metadata, if any>,
  "lints": <effective lint configuration>,
  "tools": <enabled agent tools and their targets>,
}
```

### Render failure handling

A render failure during a phase transition aborts before the ref update (Part III §24 step 10). The transition does not happen if rendering fails. This avoids the partial-state failure mode where the ref advances but the rendered surfaces are inconsistent.

For non-transition renders (e.g., regenerating the rendered `docs/` tree after editing a template), failure prints a diagnostic and exits non-zero; the rendered output is not partially overwritten.

### Atomic multi-file rendering

A single render pass typically writes N files (per-crate READMEs, mock-root templates, agent rule files, etc.). The pipeline maintains all-or-nothing consistency at the tree level:

1. Render every output file to a sibling staging area: `<target_root>/.mock.staging.<pid>.<ts>/`.
2. Once all writes succeed and verify against expected content, run a single `rename(.mock.staging.<pid>.<ts>, .mock)` (or per-target equivalent) to swap the staging tree into place. Standard POSIX `rename(2)` is atomic for directory-to-directory swaps on the same filesystem.
3. On any write failure mid-pass: the staging directory is removed; the previous `.mock/` tree is untouched.
4. Power-loss between step 1 and step 2: the next `mock sync` (§44) discovers the orphaned staging directory and removes it; the previous tree remains canonical.
5. Power-loss between step 2 and the parent process exit: the rename has already committed; the new tree is canonical; subsequent operations see consistent state.

For local-only targets that span filesystem boundaries (rare, but possible if `.claude/` is symlinked elsewhere), the renderer falls back to per-file atomic-write (tempfile + rename) and surfaces a doctor finding (D043) recommending consolidation.

Windows pre-ReFS lacks directory-rename atomicity. The Windows path (§58) uses per-file atomic-write throughout, with a `.mock/.render-manifest` file written last to mark consistency; `mock sync` reads this manifest to detect partial renders. The semantic guarantee is the same; the mechanism differs.

### Deterministic rendering

Render outputs are byte-deterministic given the same input ref tree. Three sources of non-determinism that the pipeline closes:

1. **Map iteration order.** All template-side iteration over map-shaped data (lints, hosts, imports, etc.) sorts by key before iterating. The renderer's data model exposes `BTreeMap`-shaped values, not `HashMap`.
2. **Timestamps.** Templates that need a timestamp use the commit time of the ref being rendered (captured into the data model as `ref_committer_time`), not wall-clock-now. Wall-clock-now substitution is forbidden at template engine level (the substitution macro is not exposed).
3. **External tool output.** Renders that embed output from external tools (e.g., dependency-graph SVG from Graphviz) pin the tool version in `mockspace.toml`'s `[render.tools]` and snapshot tool output to a stable form; layout-engine non-determinism is handled by post-processing to a canonical form or by capturing one rendered SVG and committing it as input.

Deterministic rendering is what makes the staleness check in §44 work: a fresh clone re-rendering an unchanged ref produces byte-identical output. Drift detection compares tree hashes.

### Round artefacts never render

Round artefacts (topic files, manifests, anchors, comments, sketches, benches, research) live on `refs/mock/round/<slug>` and similar. They are NOT rendered to source-tree or to forge. They live in `.mock/round/<slug>/` for the developer to interact with; that interaction surface is gitignored.

The PR projection (Part VII §48) is the one exception: round metadata feeds the PR body, but the PR body is render output to the forge API, not to source-tree.

## 48. PR lifecycle

### Branch creation

The round's source-side branch is created off the primary host's `default_base_branch` (typically `dev`) at `mock round new`. A scaffolding commit lands on the branch immediately so the PR can open.

### PR creation

PR auto-opens at PLAN(DOC) → APPLY(DOC) transition per `auto_open_pr`. The PR has a managed-body section.

### PR body autogeneration

The body has a managed section with HTML-comment delimiters and a clear visible warning:

```markdown
<!-- mockspace-managed -->
> Auto-generated. Edits inside this block are overwritten on each
> phase transition. Put notes above this block to preserve them.

## Description

[Generated description: round title, topic summaries, manifest claims,
benches referenced, sketches referenced.]

## Phase status

PLAN(DOC) sealed at 2026-05-18T14:30:00Z.
APPLY(DOC) in progress.

## Manifests

- manifest.doc.locked.toml (5 [[change]] blocks)
- manifest.src.toml (in draft)

[etc.]
<!-- /mockspace-managed -->
```

Outside the managed block, the author can add free-form notes. Mockspace never touches content outside the delimiters.

### Conditional updates

Etag-conditional when the forge supports it; on 412 conflict: refetch, retry once. On retry conflict: invoke `on_pr_body_conflict` per profile (§35).

**Scope of profile action.** The profile's `auto-overwrite-managed-section` action overwrites ONLY the content between the `<!-- mockspace-managed -->` and `<!-- /mockspace-managed -->` delimiters. Content outside the managed section is human-authored and is never overwritten, regardless of profile. If the conflict's incoming version touches outside-managed content, the action falls through to `backup` (write `.mock/pr.backup.<ts>.md`, emit warning) and the conflict requires human resolution. This wall is structural; no profile setting can pierce it.

If delimiters missing (author or someone else edited them out): always refuse + backup, regardless of profile. Suggest `mock pr regen --force` to re-establish.

### Auto-merge

`auto_merge_on_done = false` by default. When true, mockspace issues the merge API call at `mock close`; respects branch protection; round state advances regardless.

### Token scopes

- **GitHub**: classic PAT with `repo`, or fine-grained PAT with `contents:read-write` + `pull-requests:read-write`.
- **Forgejo/Codeberg**: PAT with `repository:write` + `issue:write`.

### Retry policy

3 attempts, [1, 4, 16] second backoff. 429 honors `Retry-After` header; no in-invocation retry on rate limit (the user runs `mock forge sync --resume-rate-limited` later).

### Comment ingestion

Mandatory unless `--no-comments`. Paged with checkpointing. `.comments.status` records `complete` / `partial` / `skipped:<reason>`. The comments land in `comments/` under the round's mock-side ref tree at close.

## 49. Audit-trail commit trailers

Every workflow-transition commit carries trailers:

```
Workflow-Transition: plan_doc -> apply_doc
Workflow-Round-Slug: 202605181400-arvo-graph-csr
Workflow-Branch: round/202605181400-arvo-graph-csr
Workflow-Machine: alice-laptop.example.com
Workflow-Tool-Version: mockspace 1.2.3
Workflow-User: Alice Example <alice@example.com>
Workflow-Timestamp: 2026-05-18T14:30:00Z
Mockspace-Version: 1.2.3
```

Commit history IS the durable audit trail. Do not rely on reflog for anything that must survive a fresh clone.

In `--strict` mode, mockspace verifies every workflow-transition commit is signed; refuses to read trailers from unsigned commits.

### Trailer parsing

The trailers are parsed via git's standard trailer-block conventions: at the end of the commit message, separated from the body by a blank line, formatted as `Key: Value`. Each trailer key matches `[A-Z][a-zA-Z0-9-]*` (per RFC 5322-shaped conventions).

Mockspace adds its trailers atop any user-provided trailers in the message. The user-provided trailers (`Co-Authored-By:`, `Signed-off-by:`, etc.) are preserved.

## 50. Version compatibility

Cargo-style versioning.

- **`[mockspace].version`** declares intended major.minor.
- **`Mockspace-Version:`** commit trailer records the exact tool version that wrote each commit.

### Compatibility matrix

| Binary major | Binary minor | Status |
|---|---|---|
| same major | same or higher minor | proceed |
| same major | lower minor than declared | refuse (upgrade binary) |
| higher major | any | refuse (run `mock upgrade` after upgrade) |
| lower major | any | refuse (upgrade binary) |

### Forward-incompatible read fallback

Older binary on newer-minor refs: read operations proceed (read schema from trailer); writes refuse.

### Reading historical refs

Closed/archived refs record their writing version. Mockspace ships parsers for all historical schemas indefinitely.

### `mock upgrade`

Migrates active artefacts to a new version. Single commit on harness + round refs. The migration is mechanical: TOML schema bumps, field renames, default-value additions. The author reviews the resulting diff like any other harness commit.

### Schema evolution policy

Concrete policy for evolving the schema across minor and major releases:

- **Field addition with default.** Silent migration at load time. The default value is written into the config when the file is next saved by mockspace. No version bump required if the default preserves existing behaviour.
- **Field deprecation.** Two-minor-version warning window. Minor `N`: deprecation warning printed at load. Minor `N+1`: same warning, plus the field is recorded under `[deprecated]` if rewritten. Minor `N+2`: field is refused at load with structured diagnostic pointing at the replacement and the `mock upgrade` migration command.
- **Field rename.** Counts as deprecation of the old name plus addition of the new name. Migration script for `mock upgrade` writes the new name and removes the old.
- **Breaking grammar change (new reserved prefix, type-shape change).** Requires major version bump. Old configs are refused at load with a structured diagnostic. `mock upgrade --to-major <n>` is the migration command; without it, the binary refuses.

Migration scripts ship with the binary under an internal registry keyed by `(from_version, to_version)`. The binary always carries every migration step needed to upgrade an artefact from any historical version to the current. There is no "go install an older mockspace first to migrate" flow.

For projects that need to roll back to an older mockspace version: `mock upgrade --rollback-target <version>` writes a config that the older binary will accept. The rollback is not lossy for fields the older binary supports; fields introduced after the target version are recorded under a `[forward-state]` section that the current binary preserves but the older binary ignores.

## 51. Migration from filesystem-only mockspace

For projects currently using filesystem-based mockspace (`mock/` directory tracked in `refs/heads/*`), the migration to ref-based is one-time and automated.

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

`.git/mockspace/migrate-<timestamp>.log` (JSON Lines) records every ref created, every file preserved, success/failure per step. Each line is a discrete journal entry; line-buffered writes mean a crashed migration leaves a truncated final line, which the recovery code detects.

Migration is additive until `mock migrate finalize`. The old `mock/` directory stays on `refs/heads/*` until finalize removes it; consumers can run mockspace v2 against the new refs while the legacy directory still exists.

### Idempotency under crash and resume

Migration is a sequence of discrete steps (harness, rounds, tasks, research, finalize). Each step is a transactional unit that produces or extends one or more `refs/mock/*` orphan refs. Each step is idempotent on resume because every output is content-addressed by tree SHA.

The recovery algorithm:

1. On `mock migrate` (no subcommand) or `mock migrate --resume`, walk the journal log lines in order.
2. The last complete journal entry names the step that succeeded and its post-state (the ref SHAs it produced).
3. If the file ends with an incomplete entry: the prior step's post-state IS observed; the in-flight step had not completed.
4. For each subsequent step the user requests:
   - Compute the would-be-written ref SHA from current source `mock/` content.
   - If the ref already exists at that SHA: log "skip (already migrated)", record a no-op journal entry.
   - If the ref exists at a different SHA: refuse with `D029` (migration drift); the source `mock/` has changed since the prior migration step. User runs `mock migrate --rollback` or `mock migrate --force-redo <step>`.
   - If the ref doesn't exist: write it, append journal entry on success.

The result: re-running migration after a crash is safe; the only failure mode is "the source filesystem changed between attempts" and that surfaces structurally, not as silent corruption.

`mock migrate verify` is the dry-run shape: walks the journal, re-computes expected SHAs from current source, compares to recorded state, reports any drift without mutating refs.

### Edge cases

Partial historical rounds, naming collisions, missing files, ambiguous phase: each surfaces structured findings during `--dry-run`. The author reviews, optionally edits the source `mock/` to resolve before re-running migration.

### `main` migration timing

The shape change (`mock/` removed from `refs/heads/*`) lands via the project's normal release PR. The author chooses when (typically at a major release boundary so the discontinuity matches a public version bump).

### Old release tags

Tarballs cut from pre-migration release tags retain `mock/` content. Documented; not remediated.

## 52. Day in the life of a round

```
$ mock init                       # one-time per fresh clone
                                  # interactive trust prompts on first imports

$ mock round new ref-redesign     # creates round/<slug> + refs/mock/round/<slug>
                                  # auto-opens draft PR

$ mock topic new motivation       # opens editor on .mock/round/01_topic.motivation.md
$ mock commit -m "draft motivation topic"

$ mock topic new architecture
$ mock commit

$ mock sketch new ref-redesign cons-list-shape
$ mock sketch run mock/research/sketches/ref-redesign/01-cons-list-shape.rs
                                  # compile-probes the sketch; writes the result

$ mock bench new structural-decomposition
$ mock bench run structural-decomposition
                                  # builds variants; runs; analyses; writes findings

$ mock phase plan                 # refuses if .mock/round/ dirty
                                  # scaffolds manifest.doc.toml; transitions

# edit .mock/round/manifest.doc.toml (structured verify blocks)
$ mock commit

$ mock phase apply                # validates manifest; runs verifier in temp worktree
                                  # captures anchor; transitions; pushes round ref
                                  # forge API: opens PR

# edit doc-side files; commit on round/<slug> via git
$ mock phase finish               # transitions to PLAN(SRC)

# edit manifest.src.toml
$ mock commit
$ mock phase apply                # validates + verifier; captures src anchor

# edit source code as claimed; commit on round/<slug>

$ mock phase finish               # transitions to DONE
$ mock close                      # fetches PR comments (mandatory, resumable)
                                  # freezes round ref
```

The example shows a single-topic round. A multi-topic round inserts more `mock topic new <name>` invocations during the TOPIC phase. A round with sister-correction adds a corrective topic file mid-flow that names the topic it corrects. The remaining commands are identical.

# Part VIII. Reference

## 53. Manifest schema

```toml
mockspace_version = "1.0"
round_slug = "202605181400-arvo-graph-csr"
phase = "doc"                              # or "src"

[scope]
description = "Add CSR backend to arvo-graph; deprecate dense-matrix variant."
# in_scope_tasks accepts any valid mock:// task or step URI. Listing a
# step URI scopes the manifest to that step only; listing a bare task
# URI scopes to the task as a whole.
in_scope_tasks = [
  "mock://task/arvo::graph::csr-backend",
  "mock://task/arvo::graph::dense-matrix-deprecation",
]
out_of_scope = [
  "Renaming the graph crate (separate concern).",
]

[acceptance]
criteria = """
1. CSR backend implements the same trait surface as dense-matrix.
2. Bench `structural-decomposition` shows CSR >= 25% faster at n>=512.
3. Doc updates name CSR as the default; dense-matrix retained behind feature.
4. Every claimed file has a passing verifier at seal time.
5. mock doctor returns clean on the resulting state.
"""

# Per-file changes. Each is a claim.
[[change]]
task = "mock://task/arvo::graph::csr-backend"
file = "crates/arvo-graph/DESIGN.md"
description = "Add CSR backend section; promote CSR to default; demote dense to feature."
[change.verify]
all_of = [
  { kind = "grep_present", pattern = "## CSR backend", file = "crates/arvo-graph/DESIGN.md" },
  { kind = "grep_present", pattern = "default backend: `Csr`", file = "crates/arvo-graph/DESIGN.md" },
]

[[change]]
task = "mock://task/arvo::graph::csr-backend"
file = "crates/arvo-graph/src/csr.rs"
description = "Implement CSR backend: type, trait impls, narrow constructors."
[change.verify]
all_of = [
  { kind = "function_present", name = "Csr::new", file = "crates/arvo-graph/src/csr.rs" },
  { kind = "type_implements_trait", type_ = "Csr", trait_ = "GraphBackend", file = "crates/arvo-graph/src/csr.rs" },
]

[[change]]
task = "mock://task/arvo::graph::dense-matrix-deprecation"
file = "crates/arvo-graph/src/dense.rs"
description = "Move dense behind feature(dense-matrix); add deprecation note."
[change.verify]
all_of = [
  { kind = "grep_present", pattern = "#\\[cfg\\(feature = \"dense-matrix\"\\)\\]", file = "crates/arvo-graph/src/dense.rs" },
  { kind = "grep_present", pattern = "// DEPRECATED:", file = "crates/arvo-graph/src/dense.rs" },
]

# Required when superseding a deprecated manifest (after replan):
[[deprecated_accounting]]
file = "crates/arvo-graph/src/old_helper.rs"
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
9. **Verifier checks pass** against source-side branch tip, executed in a temporary worktree (`git worktree add --detach <temp> <tip-sha>`). All-pass-or-no-transition.
10. **Deprecated accounting complete.** Every `file` from `manifest.<phase>.deprecated.<n>.toml` must appear either as `[[change]].file` in the new manifest OR in `[[deprecated_accounting]]` with `omitted_reason`. Paths canonicalised (resolve `..`, `.`, trailing slashes, symlinks resolved at capture time and re-verified) before comparison.

Validation is all-pass-or-no-transition. On failure: structured five-element diagnostic.

## 54. Verifier catalog

Mockspace ships a strict, structured set of verifier kinds. No free shell execution. New kinds added upstream by contributing to mockspace core or to a language-specific extension.

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

### Adding new verifier kinds

Two paths exist; both preserve the closed-catalog discipline:

**Built-in kinds.** Generic kinds (`grep_present`, `path_exists`, etc.) ship in the mockspace binary. New built-in kinds require:

1. Propose the kind upstream (mockspace-core).
2. The kind is reviewed and merged.
3. Bump the mockspace-version requirement.
4. Consumers using the new kind set `mockspace_version` to the version that ships it.

**Lint-pack-declared kinds.** A pack declared under `[lint-crates]` (§5) can register additional verifier kinds via `pub fn verifier_kinds() -> &'static [VerifierKind]`. The kinds are imported alongside the pack's lints. Manifests reference them under the namespaced form:

```toml
[change.verify]
kind = "stack-lints::function-signature-match"
function = "Foo::bar"
expected_args = ["UFixed<I, F, S>", "USize"]
file = "crates/foo/src/lib.rs"
```

The trust class of a lint-pack-declared verifier kind is the same as a custom lint: native Rust code, signature-verified and SHA-pinned via the import system (§30). Manifests cannot author verifier kinds; they can only invoke kinds that the harness imported.

This is healthier than per-project shell escape hatches: verifier kinds become a shared vocabulary; each kind is implemented once and vetted; the catalog can grow without core releases via signed lint packs; no project-specific verifier-RCE surface.

### No shell escape hatch

Earlier drafts permitted an opt-in `command_succeeds` verifier behind `allow_shell_verifiers = true`. That escape hatch was removed. The structural defence is that verifier kinds are a closed, structurally-typed catalog. A project that needs a check the catalog does not yet support contributes the new kind upstream rather than reaching for a per-project shell escape.

The rationale: a shell-form verifier reachable from a PR-author-controlled manifest is a code-execution surface. Even with strict env/bins policy, the manifest itself can construct arbitrary command strings. Lockfile pinning and signing do not help here because the manifest lives in the consumer's own source-side branch and is authored fresh per PR. The only structural answer is "no free-shell at the verifier layer at all."

### Regex and parser hardening

`grep_present` / `grep_absent` use Rust's `regex` crate (linear-time, no catastrophic backtracking). PCRE-style backtracking engines are rejected at the verifier-implementation layer.

`yaml_field_equals` uses safe-load mode only; YAML custom tags and YAML anchors that reference external content are refused with a structured diagnostic.

Each verifier execution has a wall-clock budget of `verifier_timeout_seconds` (default 30s). Exceeding the budget is a verifier failure.

### `file` field constraints

Every verifier kind takes a `file` field naming a path within the source tree. The field is PR-author-controlled (it comes from manifests in the source-side branch). Mockspace applies path-traversal defence:

- The path MUST be relative to the temporary worktree's root.
- Absolute paths are refused.
- `..` segments are refused after lexical canonicalisation.
- Symbolic links that resolve outside the worktree root are refused.
- Special paths (`/dev/*`, `/proc/*`, named pipes, sockets) are refused; only regular files and directories are accepted.
- Maximum file-size budget (default 16 MB; configurable via `[verifier].max_file_bytes`); verifier short-circuits if exceeded.

Mockspace reads file contents via standard fs APIs, not via shell or external commands. There is no command-substitution evaluation on `file` values.

## 55. Findings catalog

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
| D022 | Non-pinned external import resolving to executable |
| D023 | Verifier ran against working tree with parent dirty (fallback used) |
| D025 | External mirror force-update detected; not yet accepted |
| D026 | Import package signed by unrecorded key |
| D027 | Import package signature invalid or unsigned |
| D029 | Migration journal partial or drifted |
| D030 | Import has not been observed recently (per-developer freshness) |
| D031 | Declared signing key does not match commit signature |
| D032 | Import has no recent transparency-log witness (when log configured) |
| D036 | Hook value parses as inline shell (use a `hooks/` file instead) |
| D037 | Race conflict could not be preserved on remote; local state retained |
| D038 | `.mock/` parent appears to be a cloud-sync directory |
| D039 | Lockfile drift: declared imports don't match lockfile entries |
| D040 | Anchor blob missing from `.anchor.<phase>.blobs/` storage |
| D041 | `mock://~/...` URI found outside a package-execution context |

(Findings D021, D024, D028, D033, D034, D035 are retired and not reused.)

Each finding has a five-element diagnostic message structure (invariant, situation, blocked_because, suggested_command, recovery). Auto-repair routines available for D001, D002, D008, D012, D013, D018, D023, D039 when state can be unambiguously reconciled.

## 56. Threat model

Mockspace is a tool. Its trust model is git's trust model: whoever has push access to the repository can change project state, and reviewers review diffs. Mockspace does not introduce a project-internal trust layer above git's existing access-control mechanisms.

This section names what mockspace mitigates against external attack classes (compromised hosts, MITM, malicious imports) and what is out of scope (handled by git, the forge, the developer's machine, the install channel, or the project's review process).

### In scope (mockspace mitigates structurally)

- **MITM substitution of imported package content.** Lockfile pinning by SHA plus signature verification means a host that serves different bytes than the maintainer published is detected on every fetch (D026).
- **Stale signed content served by a compromised host (freeze attack).** Optional transparency-log witnessing (when a project opts in) lets consumers detect content that signature-validates but lacks a recent cross-witness. Per-developer freshness state in `.git/mockspace/observations.toml` surfaces stale imports via D030 / D032; the freshness signal is advisory, not blocking.
- **Surreptitious maintainer key rotation.** TOFU per-developer: a fingerprint change between fetches by the same developer fires D026. The developer's explicit acknowledgement (`mock import rotate --accept-new-key`) is required. Projects using transparency logs additionally verify a log-recorded rotation entry signed by the prior key.
- **Verifier-string code execution from PR content.** Manifest verifier kinds are a closed, structurally-typed catalog. No shell escape exists at the verifier layer.
- **URI path traversal / namespace shadowing.** The URI grammar rejects `..`, normalises lexically, refuses root-escape. Reserved prefixes disambiguate scope; host names cannot collide with reserved prefixes.
- **Inline executable code in TOML.** Forbidden at the schema layer; hooks are file paths or `mock://` URIs only.
- **Concurrent multi-machine writes losing work.** Side-branch preservation is pushed to remote BEFORE local reset; D037 fires on any failure to preserve.
- **Anchor-storage path collisions.** Content-addressed storage eliminates the failure mode at the data-layout level.
- **Migration crash leaving inconsistent state.** Idempotent step recovery via content-addressed re-derivation; D029 surfaces drift.

### Out of scope (handled by git, the forge, the user)

- **Who can change project state.** Project access control (who can push to the repo, who can merge PRs, what review gates apply) is the forge's and the project's concern. Mockspace operates on whatever state lands in the repo; it does not arbitrate who is allowed to put state there.
- **Compromise of the mockspace binary itself.** The user's install channel (package manager, `cargo install`, binary download) is the root of trust.
- **Compromise of the developer's machine.** Hooks run with the developer's shell environment by design (same trust model as Cargo `build.rs`, npm scripts, git hooks).
- **Insider attacks within the team.** A team member with commit access can change any project state. The defence is review, governance, and forge access control.
- **Coordinated host + canonical-log compromise.** An attacker who controls both the package's host AND the project's transparency log can produce coherent malicious records. Cross-host federation is the practical mitigation.
- **Denial-of-service against forge or host.** Mirror federation defends against single-host outage; mockspace does not defend against coordinated DDoS.
- **Supply-chain compromise of mockspace's own dependencies.** Cargo and the Rust ecosystem provide their own supply-chain layer.

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

## 57. Crate organisation

### `mockspace` (core, language-agnostic)

- State machine, ref management, hook protocol, profile dispatcher.
- Import resolver + cache + lockfile manager.
- Signing verification (git verify-commit wrappers).
- Template renderer.
- Config parser.
- Standalone `mock` binary.
- Embedded baseline handlers and helpers.
- `mock doctor`, `mock migrate`, `mock sync`, `mock commit`, etc.

### `mockspace-config`, `mockspace-template` (extracted library crates)

Reusable library crates extracted from mockspace core:

- `mockspace-config`: serde-backed parser for `mockspace.toml`. Language-agnostic.
- `mockspace-template`: minijinja-based template rendering engine. Language-agnostic.

These two crates are the foundation that other tools (homma-like workspace orchestrators) consume. They survive any future refactor of mockspace internals.

### `mockspace-rs` (Rust toolchain extension)

For v1, **`mockspace-rs` lives inside the mockspace repo as an additional crate** rather than as a separate git repo. The "separate repo + published host" plan is a later refinement, deferred until the host contract (§28-§30) and the import resolver are exercised by a real external consumer. Keeping the Rust extension in-repo at v1 avoids cross-repo bootstrap chicken-and-egg during initial development and lets the host contract evolve with at least one consumer using it from inside.

Exports:

- `runner-rs` package for `.rs` hooks and lints.
- Rust-specific lints + agent rules.
- Rust-specific verifier kinds (`function_present`, `type_implements_trait`, etc.).
- `cargo mock` cargo subcommand alias.
- `build.rs` bootstrap convenience.

`build.rs` sketch:

```rust
fn main() {
    mockspace_rs::bootstrap::install()
        .expect("mockspace bootstrap failed");
}
```

`bootstrap::install()` owns all activation logic internally. The consumer's build.rs is one unconditional call. The function wires mockspace's integration surfaces into the calling repo:

- generates hook proxy scripts and writes them under the configured hooks directory
- writes the `cargo mock` alias into `.cargo/config.toml`
- sets `git config core.hooksPath` to point at the hooks directory
- runs the binary-path resolution chain and bakes the result into the proxies

Internal gating reads `USE_MOCKSPACE=1` as the activation switch (default off; opt-in); when unset, `install()` returns Ok without writing anything. Future changes to activation conditions land inside the function without touching any consumer build.rs.

The function name reads as "install mockspace's integration into this repo", parallel to other tooling-integration verbs (`cargo install`, `husky install`, etc.). The companion operations live alongside in the same module: `bootstrap::refresh()` re-runs resolution and rewrites stale proxies (called by `mock doctor`); `bootstrap::uninstall()` removes the integration; `bootstrap::status()` reports the current state.

`homma agent regen --repo <name>` sets `USE_MOCKSPACE=1` explicitly when running per-repo regen.

#### Invoking mock: one resolver, one call helper

Every place that needs to invoke the mock binary (hook generation, internal sub-process dispatch from within mock itself, third-party Rust callers, future tooling) goes through one shared function pair in `mockspace-rs`:

```rust
// The resolver. Single source of truth for the priority chain.
pub enum ResolvedInvocation {
    /// Resolution steps 1, 2, 3. Run this absolute path directly.
    Absolute(PathBuf),
    /// Resolution step 4. Run as `cargo mock <args>` (cargo handles lookup).
    CargoAlias,
}

pub fn resolve_invocation() -> Result<ResolvedInvocation, ResolutionError>;

// The call helper. Single point of maintenance for spawning mock.
pub fn mockspace_call<I, S>(args: I) -> Result<ExitStatus, CallError>
where I: IntoIterator<Item = S>, S: AsRef<OsStr>;
```

`mockspace_call(["doctor"])` spawns `mock doctor` using whatever the resolver returned, transparently. Any future tweak to the priority order, addition of resolution steps, or change to spawn semantics lands in these two functions and propagates everywhere automatically.

Hook generation calls `resolve_invocation()` once at write-time to bake the result into the proxy script. Internal sub-process dispatch (e.g., `mock doctor` invoking `mock refresh`) calls `mockspace_call()` directly. Both routes through the same resolver; the only difference is timing (bake-once vs invoke-each-time).

Resolution order:

1. **`MOCKSPACE_BIN_PATH` env var** if set and points at an executable file. This is the highest-precedence override. Bootstrap-generated proxies set this when launching; the running binary reads it from env and consistency is preserved across the proxy → binary handoff.
2. **`[mockspace] mock_bin_path` in `mockspace.toml`** if set. The explicit-config case.
3. **`which mock` on `$PATH`** (cargo-install, homebrew, package-manager case).
4. **`cargo mock` probe**: run `cargo mock --version` (or similar lightweight probe); on success, the hook is generated to invoke `cargo mock hook <event>` rather than an absolute path. Belt-and-suspenders for environments where mock is installed but not directly on PATH (cargo bin shim, alias-resolved location).
5. **Error**: `mock doctor` reports "Cannot resolve mock binary; set `MOCKSPACE_BIN_PATH`, set `[mockspace] mock_bin_path`, or run `cargo install mockspace`" and refuses to write hooks.

`mockspace.toml` shape:

```toml
[mockspace]
# Optional. When unset, falls through the resolution order above.
# SHOULD be relative to this mockspace.toml's directory; see "Portable
# paths" below. Absolute paths work but trigger a warning.
mock_bin_path = "target/release/mock"      # mockspace repo's own dogfood case
# mock_bin_path = "../../bin/mock"        # monorepo where mock lives elsewhere
```

The mockspace repo itself sets the explicit path to its locally-built binary. Downstream consumers normally leave the field unset and rely on the `which mock` step. Either case can override for any reason (alternate install location, sandbox, vendored binary).

#### Portable paths: relative anchored at the containing config

Any path-shaped config field that names a location *inside the repo* must be portable across clones and machines. A path like `/Users/alice/Dev/foo/target/release/mock` works on Alice's laptop and breaks on every other machine that clones the repo. The discipline is:

- **`mock_bin_path` (and any other in-repo path field) is relative to the directory containing the `mockspace.toml` that declares it.** When unset or relative, the resolver expands `mockspace_toml_dir.join(value).canonicalize()` to get the absolute path it baked into proxies or passed to spawn.
- **Absolute paths (`starts_with("/")` on Unix; drive-letter on Windows) trigger a warning at config-load time.** The value still resolves and the field still works. The warning is structured (per the diagnostic format in §53), references the field name and the offending value, and suggests the relative form.
- **External path fields (system tools, vendored binaries living outside any repo) are exempt** from the warning. The schema marks each path field as `portable: true` or `portable: false`; only the portable-true fields warn on absolute. `mock_bin_path` is portable-true.

The warning generation is one shared helper. The signature lands as something like:

```rust
/// Emit a structured warning if `path` is absolute. No-op otherwise.
/// Called from the loader for every portable-true path field.
pub fn warn_if_absolute_portable_path(
    path: &Path,
    field: &str,             // "[mockspace] mock_bin_path"
    config_path: &Path,      // path to the mockspace.toml that declared the value
    sink: &mut DiagnosticSink,
);
```

Every portable-true field hits this helper at load time. The helper is the single point of maintenance for the relative-path policy: future refinements (stricter form, suggested fix-patches, lint integration) live there, not scattered per field.

`mock doctor` runs the same check independently when it surveys the config, so the warning surfaces even on `doctor`-only runs.

Hook script template (steps 1-3 emit the absolute-path form; step 4 emits the cargo-alias form):

```sh
# Absolute-path form (resolution steps 1, 2, 3)
MOCKSPACE_BIN_PATH="/abs/path/from/resolution" exec "/abs/path/from/resolution" hook pre-commit "$@"

# Cargo-alias form (resolution step 4)
exec cargo mock hook pre-commit "$@"
```

The absolute-path form bakes `MOCKSPACE_BIN_PATH` as an env-var on the exec line so the running binary's own resolution (for any sub-invocations it spawns) sees the same path. This closes the proxy → binary → sub-process consistency: the bootstrap's resolution decision propagates through env all the way down.

The cargo-alias form does not bake `MOCKSPACE_BIN_PATH` because the alias's own resolution is what produced the working invocation in the first place; cargo handles the path lookup. Sub-processes spawned by mock when invoked this way fall through resolution steps 1-4 themselves; in practice they hit step 4 again, which is fine.

The `cargo mock` alias in `.cargo/config.toml` continues to work for interactive use, independently of how hooks are wired. Same hook generation logic, same hook protocol (§35), same on-disk hook directory layout. Everything that varies between repos is captured in the resolved path string (or in the fallback to the alias form).

`mock doctor` re-runs the resolution and rewrites stale hooks. If the binary moves (reinstall, upgrade, build mode change), `cargo mock doctor` puts the hooks back in sync.

#### Transition from v1 inside the mockspace repo

While v2 is being implemented inside the mockspace repo, neither v1's bootstrap nor v2's bootstrap should fire: v1's hooks don't recognise v2-shaped refs, and v2's binary isn't yet ready to handle the in-flight ref schema. The workspace homma gate (generated by `homma agent regen`) requires `git config core.hooksPath` to be set, however, to satisfy its 3-of-3 adoption check.

The transition bridge during v2 development: an empty `.git/v2-dev-no-hooks` directory pointed at by `core.hooksPath`. git's hook lookup finds no executables, runs commits unchecked, and the homma gate sees the config as set. No mockspace hook fires.

Once v2's `bootstrap::install()` lands (Phase 5) and a working v2 binary exists at the resolved path, setting `USE_MOCKSPACE=1` in the environment activates the bootstrap on the next `cargo check`. `install()` resolves the binary path, writes the v2 hooks, and re-points `core.hooksPath` at the hooks directory. The transition bridge retires (`rmdir .git/v2-dev-no-hooks`).

See `mock/research/202605191000_bootstrap-circularity.md` for the full design rationale, including failure modes the bridge does not cover.

#### Future: split into separate git repo

When the in-repo extension is stable and the host contract has been exercised by at least one external project, `mockspace-rs` can be split into its own git repo and published as a first-party `@/runner-rs` host. The split is a refactor that does not change the consumer-side API; `mock_bin_path` resolution continues unchanged (the binary is wherever the user installed it).

### Lint-pack ABI versioning (legacy, gradually replaced by imports)

```toml
[package.metadata.mockspace]
abi_version = "1.0"
```

Loader refuses mismatch. Mockspace ships ABI bridges for one-minor-back during the imports-system transition.

### `mockspace-ts` and other-language extensions

Future. Same shape: separate host, published exports, language-specific runner + verifier kinds.

## 58. Platform notes

Mockspace is Unix-primary. Linux and macOS are first-class.

Windows usage is supported with caveats:

- Default helpers are bash; Windows users need git bash (ships with Git for Windows).
- `flock(2)` works on Linux and macOS; Windows uses git's `.git/refs/<...>.lock` as canonical mutex fallback.
- Nested git worktrees are not used.
- CRLF: scripts in the harness ref must use LF; `.gitattributes` enforces `* text eol=lf`.
- Path separators: mockspace uses `/` internally; Windows-style `\\` is not accepted.

Windows polish is a known gap. Contributions welcome.

## 59. Future directions

### vehje as hook language

The hook protocol currently defaults to bash, with language-specific runners (mockspace-rs for Rust, mockspace-ts future for TypeScript) extending the shape.

A future evolution is to support **vehje** (a programming language being developed in the workspace this spec targets) as a hook language. Vehje is designed for boundedness and safety; if its capabilities mature to cover hook use cases (file ops, structured I/O, git plumbing wrappers), it could become a sandboxed alternative to bash for projects that opt into it.

Specifically:

- Vehje-shaped hooks could declare what they require (file-read, file-write, network, command-exec). The runtime grants only declared permissions.
- Hook scripts in `.vehje` would route through a `runner-vehje` package similar to `runner-rs`.
- Existing bash hooks continue to work indefinitely; vehje is additive.

Out of scope for v1. Noted here so future agents know the direction exists.

### Workspace-aware template fragments

Projects within a workspace family (multiple repos sharing conventions) often want shared template fragments: a license block, a workflow-description, a shared principles section. The current shape requires per-repo copies that drift.

A future feature: workspace-level template fragments imported by per-repo `mockspace.toml`. The fragments live on a workspace-level mockspace host; per-repo templates `{% include %}` them by `mock://` URI. Drift is impossible because the fragment is one source of truth.

### Multi-active-round mode

`default_one_active_round = true` is the default. Multi-active mode is named in config (`default_one_active_round = false`) but not implemented in v1. Deferred until consumer demand warrants.

### Tool extension contract

Project-local CLI extensions (a `mock <name>` sub-command surface comparable to `yarn <script>` or `cargo` aliases). The shape, discovery, runtime, sandbox, and ABI are entirely unspecified for v1. A reserved `tools/` directory at the harness ref root is documented in §22 as a future-use placeholder; v1 implementations should not write to it or assume any behaviour. Future work.

## 60. Open questions

These are real questions the spec does not yet answer. Tracked here so they don't get lost; resolved in subsequent rounds.

### Cross-machine round-ref conflict resolution: detailed UX

`mock phase resolve <slug>` is specified at high level (Part III §24). Detailed prompt flow including rebase-conflict surfacing is worth a future round.

### Verifier extensibility process

The verifier catalog grows over time. Formalising the contribution process (RFC template, review gate, version bump) is worth a future round. For v1: PRs to mockspace-core or to a language extension.

### `package.toml` `[dependencies]` semantics

Recursive resolution, version-range constraints, cycle detection, diamond conflict resolution: to be specified before this design ships in production.

Working direction (Nix-flake-style flat lockfile): imports specified by the consumer flatten into a single `mockspace.lock`; the consumer sees the entire dependency closure explicitly. Packages may declare `[dependencies]` for documentation, but mockspace does not recursively resolve them at the consumer side. The consumer adds each transitive dependency to their own `[imports]` list explicitly. The trade is "more explicit, less convenient" versus Cargo-style recursive resolution; for the small-time scope this design targets, explicitness wins.

If consumer pain warrants, a `mock import resolve <uri>` command can pre-compute the closure and suggest imports to add.

### Self-hosting timeline

Mockspace IS itself a mockspace user. Under v2, mockspace's own rounds live on `refs/mock/round/*`. Deferred until externally validated.

### Archive ref unbounded growth at scale

For 5000+ archived tasks, single archive ref's enumeration cost may require sharding (e.g., `refs/mock/task-archive/<year>`). Defer until measurement warrants.

### Migration timing for `main`

When does the `mock/` removal land on `main`? Per-project decision aligned with release cadence. The spec does not impose a timeline; each project chooses.

### Step-key TOML quoting

Step keys follow the slug charset `[a-z][a-z0-9-]{0,62}`, which includes hyphens. TOML bare keys do not allow hyphens, so `[steps.define-grammar]` requires quoting at the table-header level (`[steps."define-grammar"]` or equivalent). The §16 example uses bare-looking syntax for readability; the canonical TOML serialisation may quote keys when needed. Future round: confirm whether mockspace's TOML emitter always quotes step keys for consistency, or only when the charset requires it.

### `mockspace-rs` extraction trigger

§57 keeps `mockspace-rs` in the main mockspace repo for v1. The extraction trigger ("the host contract has been exercised by at least one external project") is qualitative. A future round should define a sharper criterion (e.g. "two external host consumers have shipped" or "host ABI has been frozen for one minor cycle") before splitting.

### `.git/mockspace/index.bin` schema

§20 names the cache file but does not specify its on-disk schema. A future round should pin the binary format (versioning, field set, evolution rules) before the storage layer ships, since the cache is per-developer regenerable but consumers will rely on its read shape.

### `in_scope_tasks` naming

§53 documents that `in_scope_tasks` accepts both task and step URIs. The field name is a slight misnomer when steps are included. A future round may rename (e.g. `in_scope_refs`) or keep the present name and treat the documentation note as sufficient. Low priority; cosmetic.

### Manifest claim field name parity with task identity

§16 manifest examples use `task = "mock://task/..."` for both task claims and step claims. The field name is "task" even when the URI is a step ref. Consistent with the `in_scope_tasks` decision above; future-revisit if discovered to be confusing.

## 61. Boundary: workspace-level tools

Mockspace is **project-scoped.** It operates on one repository.

Mockspace's only cross-repo surface: read-only consumption of external refs via `mock://ext/<host>/...`. Awareness and utilisation, not management.

A workspace-level tool is **workspace-scoped.** It operates on N repositories simultaneously, coordinating their state. (In the workspace this spec was written for, that tool is called homma; the boundary is the same regardless of name.)

### The boundary

- **Mockspace knows:** one project's refs, harness, index cache, imports, exports; read-only references to other projects' refs.
- **Mockspace doesn't know:** other projects' workflow state, "the workspace," multi-repo aggregation.
- **A workspace tool knows:** workspace membership, per-repo forge bindings, cross-repo orchestration, multi-repo migration.
- **A workspace tool doesn't know:** specific projects' mockspace internals.

### Composition

- Solo project: mockspace; no workspace tool.
- Workspace without per-project mockspace: workspace tool only; each project does its own thing.
- Workspace with per-project mockspace: both, composed. The workspace tool's per-repo aggregation reads target-2 renders (local-only outputs) from each repo's mockspace-initialised local clone.

### What the workspace tool does NOT do

- Does not author mockspace rounds. Each project's mockspace is the author of its own rounds.
- Does not edit per-project lint configuration. Each project's harness is its own.
- Does not override per-project trust acceptances. TOFU is per-developer per-machine; workspace tools don't override.

The discipline is the same as in Unix: small tools, sharp boundaries, composed by users.

---

## End

This specification ends here. Implementations have a complete contract: state machine, storage layout, URI scheme, manifest grammar, verifier catalog, hook protocol, env/bins policy, agent integration, render pipeline, CLI surface.

The accompanying audit-trail document `202605171033_ref-based-mockspace-redesign.superseded.md` is preserved in this repository as the design-process record that produced this spec. It is not required reading; it is durable record. Future maintainers reading this spec should not need to consult the superseded document to implement.

What is missing from this spec is implementation: source code, tests, packaging. Those are the next rounds.

