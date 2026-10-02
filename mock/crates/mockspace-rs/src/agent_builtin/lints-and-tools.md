# Lints and tools, and which a given one is

**A check is a lint or it is a tool. There is no third kind of check, and
inventing one is how a gate stops gating.** A tool that writes files instead of
judging them is a maker, which is a tool of its own kind rather than a check.

## The default is a lint

A lint runs at a gate. The engine hands it its input, it answers a question
nobody asked it, and its findings block a commit, a build or a push.

**That covers most of what a project wants to check, and where it covers
something it is the better answer**, because a lint that runs pre-commit stops
bad state being committed at all, which no report can do.

Declared per repository under `[lints.<name>]`, with a gate severity and
optional per-scope overrides.

## A tool declares what it is for, with no default

`purpose` is either `Check(reason)` or `Make { writes }`, and a tool cannot
compile without one.

## A check tool is what a lint structurally cannot be, and there are exactly two

**`takes-a-question`.** It needs a question from the person running it, and a
gate has nobody to ask. A configured default does not rescue it: a search pinned
to one fixed phrase forever answers nothing anyone wanted to know.

*Enforced*: it declares at least one required argument, or the claim is false.

**`no-failing-case`.** The answer is the output and no threshold separates pass
from fail. An inventory, a ranking, a list of candidates for a judgement
somebody still has to make. Gating on one means inventing a threshold nobody
justified, and an invented threshold is worse than no gate, because people
defend numbers.

*Enforced*: a run may not return a finding that blocks a gate; one that does
exits 2, as a broken contract.

**Anything that looks like a third reason is either a cost concern or a gap in
the lint contract, and the honest fix is to grow the lint contract.**

## A maker writes files, and is held to the paths it names

A generator, a scaffolder, a renderer, an exporter: its product is what it
writes, so the question a gate asks does not arise and it gives no reason.

**It declares what it writes**, as patterns relative to the repository root,
anchored: `CHANGELOG.md` is the one at the root, not every file of that name.

*Enforced before it runs*: at least one pattern, none empty, none leaving the
repository, none with a literal `.git` segment, none made only of wildcards
(`*`, `?`, `**` in any arrangement; `**/?*` admits everything as surely as
`**`).

*Enforced while it runs*: the engine compares the worktree before and after,
prints every changed path, and fails the run, exit 2, on a write outside the
declaration.

**A maker has no way to express a gating judgement.** Its findings report what
it produced, at warning or info; a finding at error at any gate is a contract
fault, exit 2. A make that fails returns `Inconclusive`, its reason saying what
failed and on which input, and exits nonzero as any inconclusive run does.
Gating stays with checks and lints. What a maker produced is visible only
through the paths it declared, shown by `cargo mock tools --long`, and the list
of what it wrote that the engine prints after each run.

**It fails closed.** Where git cannot report the tree, the maker is not run;
where it cannot after the run, the run is inconclusive. It sees what
`git status` sees: ignored paths are asked about only inside the declaration,
so a gitignored output it declared is seen, while an ignored write outside the
declaration is not; nor is anything under `.git/`, a write outside the
worktree, or a permission-only change to a file already modified.

Declared per repository under `<mock>/tools/<name>/`, one directory each, or
shipped by a lint pack, and listed with their kind by `cargo mock tools`.
`cargo mock help <name>` prints one in full.

## A tool's findings are the same type a lint produces

Severity configuration then works unchanged, rendering is shared, and a check
tool that turns out to be gateable becomes a lint without rewriting a line of
its findings.

**A third finding type is how a gate stops gating.** Before this contract
existed, something needed findings from a check that was not a lint, had no
shape to put them in, and grew its own: a bespoke struct with its own kind and
message, printed with the word ERROR, wired to nothing. A registry declaring one
identifier twice exited zero, exactly as a sound one did.

## Three outcomes, because two cannot say "do not trust this run"

`Clean` carries the count it examined. **Required, not decoration**: a clean
verdict over an empty population is vacuous, and that count is the only thing
distinguishing it from a real pass. For a maker it is a make that succeeded.

`Findings` is what it found. A maker's are advisory, warning or info, and a
maker's failed make is `Inconclusive` instead.

`Inconclusive` is the run whose own controls failed. Without it a broken check
must either report empty, claiming a pass it never established, or invent a
finding, lying about what it checked. It blocks every gate by design, and that
is a statement about the instrument rather than about the corpus.

## Deciding, for something you are about to write

1. **Does running it produce files?** Tool, `Make`, with what it writes.
2. **Does it need an argument from a person?** Tool, `takes-a-question`.
3. **Is there a state it should refuse?** Lint. Write the severity.
4. **Is the output an inventory or a ranking with no pass line?** Tool,
   `no-failing-case`.
5. **None of these?** It is a lint whose refusal you have not decided on yet.
   Decide it.

**A maker that also judges what it wrote is two things**, since its own findings
cannot block: the make, and a lint over its output, so the output is held at
the gate whether or not anyone reran the maker.

**A directory of checks that are neither is a suite of lints that run only when
somebody remembers, plus a handful of reports nobody can find.**
