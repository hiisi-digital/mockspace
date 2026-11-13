# Seat 001 on op's second answers

Read: `000_op.md`, section "Answers to the revised consolidation" (head fd0ad8e), and `014_consolidation.md` (shadow tree at 2.3, D6). Probes for this file are in `015_probes/` (`guard.sh`, s1, d1, j1, with outputs). Every probe creates its own scratch directory under a fixed base, redirects `HOME` into it, refuses a scratch name that is not a single path component, and runs git and cargo under `timeout` with stdin from `/dev/null` (`guard.sh`). git 2.43.0, spikes, no benches. The renderer in j1 is a toy that I wrote to have something to merge against; what it shows is how git's machinery behaves on that shape, not how mockspace's real renderer behaves.

Gates. Canon gate: op's answers govern. Settled by him and taken as given here: instance root `.mock`, `git~1` dropped, git-shook auto-installed, purge epoch, one shadow tree under a name that is not `@`, two-way. Test gate: no code changes in this round; the 001 suite run stands.

## 1. Could the instances live in a platform state directory?

Op's thought: put the instances in `$XDG_STATE_HOME` or `~/.local/state` (`%LOCALAPPDATA%` on Windows), and let commands in the git tree associate them. Answer from probe s1 (a fake `XDG_STATE_HOME` inside the scratch, one linked worktree of the data branch at `<state>/mockspace/<clone id>/research`, a sketch in it with `lib = { path = "../../../crates/lib" }`):

- Loss surface: it is as good as the common-dir placement of 005 and 008. `clean -ffdx`, `checkout -f` and `reset --hard` to a pre-migration commit, `stash -u` and `checkout dev` all leave the edit (`edit survives=1` five times). Nothing run in the git tree reaches it, double-f clean and `worktree remove` included.
- The `../` paths: cargo at the state path fails. A link `.mock/research` pointing at it fails too, for the physical-`..` reason found in 003 q2. This is the same wall as r1 and q2, and it is the research manifests that hit it: 13 of the 24 `Cargo.toml` under this repository's `mock/research` hold a path that climbs to the repository root (`mockspace-bench-harness = { path = "../../../../bench-harness" }`). Association by symlink, junction or by a command does not change where `..` lands for cargo or the shell.
- Deleting the clone: the bytes at the state path survive (`files still there: N.md sketch`, the edit intact), which in-tree placement cannot say. But the worktree's `.git` file points into the deleted clone, so `git status` there says `not a git repository`; the data's own commits lived in the deleted object database and are gone with it. Uncommitted edits survive, unpushed commits do not. After a moved (not deleted) clone, `git worktree repair <paths>` fixes the pointers (008 t5).

So the answer is split, and the split is checkable by the engine.

What can move out of the tree:
- The user cache (already `${XDG_CACHE_HOME:-$HOME/.cache}/mockspace/...` in 014) and any plain-copy snapshot. These are copies, nobody reads them by path.
- The instance of a namespace that no tool reads by path and whose files carry no relative path out of themselves. Candidates I can name only as candidates: the shadow tree (the engine reads templates through `mock://`; people edit them, which is the editor-discoverability cost below) and, possibly, closed design rounds. I have no census; this is an impression until one is run.

What cannot:
- Research, and any namespace whose files hold paths that leave it: `../` resolves physically, so the instance must sit at the depth the data was written for inside the worktree. That is the depth rule of 012 section 1.3, now with `.mock` as the root: `<worktree>/.mock/<ns>`.
- Anything a person or an agent opens by path in an editor or a search of the repository. Outside the tree, it is invisible to the repository's own tools.

Costs of the state directory that I can state without a probe (impression): the key must name the clone (I used a hash of its path), so a moved clone loses its association until `repair` and two clones of one remote have two instances; uninstalling leaves the state behind; `%LOCALAPPDATA%` plus cargo target directories is a long-path risk on Windows; and the XDG specification keeps state for things like history and logs while data a person made belongs in `XDG_DATA_HOME`, so if instances ever move it is the data directory that fits, not the state one.

Verdict, as a rule the engine can enforce and not a global choice: the default instance root is `.mock` in the worktree. A namespace may declare an external root, and `instantiate` refuses that where a file in it holds a relative path that leaves the namespace (the same scan that checks depth) or where the namespace is marked as read by path. Nothing else about the design changes, because the engine reaches everything through `mock://<ns>`. It is a per-namespace option, not the default, and for research it is closed.

## 2. The shadow tree's name

Probe d1. Each candidate is made a directory at the root of a repository with a file, committed, with a branch of the same name created wherever git allows one (the worst case), and then the bare argument, with no `--`, goes to `log`, `diff`, `show`, `checkout`, `rev-parse`, `add`, `restore`, `status`. `AMBIG` is git's "ambiguous argument: both revision and filename" failure. `AS-REV` is git treating the word as the revision and acting on it.

