# Reference syntax

One syntax for every reference a mockspace project makes, resolved when
documents are generated.

## The form

`{{ root::selector... }}` in any `*.md.tmpl`.

| Written | Refers to | Renders as |
|---|---|---|
| `{{ reg::spike::foo }}` | a registry row | a link to it |
| `{{ reg::spike::foo::question }}` | one field of a row | that field's value |
| `{{ reg::spike }}` | a whole namespace | its table, inline |

A namespace rendering as a page writes `<KEY>.md` into the docs directory:
`vocab` becomes `VOCAB.md`. There is no prefix, because the namespace name is
already the document's subject. A namespace whose page would overwrite a
document rendered from a template is reported rather than silently winning.
| `{{ seed::DESIGN::844 }}` | a place in a file | a line link |

The braces are required, and they are not ceremony. They make a reference
something the author states rather than something the renderer guesses from a
pattern. Without them, a project with a root named `core` would silently link
`core::mem::12`, and prose about code would be rewritten by accident.

References inside code fences are never touched: an identifier in a fence is
far more likely to be an example than a reference.

An expression that resolves to nothing is left exactly as written, braces and
all, and reported. A reader sees an obviously unresolved reference rather than
a plausible-looking wrong one.

Provenance fields in registry data need no braces. The field is already
declared to hold references, so there is nothing to disambiguate.

## Identity: slugs, not numbers

A row is `namespace::slug`, where the slug is snake_case and unique within its
namespace: `vocab::xpbd`, `spike::actuator_fit_converges`.

Slugs rather than numbers because a number carries no meaning, and an
identifier carrying no meaning has to be *managed*: never reused, never
renumbered, never reordered, since any of those silently repoints every
reference to it. A slug needs none of that discipline. It says what it refers
to, it survives reordering, and it stays readable in prose.

## Roots

`reg` is the registry. `mock` is the mock directory and `live` is the
repository, both builtin, so `{{ mock::DESIGN::12 }}` works with no
configuration.

Anything project-specific is declared, because not every project has one:

```toml
[ref.roots.seed]
path = "mock/research/seed"
```

A root is a table rather than a bare string so it can gain options later
without breaking projects that already declared one.

The root name is load-bearing. A repository commonly holds several files of one
name (its own `IDENTITY.md` and a corpus containing another), and a bare
`IDENTITY.md:46` silently resolves to whichever the reader assumed. Roots
resolve from the repository root, so one row can cite design material, live
documents, and shipping code side by side, and the root name says which kind
each is. Where several apply, list them in precedence order: the first is the
one to follow, the rest are context.

## Declaring a namespace

A reference resolves only into a namespace some project declared, so the
declaration is part of this syntax rather than a separate subject.

```toml
[[registry.namespace]]
key = "spike"
title = "Spikes"
description = "A focused implementation that answers a question."

[[registry.namespace.field]]
name = "question"
type = "string"
required = true
description = "The question the spike answers, in one line."
```

A field's `type` is `string`, `integer`, `boolean`, `string[]`, **`ref` and
`ref[]` for a field that holds citations**, or **the name of a namespace, for a
field that holds rows of it**. A citation and a row reference are both strings on
the wire, so both validate as strings; what the type adds is meaning, and it is
what makes the field get checked at all.

A type naming neither a builtin nor a declared namespace is reported. It used to
become a plain string, so a misspelled type, or one naming a namespace that was
never declared, produced a field that read as constrained and was not.

**Name the field whatever the subject calls it.** Reference validation was once
keyed on the literal name `provenance`, which checked one field for one consumer
and silently ignored every other reference-bearing field a project declared. A
project may now declare several, called `rests_on`, `supersedes`, `evidenced_by`
or anything else, and each is validated because its type says what it is.

### A field that holds rows

Declare the field's type as the namespace it points into. The singular form holds
one, the `[]` form holds several.

```toml
[[registry.namespace.field]]
name = "slot"
type = "slot"
description = "The slot this answers."

[[registry.namespace.field]]
name = "supersedes"
type = "ruling[]"
description = "Rulings this one replaces."
```

```toml
[[answer]]
id = "niri"
slot = "display"          # a bare slug, never "slot::display"
```

