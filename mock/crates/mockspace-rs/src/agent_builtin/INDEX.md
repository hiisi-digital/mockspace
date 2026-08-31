# Mockspace builtin agent rules

What mockspace is and how to use it. Extracted from the binary on install and
refresh, landing at `mock/target/agent/`, gitignored.

**Never hand-edit the extracted copies.** They are overwritten on the next
refresh; the source is inside the mockspace crate.

| File | Subject |
|---|---|
| `phases.md` | The six phases, what each is, what runs in it |
| `verbs.md` | The four transitions, their validity matrix, what each costs |
| `sides.md` | Doc and src: what a manifest carries, the tree layout, which verifiers run when |
| `anchors.md` | What is snapshotted at apply, what is not, and why not git history |
| `suppressions.md` | The five directives, where each attaches, what none of them can do |
| `commands.md` | How mockspace is invoked, and why the subcommand list is read rather than written |
| `identity.md` | Slugs, task ids, ref paths, hashes, and identifier against content |
| `lints-and-tools.md` | The two kinds of check, which a given one is, the contract each is held to |

## Not here

**Project conventions, domain rules, repository workflow.** Those are the
consumer's own, at `mock/agent/`, and mockspace never touches them.

**Nor anything about what surrounds a repository.** Mockspace cannot know
whether a workspace harness, a team convention or a second trunk exists around
it, so it does not describe one. A test refuses a builtin that names something
outside mockspace.
