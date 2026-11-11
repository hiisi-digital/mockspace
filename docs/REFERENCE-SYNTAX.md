# Reference syntax

One syntax for every reference a mockspace project makes, resolved when
documents are generated.

## The form

`{{ root::selector... }}` in any `*.md.tmpl`.

What stands between the braces is a path in the letkajenkka standard (`foo::bar::baz`),
read by the `letkajenkka` crate, through the `refpath` module of `mockspace-registry`. The standard fixes the
shape and the character set; mockspace fixes what the roots, namespaces and fields mean,
and folds its own names into the standard's character set where it resolves.

| Written | Refers to | Renders as |
|---|---|---|
| `{{ reg::spike::foo }}` | a registry row | its slug, as plain text |
| `{{ reg::spike::foo::question }}` | one field of a row | that field's value |
| `{{ reg::spike }}` | a whole namespace | its table, inline |
| `{{ seed::design::844 }}` | a place in a file | a line link |

The registry writes no page into the docs directory. A namespace is read, validated and
queried, and a table of it is written wherever a template asks for one with
`{{ reg::spike }}`, in that template's own document. A row
reference has no document to link to, so it renders as the row's slug. A page an
earlier version wrote is removed by the sweep of `docs/`. Row text is checked where a
template or a table renders it; a row nothing renders is not scanned for references
that resolve to nothing.

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

A row's whole identity is that pair, its key path. The files a registry is
written in are no part of it: any number of `*.toml` files, under any names and
in any directories under `registry/`, make one registry, and splitting,
merging, renaming or moving a file changes nothing a reader, a check or a
rendered document sees, except `pathof`, whose answer is that file. A namespace
lists its rows by key path rather than by the order of the files holding them,
and a grouped table orders its groups by the group value, with rows that have
no group value last under a heading of their own. The same key path in two files is an
error naming both, and a file and line in a report is only a place to look.

## Roots

`reg` is the registry. `mock` is the mock directory and `live` is the
repository, both builtin, so `{{ mock::design::12 }}` works with no
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

A field's `type` is `string`, `integer`, `boolean`, `string[]`, `date`, `time`,
`datetime`, `span`, **`ref` and `ref[]` for a field that holds citations and links
to rows of any namespace**, or **the name of a namespace, for a field that holds
rows of it**. A citation and a row reference are both strings on the wire, so both
validate as strings; what the type adds is meaning, and it is what makes the field
get checked at all.

Four builtins hold a moment or a length of time, each a string held to one form,
which the generated schema carries and the gate enforces on every row: `date` as
`2026-10-28`, `time` as `18:49` with seconds and a fraction of one where wanted,
`datetime` as a date and a time joined by `T` or a space with an offset (`Z`,
`+02:00`) where the moment is fixed, and `span` as an ISO 8601 duration with at
least one part, `P3D` or `PT1H30M`. The form is checked and the calendar is not, so
`2026-02-30` passes. The value is written quoted: a bare TOML date such as `when = 2026-10-28` is a different
TOML type, which the schema cannot tell from a string, and the loader refuses it as
`unquoted-time`.

A type naming neither a builtin nor a declared namespace is reported. It used to
become a plain string, so a misspelled type, or one naming a namespace that was
never declared, produced a field that read as constrained and was not.

A field may also say which values it takes. `values` lists a closed set, written
as strings and held in the field's own type: the generated schema makes it an
`enum` of strings on a string field, of numbers on an `integer` and of `true` and
`false` on a `boolean`, so a value outside it is refused where it is typed. On an
`integer`, `min` and `max` give its range, each optional, which the schema makes
`minimum` and `maximum`; with `values` beside them, both hold. Each of these is
reported at the declaration where the schema could not hold it: a bound on any
other type, a `min` above its `max`, and a listed value that does not read as its
field's type, an integer not written plainly (`+5`, `05`), one outside the
field's own range, or one of a `date`, `time`, `datetime` or `span` field not in
the form its time pattern gives it. Left to the rows, each would
fail at every row naming the row rather than the declaration, or read as held
while nothing held it. An editor drawing the registry reads the same keys: a
closed set is a choice, whatever its type, and an integer with a range and no
closed set a slider.

```toml
[[registry.namespace.field]]
name = "priority"
type = "integer"
min = 1
max = 5
description = "How soon, from 1, now, to 5, whenever."
```

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
is a near one, so a typo and a genuinely absent row do not read alike. It also keeps
its reference form wherever it is rendered, rather than vanishing, so the
unresolved-token check reports it a second time in the generated document.

A namespace whose key is also a builtin type name is refused at the declaration. A
field declaring that type would mean the builtin, silently, and nothing could ever
reference the namespace.

**A namespace declaring `value_field` cannot be referenced this way**, and that is
refused at the declaration too. A reference into such a namespace renders that
field's text rather than the slug, which is right for a constant stated once and
wrong for a relation: the reference exists to name a row, and the text arrives in a
table cell after the cell's own escaping has run, so a value carrying a pipe ends
the column early.

**A reference to a row gives its slug as plain text**, in a table cell and in a
`{{ ns::slug }}` in prose alike, because the registry writes no page for it to link to.

### One field for a relation

