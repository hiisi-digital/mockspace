# A registry of terms and concepts, resolved at doc-generation time

**Date:** 2026-07-18
**Designed for:** mockspace v2, where it lands as a first-class feature.
**Prototyped in:** v1, the shipping tool in `src/`, as a proactive backport.

## Which project this is for, and why it is built twice

This repository carries two mockspace projects at once. **v1 is the live tool consumer repositories run
today**: the crate at the repository root, source in `src/`, developed without the design-round ceremony. **v2
is the decomposed rewrite** under `mock/crates/`, and it owns the rounds in `mock/design_rounds/`.

v1 is at the end of its road and is marked for deprecation once v2 is ready to replace it. Features added to
it now are deliberate backports of v2 features, built early so they can be test-driven across real
mockspace-accelerated repositories before they harden.

That inverts what matters. **The design is the durable artifact; the v1 implementation is the experiment.**
v1 stability outside our own use is explicitly not a goal for now, so the implementation may move fast and
change shape. What must be right is the model, because the model is what v2 inherits, and a model is far more
expensive to change once it is first-class than while it is a prototype.

The practical consequence for anyone working on this: do not spend effort on v1 migration paths, compatibility
shims, or defensive handling of shapes we can simply change. Do spend effort on whether the model is correct,
and treat every awkwardness encountered while using it in a real repository as design feedback rather than as
a v1 wart to work around. Those awkwardnesses are the entire reason to build it here first.

An earlier draft of this document was filed as a v2 design round. That would have set v2's phase gate, since
mockspace derives phase from flat files in `mock/design_rounds/`, for work belonging to neither project. The
v1-versus-v2 distinction is easy to miss and worth stating before anything else.

## The problem, from the consumer that surfaced it

A project accumulates identifiers for the things its documents refer to: probes, measurements, constants,
invariants, vocabulary. Nothing in mockspace holds them, so each project invents a convention per document
family, and the conventions drift.

The consumer that surfaced this had a design corpus using several at once. Claims were bracketed with a phase
number and a letter run. Probes and measurements shared one numeric space and were told apart only by a
parenthetical inside the line, so a reader could not answer "what are all the measurements" without reading
several hundred lines and judging each. Open questions carried three different prefixes depending on which
section asked them. Lanes, constants, and invariants had prose names and no identifiers at all.

Building a local registry to fix that found real defects in the corpus within hours: an identifier cited twice
and defined nowhere, a document contradicting its own ruling, a table referred to eight times that did not
exist, and a set of rules whose violations were known to be silent and which nothing checked. None of those
were visible to ordinary reading, because **a gap is only visible against an enumeration.**

That is the general case. Any project whose documents refer to named things benefits from being able to
enumerate them, and gets defect-finding as a side effect of the enumeration existing.

## Why this belongs in mockspace rather than in the consumer

The feature is close to useless without the document generator, and mockspace owns the document generator.

Two of the three capabilities only exist at render time. Turning every `SPK-042` in prose into a link to that
row's definition is a rendering act. Inlining a constant's value where a document references it is a rendering
act. A consumer building this alone must either reimplement generation or post-process its output, which is
the reinvent-in-the-consumer shape the workspace forbids.

The third capability, validation, wants the lint framework that already exists here.

Everything the feature needs is already present: TOML parsing, the lint pack, placeholder substitution, and
`render_passthrough_templates` as the single point every `*.md.tmpl` flows through.

## The shape

### Layout: free-form, arbitrarily nested, schema chosen by content

Registry data lives under `<mock>/registry/`, arbitrarily nested, with **every** `*.toml` beneath it loaded.
Schemas live in `<mock>/registry/.schemas/` and are generated rather than authored. No glob collision: data is
TOML, schemas are JSON.

**A file's schema is chosen by the array-of-tables key it contains, never by its path.** A file holding
`[[spike]]` rows validates against the spike schema wherever it sits.

That choice is what makes the nesting genuinely free, and it buys an organisation the path-driven alternative
forbids: a project can file by subject rather than by kind. A `registry/domains/water.toml` may hold
`[[spike]]`, `[[bench]]`, and `[[constant]]` rows that are all about water, and the registry remains queryable
by kind because the key drives it. Path-driven schema selection would force `spikes/`, `benches/`,
`constants/` forever, which is the one organisation that cannot express a cross-cutting subject.

It also dissolves the file-size problem: five hundred rows of one kind split across as many files as the
author wants, with no configuration.