| name | log, diff, show | checkout | other verbs | can be a ref? |
|---|---|---|---|---|
| `@` | AMBIG | AS-REV | ok | yes (HEAD alias) |
| `shadow` | AMBIG | AS-REV | ok | yes |
| `_shadow`, `#shadow`, `%shadow`, `=shadow`, `+shadow`, `$shadow` | AMBIG | AS-REV | ok | yes |
| `-shadow` | err | err | err | no, and an option |
| `HEAD` | AMBIG | ok | ok | yes |
| `shadow.lock` | ok | ok | ok | no |
| `.shadow` | ok | ok | ok | no |

Reading it:

1. Any name that can be a ref is ambiguous the moment a ref of that name exists, and `git checkout <name>` then acts on the ref, switching branch and not touching the path. The directory is not at fault; the name space is shared with the branches and tags of everyone who clones, and nobody controls it. `@` is the case op already saw; `shadow`, `_shadow` and the punctuation names are the same failure one branch away.
2. The names git can never accept as a ref component are immune: a component starting with a dot (`.shadow`) and one ending in `.lock`. `.shadow` is ok on all eight verbs. `.mock`, op's instance root, is in that class, so `git log .mock` and `git checkout .mock` are never ambiguous; that is a quiet reason to be glad of op's choice.
3. The punctuation names also fail before git sees them. In bash, unquoted at the start of a word, `#shadow/f` is a comment and printed nothing, and `$shadow/f` expanded to `/f` (d1 shell section). `%shadow` is a job spec to `fg`, `kill` and `bg`, and `=shadow` is a command-path expansion in zsh, which is not installed here (impression). `-shadow` is an option. The leading dot and the leading underscore are the only ones the shell leaves alone.
4. Windows: a leading-dot directory is legal, and none of the candidates is a reserved device name (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`), none ends in a space or a dot, none holds `< > : " / \ | ? *`. I could not test it (impression, from the documented rules).

Proposal, with the reason that removes the question rather than picking a spelling: the shadow tree is one mock tree of its own, an ordinary namespace on its own ref, and it has no marker directory inside it. `mock://<name>/crates/foo/README.md.tmpl` is the shadow of the git-tree file `/crates/foo/README.md`; physically it is `.mock/<name>/crates/foo/README.md.tmpl`. Then the only place the name appears is as a path component under `.mock/` (immune, point 2) and in `mock://` and in the ref name, where it is a namespace the validator already constrains (`refs.rs:80-102`). No directory named `@` or anything else sits at the root of a data worktree to be mistaken for a revision. The namespace name is op's to give; `shadow` is fine because nothing parses `.mock/shadow` as a revision.

If op wants the shadow files inside a namespace that holds other things, the marker directory is `.shadow`, which is the one spelling in d1 that is never a ref, never shell syntax, and not special to git (git's special names are `.git*`, and `.shadow` is not one).

## 3. The two-way shadow tree as git commands

Terms: T is a shadow file (the template), G the generated git-tree file, committed in the git tree, `render(T, data@Mock-Ref) = G`. A person edits G in an ordinary pull request. The two directions.

### 3.1 Regeneration without trivial conflicts

Two branches that each regenerate G from different data produce a conflict in G on merge (j1: `UU G`, 1 marker). The repository already holds the machinery for this: a merge driver that keeps one side and writes a note that a regeneration is owed, and a later hook that regenerates and checks (`tests/generated_files_kept_ours.rs:1-13`, the `mockspace-kept-ours` file, and the append driver registered in `src/bootstrap/append_driver.rs`). j1 repeats the shape with plain git: `G merge=gen` in `.gitattributes` and `merge.gen.driver = true` (keep ours): the merge completes with no markers, and the regeneration from the newer data afterwards yields the right file. Under the new arrangement the inputs are mock trees, so the rule for the regeneration's input is: the data tip named by the newest `Mock-Ref` among the merged sides; if the two sides' `Mock-Ref` commits are not ancestors of one another, the data refs merge first, by git, in the mock tree, before G is regenerated. G is derived, so it never needs a human to resolve a hunk; the only thing a merge of G has to carry is the "owed" note until the gate has regenerated and looked.

### 3.2 An edit to G becomes a template change

The recipe, in git's own vocabulary. Base, other and current for a three-way file merge:

- base = G at the merge-base of the PR: `git show <merge-base>:G`. The gate guarantees G there equals `render(T at that time)`. Verify it before trusting it (`render(T_then) == base`), and refuse if not (a PR cut when the gate was bypassed).
- other = G at the PR tip: `git show <tip>:G`.
- current = T as it is now in the shadow tree.

`git merge-file -p T base other` applies the PR's change to the template. Accept the result T' only after the round trip: `render(T') == merge(render(T), base, other)`. That left side is the whole proof; if it fails, nothing is absorbed. The absorbed T' is a normal commit in the shadow tree's worktree (it is a data commit, with the PR's sha in its message), and G is regenerated from it, so G ends up as the PR wrote it plus any template change that landed since.

Probe j1, eleven edits to a generated file whose template has a scalar placeholder, a table placeholder and literal prose (G is the file `render(T)` makes; "absorbed" means merge-file was clean and the round trip held):

| edit | result |
|---|---|
| edit a literal prose line | absorbed |
| insert a paragraph after literal text | absorbed |
| delete a literal line | absorbed |
| edit the literal line directly after the table | absorbed |
| PR edit while the template changed on another line | absorbed (both survive) |
| edit a line a placeholder produced (`version: 1.2`) | conflict |
| edit a row of the generated table | conflict |
| delete a generated table row | conflict |
| edit the title a placeholder produced | conflict |
| PR edit while the template changed the same line | conflict (a true conflict) |
| PR empties the file | conflict |

The base matters. op's wording, "the latest generated end result vs current", is the right idea with one qualification found by probe: the base must be the G the PR was derived from, not the latest G on the trunk. j1 last section: with base = G at the merge-base, a template change that landed after the PR was cut survives in T' (`Closing paragraph, upstream.`); with base = the latest G the same merge reads the PR as reverting that change and puts `Closing paragraph.` back.

When it runs: a PR cannot carry a template change, because templates live in a mock tree, which is exactly op's point. So absorption is a step the engine offers ("whoever next works on mockspace", op's words) and the gate prompts for: it finds files where `G at HEAD` differs from `render(T)` with T and its data unchanged, which is the signature of a human edit. Until absorbed, a regeneration must not overwrite that G, which is the existing "kept ours" rule extended from merge time to every regeneration (a note per pending file, removed only by an engine that has absorbed or declined). If a regeneration does happen first, nothing is lost: the PR's `G` is in history, and absorption can still be run later against `<merge-base>` and `<merge commit>`.

### 3.3 Where the round trip cannot be automatic

From the table, and from what the toy cannot show (marked as impressions):

1. The edit lands on text a placeholder, loop or include produced (four rows above). The right change is in the data (a registry row, a name) or in the template's logic. The engine can say which, if the renderer keeps a source map from output lines to template spans and data keys; merge-file cannot, it only sees a conflict. So: report, with the source, and propose nothing.
2. A true conflict with a concurrent template change on the same lines: a person decides, in the shadow tree, with git's conflict markers.
3. Whole-file operations: the PR deletes G, empties it, or renames it. A deleted or emptied G is a decision about the template (remove the file? the shadow file goes too). A rename moves a shadow file by the mapping and is deterministic only if the output name does not come from a placeholder.
4. A new hand-written file in the git tree that no template makes. Either it is adopted (the engine writes a literal template equal to the file and marks it adopted, so from then on it is shadowed and editable in both directions) or declared hand-written. A choice, not an inference.
5. Renderers whose output is not line-aligned with the template: a formatter after render, reflowed paragraphs, reference expansion that rewrites link text. The three-way alignment degrades, and the round-trip check is what stops a wrong absorption (the toy has no such renderer; impression for the real one).
6. Outputs built from several templates or includes: ownership of a hunk is ambiguous without the source map.
7. A base that does not equal `render(T_then)`: the PR predates a bypassed gate. Refuse, name the commit.
8. Binary or non-text outputs.
9. Several PRs editing the same generated file: absorbed one after another, each against its own merge-base, and git's conflicts decide between them, as in any three-way merge.

What this leaves automatic is the common case op wants, a documentation PR that edits prose in a generated file: literal-line edits, insertions and deletions in literal text, including concurrently with unrelated template edits. Everything that touches generated spans is reported with its source, and nothing is changed behind a person's back.

## 4. What changes in the build

1. Instance root `.mock`, default in the worktree; per-namespace external root allowed only where the scan passes (section 1).
2. The shadow tree is a namespace of its own; `mock://<name>/<git path>(.tmpl)` is the mapping; no `@`, no marker directory; `.shadow` if one is ever needed.
3. Generated files carry a merge driver that keeps one side and writes the owed note; regeneration takes the data of the newest `Mock-Ref`.
4. `absorb` as an engine verb: base from the PR's merge-base, `git merge-file`, round trip, a commit in the shadow tree, then regeneration; refuses with the conflict and the source when it cannot.
5. Tests owed, red first: each row of the j1 table as a case (including both base choices), the pending-note rule across a regeneration, and the external-root refusal on a file with an escaping relative path.

## 5. Open

1. A census of which namespaces any tool or person reads by path (decides what may go external).
2. The real renderer's line alignment, formatter, includes and source map: the toy above proves git's side only.
3. How the gate detects "T and its data unchanged" cheaply, and where the pending note lives so a clone that never ran the engine cannot overwrite a human edit.
4. Windows and case-insensitive behaviour of `.mock` and the namespace names: unprobed.