**The value is a bare slug.** The type already names the namespace, and writing it
again in every value is a second place for the two to disagree; a qualified value
is refused rather than accepted as a second spelling, because one thing written two
ways has to be normalised everywhere it is read and the normalisations drift.

A slug naming no row is reported, with the nearest existing slug named where there
is a near one, so a typo and a genuinely absent row do not read alike.

A namespace whose key is also a builtin type name is refused at the declaration. A
field declaring that type would mean the builtin, silently, and nothing could ever
reference the namespace.

### Asking what points at a row

`{{ refsto(x) }}` is the inverse, derived from the typed fields rather than stored,
so there is no second list to keep in step. A row states what it points at; what
points at it is computed.

This is the direction most questions are asked in. Where a namespace enumerates
what a project must answer, `refsto` over one of its rows is how a reader finds out
whether anything answers it, and **an empty result is the finding** rather than an
absence of one. A row that does not exist does not resolve at all, so a typo cannot
read as a gap.

`key` is the array-of-tables name, singular, and it is the first selector
segment of every reference into the namespace. A file holding `[[spike]]` rows
validates against the spike schema wherever it sits, so a project files by
subject rather than by kind and the registry stays queryable by kind.

**A row's identity is its slug**, under the `id` key, snake_case and unique
within its namespace. Slugs rather than numbers because a number carries no
meaning and an identifier carrying no meaning has to be managed: never reused,
never renumbered, never reordered, since any of those silently repoints every
reference to it.

**There is no `prefix`.** An earlier design derived an identifier pattern from a
per-namespace prefix, so a row was `SPK-042`. That is gone: the schema generator
emits the slug grammar directly, and a project still declaring `prefix` has it
silently ignored, because the namespace struct no longer carries the field.

The older document describing the prefixed form is
`docs/research/202607181700_registry-of-terms-design.md`. It is kept as the
record of how this was reasoned, and it is superseded on **two** points.

**Identity**, by this section: a row is a slug, not a prefixed number.

**How a reference is recognised**, by the form at the top of this document. That
design specifies pattern-scanning, where "a bare `SPK-042` becomes a link". This
one requires braces, and says why: without them a project with a root named
`core` would silently rewrite prose about `core::mem::12`.

No claim is made here about the rest of it.

## Anchors: prefer headings to line numbers

`{{ seed::DESIGN::#the-four-lanes }}` cites a heading. `{{ seed::DESIGN::844 }}`
cites a line.

Prefer the heading, because **line numbers fail silently**. An edit anywhere
above a cited line shifts it, the citation still resolves, and it now points at
different content. The check passes and the answer is wrong, which is the worst
failure shape there is. The only case that fails loudly is a line past the end
of the file, and that is the case that matters least.

A heading fails loudly instead. Rename it and the citation stops resolving,
which is a report rather than a lie. Heading slugs match the form forges
generate, so the same anchor a reader clicks is the one the link produces.

Line citations are honest in exactly one situation: a root whose contents do not
move. Declare that:

```toml
[ref.roots.seed]
path = "mock/research/seed"
frozen = true
```

A line citation into a root not declared frozen is reported. Freezing a root is
a claim that its files are settled, and it is what turns a line citation from a
hazard into a fact.

## Paths

A citation's path may have any depth, and the last segment is always the line:
`{{ mock::crates::numeric::DESIGN::12 }}` needs no root of its own.

Extensions may be omitted. `{{ mock::DESIGN::12 }}` finds `DESIGN.md.tmpl`
without the author tracking whether a document is a template here and rendered
output there. Exactly one match resolves; several is an error rather than a
guess, because the author meant one of them and picking silently would point
the citation somewhere they did not choose.

## What is checked

The same validator runs over registry data and over prose, so a citation cannot
rot in one and pass in the other.

Reported: a reference to a row nothing declares; a field a row does not carry;
a slug declared twice in one namespace, which no per-file schema can catch
because each file is valid alone; a citation that does not parse; a root nobody
declared; a citation matching several files; and one whose line is past the end
of its file. A row reference naming no row, or written qualified rather than as a
bare slug. A field type naming neither a builtin nor a namespace, and a namespace
whose key shadows a builtin type.

Everything a JSON Schema can express (required fields, slug shape, types,
unknown keys) is checked by running the generated schema through a TOML
validator instead. Two implementations of one contract drift, and the schema is
the one an editor already uses.
