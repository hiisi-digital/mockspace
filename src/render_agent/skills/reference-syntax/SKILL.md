# Reference syntax

A reference is `{{ root::selector }}`, written in any `*.md.tmpl` and in any
registry field declared to hold one. The rule carries the syntax and this
project's own roots and namespaces. This is the rest: fields that hold a row,
the three functions that ask about a thing rather than render it, and how a
citation is addressed.

## Fields that hold a row

A field's declared `type` is either a builtin (`string`, `string[]`, `integer`,
`boolean`, `ref`, `ref[]`) or **the name of a namespace**, and the second form
makes the field hold references to rows in that namespace. `type = "slot"` holds
one, `type = "slot[]"` holds several.

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
