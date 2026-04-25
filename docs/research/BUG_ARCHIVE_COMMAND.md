# Bug: No way to archive an abandoned design round

## Problem

When a design round is abandoned (deprecated in TOPIC or DOC phase,
no intention to continue), there is no `cargo mock` subcommand to
archive the files. `cargo mock close` requires DONE phase (both
changelists locked), which is impossible for abandoned rounds.

The deprecated topic files and changelists litter the `design_rounds/`
root with no automated way to clean up. Manual `mkdir` + `mv` is
required.

## Expected behavior

`cargo mock archive` (or `cargo mock close --abandoned`) should:
1. Move all root-level topic files and deprecated changelists to a
   timestamped subdirectory
2. Write `.meta` with the current commit hash and date
3. Optionally write `.history` (git log since the topic files were committed)
4. Work from ANY phase, not just DONE

## Workaround

Manual archive:
```sh
mkdir -p mock/design_rounds/<timestamp>-abandoned
mv mock/design_rounds/*_topic.*.md mock/design_rounds/<timestamp>-abandoned/
mv mock/design_rounds/*_changelist.*.md mock/design_rounds/<timestamp>-abandoned/
git add mock/design_rounds && git commit -m "chore: archive abandoned round"
```

## Context

Discovered while working on the saalis project. A design round was
started, the doc changelist was written and locked, the src changelist
was written, then fundamental design errors were found in the
implementation. The round was deprecated (`cargo mock deprecate`
twice to get back to TOPIC), source was nuked (`cargo mock --nuke`),
but the topic and deprecated changelist files couldn't be archived
via any subcommand.
