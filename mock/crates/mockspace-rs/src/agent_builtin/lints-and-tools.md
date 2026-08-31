# Lints and tools, and which a check is

**A check is a lint or it is a tool. There is no third kind, and inventing one
is how a gate stops gating.**

## The default is a lint

A lint runs at a gate. The engine hands it its input, it answers a question
nobody asked it, and its findings block a commit, a build or a push.

**That covers most of what a project wants to check, and where it covers
something it is the better answer**, because a lint that runs pre-commit stops
bad state being committed at all, which no report can do.

Declared per repository under `[lints.<name>]`, with a gate severity and
optional per-scope overrides.

## A tool is what a lint structurally cannot be, and there are exactly two

**`takes-a-question`.** It needs a question from the person running it, and a
gate has nobody to ask. A configured default does not rescue it: a search pinned
to one fixed phrase forever answers nothing anyone wanted to know.

*Enforced*: it declares at least one required argument, or the claim is false.

**`no-failing-case`.** The answer is the output and no threshold separates pass
from fail. An inventory, a ranking, a list of candidates for a judgement
somebody still has to make. Gating on one means inventing a threshold nobody
justified, and an invented threshold is worse than no gate, because people
defend numbers.

*Enforced*: a run may not return a finding that blocks a gate.

**Anything that looks like a third reason is either a cost concern or a gap in
the lint contract, and the honest fix is to grow the lint contract.**

Declared per repository under `<mock>/tools/<name>/`, one directory each, listed
by `cargo mock tools`.

## A tool's findings are the same type a lint produces

Severity configuration then works unchanged, rendering is shared, and a tool
that turns out to be gateable becomes a lint without rewriting a line of its
findings.

**A third finding type is how a gate stops gating.** Before this contract
existed, something needed findings from a check that was not a lint, had no
shape to put them in, and grew its own: a bespoke struct with its own kind and
message, printed with the word ERROR, wired to nothing. A registry declaring one
identifier twice exited zero, exactly as a sound one did.

## Three outcomes, because two cannot say "do not trust this run"

`Clean` carries the count it examined. **Required, not decoration**: a clean
verdict over an empty population is vacuous, and that count is the only thing
distinguishing it from a real pass.

`Findings` is what it found.

`Inconclusive` is the run whose own controls failed. Without it a broken check
must either report empty, claiming a pass it never established, or invent a
finding, lying about what it checked. It blocks every gate by design, and that
is a statement about the instrument rather than about the corpus.

## Deciding, for a check you are about to write

1. **Does it need an argument from a person?** Tool, `takes-a-question`.
2. **Is there a state it should refuse?** Lint. Write the severity.
3. **Is the output an inventory or a ranking with no pass line?** Tool,
   `no-failing-case`.
4. **None of these?** It is a lint whose refusal you have not decided on yet.
   Decide it.

**A directory of checks that are neither is a suite of lints that run only when
somebody remembers, plus a handful of reports nobody can find.**
