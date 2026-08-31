# TODO: an optional guard against blanket staging

**Date:** 2026-07-26
**Status:** owed work, not started. Recorded so the next mockspace session picks it up.
**Requested by:** op, after the incident below.

## What is wanted

An optional hook that blocks a tool call which stages the whole working tree, whether that call arrives
as a shell command or as an MCP git tool.

## Why, concretely

`git add -A`, run from a shell whose working directory had drifted several calls earlier, staged and
committed 67 files and 22,631 insertions into the wrong repository. The content was sketch build
artifacts including a binary `libsketch.rmeta`, the commit message described an entirely different
repository's design decisions, and it landed directly on `dev`, which the branch flow forbids.

It was unpushed, so `git reset --soft` recovered it completely. The same slip against a pushed branch
is not recoverable under the destructive-action guards.

The mechanism worth naming: the wrong working directory was the mistake, but `add -A` is what turned a
wrong-directory mistake into a wrong-content one. Staging explicit paths would have staged nothing at
all, because the intended files were not in that repository. The blast radius comes from `-A` coupling
a commit's content to whatever happens to be lying around, and in this workspace that routinely
includes generated output, sketch artifacts, and op's own in-flight work.

## What to catch

Shell commands, allowing for `-C <dir>` and for the command sitting anywhere in a compound line:

- `git add -A`, `git add --all`, `git add .`, `git add :/`
- `git commit -a` / `git commit --all`, which stages every tracked modification and is the same class
  of blanket action wearing different clothes
- `git add` with no pathspec at all, which is a no-op today but reads as intent

MCP tools, by name shape and by argument shape rather than by an enumerated list, since server naming
varies:

- a git-add-shaped tool whose arguments carry an all / stage-all / update-all flag
- a git-commit-shaped tool with the same, since several expose commit-with-staging as one call

The `Invocation` context added during the attribution arc already carries both the command text and the
tool name, so the material for this check is in place.

## Where it fits

Two candidate shapes, and the choice is not obvious.

**A builtin guard hook**, alongside `no-yagni-guard.sh` and the others, with its patterns baked at
generation time the way `check-message.sh` bakes attribution policy. Consistent with the existing
guards, and the check really is about the shape of an invocation rather than about content being
linted.

**A new lint kind.** The lint traits are keyed on what a lint is given, and a command is not an
authored message, so it would not fit `MessageLint`; it would want a fifth kind. That buys the lint
system's configuration and severity machinery for free, at the cost of another trait and another
boundary entry.

Recommendation is the guard hook. The `LintPack` boundary was deliberately made extensible so a fifth
kind is cheap, but a kind should exist because something needs the lint machinery, and a yes-or-no
block on a command shape does not need severities or finding kinds.

## Configuration, and the default

Every pattern configurable, per the standing rule that nothing is hardcoded.

**Default off.** By the defaults test op stated: a default must either fit everyone, or be very
sensible and likely shared across all consumers anyway. Blanket staging is genuinely risky, but plenty
of projects use `git add -A` as their normal rhythm and would read a block as hostile. So it is opt-in,
which is also how op phrased the request: an *optional* hook.

For this workspace specifically, turn it on. The preference is already recorded as a memory, and a
recorded preference that only an agent's diligence enforces is exactly the kind of thing this arc has
been converting into a mechanical check.

## Related finding: the guard it would sit beside does not belong here either

Noted by op while this was being written, and confirmed by reading it.

`no-yagni-guard.sh` is a mockspace **builtin**, generated into every consumer repo. What it does is
grep a commit message for YAGNI-flavoured wording and warn:

> This project embraces the ideal when designing -- extensible, trait-based, registered. Shortcuts
> justified by 'you ain't gonna need it' are not welcome.

That is this workspace's design philosophy stated as a sentence and shipped in the engine. A different
project could hold the opposite position perfectly reasonably, and YAGNI is a mainstream discipline
rather than a defect. An engine has no business having an opinion on it.

Same class as the 12 loimu-specific lints the relocation audit found in core, and the same fix: it
belongs in the opt-in pack, where a project that shares the opinion imports it. Its patterns should be
configuration rather than a baked constant while it moves.

Worth a wider sweep at the same time, because the other builtins were never audited on this axis
either. `bench-and-sketch-discipline`, `readmes`, and the `real-code-guard` skill all sound like they
may encode workspace-specific conventions rather than general mockspace mechanics. The general test to
apply: would a project that had never heard of this workspace want this, and would it be wrong for
them if it fired? Anything failing that goes to the pack.

Tracked with the relocation work rather than here; recorded here because this is where it surfaced.

## See also

The workspace memory `feedback-never-git-add-dash-a`, which records the preference and the incident.
`.shared/state/mockspace-attribution-and-commit-lints.md`, the arc this was raised during.
