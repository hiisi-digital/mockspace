Source: kaski channel (coord/kaski of clause-dev), briefs/fixup/review-mockspace-2.md, first posted 2026-11-13 08:30 UTC, read 2026-11-15.

# Collected review, pass two: mockspace one-pass, extra-lints one-pass, extension-carriers, tools-upstream

**Heads.** Three match: mockspace `c3bfe78` and `15f60ef`, extra-lints one-pass `db0f2d5`. **`work/tools-upstream` does not match**: the remote is at `2d804f9`, one commit past `eda6a33` (`test: run the scratch consumer under a plain cargo test`, 08:13). I read `2d804f9`.

**Limits.** No suite was run and nothing was built, on your instruction, so the test gate was not carried out and every pass count below is a helper's claim. I read test bodies and diffs only. Canon gate: neither repository declares a canon; op's order of 13 November is the governing text for branches 3 and 4 and both are inside it. This is one expert's reading.

---

## 1. mockspace `work/mockspace-lint-one-pass` at `c3bfe78`: MERGE AFTER FIXES

### Pass-one findings
- **M1, fixed.** `origin/dev` (`2c295e6`) is an ancestor and `git merge-tree` is clean. The port goes through the view in `lint-rules/src/changelist_required_scaffold.rs:46-150`, `lint-rules/src/no_empty_crate.rs:117-181` and `src/entry/edit.rs:254-275`. I found no behaviour change in it by reading.
- **The four red tests.** bench-harness, `README.md`, `cargo-mock/` and the private-project test do not differ between `origin/dev` and the branch, so all four are identical on both sides by construction.
  - `the_readme_names_no_pin_key_the_descriptor_does_not_read` (`cargo-mock/tests/the_readme_pin_example_is_what_the_descriptor_reads.rs:34`): cannot pass on `dev` as written. The scanner takes the first `mockspace_` per line, `README.md:28` carries two keys on one line, so it finds 2 and asserts `>= 3`.
  - `no_tracked_file_names_a_project_a_reader_cannot_see` (`tests/the_public_tree_names_no_private_project.rs:118`): cannot pass on `dev`. `ikiuni` is in its `PRIVATE` list and in `src/render_agent/skills/reference-syntax/SKILL.md:58` on `dev`.
  - The two disasm tests: by reading these are `identical_work_in_separate_crates_is_flagged_duplicate` and `different_work_is_not_flagged_duplicate` (`bench-harness/src/disasm/tests.rs`). They are the only ones that assert on real dylibs without a skip, and the host has GNU objdump, which the file says it was never verified against. The report does not name them, so the identification is mine.
- **M2, partly.** Details in the table.

| Repo | Breaks on landing | Port |
|---|---|---|
| arvo | yes: 70 lints, pack | patch applies to `origin/dev` `9dfd5339`, 56 files, every lint file naming a context is in it |
| huurre | yes: 61 lints, pack | patch applies to `07167eb`, 60 files, same check |
| hilavitkutin | yes: 2 lints, pack | patch applies to `c42d707`, both lints |
| tassu | yes: pack only | heals with branch 2 |
| kaski | yes: 96 lints, 32 tools, lock pins the pack at `489996a` | **none to read**: no patch on the channel, and the pushed `work/mockspace-lint-one-pass` (`cb08294`) holds benches only and is 254 commits behind `dev` |
| muisti | yes: four lints on ref `mock/mock` read `ctx.registry` as a field | **none**: no patch, nothing pushed |
| homma, applets | no: homma's `ask` uses only the `Tool` API, which the branch does not touch | not needed |
| kirjo, lato, riimu, viola | helper says no; not cloned here, unchecked | |
| kamu, vehje | unknown, never run (ticketed) | |

- The stack-lints pack is the same repository as extra-lints: `ls-remote` gives `489996a` for `dev` under both names, so branch 2 covers it.
- Patches were applied to a scratch index only. No leftover old-API read remains outside `#[cfg(test)]` code. The tool `Cargo.lock` updates the report mentions (arvo, huurre) are not in the patches.

### Blocking
1. **kaski and muisti have no port anyone can check** [drift]. "Lands right after" rests on an uncommitted working tree, against a kaski `dev` that moved 254 commits. Put both on the channel as patches against current `dev`, as the other three are, before landing.
2. **No embedded agent rule describes the new contract** [public-surface]. `src/agent_builtin/` does not exist in v1; the embedded text is `src/render_agent/`. The branch changes one word there (`catalogue.rs`, "reach"). The view contract and `lints-read-through-the-view`, a new hard error over every consumer's `<mock>/lints/`, are described only in `docs/USAGE_GUIDE.md:540-590`. I passed this in pass one; the rule as you quote it does not allow it, and branch 3 is held to it. A short paragraph in the generated rules is enough.

### Non-blocking, unchanged since pass one
- `reading/pool.rs:253-261` and `:232`, the sleeps in `reading/memo.rs:82` and `reading/tests/order.rs:30,51`, the deny-list guard.
- No stale old-API name in `src/render_agent`, the READMEs or the guide.

### Verified
- 31 commit subjects in shape, no bylines.
- The tests `dev` added build their contexts through the testkit.
- `admits_edit` has one caller.

---

## 2. mockspace-extra-lints `work/mockspace-lint-one-pass` at `db0f2d5`: MERGE, straight after 1

- Unchanged since pass one, same sha, six commits, `dev` `489996a` is an ancestor.
- Between `bd62a68` and `c3bfe78` the lint-rules surface it uses moved in one place only (`changelist_required` became `pub`, `admits_edit` gained a view), and the pack references neither. It applies by reading.
- "742 tests pass against c3bfe78" is the helper's claim.
- Manifests point at mockspace `dev`, so it builds only after 1 lands.