### Declaration: namespaces in `mockspace.toml`

```toml
[[registry.namespace]]
key = "spike"                  # the [[spike]] table name
prefix = "SPK"                 # identifier prefix; id pattern derives from it
title = "Spikes"
description = "A focused implementation that answers a question."

[[registry.namespace.field]]
name = "question"
type = "string"
required = true
description = "The question the spike answers, in one line."
```

`id` is the only universal field. Its pattern is derived from the prefix, so a malformed identifier is a
schema error rather than a convention nobody checks.

**Deliberately not universal: provenance, status, ownership.** Those are the first consumer's fields, because
that consumer has an external oracle its registry indexes into. A project with no oracle owes no provenance.
Baking them in would shape the general feature around one consumer, which is the failure mode this section
exists to avoid.

### Generation: schemas and editor support, not hand-maintained

mockspace generates `.schemas/<key>.schema.json` per namespace and the tool configuration that binds them.

This is not a convenience. The first consumer hand-wrote eleven schemas sharing a common definition block, and
the TOML language server could not resolve cross-file references, so the shared block had to be duplicated
into all eleven. Eleven hand-maintained copies of one definition is precisely the second-copy drift the
discipline forbids everywhere else. Generating them makes the duplication free and unable to drift.

The payoff beyond validation is editor support: completion, hover documentation, and inline errors, all from
the descriptions the project already wrote in its namespace declaration.

### Resolution: references become links, constants become values

At generation, every rendered document is scanned for identifiers in a declared prefix.

A bare `SPK-042` becomes a link to that row's entry in the generated registry page. A reference to a row that
does not exist is a lint error, which is the dangling-reference check that makes the registry trustworthy
rather than aspirational.

A namespace may declare a `value_field`, and a reference in value position renders that field inline instead
of linking. A document then states a constant once, in the registry, and every mention of it renders the
current value. This is the capability that stops constants being copied into prose and drifting.

### Lints

Dangling reference (an identifier is referenced and not defined) and duplicate identifier are errors: both
mean the registry is lying. Orphan (defined and never referenced) is a warning at most, since a registry row
may legitimately exist before anything cites it.

Status fields state what is true by construction rather than workflow state, because reference rot has one
cure: stop maintaining status bits and derive them.

## What this is not

Not a replacement for the project's own design corpus. The registry is an index into it. Every row may name
where it came from, and the index never becomes the truth.

Not a glossary renderer. A glossary is one possible namespace, not the model.

Not a task tracker. `status` describes the thing, never anyone's progress on it.

## Open questions for the DOC phase

How much schema expressiveness belongs in the TOML declaration before it becomes JSON Schema written badly in
another syntax. The proposal above covers scalar types, requiredness, and descriptions; enums and arrays are
the first things a real consumer will want, and an escape hatch to a hand-written fragment may be the honest
answer for anything past that.

Whether the generated registry pages are one per namespace or one combined document, and whether a project can
override their placement.

Whether reference resolution applies inside code fences, where an identifier is more likely to be an example
than a reference. The default should probably be no.

## First-party feedback, from using it in a real consumer

The point of prototyping here is to find what is wrong with the model while it is still cheap to change.
Recording what real use surfaced, rather than fixing each one reflexively, because some of these are model
questions rather than bugs.

**Structured field values render as raw TOML.** A provenance field holding an array of inline tables renders
in the generated table as `{ file = "IDENTITY.md", line = 46 }`. It is honest and it greps, but it is not what
a reader wants, and the obvious fix is wrong: teaching mockspace that a table with `file` and `line` keys
should render as `file:line` would bake one consumer's schema into the general feature.

The general shape is probably a per-field render hint in the namespace declaration, something like
`format = "{file}:{line}"` applied to each element. That is a real design question and it should be answered
once, deliberately, rather than by special-casing the first shape encountered.

**Rows carry structure that a table flattens away.** The same field is an array in the data and a
comma-joined string in the table. For provenance, ownership, and members, the array is the truth and the join
is a presentation compromise. This is an argument that the generated page should not be the only view: the
data is richer than a table, and a project may eventually want a per-row page for namespaces with real depth.

**The `id` column bug is worth remembering as a category.** It read from the row's field map rather than from
the row's own identifier, and it worked only because the loader happens to put the identifier in both places.
A test using a hand-built row rather than a loaded one caught it. The lesson generalises: tests that build
fixtures by hand find assumptions that tests going through the loader cannot, because the loader's
conveniences hide them.
