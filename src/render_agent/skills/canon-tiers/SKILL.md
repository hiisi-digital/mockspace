# The canon, design, code chain

Three tiers; each governs the one below, in one direction only.

## The canon is typed rows

- Typed rows in a registry, in namespaces the project declares: declared fields, a snake_case slug each, provenance saying what each row rests on.
- `canon_paths` in `mockspace.toml` says which of that is the canon, and is what `mock check` refuses a write to while a panel is open. **Here it is {canon_location}.**
- Rows, not prose: a typed field can be pointed at from a design, counted, checked for a slug that names nothing, and rendered into many documents that cannot drift.

{canon_view_paragraphs}

## Where a superseded canon goes

- Canon is never deleted, only demoted. A superseded row stays referenceable; this is the one exception to nuking, because the canon is the only tier carrying reasoning rather than consequence.
- A row's `provenance` is an array of references; the root name carries the meaning (the corpus it derived from, the document that supersedes it, the code that implements it). Array order is precedence: the first is the one to follow.

## The reproduction property

- Nuke the code and lose nothing: the design says what the code was.
- Nuke the design and lose little: an equivalent design can be written from the canon.
- The canon is the only tier not reproducible from anything above it.
- A design is good enough when two implementers reading it independently produce working implementations of the same thing. A canon is good enough when two designers reading it independently produce designs yielding equivalent working units.

## The mutation order, at length

- To change code, nothing has to be nuked first, because code is the leaf and nothing depends on it. Two constraints still bind: the round ceremony (topic, doc changelist, lock, source changelist, lock, close) with its phase gates, and nothing may appear in code that is not in the design. A change introducing something the design does not say is an undeclared design change wearing the leaf tier's freedom.
- Each tier is unconstrained downward and fully constrained upward.
- To change a design: nuke the code under it first, not migrated or adapted, then rewrite it from the changed design.
- To change a canon row: nuke every design that declares that row, and the code beneath those. Not every design in the project, only the declared dependents. Each design's canon declaration makes that scoping possible.
- Adding a row nukes nothing, since nothing declares it yet. Adding a field nukes nothing: the trigger is invalidation, not editing. Changing what a field says, or removing one, invalidates the row's declared dependents.
- A row is one aspect of the canon: one row is one thing. A row carrying two unrelated subjects drags unrelated designs into every nuke. Fix it by splitting the row, not refining the granularity below it.
- `cargo mock --nuke` takes the source down; `--nuke=docs` takes the designs and the source under them. It names every file, asks (`--y` answers) and refuses an uncommitted tree.
- A lower tier surviving a change above it is a claim about something that no longer exists.

## What this means for canon work

- A canon row's dependents declared stale is the precondition of the edit, not a quality complaint. While a design declares a row, the row is frozen; nuking the declared dependents unfreezes it.
- While editing a row, do not consult a live dependent design or its shipped source: every observation from it is a fact about a document already declared dead.

## Telling which tier something is

- Cost of being wrong: the code is wrong, it is a design; every design built on it is wrong, it is canon. If it can be regenerated from the tier above without loss, it is not that tier.
- What it survives: a canon survives a total rewrite of every implementation, in a different style, language or decade. A design does not.
- Design rounds sit below the canon. A round that finds the canon wrong produces a canon change (rows, with the mutation order); it does not settle the matter inside the round.

## Relations between rows

- Relations beyond provenance, with invalidation cascading through them, are not specified. Do not invent a relation or cascade mechanism, and do not read its absence as rows being independent: absence means undecided.

## What is built and what is not

- Built: the registry, its namespaces and typed fields, provenance validation, the reference syntax rendering rows into documents, and `canon_paths` as the path `mock check` refuses a write to while a panel is open.
- Not built: the mutation-order guard (refusing an edit to a live canon row while a design that declares it remains), the design-declares-canon rule, and the failure on naming a canon row that does not exist. None has a lint, phase gate or hook behind it in mockspace itself; the rules above bind on how canon and designs are written regardless. A repository may carry lints of its own for them (`cargo mock lints`).