---

## 3. mockspace `work/extension-carriers` at `15f60ef`: MERGE AFTER FIXES

It adds one carrier, tool parameters. Agent text from a pack, registry namespaces and a maker's `writes` are not added, and the report does not say why. Your brief asked for at least the first, so that is met.

### Blocking
1. **Nothing tests the one line that makes the carrier exist** [tests]. `src/entry/dispatch.rs:436` (`l.pack.configure_tools`) is the only call site. `src/entry/tool_config_tests.rs:90` calls `configure_tools` itself, "the way the dispatcher does". Delete the dispatch line and every test stays green while every real run ignores declared keys and runs on defaults, which is the failure the carrier is for. Fix: move the two configure calls into one function that `run_inner` and the test both call, or test through `run_inner`.

### Non-blocking
- `src/tool_catalogue.rs:224`: the `mock help <tool>` key listing, claimed in the guide and the rule, has no test.
- `tool_config_tests.rs:91` discards the exit code; the refusal's exit 2 is not asserted.
- Values arrive as text, so a tool cannot tell `3` from `"3"`. Documented, and the same as lints.

### Verified
- Small and general: two defaulted trait methods (`lint-rules/src/tool.rs:350-360`), no existing tool changes.
- One configure site and one run site (`src/entry/tool.rs:317`), and the undeclared-key refusal sits before it (`:209-232`).
- Builtin: the `catalogues` rule gains the sentence and the `TOOLS SET HERE` block (`src/render_agent/catalogue.rs:63-85`). What it says matches the source, and the guide has a `Tools` section.
- `tools` is in `ROOT_KEYS`.
- Failing cases and controls are present in `params.rs`, `tool_config.rs` and the catalogue tests.
- One commit, in shape.

### Against branch 1
`git merge-tree` gives one conflict, `src/render_agent/catalogue.rs`: both edit the same sentence. No semantic conflict; one-pass touches none of `tool.rs`, `dispatch.rs`, `entry/tool.rs`, `tool_catalogue.rs`. Land one-pass first and let this small branch take the merge: keep "where it comes from" and add the tools sentence and block.

---

## 4. mockspace-extra-lints `work/tools-upstream` at `2d804f9`: progress note, no verdict

No `ready` was posted for this branch after `f16a057`; the 08:10 `ready` is for the carriers.

1. **Drift from kaski's `canon`: fixed** as of kaski `35e9b8c2b`. Every non-test file is identical apart from module wiring in `lib.rs`. The missing tests are the ones over kaski's committed corpus. kaski still carries its own copy, so two implementations stand until it switches.
2. **`bevy-quote-check`: fixed.** `bevy.rs` and its tests are gone; the remaining `bevy` strings are test fixtures.
3. **`tackle`'s subject: fixed** through the carrier (`crates/tools/tackle/src/lib.rs:39,57,81`), empty by default. Rustdoc at `:19-21` still explains itself in kaski's and kamu's terms.
4. **Published prose: partly.**
   - `ticket-model` README and `lib.rs`, and the tagline, are corrected.
   - Not fixed: `kaski-tickets` still names the crate in `crates/tools/tickets/src/{close_gone.rs:9, ids.rs:5, landing.rs:9, lib.rs:10, refs.rs:8, repoint.rs:10, write.rs}`.
   - New: `crates/planning/src/lib.rs:7-8` cites `DESIGN.md.tmpl` and `mock/WORKFLOW.md.tmpl`, neither of which is in this repository.
5. **Left to port:**
   - The trial merge with one-pass still conflicts in 11 paths (`src/lints/a_git_dependency_is_not_pinned.rs`, seven lint test files, two root tests).
   - 14 files under `crates/registry-lints/src` read the disk or a removed field themselves (`tree_walk.rs`, `canon_rows.rs`, `a_retired_word_stays_out.rs` the heaviest).
   - The one-pass guard test covers only `/src` (`tests/the_lints_read_through_the_view.rs:6`) and must be widened to `crates/registry-lints/src`.
   - Four manifest lines point at `work/extension-carriers` behind a FIXME and go back to `dev`.

Five new commits, in shape, no bylines.

---

## Landing order

1. **mockspace one-pass**, after its two fixes. From each consumer's next launcher resolve, the commit gates of kaski, arvo, huurre, hilavitkutin, muisti and tassu fail to build.
2. **extra-lints one-pass**, the same minute. tassu heals. The pack half heals for the rest.
3. **Consumer ports**, each committed in its own repository. A port cannot pass its own gate before 1 and 2, so the window cannot be zero.
   - arvo, huurre, hilavitkutin: one commit each from the patches, plus the tool lock updates for arvo and huurre.
   - kaski and muisti: unbounded until blocking item 1 of branch 1 is met. kaski's commit must also move its lock off `489996a`.
4. **extension-carriers**, after its test fix, merged with the new `dev`. It breaks nobody.
5. **tools-upstream**: merge extra-lints `dev`, port, repoint the manifests, post `ready`, review. Then kaski drops its copies.

I did not establish how often the launcher re-resolves a branch pin.

Scratch notes are `rv2-*` under `/tmp/claude-0/-home-user/043fb1ca-2567-5c5e-93b5-7a1c2a432e48/scratchpad/` (the three patches, the carriers diff, test bodies). No repository file was changed. I ran `git fetch` in the clones, and trial merges that write loose objects only.

