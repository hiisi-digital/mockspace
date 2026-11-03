# Merging

Three parts: files that merge by a driver, files that only grow, and a merge commit inside a design round.

## Files that merge by a driver

`cargo mock` writes the `.gitattributes` lines and registers each driver in the clone's git config. `cargo mock register-drivers --global` writes them into the user's git config too, with commands naming no clone: a later clone, a worktree or a fresh container then calls them.

- Generated documents: the top level of `docs/` goes to `mockspace-generated`.
  - It keeps this side's copy of a generated file (read from its header, or from the diagram beside a rendered image) and merges any other file in `docs/` as git does.
  - Only where the regenerating hook is installed: `post-merge` for a merge, `post-rewrite` for a rebase (`git rev-parse --git-path hooks/<name>`), generated under `<mock>/target/hooks` or buildable there; a fresh worktree has none and the hook runs `cargo mock --generate-only` itself.
  - The regeneration is a follow-up commit (`chore: regenerate generated files after merge`, or `after rebase`) or, for a merge that stopped, part of the concluding commit. A rebase's replayed commits may carry the upstream's copy; the tip does not.
- Cargo lockfiles: the mock directory's `Cargo.lock` and each path under `lockfiles` in `mockspace.toml` go to `mockspace-lockfile`, same condition: where a regeneration follows it text-merges, keeps ours on conflict, writes the path down, and the follow-up resolves with `cargo metadata` and commits the lock with every member. Elsewhere the lock conflicts as text.
- Registry rows: `merge-registry`, by `[[namespace]]` and `id`. Additions on both sides stay; only a row changed two ways conflicts, with markers around that row alone. A row deleted on one side and untouched on the other is deleted.
- Closed rounds: `<mock>/design_rounds/*/.meta` goes to `merge-round-meta`; two closed rounds with one id exit 1 and name `cargo mock rename-round <id>`.
- Test history: the `test-history` skill.
- Cherry-pick, revert, `git am`, `git checkout -m` and stash application regenerate nothing: a generated file conflicts there. Run `cargo mock` once the sources are resolved, never pick a side.

## Files that only grow

`mockspace-append` runs `cargo mock merge-additions` for a list of `mod` lines, a table of strings, a log of known gaps.

- Neither side changed or removed a base line: both sides' additions stay, each in its own spot. Both added at one spot: ours first, then theirs, each in its own order.
- Either side edited or removed a base line, both added one of the same lines at one spot, or the file has no base or no final newline: merged as text, markers labelled `ours` and `theirs`, git's exit status. It never selects a side.
- The same line added at two different spots stays at both. One key given two values by the two sides stays twice (invalid TOML); the file's parser refuses it.
- Mockspace names each source package's `SHAME.md.tmpl` and `BACKLOG.md.tmpl`. A project names the rest:

```toml
append_only = ["content/strings/en.toml", "<crate directory>/src/tests/mod.rs"]
```

- Each entry is a `.gitattributes` pattern relative to the repository root, as narrow as the file. A directory is not an entry (attributes are not inherited by what is inside it; a trailing `/` is dropped with a warning). Name the files. Run `cargo mock` to write the lines; an entry removed from the list is removed from `.gitattributes` on the next run. Any line ending `merge=mockspace-append` is the driver's block, a hand-written one included, and is rewritten.
- A file a tool regenerates (a results index the bench driver rewrites, a lockfile) is not append-only; regenerate it after the merge.

## A merge inside a design round

A merge commit is judged by what it adds beyond its parents, per staged file.

- Adds nothing: a file byte-identical to one parent or to git's three-way merge; a conflict settled by keeping lines of both sides, when every staged line is a line some parent carries, no line appears more often than the base has it plus what each parent added, every line all parents carry is still there as often as the parent with the fewest, and the sides keep their own order (where one side moved a shared line, the whole of either side's order, never a mix).
- Is an edit, gated as one: a line neither parent has, a line repeated beyond that, a line every parent has that is gone, shared lines in an order no single parent has. A refused merge says which.
- `git merge <trunk>` is committed once, in whatever phase the round is in, designs and source together. Consolidate both sides' sections one after the other, in either order, each in its own order. Keep no line you wrote yourself in the merge commit: a new line goes into its own commit, in the phase that permits it.
- The order test is a greedy longest-common-subsequence split in linear space, so a rare consolidation only another split explains is refused. Up to four parents are tried in every order; more are tried in the given order and its reverse.
- Time is capped per comparison; a file where one comparison passes the cap is reported by name and gated as authored. No merge base (unrelated histories, shallow clone): the repeat bound cannot be computed, the file is gated as authored with a message; a file taken whole from one parent or reproduced by git's merge still passes.
- The panel-canon gate takes the same merge filter: canon staged by a merge is judged by what it adds, and only canon paths are tested. An octopus is covered by the line tests, not the three-way reproduction.
- The editor's write guard follows the same reading during a merge: a Write or Edit to a path git lists as unmerged (whole path, never a directory holding one) passes in any phase. Every other path stays gated, and so does every Bash command that writes. A path stops being open once its resolution is staged. Check with `git ls-files -u`.

## A registry directory kept on a ref

- A directory declared in `[[registry.store]]` of `mockspace.toml` is kept on `refs/mock/<namespace>` (`refs/heads/mock/<namespace>` on the remote) and is not in the tree. A tree merge never meets it, and the tree copy of a mounted directory is not read.
- Writes go through `mock registry-append <namespace>::<id> <field> <value> [--local]`: fetch, run the registry checks on the files as they would stand, push with a lease. When another writer pushed first, the row merge runs and the checks run again: rows added on both sides stay; a row changed two ways is refused and nothing is pushed. A failed fetch is an error, never an empty registry.
- A delegate at `.mockspace/hooks/registry-append` (repository root or any directory above, nearest wins) runs first: exit 0 means it did the whole step, 3 leaves it to mockspace, anything else is a failure. `--local` skips it.
- The writer reads rows, not documents: removing a row a generated document names is not caught at the write, nor by a plain lint-only commit or push. The next `cargo mock`, a commit concluding a merge, or the post-merge and post-rewrite hooks report the dangling reference.
