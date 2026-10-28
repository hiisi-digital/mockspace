# Reference syntax

A reference is `{{ root::selector }}`, written in any `*.md.tmpl` and in any
registry field declared to hold one. The rule carries the syntax and this
project's own roots and namespaces. This is the rest: fields that hold a row,
the three functions that ask about a thing rather than render it, and how a
citation is addressed.

## Fields that hold a row

A field's declared `type` is either a builtin (`string`, `string[]`, `integer`,
`boolean`, `ref`, `ref[]`, `date`, `time`, `datetime`, `span`) or **the name of a
namespace**, and the second form makes the field hold references to rows in that
namespace. `type = "slot"` holds one, `type = "slot[]"` holds several.

The four time types are strings held to one form each, which the gate enforces on
every row: `date` as `2026-10-28`, `time` as `18:49` (seconds and a fraction of one optional),
`datetime` as a date and a time joined by `T` or a space with an optional offset
(`2026-10-27T18:49:00Z`, `2026-10-27 18:49`), and `span` as an ISO 8601 duration
with at least one part (`P3D`, `PT1H30M`). The form is checked, not the calendar.
Write the value quoted: a bare TOML date (`when = 2026-10-28`) is refused as `unquoted-time`.
A namespace may not be named after any of them.

The value is a bare slug, `"display"`, never `"slot::display"`: the type already
says which namespace, and one thing written two ways is one thing that can
disagree with itself.

Three things are reported rather than passed:

- **A slug naming no row**, so a relation cannot rot the way a hand-maintained
  list does.
- **A type naming neither a builtin nor a namespace**, rather than quietly
  becoming a string field that constrains nothing.
- **A target declaring `value_field`**, which renders a value rather than a link
  and so cannot carry a relation.

## The values a field takes

`values` lists a closed set, as strings, held in the field's own type: the schema
makes an `enum` of strings, of numbers on an `integer` and of `true` and `false`
on a `boolean`. On an `integer`, `min` and `max` give its range, each optional;
with `values` beside them, both hold. These are reported at the declaration
rather than left to fail at every row:

- **A bound on a field that is not an `integer`**, or a `min` above its `max`,
  as `misplaced-field-bound`.
- **A listed value no row can hold**, one that does not read as its field's type,
  an integer not written plainly (`+5`, `05`), or one outside the field's own
  range, as `unfit-field-value`.

## The three functions

Each answers a question about a thing rather than rendering it.

`{{ pathof(x) }}` is where x is **declared**, the file to open to change it: a
crate's directory, a cited file, or the TOML a registry row sits in.

`{{ sourcesof(x) }}` is what x **rests on**, its provenance, plural because
provenance is an array. The root name carries what each reference means: the
corpus a row was derived from, the live document that supersedes it, the code
that implements it. Array order is precedence, so the first is the one a reader
should follow and the rest are context.

`{{ refsto(x) }}` is what **points at** x, derived from the typed fields above
rather than stored, so nothing has to be kept in step. It is the direction most
questions are asked in, and an empty answer is the finding: nothing answers that
row.

## Narrowing a result

A postfix chain narrows what a function returned:

```
{{ pathof(crates::store).dir() }}
```

Four methods read a path (`dir`, `filename`, `stem`, `ext`) and three read a list
(`first`, `last`, `count`), applied left to right. An unknown method is reported
rather than ignored, because a method that silently does nothing reads as one
that worked.

## Addressing a citation

A citation is `root::path::anchor`. The anchor is a heading (`#the-four-lanes`)
or a line number, and **the heading is the one to prefer**. A line number fails
silently: an edit above it shifts the target, the citation still resolves, and it
now points at different content. A heading fails loudly when renamed, which is a
report rather than a lie. Line numbers are honest only in a root declared frozen.

A path may have any depth and the extension may be omitted, so `mock::DESIGN::12`
finds `DESIGN.md.tmpl` without anybody tracking which form exists where. Two
matches is an error rather than a guess.

A namespace is addressed by its own name: `law::keys`, `vocab::xpbd`. There is no
prefix, because slot zero is either a declared root or a declared namespace and
the two cannot collide. The older `reg::law::keys` still resolves.

## Slugs, never numbers

Registry rows are identified by a snake_case slug. A number carries no meaning
and so has to be managed: never reused, never renumbered, never reordered, since
any of those silently repoints every reference to it.

## A row is its key path, never its file

A row is `namespace::slug`, and that is all of its identity. Any number of
`*.toml` files, under any names and in any directories under `registry/`, make
one registry, so splitting, merging, renaming or moving a file changes nothing a
reference, a check or a rendered page sees, except `pathof`, whose answer is that
file. A namespace lists its rows by key path, not by the order of the files
holding them. A grouped table orders its groups by the group value, and rows
with no group value come last under a heading of their own. The same key path
in two files is an error naming both, and a file and line in a report is where to look, never
who the row is. Name a row by its key path, and never write a rule or a check
that depends on which file it sits in.

## Merging registry files

Two branches that each add a row to the same place in a registry file do not
conflict in a `git merge`. `.gitattributes` sends every `*.toml` under
`{mock_dir}/registry/` to the `mockspace-registry` merge driver, which `cargo mock`
registers in the clone and which runs `cargo mock merge-registry`. It keys a row
by its `[[namespace]]` and `id`, as the loader does, and never by the file it
sits in or its position:

- a row added on one side or both under different ids is kept, each side's own
  order preserved, and the result is the same whichever side is ours;
- a row identical on both sides is kept once;
- a row changed on one side takes that change, and one deleted on one side and
  untouched on the other is deleted;
- a row changed differently on both sides, deleted against a change, or added
  twice with different content is a conflict, written with markers around that
  row alone. A head, or a plain `[table]`, both sides changed is one too. Resolve
  it keeping both intents, never one side.

A file the driver cannot read as rows (invalid TOML, a row with no `id`, an `id`
twice in one file) merges as text, as git does without a driver. The driver works
inside one file: the same `id` added under two different files is two additions
to it, and the registry's duplicate check reports it afterwards.
