# The canon, design, code chain

Three tiers, and they govern each other in one direction only. The rule states
what they are and the order they may change in. This is the reasoning under it:
where the canon actually lives, what each tier owes, and how to tell which one a
document belongs to.

## The canon is the registry

The canon is typed rows in `{mock_dir}/registry/`, in namespaces the project
declares. Not prose, not a directory of documents: rows with declared fields, a
snake_case slug each, and provenance saying what every row rests on.
`canon_paths` in `mockspace.toml` is where the project says which of that is its
canon, and it is what `mock check` refuses a write to while a panel is open.

Rows rather than prose because a canon is read in every direction and prose only
reads in one. A typed field can be pointed at from a design, counted, checked for
a slug that names nothing, and rendered into twenty documents that cannot drift
from each other. A paragraph can do none of that, and the drift is silent every
time.

`{canon_location}` is the reading surface, not the authority. It holds `.md.tmpl`
documents that pull the canon in with `{{ <ns> }}` for a whole namespace or
`{{ <ns>::<slug>::<field> }}` for one field, so the canon reads as a document
and prints as a PDF without anybody transcribing it. **Those documents
are a view.** Changing what the canon says means changing rows; editing the view
changes nothing and is overwritten the next time documents generate.

**So a canon file is `.md.tmpl` and it is generated from rows.** A `.md` sitting
there holding canon prose that no row backs is the failure this shape exists to
prevent: it reads exactly like canon and nothing points at it, nothing checks it,
and nothing else renders from it.

## Where a superseded canon goes

Canon is never deleted, only demoted. A superseded row stays referenceable so the
next canon can be built with the old one in view, and that is the one exception
to nuking, because the canon is the only tier carrying reasoning rather than
consequence.

Provenance is how the relation is written down. A row's `provenance` is an array
of references and the root name carries the meaning: the corpus a row was derived
from, the live document that supersedes it, the code that implements it. Array
order is precedence, so the first reference is the one a reader should follow and
the rest are context. A project naming its roots `seed`, `mock` and `live` gets
three readable kinds of citation without inventing a field for each.

## The reproduction property, which is where the order comes from

Nuke the code and lose nothing: the design says what the code was, so the code is
a mechanical transcription and nothing more. Nuke the design and lose little: an
equivalent design can be written from the canon, and it may differ from the
original and still be valid. The canon is the only tier that is not reproducible
from anything above it.

Two acceptance tests follow. A design is good enough when two implementers,
reading it independently, produce working implementations of the same thing. A
canon is good enough when two designers, reading it independently, produce
designs that yield equivalent working units.

## The mutation order, at length

To change the code: nothing has to be nuked first, because code is the leaf and
nothing depends on it. That is the only sense in which it is free, and "just
change it" overstates it into no constraints at all. Two still bind it, and
neither belongs to the mutation order. The round ceremony applies in full: topic,
doc changelist, lock, source changelist, lock, close; the phase gates enforce it.
And nothing may appear in code that is not in the design. A change that
introduces something the design does not say is not a code change at all. It is
an undeclared design change wearing the leaf tier's freedom, while actually
mutating the tier above, and it is the most common failure here precisely because
it does not feel like editing a design at the moment it happens.

The leaf is unconstrained downward and fully constrained upward. That
generalises: each tier is unconstrained toward what depends on it and constrained
by what it depends on, the same statement the mutation order makes about the
tiers above it, from the other direction.

To change a design: the code under that design is nuked first, not migrated, not
adapted, then rewritten from the changed design.

To change a canon row: every design that declares that row is nuked first, and
therefore the code beneath those designs. Not every design in the project, only
the declared dependents. The declaration each design carries is what makes that
scoping possible; without it, the only honest blast radius would be everything.

Two consequences follow from the same reasoning, and neither is an exception
carved into the rule. Adding a new row nukes nothing, since nothing declares it
yet. Adding a field to a row also nukes nothing: the trigger is invalidation, not
editing, and a purely additive change leaves every prior statement standing, so a
design already derived from that row is still derivable from it as it now reads.
Changing what a field says, or removing one, invalidates, and that row's declared
dependents go.

Row granularity is the right unit because one row is one thing: one aspect of the
canon, the way a chapter works in academic literature. A row carrying two
unrelated subjects drags an unrelated design into every nuke it never actually
depended on. The fix for that is splitting the row, not refining the granularity
below it.

A lower tier that survives a change above it becomes a claim about something that
no longer exists. It still gets read, and it still gets defended, because it is
concrete and detailed and looks authoritative next to the abstract statement that
replaced it.

## What this means for canon work

Declaring a canon row's dependents stale is not a quality complaint. It is the
precondition the mutation order requires: while a design that declares a row is
live, that row is frozen, and nuking its declared dependents is what unfreezes
it. An agent that consults a live dependent design or its shipped source while
editing the row it declares is reattaching a tier that had to be detached for the
edit to be permitted, and every observation it brings back is a fact about a
document already declared dead.

## Telling which tier something is

Ask what it costs to be wrong. If being wrong means the code is wrong, it is a
design. If being wrong means every design built on it is wrong, it is canon. If
it can be regenerated from the tier above without loss, it is not that tier.

Ask what it survives. A canon survives a total rewrite of every implementation,
in a different style, a different language, a different decade. A design does
not, and is not meant to.

Design rounds are below the canon, not beside it. A round is where a design gets
argued into shape, so it reasons from the canon and never the other way round. A
round that finds the canon wrong produces a canon change, which is a change to
rows and takes the mutation order with it; it does not settle the matter inside
the round and carry on.

## What canon rows owe each other is not formalised yet

Rows are expected, eventually, to declare relationships to one another beyond
provenance, with invalidation cascading through those relations the way changing
a row already cascades to the designs that declare it. That is anticipated and
not specified. It stays unspecified on purpose: how it should work in practice is
not yet known, and the plan is to dogfood the three-tier shape first, see what
actually happens across real canon work, and formalise the relation and cascade
mechanism from that experience rather than from guesswork now. Do not invent a
relation or cascade mechanism ahead of that. And do not read its absence as
meaning rows are independent of one another; absence here means undecided, not
decided-independent.

## What is built and what is not

Built: the registry itself, its namespaces and typed fields, provenance
validation, the reference syntax that renders rows into documents, and
`canon_paths` as the path `mock check` refuses a write to while a panel is open.

Not built: the mutation-order guard, refusing an edit to a live canon row while
any design that declares it remains; the design-declares-canon rule; and the
failure on naming a canon row that does not exist. **None of these has a lint,
phase gate, or hook behind it yet**, and the lint gate is where the design-facing
checks are expected to land. Every rule above binds on how canon and designs are
written regardless, because that is what the rule says, not because something
currently catches a violation of it.
