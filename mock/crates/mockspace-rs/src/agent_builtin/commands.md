# The `cargo mock` command surface

Invoked as `cargo mock <subcommand>` from the repository root.

## Never write the list of subcommands from memory

```bash
cargo mock tools           # every subcommand and every project tool, one line each
cargo mock tools --long    # usage and declared arguments
```

**That command is the surface.** It reads the same table the dispatcher does, so
it cannot disagree with what actually runs, and it lists this repository's own
tools alongside the builtins.

A list written into a document instead goes stale the moment a subcommand is
added, renamed or retired, and nothing reports it. This file carried one for
long enough to describe five subcommands that did not exist and to miss fifteen
that did, so what follows says what the categories are and does not enumerate
them.

## How mockspace is reached

**Through the installed launcher, invoked as `cargo mock`.** There is no cargo
alias, no `build.rs` bootstrap and no proxy crate. Anything describing one is
describing a mechanism that was dissolved; `bootstrap_from_buildscript` survives
only as a tombstone that fails the build with the migration steps.

On any normal invocation the engine writes the generated validator, ensures the
durable hooks that delegate to it, and points `core.hooksPath` at them. The
durable hooks live outside the repository, in the user config home; the
per-repository validator lives under the mock directory's `target/`.

**Deleting `target/` does not uninstall anything.** The durable hooks and
`core.hooksPath` survive it. What goes is the generated validator, and the hook
then blocks and says to run `cargo mock`.

## What the categories are

**Bare `cargo mock`, with no subcommand**, regenerates the documents and the
agent instructions under `.claude/` and `.github/` from `mock/agent/` templates,
or from the builtin templates where a repository has none. Those generated files
are never hand-edited: edit the template and regenerate.

**Round transitions** move a design round between phases by renaming its files,
so the phase follows from the rename. Do not hand-rename to lock or close; the
subcommand writes the bookkeeping that goes with it. `phases.md` and `verbs.md`
carry the state machine.

**Gate control** points `core.hooksPath` at the mockspace gate, or restores
git's default.

**Reports** answer a question and change nothing: the readiness report, the
round's current phase, the registry query, the tool listing.

**Runners** drive something else: the test trees, the bench harness, the
document rendering.

**Message linting** checks one commit message or forge body against the
configured attribution and style policy.

## Two things that are easy to get wrong

**A subcommand that is not in `mock tools` does not exist**, whatever a document
says. Check before telling somebody to run it.

**A project's own tools appear in that listing too**, under their own heading,
and they are declared per repository rather than shipped by mockspace.
`lints-and-tools.md` says what belongs in one.