A relation between rows goes in one field typed `ref[]`, whichever namespace the
other row is in. An item of it is one of two shapes that cannot be mistaken for each
other: a citation has a root, a path and an anchor, so three segments or more
(`research::dir::file::heading`), and a link to a row has two, a declared namespace
and a slug.

```toml
[[registry.namespace.field]]
name = "related"
type = "ref[]"
description = "Every row this one relates to, written as namespace::slug."
```

```toml
[[answer]]
id = "niri"
related = ["slot::display", "ruling::wayland_first", "research::notes::niri::tiling"]
```

The first segment of a link is checked against the declared namespaces, and a slug
naming no row is reported as `unknown-row-reference`, a slug that is not a slug as
`malformed-row-reference`. Only the first is scoped: it gates the commit or push that
changes the file holding the link, and is a warning on a plain run or one that leaves
that file alone, so a row can be retired before every citation of it is gone. A
template citing a row is scoped the same way, to the template. A slug that is not a
slug, and every other finding, gates every run. A root cannot carry the name of a
namespace
(`namespace-root-collision`), so a two-segment item is a link exactly when it names
one. In a table cell a link gives the row's slug as plain text, as any reference to
a row does.

**A field per target namespace is the wrong shape.** A schema with a field for each
namespace a row can relate to is a list of places a relation could be written, and a
relation written in two of them says the same thing twice. The link is written once,
on one row; the other end is derived, by `refsto` and by `cargo mock links
<namespace>::<slug>`, which lists both ends of a row's links with the field each one
is written in.

### Asking what points at a row

`{{ refsto(x) }}` is the inverse, derived from the typed fields and from the links
held in `ref` fields rather than stored, so there is no second list to keep in step.
A row states what it points at; what points at it is computed.

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

`{{ seed::design::the_four_lanes }}` cites a heading. `{{ seed::design::844 }}`
cites a line, and `{{ seed::design::844..860 }}` a span of lines.

Prefer the heading, because **line numbers fail silently**. An edit anywhere
above a cited line shifts it, the citation still resolves, and it now points at
different content. The check passes and the answer is wrong, which is the worst
failure shape there is. The only case that fails loudly is a line past the end
of the file, and that is the case that matters least.

A heading fails loudly instead. Rename it and the citation stops resolving,
which is a report rather than a lie. A heading is named by the slug a forge gives it, folded into the one
character set (`-` becomes `_`), so `The four lanes` is `the_four_lanes`, and the link
rendered for it carries the forge's own slug, `the-four-lanes`, so the anchor a reader
clicks is the one the link produces. A heading that opens on a digit has no such name
and cannot be cited by heading.

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

A citation's path may have any depth, and it ends in an anchor, a line, a span of
lines or a heading: `{{ mock::crates::numeric::design::12 }}` needs no root of its own.

The syntax is the letkajenkka standard, one parser for every reference, and its
character set is lowercase letters, digits and underscores, opening on a letter. A
name is never written with an uppercase letter, a hyphen, a dot or an extension: the
host folds its own naming into that set. `{{ mock::design::12 }}` finds
`DESIGN.md.tmpl` without the author tracking whether a document is a template here and
rendered output there, and `my_crate` is the directory `my-crate`. Exactly one match
resolves; several is an error rather than a guess, because the author meant one of
them and picking silently would point the citation somewhere they did not choose. No
spelling tells two such files apart, so one is renamed or removed. A file or directory
whose name cannot fold, because it opens on a digit or holds a dot, cannot be cited.

The spelling before the standard (uppercase, extensions, `::#heading`, spaces around
the separator) is refused, and the refusal names the migration that rewrites it:
`cargo mock migrate references-to-letkajenkka [--dry-run]`. It rewrites a reference
only where the new spelling resolves to the file and line the old one did, and lists
the rest. The same migration folds each preset shorthand of `extends` in the
`[lints.<name>]` tables of `mockspace.toml` into the standard's names (`stack-lints::no-heap`
becomes `stack_lints::no_heap`), and it reads a registry kept on a ref through the
registry's own stores and writes it back to that ref, never into the tree.

## What the standard leaves to mockspace

`refsto` is an operation the standard reserves, and `pathof` and `sourcesof` are
mockspace's own, each taking one query: `{{ pathof(mock::design::12) }}`. The postfix
chain after a finished operation, `{{ pathof(crates::store).dir() }}`, is mockspace's too
and is no part of the standard's grammar, which has no step after an operation. The
standard names sets, wildcards and constraints; a reference here is a plain path of
names, so each of those is refused where a row or a citation is expected.

## What is checked

The same validator runs over registry data and over prose, so a citation cannot
rot in one and pass in the other.

Reported: a reference to a row nothing declares; a field a row does not carry;
a slug declared twice in one namespace, which no per-file schema can catch
because each file is valid alone; a citation that does not parse under the standard; a root nobody
declared; a citation matching several files; and one whose line is past the end
of its file. A row reference naming no row, or written qualified rather than as a
bare slug. A field type naming neither a builtin nor a namespace, and a namespace
whose key shadows a builtin type.

Everything a JSON Schema can express (required fields, slug shape, types,
unknown keys) is checked by running the generated schema through a TOML
validator instead. Two implementations of one contract drift, and the schema is
the one an editor already uses.
