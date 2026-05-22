# Mockspace builtin agent rules

This directory carries the canonical descriptions of mockspace
itself. Read these when you need to understand what mockspace is
and how to use it.

The files are extracted from the mockspace binary on
`cargo mock install` / `refresh` and land at
`mock/target/agent/`. They are gitignored. Edits to the extracted
copies are blown away on next refresh; the source of truth lives
inside the mockspace crate.

## Files

| File | Subject |
|---|---|
| `phases.md` | The six-phase state machine. What each phase means, what enters and exits it. |
| `verbs.md` | The four transition verbs (`plan`, `apply`, `finish`, `replan`) and the full validity matrix. |
| `sides.md` | Doc and src vocabulary. What a manifest is, what each side carries, how the two sides relate. |
| `anchors.md` | What an anchor captures, when, why. Content-addressed snapshot storage. |
| `suppressions.md` | The five comment-canonical directives (`lint:allow`, `lint:scope-add`, `lint:defer`, `lint:file-disable`, `lint:prop`) plus Rust attribute aliases. |
| `commands.md` | The `cargo mock` subcommand surface as it ships today. |
| `identity.md` | Slugs, task IDs, ref paths, content hashes. How mockspace names things. |

## What's NOT here

These are mockspace-internal facts only. Project-specific
conventions, domain rules, repo workflows belong in the consumer's
own agent rules at `mock/agent/<whatever>`. Those stay
consumer-authored and mockspace never touches them.

When the mockspace binary updates, this directory updates with it
(via `cargo mock refresh`). When the consumer updates their own
project rules, that's their working tree to manage.
